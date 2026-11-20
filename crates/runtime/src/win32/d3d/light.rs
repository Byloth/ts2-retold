//! Lights (`IDirect3DLight`): the light sources Direct3D's lighting uses for
//! vertices the program has not lit itself. A light takes effect in the
//! viewport it is added to.

use crate::Machine;
use crate::win32::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::win32::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS};

const IID_IDIRECT3DLIGHT: Guid =
    guid(0x4417C142, 0x33AD, 0x11CF, [0x81, 0x6F, 0x00, 0x00, 0xC0, 0x20, 0x15, 0x6E]);
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
const D3DERR_LIGHTHASVIEWPORT: u32 = ddraw::dd_error(751);
const D3DERR_LIGHTNOTINTHISVIEWPORT: u32 = ddraw::dd_error(752);
const D3DLIGHT_ACTIVE: u32 = 1;
/// `D3DLIGHT2`: size, type, colour (four floats), position and direction
/// (three floats each), range, falloff, three attenuation factors, the two
/// spotlight angles, and flags. `D3DLIGHT` is the same without the flags.
const LIGHT2_WORDS: usize = 20;
const LIGHT_WORDS: usize = LIGHT2_WORDS - 1;
const FLAGS: usize = LIGHT2_WORDS - 1;

#[derive(Debug)]
pub struct Light {
    references: u32,
    pointer: u32,
    /// The `D3DLIGHT2` last set.
    pub data: [u32; LIGHT2_WORDS],
    /// The viewport it was added to.
    viewport: Option<usize>,
}

impl Machine {
    fn light_index(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let light = self.os.ddraw.lights.get(index)?;
        (light.references > 0 && light.pointer == this).then_some(index)
    }
}

/// `IDirect3D3::CreateLight`.
pub(super) fn create_light(m: &mut Machine, a: &[u32]) -> u32 {
    if a[2] != 0 {
        return CLASS_E_NOAGGREGATION;
    }
    if !ddraw::is_live_object(m, a[0]) {
        return DDERR_INVALIDOBJECT;
    }
    let index = m.os.ddraw.lights.len();
    let pointer = m.interface_pointer(&LIGHT, index as u32);
    m.os.ddraw.lights.push(Light { references: 1, pointer, data: [0; LIGHT2_WORDS], viewport: None });
    if m.memory.write(a[1], 4, pointer).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

/// `IDirect3DViewport3::AddLight`. The viewport holds a reference to the light.
pub(super) fn add_to_viewport(m: &mut Machine, viewport: usize, light: u32) -> u32 {
    let Some(index) = m.light_index(light) else { return DDERR_INVALIDOBJECT };
    let light = &mut m.os.ddraw.lights[index];
    if light.viewport.is_some() {
        return D3DERR_LIGHTHASVIEWPORT;
    }
    light.viewport = Some(viewport);
    light.references += 1;
    m.os.ddraw.viewports[viewport].lights.push(index);
    DD_OK
}

/// `IDirect3DViewport3::DeleteLight`.
pub(super) fn remove_from_viewport(m: &mut Machine, viewport: usize, light: u32) -> u32 {
    let Some(index) = m.light_index(light) else { return DDERR_INVALIDOBJECT };
    let light = &mut m.os.ddraw.lights[index];
    if light.viewport != Some(viewport) {
        return D3DERR_LIGHTNOTINTHISVIEWPORT;
    }
    light.viewport = None;
    light.references -= 1;
    m.os.ddraw.viewports[viewport].lights.retain(|&other| other != index);
    DD_OK
}

fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DDERR_INVALIDPARAMS };
    let Some(index) = m.light_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECT3DLIGHT {
        m.note(|| format!("interface {} not offered", ddraw::format_guid(&wanted)));
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.ddraw.lights[index].references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DDERR_INVALIDPARAMS }
}

fn add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.light_index(a[0]) else { return 0 };
    let light = &mut m.os.ddraw.lights[index];
    light.references += 1;
    light.references
}

fn release(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.light_index(a[0]) else { return 0 };
    let light = &mut m.os.ddraw.lights[index];
    light.references -= 1;
    light.references
}

fn set_light(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.light_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let mut words: [u32; LIGHT2_WORDS] = m.read_words(a[1]);
    match words[0] as usize {
        size if size == 4 * LIGHT2_WORDS => {}
        // The older structure has no flags; such a light is simply on
        // (UNVERIFIED against DirectX 6).
        size if size == 4 * LIGHT_WORDS => words[FLAGS] = D3DLIGHT_ACTIVE,
        _ => return DDERR_INVALIDPARAMS,
    }
    // Types run from point (1) to "GL spot" (5).
    if !(1..=5).contains(&words[1]) {
        return DDERR_INVALIDPARAMS;
    }
    m.os.ddraw.lights[index].data = words;
    DD_OK
}

fn get_light(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.light_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let mut words = m.os.ddraw.lights[index].data;
    // As many words as the caller's structure has, under the caller's size.
    let count = match m.memory.read(a[1], 4) {
        Some(size) if size as usize == 4 * LIGHT2_WORDS => LIGHT2_WORDS,
        Some(size) if size as usize == 4 * LIGHT_WORDS => LIGHT_WORDS,
        _ => return DDERR_INVALIDPARAMS,
    };
    words[0] = 4 * count as u32;
    if m.write_words(a[1], &words[..count]) { DD_OK } else { DDERR_INVALIDPARAMS }
}

interface! { static LIGHT = "IDirect3DLight";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    Initialize(2),
    SetLight(2) => set_light,
    GetLight(2) => get_light,
}
