//! Direct3D, as the DirectX 5 "immediate mode" interfaces the game uses
//! (`IDirect3D2`, `IDirect3DDevice2` and friends).

pub mod caps;

use super::HostResult;
use super::com::{Guid, guid, interface};
use super::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT};
use crate::Machine;

pub const IID_IDIRECT3D2: Guid = guid(
    0x6AAE1EC1,
    0x662A,
    0x11D0,
    [0x88, 0x9D, 0x00, 0xAA, 0x00, 0xBB, 0xB7, 0x6A],
);
pub const IID_IDIRECT3D3: Guid = guid(
    0xBB223240,
    0xE72B,
    0x11D0,
    [0xA9, 0xB4, 0x00, 0xAA, 0x00, 0xC0, 0x99, 0x3E],
);
/// Identifies the hardware device.
const IID_IDIRECT3DHALDEVICE: Guid = guid(
    0x84E63DE0,
    0x46AA,
    0x11CF,
    [0x81, 0x6F, 0x00, 0x00, 0xC0, 0x20, 0x15, 0x6E],
);

/// Guest copies of the constant data handed to enumeration callbacks.
#[derive(Debug, Clone, Copy)]
pub struct DeviceStrings {
    guid: u32,
    description: u32,
    name: u32,
}

fn enum_devices(m: &mut Machine, a: &[u32]) -> HostResult {
    let (callback, context) = (a[1], a[2]);
    if !ddraw::is_live_object(m, a[0]) {
        return Ok(DDERR_INVALIDOBJECT);
    }
    let strings = match m.os.ddraw.device_strings {
        Some(strings) => strings,
        None => {
            let guid = m.os.allocate(&mut m.memory, 16);
            m.memory
                .load(guid, &IID_IDIRECT3DHALDEVICE)
                .expect("block was just allocated");
            let strings = DeviceStrings {
                guid,
                description: m.os.string(
                    &mut m.memory,
                    b"Microsoft Direct3D Hardware acceleration through Direct3D HAL",
                ),
                name: m.os.string(&mut m.memory, b"Direct3D HAL"),
            };
            m.os.ddraw.device_strings = Some(strings);
            strings
        }
    };
    // Both descriptions live on the program's stack during the callback.
    let saved = m.cpu.regs[4];
    let hardware = saved - 4 * caps::WORDS as u32;
    let emulation = hardware - 4 * caps::WORDS as u32;
    m.write_words(hardware, &caps::hardware());
    m.write_words(emulation, &caps::emulation());
    m.cpu.regs[4] = emulation;
    let answer = m.call_guest(
        callback,
        &[
            strings.guid,
            strings.description,
            strings.name,
            hardware,
            emulation,
            context,
        ],
    );
    m.cpu.regs[4] = saved;
    // There is one device, so the answer (continue or stop) changes nothing.
    answer?;
    Ok(DD_OK)
}

interface! { pub(crate) static DIRECT_3D_2 = "IDirect3D2";
    QueryInterface(3) => ddraw::query_interface,
    AddRef(1) => ddraw::add_ref,
    Release(1) => ddraw::release,
    EnumDevices(3) => enum_devices,
    CreateLight(3),
    CreateMaterial(3),
    CreateViewport(3),
    FindDevice(3),
    CreateDevice(4),
}

interface! { pub(crate) static DIRECT_3D_3 = "IDirect3D3";
    QueryInterface(3) => ddraw::query_interface,
    AddRef(1) => ddraw::add_ref,
    Release(1) => ddraw::release,
    EnumDevices(3) => enum_devices,
    CreateLight(3),
    CreateMaterial(3),
    CreateViewport(3),
    FindDevice(3),
    CreateDevice(5),
    CreateVertexBuffer(5),
    EnumZBufferFormats(4),
    EvictManagedTextures(1),
}
