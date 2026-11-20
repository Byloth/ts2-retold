//! DDRAW: DirectDraw.
//!
//! What the program sees is one display adapter with hardware 3D, described
//! in [`caps`]. Layouts and constants follow the DirectX 6 headers.

mod caps;
pub mod surface;

use super::com::{E_INVALIDARG, E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use super::d3d::{self, DIRECT_3D_2, DIRECT_3D_3, IID_IDIRECT3D2, IID_IDIRECT3D3};
use super::{HostResult, functions};
use crate::win32::user32::{Display, WM_DISPLAYCHANGE};
use crate::{Machine, Stop};

pub const DD_OK: u32 = S_OK;
pub const DDERR_INVALIDPARAMS: u32 = E_INVALIDARG;
pub const DDERR_INVALIDOBJECT: u32 = dd_error(130);
pub const DDERR_INVALIDMODE: u32 = dd_error(120);
pub const DDERR_NOTFOUND: u32 = dd_error(255);
pub const DDERR_INVALIDDIRECTDRAWGUID: u32 = dd_error(561);
pub const DDERR_NOEXCLUSIVEMODE: u32 = dd_error(225);
pub const DDERR_EXCLUSIVEMODEALREADYSET: u32 = dd_error(581);

const fn dd_error(code: u32) -> u32 {
    0x8876_0000 | code
}

const IID_IDIRECTDRAW: Guid =
    guid(0x6C14DB80, 0xA733, 0x11CE, [0xA5, 0x21, 0x00, 0x20, 0xAF, 0x0B, 0xE5, 0x60]);
const IID_IDIRECTDRAW4: Guid =
    guid(0x9C59509A, 0x39BD, 0x11D1, [0x8C, 0x4A, 0x00, 0xC0, 0x4F, 0xD9, 0x30, 0xC5]);
const IID_IDIRECTDRAW2: Guid =
    guid(0xB3A6F3E0, 0x2B43, 0x11CF, [0xA2, 0xDE, 0x00, 0xAA, 0x00, 0xB9, 0x33, 0x56]);

const DDSCL_FULLSCREEN: u32 = 0x0001;
const DDSCL_NORMAL: u32 = 0x0008;
const DDSCL_EXCLUSIVE: u32 = 0x0010;

const DDENUMRET_OK: u32 = 1;
/// Refresh rate of every mode, in hertz.
const DISPLAY_REFRESH_RATE: u32 = 60;
const DDEDM_REFRESHRATES: u32 = 0x1;

/// `DDSURFACEDESC`, as an array of 27 words. All its fields are 32 bits wide.
pub mod surface_desc {
    pub const WORDS: usize = 27;
    /// `DDSURFACEDESC2` of DirectDraw 4 is the same with three more
    /// capability words and a texture stage after `CAPS`.
    pub const WORDS_V2: usize = 31;
    pub const SIZE: usize = 0;
    pub const FLAGS: usize = 1;
    pub const HEIGHT: usize = 2;
    pub const WIDTH: usize = 3;
    pub const PITCH: usize = 4;
    pub const BACK_BUFFER_COUNT: usize = 5;
    /// Mip-map count, z-buffer depth or refresh rate, depending on the flags.
    pub const REFRESH_RATE: usize = 6;
    pub const SURFACE: usize = 9;
    /// `DDPIXELFORMAT`: size, flags, FourCC, bit count, red, green, blue and alpha masks.
    /// `DDCOLORKEY` for copies from the surface: low and high value.
    pub const SOURCE_BLIT_COLOR_KEY: usize = 16;
    pub const PIXEL_FORMAT: usize = 18;
    pub const CAPS: usize = 26;

    pub const DDSD_CAPS: u32 = 0x0001;
    pub const DDSD_HEIGHT: u32 = 0x0002;
    pub const DDSD_WIDTH: u32 = 0x0004;
    pub const DDSD_PITCH: u32 = 0x0008;
    pub const DDSD_PIXELFORMAT: u32 = 0x1000;
    pub const DDSD_REFRESHRATE: u32 = 0x0004_0000;
}

const DDPF_PALETTEINDEXED8: u32 = 0x20;
const DDPF_RGB: u32 = 0x40;

/// One DirectDraw object.
#[derive(Debug)]
struct DirectDraw {
    references: u32,
    /// Interface pointers already handed out, in the order of [`INTERFACES`].
    pointers: [u32; INTERFACES.len()],
    /// Window and flags given to `SetCooperativeLevel`.
    window: u32,
    cooperative_level: u32,
}

#[derive(Debug, Default)]
pub struct State {
    objects: Vec<DirectDraw>,
    /// Guest strings describing the driver, created on first enumeration.
    driver_strings: Option<(u32, u32)>,
    pub(crate) device_strings: Option<d3d::DeviceStrings>,
    /// The desktop's display mode, while the program has changed it.
    desktop: Option<Display>,
    pub(crate) surfaces: Vec<surface::Surface>,
    /// Whole-surface transfers from memory to the video backend, and back.
    /// The second kind is the expensive one.
    pub uploads: u64,
    pub downloads: u64,
    /// Frames shown by flipping.
    pub frames: u64,
}

/// The display modes the adapter offers, in the order Windows lists them.
pub fn display_modes() -> impl Iterator<Item = Display> {
    // The low resolutions were ordinary modes on accelerators of the time,
    // and the game looks for 320x240 by default.
    const SIZES: [(u32, u32); 11] = [
        (320, 200),
        (320, 240),
        (400, 300),
        (512, 384),
        (640, 400),
        (640, 480),
        (800, 600),
        (1024, 768),
        (1152, 864),
        (1280, 1024),
        (1600, 1200),
    ];
    [8, 16, 32].into_iter().flat_map(|bits_per_pixel| {
        SIZES.into_iter().map(move |(width, height)| Display { width, height, bits_per_pixel })
    })
}

/// The pixel format of a display mode, as the eight words of `DDPIXELFORMAT`.
fn pixel_format(bits_per_pixel: u32) -> [u32; 8] {
    let (flags, red, green, blue) = match bits_per_pixel {
        8 => (DDPF_RGB | DDPF_PALETTEINDEXED8, 0, 0, 0),
        16 => (DDPF_RGB, 0xF800, 0x07E0, 0x001F),
        _ => (DDPF_RGB, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF),
    };
    [32, flags, 0, bits_per_pixel, red, green, blue, 0]
}

/// Bytes per row of a surface, rounded up to a multiple of four.
pub fn pitch(width: u32, bits_per_pixel: u32) -> u32 {
    (width * bits_per_pixel.div_ceil(8) + 3) & !3
}

fn mode_description(mode: Display, refresh_rate: u32) -> [u32; surface_desc::WORDS] {
    use surface_desc::*;
    let mut words = [0; WORDS];
    words[SIZE] = 4 * WORDS as u32;
    words[FLAGS] = DDSD_HEIGHT | DDSD_WIDTH | DDSD_PITCH | DDSD_PIXELFORMAT | DDSD_REFRESHRATE;
    words[HEIGHT] = mode.height;
    words[WIDTH] = mode.width;
    words[PITCH] = pitch(mode.width, mode.bits_per_pixel);
    words[REFRESH_RATE] = refresh_rate;
    words[PIXEL_FORMAT..PIXEL_FORMAT + 8].copy_from_slice(&pixel_format(mode.bits_per_pixel));
    words
}

impl Machine {
    fn direct_draw(&mut self, this: u32) -> Option<&mut DirectDraw> {
        let index = self.object_index(this)?;
        self.os.ddraw.objects.get_mut(index).filter(|object| object.references > 0)
    }
}

/// The one adapter: the primary display, which Windows reports with a null
/// GUID. Returns its description and its driver name as guest strings.
fn driver_strings(m: &mut Machine) -> (u32, u32) {
    if let Some(strings) = m.os.ddraw.driver_strings {
        return strings;
    }
    let strings =
        (m.os.string(&mut m.memory, b"Primary Display Driver"), m.os.string(&mut m.memory, b"display"));
    m.os.ddraw.driver_strings = Some(strings);
    strings
}

/// Whether an interface pointer names a DirectDraw object still in use.
pub(crate) fn is_live_object(m: &mut Machine, this: u32) -> bool {
    m.direct_draw(this).is_some()
}

functions! { "DDRAW.dll";
    DirectDrawEnumerateA(2) => |m, a| -> HostResult {
        let (description, name) = driver_strings(m);
        m.call_guest(a[0], &[0, description, name, a[1]])?;
        Ok(DD_OK)
    },
    DirectDrawEnumerateExA(3) => |m, a| -> HostResult {
        // The flags ask for further adapters (secondary, detached,
        // non-display); there is only the primary one, on no particular monitor.
        let (description, name) = driver_strings(m);
        m.call_guest(a[0], &[0, description, name, a[1], 0])?;
        Ok(DD_OK)
    },
    DirectDrawCreate(3) => |m, a| {
        // The first argument selects the adapter: null for the primary one,
        // the small values 1 and 2 for "hardware only" and "emulation only",
        // or a pointer to an adapter's GUID. The game passes a pointer to an
        // all-zero GUID for the primary adapter, which DirectDraw accepts
        // like a null pointer. No other adapter exists to be named.
        let (selector, out, outer) = (a[0], a[1], a[2]);
        if outer != 0 {
            return DDERR_INVALIDPARAMS;
        }
        if selector > 2 && m.read_guid(selector) != Some([0; 16]) {
            return DDERR_INVALIDDIRECTDRAWGUID;
        }
        let index = m.os.ddraw.objects.len() as u32;
        let pointer = m.interface_pointer(&DIRECT_DRAW, index);
        m.os.ddraw.objects.push(DirectDraw {
            references: 1,
            pointers: [pointer, 0, 0, 0, 0],
            window: 0,
            cooperative_level: 0,
        });
        match m.memory.write(out, 4, pointer) {
            Some(()) => DD_OK,
            None => DDERR_INVALIDPARAMS,
        }
    },
}

pub(crate) fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let (this, out) = (a[0], a[2]);
    let Some(wanted) = m.read_guid(a[1]) else {
        return DDERR_INVALIDPARAMS;
    };
    let Some(index) = m.object_index(this).filter(|&i| i < m.os.ddraw.objects.len()) else {
        return DDERR_INVALIDOBJECT;
    };
    let slot = match wanted {
        IID_IUNKNOWN | IID_IDIRECTDRAW => 0,
        IID_IDIRECTDRAW2 => 1,
        IID_IDIRECTDRAW4 => 2,
        IID_IDIRECT3D2 => 3,
        IID_IDIRECT3D3 => 4,
        _ => {
            m.note(|| format!("interface {} not offered", format_guid(&wanted)));
            let _ = m.memory.write(out, 4, 0);
            return E_NOINTERFACE;
        }
    };
    let mut pointer = m.os.ddraw.objects[index].pointers[slot];
    if pointer == 0 {
        pointer = m.interface_pointer(INTERFACES[slot], index as u32);
        m.os.ddraw.objects[index].pointers[slot] = pointer;
    }
    m.os.ddraw.objects[index].references += 1;
    match m.memory.write(out, 4, pointer) {
        Some(()) => S_OK,
        None => DDERR_INVALIDPARAMS,
    }
}

pub(crate) fn add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    m.direct_draw(a[0]).map_or(0, |object| {
        object.references += 1;
        object.references
    })
}

pub(crate) fn release(m: &mut Machine, a: &[u32]) -> u32 {
    m.direct_draw(a[0]).map_or(0, |object| {
        object.references -= 1;
        object.references
    })
}

fn set_cooperative_level(m: &mut Machine, a: &[u32]) -> u32 {
    let (window, flags) = (a[1], a[2]);
    let exclusive = flags & DDSCL_EXCLUSIVE != 0;
    // Exclusive access means full screen, and excludes "normal".
    if exclusive != (flags & DDSCL_FULLSCREEN != 0) || exclusive == (flags & DDSCL_NORMAL != 0) {
        return DDERR_INVALIDPARAMS;
    }
    let Some(index) = m.object_index(a[0]) else {
        return DDERR_INVALIDOBJECT;
    };
    let taken = m.os.ddraw.objects.iter().enumerate().any(|(other, object)| {
        other != index && object.references > 0 && object.cooperative_level & DDSCL_EXCLUSIVE != 0
    });
    if exclusive && taken {
        return DDERR_EXCLUSIVEMODEALREADYSET;
    }
    let Some(object) = m.direct_draw(a[0]) else {
        return DDERR_INVALIDOBJECT;
    };
    object.window = window;
    object.cooperative_level = flags;
    DD_OK
}

fn get_caps(m: &mut Machine, a: &[u32]) -> u32 {
    if m.direct_draw(a[0]).is_none() {
        return DDERR_INVALIDOBJECT;
    }
    for (address, capabilities) in [(a[1], caps::driver()), (a[2], caps::emulation())] {
        if address == 0 {
            continue;
        }
        // The caller states how much of the structure it knows about.
        let size = m.memory.read(address, 4).unwrap_or(0) as usize;
        if !caps::SIZES.contains(&size) || !m.write_words(address, &capabilities[..size / 4]) {
            return DDERR_INVALIDPARAMS;
        }
        let _ = m.memory.write(address, 4, size as u32);
    }
    DD_OK
}

fn get_available_video_memory(m: &mut Machine, a: &[u32]) -> u32 {
    if m.direct_draw(a[0]).is_none() {
        return DDERR_INVALIDOBJECT;
    }
    for out in [a[2], a[3]] {
        if out != 0 && m.memory.write(out, 4, caps::VIDEO_MEMORY).is_none() {
            return DDERR_INVALIDPARAMS;
        }
    }
    DD_OK
}

fn enum_display_modes(m: &mut Machine, a: &[u32], description_words: usize) -> HostResult {
    let (flags, filter, context, callback) = (a[1], a[2], a[3], a[4]);
    if m.direct_draw(a[0]).is_none() {
        return Ok(DDERR_INVALIDOBJECT);
    }
    if filter != 0 {
        return Err(Stop::Unimplemented {
            dll: "IDirectDraw".into(),
            name: "EnumDisplayModes with a filter".into(),
        });
    }
    // Without the flag, modes that differ only in refresh rate are one mode
    // and the rate is reported as 0, "the adapter's default".
    let refresh_rate = if flags & DDEDM_REFRESHRATES != 0 { DISPLAY_REFRESH_RATE } else { 0 };
    for mode in display_modes() {
        let mut words = [0; surface_desc::WORDS_V2];
        words[..surface_desc::WORDS].copy_from_slice(&mode_description(mode, refresh_rate));
        words[surface_desc::SIZE] = 4 * description_words as u32;
        // The description lives on the program's stack during the callback.
        let saved = m.cpu.regs[4];
        let at = saved - 4 * description_words as u32;
        m.write_words(at, &words[..description_words]);
        m.cpu.regs[4] = at;
        let answer = m.call_guest(callback, &[at, context]);
        m.cpu.regs[4] = saved;
        if answer? != DDENUMRET_OK {
            break;
        }
    }
    Ok(DD_OK)
}

/// Changes the display mode. Only the application with exclusive access may.
fn set_display_mode(m: &mut Machine, this: u32, mode: Display, refresh_rate: u32) -> HostResult {
    let Some((window, level)) = m.direct_draw(this).map(|object| (object.window, object.cooperative_level))
    else {
        return Ok(DDERR_INVALIDOBJECT);
    };
    if level & DDSCL_EXCLUSIVE == 0 {
        return Ok(DDERR_NOEXCLUSIVEMODE);
    }
    if !display_modes().any(|offered| offered == mode)
        || (refresh_rate != 0 && refresh_rate != DISPLAY_REFRESH_RATE)
    {
        return Ok(DDERR_INVALIDMODE);
    }
    m.note(|| format!("{}x{} at {} bits", mode.width, mode.height, mode.bits_per_pixel));
    change_display(m, mode, window)?;
    Ok(DD_OK)
}

/// Puts the display in `mode`, remembering the desktop's mode the first
/// time, and tells the windows.
fn change_display(m: &mut Machine, mode: Display, full_screen_window: u32) -> Result<(), Stop> {
    let previous = m.os.user.display;
    if previous == mode {
        return Ok(());
    }
    m.os.ddraw.desktop.get_or_insert(previous);
    m.os.user.display = mode;
    // The full-screen window covers the screen whatever its size becomes.
    m.move_window(full_screen_window, 0, 0, mode.width as i32, mode.height as i32)?;
    m.broadcast(WM_DISPLAYCHANGE, mode.bits_per_pixel, mode.width | mode.height << 16)
}

fn restore_display_mode(m: &mut Machine, a: &[u32]) -> HostResult {
    let Some(window) = m.direct_draw(a[0]).map(|object| object.window) else {
        return Ok(DDERR_INVALIDOBJECT);
    };
    if let Some(desktop) = m.os.ddraw.desktop.take() {
        change_display(m, desktop, window)?;
        m.os.ddraw.desktop = None;
    }
    Ok(DD_OK)
}

/// `GetDisplayMode`: describes the mode the display is in.
fn get_display_mode(m: &mut Machine, a: &[u32], description_words: usize) -> u32 {
    if m.direct_draw(a[0]).is_none() {
        return DDERR_INVALIDOBJECT;
    }
    if m.memory.read(a[1], 4) != Some(4 * description_words as u32) {
        return DDERR_INVALIDPARAMS;
    }
    let mut words = [0; surface_desc::WORDS_V2];
    words[..surface_desc::WORDS].copy_from_slice(&mode_description(m.os.user.display, DISPLAY_REFRESH_RATE));
    words[surface_desc::SIZE] = 4 * description_words as u32;
    if m.write_words(a[1], &words[..description_words]) { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn format_guid(g: &Guid) -> String {
    format!(
        "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
        u32::from_le_bytes([g[0], g[1], g[2], g[3]]),
        u16::from_le_bytes([g[4], g[5]]),
        u16::from_le_bytes([g[6], g[7]]),
        g[8],
        g[9],
        g[10],
        g[11],
        g[12],
        g[13],
        g[14],
        g[15]
    )
}

interface! { static DIRECT_DRAW = "IDirectDraw";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    Compact(1),
    CreateClipper(4),
    CreatePalette(5),
    CreateSurface(4),
    DuplicateSurface(3),
    EnumDisplayModes(5) => |m, a| enum_display_modes(m, a, surface_desc::WORDS),
    EnumSurfaces(5),
    FlipToGDISurface(1),
    GetCaps(3) => get_caps,
    GetDisplayMode(2) => |m, a| get_display_mode(m, a, surface_desc::WORDS),
    GetFourCCCodes(3),
    GetGDISurface(2),
    GetMonitorFrequency(2),
    GetScanLine(2),
    GetVerticalBlankStatus(2),
    Initialize(2),
    RestoreDisplayMode(1) => restore_display_mode,
    SetCooperativeLevel(3) => set_cooperative_level,
    SetDisplayMode(4) => |m, a| {
        set_display_mode(m, a[0], Display { width: a[1], height: a[2], bits_per_pixel: a[3] }, 0)
    },
    WaitForVerticalBlank(3),
}

interface! { static DIRECT_DRAW_2 = "IDirectDraw2";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    Compact(1),
    CreateClipper(4),
    CreatePalette(5),
    CreateSurface(4),
    DuplicateSurface(3),
    EnumDisplayModes(5) => |m, a| enum_display_modes(m, a, surface_desc::WORDS),
    EnumSurfaces(5),
    FlipToGDISurface(1),
    GetCaps(3) => get_caps,
    GetDisplayMode(2) => |m, a| get_display_mode(m, a, surface_desc::WORDS),
    GetFourCCCodes(3),
    GetGDISurface(2),
    GetMonitorFrequency(2),
    GetScanLine(2),
    GetVerticalBlankStatus(2),
    Initialize(2),
    RestoreDisplayMode(1) => restore_display_mode,
    SetCooperativeLevel(3) => set_cooperative_level,
    SetDisplayMode(6) => |m, a| {
        // The flags only concern the old standard VGA mode 13h.
        set_display_mode(m, a[0], Display { width: a[1], height: a[2], bits_per_pixel: a[3] }, a[4])
    },
    WaitForVerticalBlank(3),
    GetAvailableVidMem(4) => get_available_video_memory,
}

interface! { static DIRECT_DRAW_4 = "IDirectDraw4";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    Compact(1),
    CreateClipper(4),
    CreatePalette(5),
    CreateSurface(4) => surface::create_surface,
    DuplicateSurface(3),
    EnumDisplayModes(5) => |m, a| enum_display_modes(m, a, surface_desc::WORDS_V2),
    EnumSurfaces(5),
    FlipToGDISurface(1),
    GetCaps(3) => get_caps,
    GetDisplayMode(2) => |m, a| get_display_mode(m, a, surface_desc::WORDS_V2),
    GetFourCCCodes(3),
    GetGDISurface(2),
    GetMonitorFrequency(2),
    GetScanLine(2),
    GetVerticalBlankStatus(2),
    Initialize(2),
    RestoreDisplayMode(1) => restore_display_mode,
    SetCooperativeLevel(3) => set_cooperative_level,
    SetDisplayMode(6) => |m, a| {
        // The flags only concern the old standard VGA mode 13h.
        set_display_mode(m, a[0], Display { width: a[1], height: a[2], bits_per_pixel: a[3] }, a[4])
    },
    WaitForVerticalBlank(3),
    GetAvailableVidMem(4) => get_available_video_memory,
    GetSurfaceFromDC(3),
    RestoreAllSurfaces(1),
    TestCooperativeLevel(1),
    GetDeviceIdentifier(3),
}

/// The interfaces of a DirectDraw object. Direct3D is an aspect of it.
static INTERFACES: [&super::com::Interface; 5] =
    [&DIRECT_DRAW, &DIRECT_DRAW_2, &DIRECT_DRAW_4, &DIRECT_3D_2, &DIRECT_3D_3];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_descriptions_are_well_formed() {
        let modes: Vec<Display> = display_modes().collect();
        assert_eq!(modes.len(), 33);
        assert_eq!(modes[0], Display { width: 320, height: 200, bits_per_pixel: 8 });
        let words = mode_description(Display { width: 800, height: 600, bits_per_pixel: 16 }, 0);
        assert_eq!(words[surface_desc::SIZE], 108);
        assert_eq!((words[surface_desc::WIDTH], words[surface_desc::HEIGHT]), (800, 600));
        assert_eq!(words[surface_desc::PITCH], 1600);
        assert_eq!(
            words[surface_desc::PIXEL_FORMAT..][..8],
            [32, DDPF_RGB, 0, 16, 0xF800, 0x07E0, 0x001F, 0]
        );
        assert_eq!(pitch(1, 8), 4);
        assert_eq!(pitch(1023, 16), 2048);
    }

    #[test]
    fn guids_have_the_memory_layout_of_the_headers() {
        assert_eq!(
            IID_IDIRECTDRAW2,
            [0xE0, 0xF3, 0xA6, 0xB3, 0x43, 0x2B, 0xCF, 0x11, 0xA2, 0xDE, 0x00, 0xAA, 0x00, 0xB9, 0x33, 0x56]
        );
        assert_eq!(format_guid(&IID_IDIRECTDRAW), "{6c14db80-a733-11ce-a521-0020af0be560}");
    }
}
