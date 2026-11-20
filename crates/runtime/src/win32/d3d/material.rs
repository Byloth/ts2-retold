//! Materials (`IDirect3DMaterial3`): the surface colours Direct3D's lighting
//! uses. The device refers to a material by a handle.

use super::device;
use crate::Machine;
use crate::win32::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::win32::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS};

const IID_IDIRECT3DMATERIAL3: Guid =
    guid(0xCA9C46F4, 0xD3C5, 0x11D1, [0xB7, 0x5A, 0x00, 0x60, 0x08, 0x52, 0xB3, 0x12]);
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
/// `D3DMATERIAL`: size, then diffuse, ambient, specular and emissive colours
/// (four floats each), specular power, texture handle and ramp size.
const MATERIAL_WORDS: usize = 20;

#[derive(Debug)]
pub struct Material {
    references: u32,
    pointer: u32,
    pub data: [u32; MATERIAL_WORDS],
}

impl Machine {
    fn material_index(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let material = self.os.ddraw.materials.get(index)?;
        (material.references > 0 && material.pointer == this).then_some(index)
    }
}

/// The handle by which devices know material number `index`. Zero is "none".
pub(super) fn handle(index: usize) -> u32 {
    index as u32 + 1
}

/// `IDirect3D3::CreateMaterial`.
pub(super) fn create_material(m: &mut Machine, a: &[u32]) -> u32 {
    if a[2] != 0 {
        return CLASS_E_NOAGGREGATION;
    }
    if !ddraw::is_live_object(m, a[0]) {
        return DDERR_INVALIDOBJECT;
    }
    let index = m.os.ddraw.materials.len();
    let pointer = m.interface_pointer(&MATERIAL_3, index as u32);
    let mut data = [0; MATERIAL_WORDS];
    data[0] = 4 * MATERIAL_WORDS as u32;
    m.os.ddraw.materials.push(Material { references: 1, pointer, data });
    if m.memory.write(a[1], 4, pointer).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DDERR_INVALIDPARAMS };
    let Some(index) = m.material_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECT3DMATERIAL3 {
        m.note(|| format!("interface {} not offered", ddraw::format_guid(&wanted)));
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.ddraw.materials[index].references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DDERR_INVALIDPARAMS }
}

fn add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.material_index(a[0]) else { return 0 };
    let material = &mut m.os.ddraw.materials[index];
    material.references += 1;
    material.references
}

fn release(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.material_index(a[0]) else { return 0 };
    let material = &mut m.os.ddraw.materials[index];
    material.references -= 1;
    material.references
}

fn set_material(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.material_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let words: [u32; MATERIAL_WORDS] = m.read_words(a[1]);
    if words[0] != 4 * MATERIAL_WORDS as u32 {
        return DDERR_INVALIDPARAMS;
    }
    m.os.ddraw.materials[index].data = words;
    DD_OK
}

fn get_material(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.material_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let words = m.os.ddraw.materials[index].data;
    if m.write_words(a[1], &words) { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn get_handle(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.material_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if !device::is_device(m, a[1]) {
        return DDERR_INVALIDOBJECT;
    }
    if m.memory.write(a[2], 4, handle(index)).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

interface! { static MATERIAL_3 = "IDirect3DMaterial3";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    SetMaterial(2) => set_material,
    GetMaterial(2) => get_material,
    GetHandle(3) => get_handle,
}
