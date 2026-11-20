//! The 3D device (`IDirect3DDevice3`).

use super::IID_IDIRECT3DHALDEVICE;
use crate::Machine;
use crate::video::PixelFormat;
use crate::win32::HostResult;
use crate::win32::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::win32::ddraw::surface::{DDSCAPS_3DDEVICE, DDSCAPS_ZBUFFER};
use crate::win32::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS};

const IID_IDIRECT3DDEVICE3: Guid =
    guid(0xB0AB3B60, 0x33D7, 0x11D1, [0xA9, 0x81, 0x00, 0xC0, 0x4F, 0xD7, 0xB1, 0x74]);
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
const DDSCL_FPUPRESERVE: u32 = 0x1000;
/// The precision-control field of the x87 control word; zero selects single precision.
const X87_PRECISION_CONTROL: u16 = 0x0300;

#[derive(Debug)]
pub struct Device {
    references: u32,
    pointer: u32,
    /// The surface it draws to, and the z-buffer attached to that surface.
    pub target: usize,
    pub depth: Option<usize>,
    viewports: Vec<usize>,
    current_viewport: Option<usize>,
    in_scene: bool,
    /// `D3DRENDERSTATE` values by number.
    render_states: [u32; 256],
    /// `D3DLIGHTSTATE` values by number: material handle, ambient light,
    /// colour model, fog mode and range.
    light_states: [u32; LIGHT_STATES],
    /// The texture surface selected into each stage.
    textures: [Option<usize>; STAGES],
    /// `D3DTEXTURESTAGESTATE` values of each stage, by number.
    stage_states: [[u32; STAGE_STATES]; STAGES],
}

const LIGHT_STATES: usize = 16;
const STAGES: usize = 8;
const STAGE_STATES: usize = 32;
const D3DERR_SCENE_IN_SCENE: u32 = ddraw::dd_error(760);
const D3DERR_SCENE_NOT_IN_SCENE: u32 = ddraw::dd_error(761);

/// Render states as a device starts with them under DirectX 6.
fn default_render_states(has_depth: bool) -> [u32; 256] {
    let mut states = [0; 256];
    for (state, value) in [
        (4, 0),                 // TEXTUREPERSPECTIVE: off
        (7, has_depth as u32),  // ZENABLE
        (8, 3),                 // FILLMODE: solid
        (9, 2),                 // SHADEMODE: Gouraud
        (14, 1),                // ZWRITEENABLE
        (16, 1),                // LASTPIXEL
        (17, 1),                // TEXTUREMAG: nearest
        (18, 1),                // TEXTUREMIN: nearest
        (19, 2),                // SRCBLEND: one
        (20, 1),                // DESTBLEND: zero
        (21, 2),                // TEXTUREMAPBLEND: modulate
        (22, 3),                // CULLMODE: counter-clockwise
        (23, 4),                // ZFUNC: less or equal
        (25, 8),                // ALPHAFUNC: always
        (29, 1),                // SPECULARENABLE
        (37, 1.0f32.to_bits()), // FOGTABLEEND
        (38, 1.0f32.to_bits()), // FOGTABLEDENSITY
        (3, 1),                 // TEXTUREADDRESS: wrap
        (44, 1),                // TEXTUREADDRESSU
        (45, 1),                // TEXTUREADDRESSV
    ] {
        states[state] = value;
    }
    states
}

/// Light states as a device starts with them: no material, no ambient
/// light, the RGB colour model, no vertex fog, fog from 1.0 to 100.0.
fn default_light_states() -> [u32; LIGHT_STATES] {
    let mut states = [0; LIGHT_STATES];
    states[3] = 2; // COLORMODEL: RGB
    states[5] = 1.0f32.to_bits(); // FOGSTART
    states[6] = 100.0f32.to_bits(); // FOGEND
    states[7] = 1.0f32.to_bits(); // FOGDENSITY
    states
}

/// Whether an interface pointer names a device still in use.
pub(super) fn is_device(m: &mut Machine, this: u32) -> bool {
    m.device_index(this).is_some()
}

/// Texture stage states as a device starts with them: stage 0 modulates the
/// texture with the vertex colour, the other stages are disabled.
fn default_stage_states() -> [[u32; STAGE_STATES]; STAGES] {
    let mut stages = [[0; STAGE_STATES]; STAGES];
    for (number, stage) in stages.iter_mut().enumerate() {
        stage[1] = if number == 0 { 4 } else { 1 }; // COLOROP: modulate / disable
        stage[2] = 2; // COLORARG1: texture
        stage[3] = 1; // COLORARG2: current
        stage[4] = if number == 0 { 2 } else { 1 }; // ALPHAOP: select argument 1 / disable
        stage[5] = 2; // ALPHAARG1: texture
        stage[6] = 1; // ALPHAARG2: current
        stage[11] = number as u32; // TEXCOORDINDEX
        stage[12] = 1; // ADDRESS: wrap
        stage[13] = 1; // ADDRESSU
        stage[14] = 1; // ADDRESSV
        stage[16] = 1; // MAGFILTER: point
        stage[17] = 1; // MINFILTER: point
        stage[18] = 1; // MIPFILTER: none
        stage[21] = 1; // MAXANISOTROPY
    }
    stages
}

/// The render target and z-buffer of a device.
pub(super) fn targets(m: &Machine, device: usize) -> (usize, Option<usize>) {
    let device = &m.os.ddraw.devices[device];
    (device.target, device.depth)
}

impl Machine {
    fn device_index(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let device = self.os.ddraw.devices.get(index)?;
        (device.references > 0 && device.pointer == this).then_some(index)
    }
}

/// `IDirect3D3::CreateDevice`.
pub(super) fn create_device(m: &mut Machine, a: &[u32]) -> HostResult {
    let (class, surface, out, outer) = (a[1], a[2], a[3], a[4]);
    if outer != 0 {
        return Ok(CLASS_E_NOAGGREGATION);
    }
    let Some(cooperative_level) = ddraw::cooperative_level(m, a[0]) else { return Ok(DDERR_INVALIDOBJECT) };
    if m.read_guid(class) != Some(IID_IDIRECT3DHALDEVICE) {
        return Ok(ddraw::DDERR_NOTFOUND);
    }
    let Some(target) = m.surface_index(surface) else { return Ok(DDERR_INVALIDOBJECT) };
    if m.os.ddraw.surfaces[target].caps & DDSCAPS_3DDEVICE == 0 {
        return Ok(DDERR_INVALIDPARAMS);
    }
    let depth = m.attached_surface(target, DDSCAPS_ZBUFFER);
    m.add_surface_reference(target);

    let index = m.os.ddraw.devices.len();
    let pointer = m.interface_pointer(&DEVICE_3, index as u32);
    m.os.ddraw.devices.push(Device {
        references: 1,
        pointer,
        target,
        depth,
        viewports: Vec::new(),
        current_viewport: None,
        in_scene: false,
        render_states: default_render_states(depth.is_some()),
        light_states: default_light_states(),
        textures: [None; STAGES],
        stage_states: default_stage_states(),
    });
    if cooperative_level & DDSCL_FPUPRESERVE == 0 {
        // Direct3D computes in single precision and leaves the processor
        // that way unless asked to preserve its state (ADR 0010).
        m.cpu.fpu.control &= !X87_PRECISION_CONTROL;
    }
    Ok(if m.memory.write(out, 4, pointer).is_some() { DD_OK } else { DDERR_INVALIDPARAMS })
}

fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DDERR_INVALIDPARAMS };
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECT3DDEVICE3 {
        m.note(|| format!("interface {} not offered", ddraw::format_guid(&wanted)));
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.ddraw.devices[index].references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DDERR_INVALIDPARAMS }
}

fn add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return 0 };
    let device = &mut m.os.ddraw.devices[index];
    device.references += 1;
    device.references
}

fn release(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return 0 };
    let device = &mut m.os.ddraw.devices[index];
    device.references -= 1;
    let (remaining, target) = (device.references, device.target);
    if remaining == 0 {
        m.release_surface(target);
    }
    remaining
}

fn get_caps(m: &mut Machine, a: &[u32]) -> u32 {
    if m.device_index(a[0]).is_none() {
        return DDERR_INVALIDOBJECT;
    }
    for (address, description) in [(a[1], super::caps::hardware()), (a[2], super::caps::emulation())] {
        // The caller's size selects how much of the description it gets.
        let size = m.memory.read(address, 4).unwrap_or(0) as usize;
        if !(4..=4 * super::caps::WORDS).contains(&size) || !size.is_multiple_of(4) {
            return DDERR_INVALIDPARAMS;
        }
        let mut words = description;
        words[0] = size as u32;
        if !m.write_words(address, &words[..size / 4]) {
            return DDERR_INVALIDPARAMS;
        }
    }
    DD_OK
}

fn add_viewport(m: &mut Machine, a: &[u32]) -> u32 {
    let (Some(index), Some(viewport)) = (m.device_index(a[0]), m.viewport_index(a[1])) else {
        return DDERR_INVALIDOBJECT;
    };
    if m.os.ddraw.viewports[viewport].device.is_some() {
        return DDERR_INVALIDPARAMS;
    }
    m.os.ddraw.viewports[viewport].device = Some(index);
    m.os.ddraw.devices[index].viewports.push(viewport);
    DD_OK
}

fn set_current_viewport(m: &mut Machine, a: &[u32]) -> u32 {
    let (Some(index), Some(viewport)) = (m.device_index(a[0]), m.viewport_index(a[1])) else {
        return DDERR_INVALIDOBJECT;
    };
    if !m.os.ddraw.devices[index].viewports.contains(&viewport) {
        return DDERR_INVALIDPARAMS;
    }
    m.os.ddraw.devices[index].current_viewport = Some(viewport);
    DD_OK
}

fn begin_scene(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let device = &mut m.os.ddraw.devices[index];
    if std::mem::replace(&mut device.in_scene, true) { D3DERR_SCENE_IN_SCENE } else { DD_OK }
}

fn end_scene(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let device = &mut m.os.ddraw.devices[index];
    if std::mem::replace(&mut device.in_scene, false) { DD_OK } else { D3DERR_SCENE_NOT_IN_SCENE }
}

fn set_render_state(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    match m.os.ddraw.devices[index].render_states.get_mut(a[1] as usize) {
        Some(state) => {
            *state = a[2];
            DD_OK
        }
        None => DDERR_INVALIDPARAMS,
    }
}

fn get_render_state(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let Some(&value) = m.os.ddraw.devices[index].render_states.get(a[1] as usize) else {
        return DDERR_INVALIDPARAMS;
    };
    if m.memory.write(a[2], 4, value).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

/// The formats a texture may have, in the order they are enumerated.
pub(crate) const TEXTURE_FORMATS: [PixelFormat; 5] = [
    PixelFormat::Rgb565,
    PixelFormat::Argb1555,
    PixelFormat::Argb4444,
    PixelFormat::Xrgb8888,
    PixelFormat::Argb8888,
];

/// `EnumTextureFormats`: calls back with each texture format until told to stop.
fn enum_texture_formats(m: &mut Machine, a: &[u32]) -> HostResult {
    let (callback, context) = (a[1], a[2]);
    if m.device_index(a[0]).is_none() {
        return Ok(DDERR_INVALIDOBJECT);
    }
    for format in TEXTURE_FORMATS {
        let answer = m.with_stack_words(&format.to_words(), |m, at| m.call_guest(callback, &[at, context]));
        if answer? != ddraw::DDENUMRET_OK {
            break;
        }
    }
    Ok(DD_OK)
}

fn set_light_state(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    // Light states are numbered from 1.
    match m.os.ddraw.devices[index].light_states.get_mut(a[1] as usize).filter(|_| a[1] != 0) {
        Some(state) => {
            *state = a[2];
            DD_OK
        }
        None => DDERR_INVALIDPARAMS,
    }
}

fn get_light_state(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let Some(&value) = m.os.ddraw.devices[index].light_states.get(a[1] as usize).filter(|_| a[1] != 0) else {
        return DDERR_INVALIDPARAMS;
    };
    if m.memory.write(a[2], 4, value).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn set_texture(m: &mut Machine, a: &[u32]) -> u32 {
    let (stage, texture) = (a[1] as usize, a[2]);
    let Some(index) = m.device_index(a[0]) else { return DDERR_INVALIDOBJECT };
    // A null pointer takes the texture out of the stage.
    let selected = if texture == 0 {
        None
    } else {
        match m.texture_index(texture) {
            Some(surface) => Some(surface),
            None => return DDERR_INVALIDOBJECT,
        }
    };
    match m.os.ddraw.devices[index].textures.get_mut(stage) {
        Some(slot) => {
            *slot = selected;
            DD_OK
        }
        None => DDERR_INVALIDPARAMS,
    }
}

fn stage_state<'m>(m: &'m mut Machine, a: &[u32]) -> Option<&'m mut u32> {
    let index = m.device_index(a[0])?;
    m.os.ddraw.devices[index].stage_states.get_mut(a[1] as usize)?.get_mut(a[2] as usize)
}

fn set_texture_stage_state(m: &mut Machine, a: &[u32]) -> u32 {
    match stage_state(m, a) {
        Some(state) => {
            *state = a[3];
            DD_OK
        }
        None => DDERR_INVALIDPARAMS,
    }
}

fn get_texture_stage_state(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(value) = stage_state(m, a).map(|state| *state) else { return DDERR_INVALIDPARAMS };
    if m.memory.write(a[3], 4, value).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

interface! { static DEVICE_3 = "IDirect3DDevice3";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    GetCaps(3) => get_caps,
    GetStats(2),
    AddViewport(2) => add_viewport,
    DeleteViewport(2),
    NextViewport(4),
    EnumTextureFormats(3) => enum_texture_formats,
    BeginScene(1) => begin_scene,
    EndScene(1) => end_scene,
    GetDirect3D(2),
    SetCurrentViewport(2) => set_current_viewport,
    GetCurrentViewport(2),
    SetRenderTarget(3),
    GetRenderTarget(2),
    Begin(4),
    BeginIndexed(6),
    Vertex(2),
    Index(2),
    End(2),
    GetRenderState(3) => get_render_state,
    SetRenderState(3) => set_render_state,
    GetLightState(3) => get_light_state,
    SetLightState(3) => set_light_state,
    SetTransform(3),
    GetTransform(3),
    MultiplyTransform(3),
    DrawPrimitive(6),
    DrawIndexedPrimitive(8),
    SetClipStatus(2),
    GetClipStatus(2),
    DrawPrimitiveStrided(6),
    DrawIndexedPrimitiveStrided(8),
    DrawPrimitiveVB(6),
    DrawIndexedPrimitiveVB(6),
    ComputeSphereVisibility(6),
    GetTexture(3),
    SetTexture(3) => set_texture,
    GetTextureStageState(4) => get_texture_stage_state,
    SetTextureStageState(4) => set_texture_stage_state,
    ValidateDevice(2),
}
