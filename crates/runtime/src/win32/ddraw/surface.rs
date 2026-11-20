//! DirectDraw surfaces (`IDirectDrawSurface4`).
//!
//! A surface's pixels have two homes (ADR 0012): a block of the program's
//! memory, which `Lock` exposes, and a surface of the video backend, which
//! draws and shows. Each home is either current or stale. An operation runs
//! on the video side when that side is current, otherwise in memory, and
//! leaves the other side stale; pixels are transferred only when an
//! operation needs a side that is stale. Flipping exchanges the two homes of
//! the front and back buffers, so interface pointers stay put while the
//! contents move, as on Windows.

use super::surface_desc::{self, WORDS_V2};
use super::{
    DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS, DDERR_NOEXCLUSIVEMODE, DDERR_NOTFOUND, DDSCL_EXCLUSIVE,
    dd_error,
};
use crate::video::{PixelFormat, Rect, SurfaceId, copy_pixels, fill_pixels};
use crate::win32::HostResult;
use crate::win32::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::{Machine, Stop};

const DDERR_INVALIDRECT: u32 = dd_error(150);
const DDERR_SURFACEBUSY: u32 = dd_error(430);
const DDERR_NOTLOCKED: u32 = dd_error(584);
const DDERR_INVALIDPIXELFORMAT: u32 = dd_error(145);
const DDERR_NOCOLORKEY: u32 = dd_error(215);
const DDERR_CANNOTATTACHSURFACE: u32 = dd_error(10);
const DDERR_NOTFLIPPABLE: u32 = dd_error(582);

const IID_IDIRECTDRAWSURFACE4: Guid =
    guid(0x0B2B8630, 0xAD35, 0x11D0, [0x8E, 0xA6, 0x00, 0x60, 0x97, 0x97, 0xEA, 0x5B]);

// DDSCAPS: what a surface is.
pub const DDSCAPS_BACKBUFFER: u32 = 0x0000_0004;
pub const DDSCAPS_COMPLEX: u32 = 0x0000_0008;
pub const DDSCAPS_FLIP: u32 = 0x0000_0010;
pub const DDSCAPS_FRONTBUFFER: u32 = 0x0000_0020;
pub const DDSCAPS_OFFSCREENPLAIN: u32 = 0x0000_0040;
pub const DDSCAPS_PRIMARYSURFACE: u32 = 0x0000_0200;
pub const DDSCAPS_SYSTEMMEMORY: u32 = 0x0000_0800;
pub const DDSCAPS_TEXTURE: u32 = 0x0000_1000;
pub const DDSCAPS_3DDEVICE: u32 = 0x0000_2000;
pub const DDSCAPS_VIDEOMEMORY: u32 = 0x0000_4000;
pub const DDSCAPS_VISIBLE: u32 = 0x0000_8000;
pub const DDSCAPS_ZBUFFER: u32 = 0x0002_0000;
pub const DDSCAPS_MIPMAP: u32 = 0x0040_0000;
pub const DDSCAPS_LOCALVIDMEM: u32 = 0x1000_0000;

const DDSD_BACKBUFFERCOUNT: u32 = 0x0020;
const DDSD_LPSURFACE: u32 = 0x0800;
const DDSD_CKSRCBLT: u32 = 0x0001_0000;
const DDSD_MIPMAPCOUNT: u32 = 0x0002_0000;

const DDLOCK_READONLY: u32 = 0x0010;
const DDBLT_COLORFILL: u32 = 0x0000_0400;
const DDBLT_KEYSRC: u32 = 0x0000_8000;
const DDBLT_WAIT: u32 = 0x0100_0000;
const DDBLT_DEPTHFILL: u32 = 0x0200_0000;
const DDBLTFAST_SRCCOLORKEY: u32 = 0x1;
const DDBLTFAST_WAIT: u32 = 0x10;
const DDCKEY_SRCBLT: u32 = 0x8;
const DDFLIP_WAIT: u32 = 0x1;
/// Position of the fill colour (or depth) in `DDBLTFX`, in words.
const BLTFX_FILL: u32 = 20;

/// The two homes of a surface's pixels.
#[derive(Debug, Clone, Copy)]
struct Store {
    /// Guest address of `height` rows of `pitch` bytes.
    memory: u32,
    video: SurfaceId,
    memory_current: bool,
    video_current: bool,
}

#[derive(Debug)]
pub struct Surface {
    references: u32,
    pointer: u32,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub caps: u32,
    pitch: u32,
    store: Store,
    /// Surfaces attached to this one: the next buffer of a flip chain, a z-buffer.
    attached: Vec<usize>,
    /// Number of back buffers, on the front buffer of a flip chain.
    back_buffers: u32,
    source_color_key: Option<(u32, u32)>,
    locked: bool,
}

impl Surface {
    fn area(&self) -> Rect {
        Rect::whole(self.width, self.height)
    }

    fn bytes(&self) -> u32 {
        self.pitch * self.height
    }
}

fn not_implemented(what: String) -> HostResult {
    Err(Stop::Unimplemented { dll: "IDirectDrawSurface4".into(), name: what })
}

impl Machine {
    pub(crate) fn surface_index(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let surface = self.os.ddraw.surfaces.get(index)?;
        (surface.references > 0 && surface.pointer == this).then_some(index)
    }

    fn new_surface(&mut self, width: u32, height: u32, format: PixelFormat, caps: u32) -> usize {
        let pitch = super::pitch(width, 8 * format.bytes_per_pixel());
        let memory = self.os.allocate(&mut self.memory, pitch * height);
        let video = self.video.create_surface(width, height, format);
        let index = self.os.ddraw.surfaces.len();
        let pointer = self.interface_pointer(&SURFACE_4, index as u32);
        self.os.ddraw.surfaces.push(Surface {
            references: 1,
            pointer,
            width,
            height,
            format,
            caps,
            pitch,
            // Fresh memory is zeroed, which is as good a content as any.
            store: Store { memory, video, memory_current: true, video_current: false },
            attached: Vec::new(),
            back_buffers: 0,
            source_color_key: None,
            locked: false,
        });
        index
    }

    pub(crate) fn surface_format(&self, index: usize) -> PixelFormat {
        self.os.ddraw.surfaces[index].format
    }

    /// The surface attached to `index` that has all of the given capabilities.
    pub(crate) fn attached_surface(&self, index: usize, caps: u32) -> Option<usize> {
        let surfaces = &self.os.ddraw.surfaces;
        surfaces[index].attached.iter().copied().find(|&other| surfaces[other].caps & caps == caps)
    }

    pub(crate) fn add_surface_reference(&mut self, index: usize) {
        self.os.ddraw.surfaces[index].references += 1;
    }

    /// Drops one reference to a surface, destroying it with the last.
    pub(crate) fn release_surface(&mut self, index: usize) -> u32 {
        let surface = &mut self.os.ddraw.surfaces[index];
        surface.references -= 1;
        let remaining = surface.references;
        if remaining == 0 {
            // The surface goes, and with it its hold on what is attached to it.
            let (video, attached) = (surface.store.video, std::mem::take(&mut surface.attached));
            self.video.destroy_surface(video);
            for other in attached {
                self.release_surface(other);
            }
        }
        remaining
    }

    /// Brings the memory home of a surface up to date.
    fn ensure_memory(&mut self, index: usize) {
        let surface = &self.os.ddraw.surfaces[index];
        if surface.store.memory_current {
            return;
        }
        let (store, area, pitch) = (surface.store, surface.area(), surface.pitch as usize);
        let mut pixels = vec![0; surface.bytes() as usize];
        self.video.download(store.video, area, pitch, &mut pixels);
        self.memory.load(store.memory, &pixels).expect("surface memory is mapped");
        self.os.ddraw.downloads += 1;
        self.os.ddraw.surfaces[index].store.memory_current = true;
    }

    /// Brings the video home of a surface up to date.
    pub(crate) fn ensure_video(&mut self, index: usize) -> SurfaceId {
        let surface = &self.os.ddraw.surfaces[index];
        let store = surface.store;
        if !store.video_current {
            let (area, pitch) = (surface.area(), surface.pitch as usize);
            let pixels = self.memory.dump(store.memory, surface.bytes()).expect("surface memory is mapped");
            self.video.upload(store.video, area, pitch, &pixels);
            self.os.ddraw.uploads += 1;
            self.os.ddraw.surfaces[index].store.video_current = true;
        }
        store.video
    }

    /// Reads a `RECT` (left, top, right, bottom) of a surface; a null
    /// pointer means the whole surface.
    fn surface_rect(&mut self, index: usize, address: u32) -> Option<Rect> {
        let surface = &self.os.ddraw.surfaces[index];
        let (width, height) = (surface.width, surface.height);
        if address == 0 {
            return Some(Rect::whole(width, height));
        }
        let [left, top, right, bottom]: [u32; 4] = self.read_words(address);
        let rect =
            Rect { x: left, y: top, width: right.checked_sub(left)?, height: bottom.checked_sub(top)? };
        rect.fits(width, height).then_some(rect)
    }

    fn describe_surface(&self, index: usize) -> [u32; WORDS_V2] {
        use surface_desc::*;
        let surface = &self.os.ddraw.surfaces[index];
        let mut words = [0; WORDS_V2];
        words[SIZE] = 4 * WORDS_V2 as u32;
        words[FLAGS] = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PITCH | DDSD_PIXELFORMAT;
        words[HEIGHT] = surface.height;
        words[WIDTH] = surface.width;
        words[PITCH] = surface.pitch;
        if surface.back_buffers > 0 {
            words[FLAGS] |= DDSD_BACKBUFFERCOUNT;
            words[BACK_BUFFER_COUNT] = surface.back_buffers;
        }
        if let Some((low, high)) = surface.source_color_key {
            words[FLAGS] |= DDSD_CKSRCBLT;
            words[SOURCE_BLIT_COLOR_KEY] = low;
            words[SOURCE_BLIT_COLOR_KEY + 1] = high;
        }
        words[PIXEL_FORMAT..PIXEL_FORMAT + 8].copy_from_slice(&surface.format.to_words());
        words[CAPS] = surface.caps;
        words
    }

    /// Sets a rectangle of a surface to one raw pixel value.
    pub(crate) fn fill_surface(&mut self, index: usize, area: Rect, value: u32) {
        let surface = &mut self.os.ddraw.surfaces[index];
        let (store, pitch, size) = (surface.store, surface.pitch, surface.format.bytes_per_pixel());
        if store.video_current {
            surface.store.memory_current = false;
            self.video.fill(store.video, area, value);
            return;
        }
        surface.store.video_current = false;
        let mut row = vec![0; (area.width * size) as usize];
        let row_bytes = row.len();
        fill_pixels(&mut row, row_bytes, area.width, 1, size as usize, value);
        for y in area.y..area.y + area.height {
            self.memory
                .load(store.memory + y * pitch + area.x * size, &row)
                .expect("surface memory is mapped");
        }
    }

    /// Copies a rectangle from one surface to another of the same format.
    fn copy_surface(
        &mut self,
        source: usize,
        from: Rect,
        target: usize,
        to: Rect,
        color_key: Option<(u32, u32)>,
    ) {
        if self.os.ddraw.surfaces[target].store.video_current {
            let source_video = self.ensure_video(source);
            let surface = &mut self.os.ddraw.surfaces[target];
            surface.store.memory_current = false;
            self.video.copy(source_video, from, surface.store.video, to, color_key);
            return;
        }
        self.ensure_memory(source);
        let (source_store, source_pitch) = {
            let surface = &self.os.ddraw.surfaces[source];
            (surface.store, surface.pitch)
        };
        let surface = &mut self.os.ddraw.surfaces[target];
        surface.store.video_current = false;
        let (store, pitch, size) = (surface.store, surface.pitch, surface.format.bytes_per_pixel());
        let span = |rect: Rect, pitch: u32| (rect.height - 1) * pitch + rect.width * size;
        let pixels = self
            .memory
            .dump(source_store.memory + from.y * source_pitch + from.x * size, span(from, source_pitch))
            .expect("surface memory is mapped");
        let start = store.memory + to.y * pitch + to.x * size;
        // With a colour key or scaling the target's own pixels matter too.
        let mut out = self.memory.dump(start, span(to, pitch)).expect("surface memory is mapped");
        copy_pixels(
            &pixels,
            source_pitch as usize,
            (from.width, from.height),
            &mut out,
            pitch as usize,
            (to.width, to.height),
            size as usize,
            color_key,
        );
        self.memory.load(start, &out).expect("surface memory is mapped");
    }
}

/// `IDirectDraw4::CreateSurface`.
pub(super) fn create_surface(m: &mut Machine, a: &[u32]) -> HostResult {
    use surface_desc::*;
    let (description, out, outer) = (a[1], a[2], a[3]);
    let Some(level) = m.direct_draw(a[0]).map(|object| object.cooperative_level) else {
        return Ok(DDERR_INVALIDOBJECT);
    };
    let words: [u32; WORDS_V2] = m.read_words(description);
    if outer != 0 || words[SIZE] != 4 * WORDS_V2 as u32 || words[FLAGS] & DDSD_CAPS == 0 {
        return Ok(DDERR_INVALIDPARAMS);
    }
    let (flags, caps) = (words[FLAGS], words[CAPS]);
    let sized = DDSD_WIDTH | DDSD_HEIGHT;
    let display = m.os.user.display;
    let display_format = PixelFormat::of_display(display.bits_per_pixel);
    let memory_caps = if caps & DDSCAPS_SYSTEMMEMORY != 0 {
        DDSCAPS_SYSTEMMEMORY
    } else {
        DDSCAPS_VIDEOMEMORY | DDSCAPS_LOCALVIDMEM
    };

    let index = if caps & DDSCAPS_PRIMARYSURFACE != 0 {
        // The primary surface is the screen: its size and format are the
        // display mode's and cannot be asked for.
        if flags & (sized | DDSD_PIXELFORMAT) != 0 {
            return Ok(DDERR_INVALIDPARAMS);
        }
        let back_buffers = if flags & DDSD_BACKBUFFERCOUNT != 0 { words[BACK_BUFFER_COUNT] } else { 0 };
        let chain = DDSCAPS_FLIP | DDSCAPS_COMPLEX;
        if (back_buffers > 0) != (caps & chain == chain) {
            return Ok(DDERR_INVALIDPARAMS);
        }
        if back_buffers > 0 && level & DDSCL_EXCLUSIVE == 0 {
            return Ok(DDERR_NOEXCLUSIVEMODE);
        }
        if back_buffers > 1 {
            return not_implemented("a flip chain with more than one back buffer".into());
        }
        let device = caps & DDSCAPS_3DDEVICE;
        let front_caps = DDSCAPS_PRIMARYSURFACE | DDSCAPS_VISIBLE | memory_caps | device;
        let front = m.new_surface(display.width, display.height, display_format, front_caps);
        if back_buffers == 1 {
            let back_caps = DDSCAPS_BACKBUFFER | chain | memory_caps | device;
            let back = m.new_surface(display.width, display.height, display_format, back_caps);
            let surface = &mut m.os.ddraw.surfaces[front];
            surface.caps |= DDSCAPS_FRONTBUFFER | chain;
            surface.back_buffers = 1;
            surface.attached.push(back);
        }
        front
    } else {
        if flags & sized != sized || words[WIDTH] == 0 || words[HEIGHT] == 0 {
            return Ok(DDERR_INVALIDPARAMS);
        }
        if flags & DDSD_MIPMAPCOUNT != 0 || caps & (DDSCAPS_MIPMAP | DDSCAPS_COMPLEX) != 0 {
            return not_implemented("a mip-mapped or complex surface".into());
        }
        let format = if flags & DDSD_PIXELFORMAT != 0 {
            let description: [u32; 8] =
                words[PIXEL_FORMAT..PIXEL_FORMAT + 8].try_into().expect("eight words");
            match PixelFormat::from_words(&description) {
                Some(format) => format,
                None => return Ok(DDERR_INVALIDPIXELFORMAT),
            }
        } else {
            display_format
        };
        let kind = caps & (DDSCAPS_ZBUFFER | DDSCAPS_TEXTURE | DDSCAPS_OFFSCREENPLAIN | DDSCAPS_3DDEVICE);
        if (kind & DDSCAPS_ZBUFFER != 0) != (format == PixelFormat::Depth16) {
            return Ok(DDERR_INVALIDPIXELFORMAT);
        }
        let index = m.new_surface(words[WIDTH], words[HEIGHT], format, kind | memory_caps);
        if flags & DDSD_CKSRCBLT != 0 {
            let key = (words[SOURCE_BLIT_COLOR_KEY], words[SOURCE_BLIT_COLOR_KEY + 1]);
            m.os.ddraw.surfaces[index].source_color_key = Some(key);
        }
        index
    };
    let surface = &m.os.ddraw.surfaces[index];
    let (pointer, width, height, format, caps) =
        (surface.pointer, surface.width, surface.height, surface.format, surface.caps);
    m.note(|| format!("{width}x{height} {format:?}, caps {caps:#x}"));
    Ok(if m.memory.write(out, 4, pointer).is_some() { DD_OK } else { DDERR_INVALIDPARAMS })
}

fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DDERR_INVALIDPARAMS };
    let Some(index) = m.surface_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECTDRAWSURFACE4 {
        m.note(|| format!("interface {} not offered", super::format_guid(&wanted)));
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.ddraw.surfaces[index].references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DDERR_INVALIDPARAMS }
}

fn add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.surface_index(a[0]) else { return 0 };
    let surface = &mut m.os.ddraw.surfaces[index];
    surface.references += 1;
    surface.references
}

fn release(m: &mut Machine, a: &[u32]) -> u32 {
    match m.surface_index(a[0]) {
        Some(index) => m.release_surface(index),
        None => 0,
    }
}

fn add_attached_surface(m: &mut Machine, a: &[u32]) -> u32 {
    let (Some(index), Some(other)) = (m.surface_index(a[0]), m.surface_index(a[1])) else {
        return DDERR_INVALIDOBJECT;
    };
    // The one attachment made by hand is a z-buffer onto a 3D surface of its size.
    let (surface, attachment) = (&m.os.ddraw.surfaces[index], &m.os.ddraw.surfaces[other]);
    let allowed = attachment.caps & DDSCAPS_ZBUFFER != 0
        && surface.caps & DDSCAPS_ZBUFFER == 0
        && (surface.width, surface.height) == (attachment.width, attachment.height)
        && !surface.attached.contains(&other);
    if !allowed {
        return DDERR_CANNOTATTACHSURFACE;
    }
    m.os.ddraw.surfaces[other].references += 1;
    m.os.ddraw.surfaces[index].attached.push(other);
    DD_OK
}

fn get_attached_surface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.surface_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let Some(wanted) = m.memory.read(a[1], 4) else { return DDERR_INVALIDPARAMS };
    let Some(other) = m.attached_surface(index, wanted) else {
        let _ = m.memory.write(a[2], 4, 0);
        return DDERR_NOTFOUND;
    };
    m.os.ddraw.surfaces[other].references += 1;
    let pointer = m.os.ddraw.surfaces[other].pointer;
    if m.memory.write(a[2], 4, pointer).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn get_caps(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.surface_index(a[0]) else { return DDERR_INVALIDOBJECT };
    // DDSCAPS2: the classic capability word and three more, unused here.
    let caps = m.os.ddraw.surfaces[index].caps;
    if m.write_words(a[1], &[caps, 0, 0, 0]) { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn get_pixel_format(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.surface_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let words = m.os.ddraw.surfaces[index].format.to_words();
    if m.write_words(a[1], &words) { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn get_surface_desc(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.surface_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if m.memory.read(a[1], 4) != Some(4 * WORDS_V2 as u32) {
        return DDERR_INVALIDPARAMS;
    }
    let words = m.describe_surface(index);
    if m.write_words(a[1], &words) { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn lock(m: &mut Machine, a: &[u32]) -> u32 {
    let (rect, description, flags) = (a[1], a[2], a[3]);
    let Some(index) = m.surface_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if m.os.ddraw.surfaces[index].locked {
        return DDERR_SURFACEBUSY;
    }
    let Some(area) = m.surface_rect(index, rect) else { return DDERR_INVALIDRECT };
    if m.memory.read(description, 4) != Some(4 * WORDS_V2 as u32) {
        return DDERR_INVALIDPARAMS;
    }
    m.ensure_memory(index);
    let mut words = m.describe_surface(index);
    let surface = &mut m.os.ddraw.surfaces[index];
    words[surface_desc::FLAGS] |= DDSD_LPSURFACE;
    words[surface_desc::SURFACE] =
        surface.store.memory + area.y * surface.pitch + area.x * surface.format.bytes_per_pixel();
    surface.locked = true;
    if flags & DDLOCK_READONLY == 0 {
        // The program may write: what the video side holds is stale from now on.
        surface.store.video_current = false;
    }
    if m.write_words(description, &words) { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn unlock(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.surface_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let surface = &mut m.os.ddraw.surfaces[index];
    if !surface.locked {
        return DDERR_NOTLOCKED;
    }
    surface.locked = false;
    DD_OK
}

fn blt(m: &mut Machine, a: &[u32]) -> HostResult {
    let (target_rect, source, source_rect, flags, effects) = (a[1], a[2], a[3], a[4], a[5]);
    let Some(index) = m.surface_index(a[0]) else { return Ok(DDERR_INVALIDOBJECT) };
    if m.os.ddraw.surfaces[index].locked {
        return Ok(DDERR_SURFACEBUSY);
    }
    let Some(to) = m.surface_rect(index, target_rect) else { return Ok(DDERR_INVALIDRECT) };
    let operation = flags & !DDBLT_WAIT;
    if operation == DDBLT_COLORFILL || operation == DDBLT_DEPTHFILL {
        let depth = m.os.ddraw.surfaces[index].format == PixelFormat::Depth16;
        if depth != (operation == DDBLT_DEPTHFILL) {
            return Ok(DDERR_INVALIDPARAMS);
        }
        let Some(value) = m.memory.read(effects + 4 * BLTFX_FILL, 4) else { return Ok(DDERR_INVALIDPARAMS) };
        m.fill_surface(index, to, value);
        return Ok(DD_OK);
    }
    if operation & !DDBLT_KEYSRC != 0 {
        return not_implemented(format!("Blt with flags {flags:#x}"));
    }
    let Some(from_index) = m.surface_index(source) else { return Ok(DDERR_INVALIDOBJECT) };
    let Some(from) = m.surface_rect(from_index, source_rect) else { return Ok(DDERR_INVALIDRECT) };
    let from_surface = &m.os.ddraw.surfaces[from_index];
    if from_surface.locked {
        return Ok(DDERR_SURFACEBUSY);
    }
    if from_surface.format != m.os.ddraw.surfaces[index].format {
        return not_implemented("Blt between different pixel formats".into());
    }
    let color_key = if operation & DDBLT_KEYSRC != 0 {
        match from_surface.source_color_key {
            Some(key) => Some(key),
            None => return Ok(DDERR_NOCOLORKEY),
        }
    } else {
        None
    };
    m.copy_surface(from_index, from, index, to, color_key);
    Ok(DD_OK)
}

fn blt_fast(m: &mut Machine, a: &[u32]) -> HostResult {
    let (x, y, source, source_rect, flags) = (a[1], a[2], a[3], a[4], a[5]);
    let (Some(index), Some(from_index)) = (m.surface_index(a[0]), m.surface_index(source)) else {
        return Ok(DDERR_INVALIDOBJECT);
    };
    if flags & !(DDBLTFAST_SRCCOLORKEY | DDBLTFAST_WAIT) != 0 {
        return not_implemented(format!("BltFast with flags {flags:#x}"));
    }
    let Some(from) = m.surface_rect(from_index, source_rect) else { return Ok(DDERR_INVALIDRECT) };
    let (target, from_surface) = (&m.os.ddraw.surfaces[index], &m.os.ddraw.surfaces[from_index]);
    if target.locked || from_surface.locked {
        return Ok(DDERR_SURFACEBUSY);
    }
    // BltFast does not clip: the copy must fit the target.
    let to = Rect { x, y, ..from };
    if !to.fits(target.width, target.height) {
        return Ok(DDERR_INVALIDRECT);
    }
    if from_surface.format != target.format {
        return not_implemented("BltFast between different pixel formats".into());
    }
    let color_key = if flags & DDBLTFAST_SRCCOLORKEY != 0 {
        match from_surface.source_color_key {
            Some(key) => Some(key),
            None => return Ok(DDERR_NOCOLORKEY),
        }
    } else {
        None
    };
    m.copy_surface(from_index, from, index, to, color_key);
    Ok(DD_OK)
}

fn flip(m: &mut Machine, a: &[u32]) -> HostResult {
    let (target_override, flags) = (a[1], a[2]);
    let Some(front) = m.surface_index(a[0]) else { return Ok(DDERR_INVALIDOBJECT) };
    if target_override != 0 || flags & !DDFLIP_WAIT != 0 {
        return not_implemented(format!("Flip with an override or flags {flags:#x}"));
    }
    let surfaces = &m.os.ddraw.surfaces;
    let Some(back) = m
        .attached_surface(front, DDSCAPS_BACKBUFFER)
        .filter(|_| surfaces[front].caps & DDSCAPS_FRONTBUFFER != 0)
    else {
        return Ok(DDERR_NOTFLIPPABLE);
    };
    if surfaces[front].locked || surfaces[back].locked {
        return Ok(DDERR_SURFACEBUSY);
    }
    // What was drawn to the back buffer becomes what the screen shows, and
    // the old front contents become the new back buffer.
    let shown = m.ensure_video(back);
    let (front_store, back_store) = (m.os.ddraw.surfaces[front].store, m.os.ddraw.surfaces[back].store);
    m.os.ddraw.surfaces[front].store = back_store;
    m.os.ddraw.surfaces[back].store = front_store;
    m.video.present(shown);
    m.os.ddraw.frames += 1;
    Ok(DD_OK)
}

fn set_color_key(m: &mut Machine, a: &[u32]) -> HostResult {
    let Some(index) = m.surface_index(a[0]) else { return Ok(DDERR_INVALIDOBJECT) };
    if a[1] != DDCKEY_SRCBLT {
        return not_implemented(format!("SetColorKey with flags {:#x}", a[1]));
    }
    m.os.ddraw.surfaces[index].source_color_key = if a[2] == 0 {
        None
    } else {
        // Without the "colour space" flag the key is the single low value
        // of the DDCOLORKEY.
        let low = m.memory.read(a[2], 4).unwrap_or(0);
        Some((low, low))
    };
    Ok(DD_OK)
}

fn is_lost(m: &mut Machine, a: &[u32]) -> u32 {
    // Surfaces are lost when another program takes the display; none can.
    if m.surface_index(a[0]).is_some() { DD_OK } else { DDERR_INVALIDOBJECT }
}

interface! { static SURFACE_4 = "IDirectDrawSurface4";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    AddAttachedSurface(2) => add_attached_surface,
    AddOverlayDirtyRect(2),
    Blt(6) => blt,
    BltBatch(4),
    BltFast(6) => blt_fast,
    DeleteAttachedSurface(3),
    EnumAttachedSurfaces(3),
    EnumOverlayZOrders(4),
    Flip(3) => flip,
    GetAttachedSurface(3) => get_attached_surface,
    GetBltStatus(2),
    GetCaps(2) => get_caps,
    GetClipper(2),
    GetColorKey(3),
    GetDC(2),
    GetFlipStatus(2),
    GetOverlayPosition(3),
    GetPalette(2),
    GetPixelFormat(2) => get_pixel_format,
    GetSurfaceDesc(2) => get_surface_desc,
    Initialize(3),
    IsLost(1) => is_lost,
    Lock(5) => lock,
    ReleaseDC(2),
    Restore(1) => is_lost,
    SetClipper(2),
    SetColorKey(3) => set_color_key,
    SetOverlayPosition(3),
    SetPalette(2),
    Unlock(2) => unlock,
    UpdateOverlay(6),
    UpdateOverlayDisplay(2),
    UpdateOverlayZOrder(3),
    GetDDInterface(2),
    PageLock(2),
    PageUnlock(2),
    SetSurfaceDesc(3),
    SetPrivateData(5),
    GetPrivateData(4),
    FreePrivateData(2),
    GetUniquenessValue(2),
    ChangeUniquenessValue(1),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;
    use crate::video::Headless;

    // Method numbers, from the interface tables.
    const DD_QUERY_INTERFACE: u32 = 0;
    const DD_CREATE_SURFACE: u32 = 6;
    const DD_SET_COOPERATIVE_LEVEL: u32 = 20;
    const DD_SET_DISPLAY_MODE: u32 = 21;
    const BLT: u32 = 5;
    const BLT_FAST: u32 = 7;
    const FLIP: u32 = 11;
    const GET_ATTACHED_SURFACE: u32 = 12;
    const LOCK: u32 = 25;
    const SET_COLOR_KEY: u32 = 29;
    const UNLOCK: u32 = 32;

    fn frame_pixel(m: &Machine, x: usize, y: usize) -> u16 {
        let video = m.video.as_any().downcast_ref::<Headless>().unwrap();
        let frame = video.last_frame.as_ref().expect("a frame was shown");
        let at = 2 * (y * frame.width as usize + x);
        u16::from_le_bytes([frame.pixels[at], frame.pixels[at + 1]])
    }

    /// Sets up DirectDraw as the game does and returns its `IDirectDraw4`.
    fn direct_draw(m: &mut Machine) -> u32 {
        let out = m.scratch(4);
        assert_eq!(m.host("DDRAW.dll", "DirectDrawCreate", &[0, out, 0]), DD_OK);
        let first = m.memory.read(out, 4).unwrap();
        let iid = m.scratch(16);
        m.memory.load(iid, &super::super::IID_IDIRECTDRAW4).unwrap();
        assert_eq!(m.method(first, DD_QUERY_INTERFACE, &[iid, out]), DD_OK);
        let dd = m.memory.read(out, 4).unwrap();
        assert_eq!(m.method(dd, DD_SET_COOPERATIVE_LEVEL, &[0, 0x11]), DD_OK);
        assert_eq!(m.method(dd, DD_SET_DISPLAY_MODE, &[640, 480, 16, 0, 0]), DD_OK);
        dd
    }

    fn create(m: &mut Machine, dd: u32, words: &[(usize, u32)]) -> u32 {
        let description = m.scratch(4 * WORDS_V2 as u32);
        m.memory.write(description, 4, 4 * WORDS_V2 as u32).unwrap();
        for &(word, value) in words {
            m.memory.write(description + 4 * word as u32, 4, value).unwrap();
        }
        let out = m.scratch(4);
        assert_eq!(m.method(dd, DD_CREATE_SURFACE, &[description, out, 0]), DD_OK);
        m.memory.read(out, 4).unwrap()
    }

    fn fill(m: &mut Machine, surface: u32, rect: Option<[u32; 4]>, color: u32) {
        let effects = m.scratch(100);
        m.memory.write(effects + 4 * BLTFX_FILL, 4, color).unwrap();
        let area = match rect {
            Some(words) => {
                let at = m.scratch(16);
                m.write_words(at, &words);
                at
            }
            None => 0,
        };
        assert_eq!(m.method(surface, BLT, &[area, 0, 0, DDBLT_COLORFILL | DDBLT_WAIT, effects]), DD_OK);
    }

    #[test]
    fn a_flip_chain_shows_what_was_drawn_to_the_back_buffer() {
        use surface_desc::*;
        let mut m = testing::machine();
        let dd = direct_draw(&mut m);
        let chain = DDSCAPS_PRIMARYSURFACE | DDSCAPS_FLIP | DDSCAPS_COMPLEX | DDSCAPS_3DDEVICE;
        let front = create(
            &mut m,
            dd,
            &[(FLAGS, DDSD_CAPS | DDSD_BACKBUFFERCOUNT), (BACK_BUFFER_COUNT, 1), (CAPS, chain)],
        );
        let out = m.scratch(16);
        m.write_words(out, &[DDSCAPS_BACKBUFFER, 0, 0, 0]);
        assert_eq!(m.method(front, GET_ATTACHED_SURFACE, &[out, out]), DD_OK);
        let back = m.memory.read(out, 4).unwrap();
        assert_ne!(back, front);

        // Fill the back buffer, then poke one pixel through a lock.
        fill(&mut m, back, None, 0x07E0);
        fill(&mut m, back, Some([10, 20, 30, 40]), 0xF800);
        let description = m.scratch(4 * WORDS_V2 as u32);
        m.memory.write(description, 4, 4 * WORDS_V2 as u32).unwrap();
        assert_eq!(m.method(back, LOCK, &[0, description, 0, 0]), DD_OK);
        assert_eq!(m.method(back, LOCK, &[0, description, 0, 0]), DDERR_SURFACEBUSY);
        let words: [u32; WORDS_V2] = m.read_words(description);
        assert_eq!((words[WIDTH], words[HEIGHT], words[PITCH]), (640, 480, 1280));
        m.memory.write(words[SURFACE] + 479 * 1280 + 2 * 639, 2, 0x001F).unwrap();
        assert_eq!(m.method(back, UNLOCK, &[0]), DD_OK);
        assert_eq!(m.method(back, UNLOCK, &[0]), DDERR_NOTLOCKED);

        assert_eq!(m.method(front, FLIP, &[0, DDFLIP_WAIT]), DD_OK);
        assert_eq!(frame_pixel(&m, 0, 0), 0x07E0);
        assert_eq!(frame_pixel(&m, 10, 20), 0xF800);
        assert_eq!(frame_pixel(&m, 29, 39), 0xF800);
        assert_eq!(frame_pixel(&m, 30, 40), 0x07E0);
        assert_eq!(frame_pixel(&m, 639, 479), 0x001F);
        // One transfer to the video side, none back.
        assert_eq!((m.os.ddraw.uploads, m.os.ddraw.downloads, m.os.ddraw.frames), (1, 0, 1));

        // The back buffer now holds the old front contents: untouched zeros.
        assert_eq!(m.method(back, LOCK, &[0, description, DDLOCK_READONLY, 0]), DD_OK);
        let after: [u32; WORDS_V2] = m.read_words(description);
        assert_eq!(m.memory.read(after[SURFACE], 2), Some(0));
        assert_eq!(m.method(back, UNLOCK, &[0]), DD_OK);
        // Locking what is on screen brings the pixels back from the video side.
        assert_eq!(m.method(front, LOCK, &[0, description, DDLOCK_READONLY, 0]), DD_OK);
        let shown: [u32; WORDS_V2] = m.read_words(description);
        assert_eq!(m.memory.read(shown[SURFACE] + 20 * 1280 + 2 * 10, 2), Some(0xF800));
        assert_eq!(m.method(front, UNLOCK, &[0]), DD_OK);
        assert_eq!(m.os.ddraw.downloads, 0, "the flipped surface's memory was still current");
    }

    #[test]
    fn copies_respect_the_source_colour_key_on_either_side() {
        use surface_desc::*;
        let mut m = testing::machine();
        let dd = direct_draw(&mut m);
        let plain = [(FLAGS, DDSD_CAPS | DDSD_WIDTH | DDSD_HEIGHT), (CAPS, DDSCAPS_OFFSCREENPLAIN)];
        let sprite = create(&mut m, dd, &[plain[0], plain[1], (WIDTH, 4), (HEIGHT, 4)]);
        let target = create(&mut m, dd, &[plain[0], plain[1], (WIDTH, 8), (HEIGHT, 8)]);
        fill(&mut m, sprite, None, 0x0000);
        fill(&mut m, sprite, Some([1, 1, 3, 3]), 0xFFFF);
        fill(&mut m, target, None, 0x1234);
        let key = m.scratch(8);
        assert_eq!(m.method(sprite, SET_COLOR_KEY, &[DDCKEY_SRCBLT, key]), DD_OK);

        let read = |m: &mut Machine, surface: u32, x: u32, y: u32| {
            let description = m.scratch(4 * WORDS_V2 as u32);
            m.memory.write(description, 4, 4 * WORDS_V2 as u32).unwrap();
            assert_eq!(m.method(surface, LOCK, &[0, description, DDLOCK_READONLY, 0]), DD_OK);
            let words: [u32; WORDS_V2] = m.read_words(description);
            let value = m.memory.read(words[SURFACE] + y * words[PITCH] + 2 * x, 2).unwrap();
            assert_eq!(m.method(surface, UNLOCK, &[0]), DD_OK);
            value
        };
        // In memory.
        assert_eq!(m.method(target, BLT_FAST, &[2, 2, sprite, 0, DDBLTFAST_SRCCOLORKEY]), DD_OK);
        assert_eq!(read(&mut m, target, 2, 2), 0x1234);
        assert_eq!(read(&mut m, target, 3, 3), 0xFFFF);
        assert_eq!(read(&mut m, target, 5, 5), 0x1234);
        assert_eq!(m.method(target, BLT_FAST, &[6, 6, sprite, 0, 0]), DDERR_INVALIDRECT);

        // On the video side: once the target lives there, copies happen there.
        let index = m.surface_index(target).unwrap();
        m.ensure_video(index);
        assert_eq!(m.method(target, BLT_FAST, &[4, 4, sprite, 0, DDBLTFAST_SRCCOLORKEY]), DD_OK);
        assert_eq!(m.os.ddraw.downloads, 0);
        assert_eq!(read(&mut m, target, 5, 5), 0xFFFF);
        assert_eq!(read(&mut m, target, 4, 7), 0x1234); // a keyed pixel of the sprite
        assert_eq!(m.os.ddraw.downloads, 1);
    }
}
