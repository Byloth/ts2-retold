//! Direct3D, as the DirectX 5 "immediate mode" interfaces the game uses
//! (`IDirect3D2`, `IDirect3DDevice2` and friends).

pub mod caps;

use super::HostResult;
use super::com::{Guid, guid, interface};
use super::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT};
use crate::Machine;

pub const IID_IDIRECT3D2: Guid =
    guid(0x6AAE1EC1, 0x662A, 0x11D0, [0x88, 0x9D, 0x00, 0xAA, 0x00, 0xBB, 0xB7, 0x6A]);
pub const IID_IDIRECT3D3: Guid =
    guid(0xBB223240, 0xE72B, 0x11D0, [0xA9, 0xB4, 0x00, 0xAA, 0x00, 0xC0, 0x99, 0x3E]);
/// Identifies the hardware device.
const IID_IDIRECT3DHALDEVICE: Guid =
    guid(0x84E63DE0, 0x46AA, 0x11CF, [0x81, 0x6F, 0x00, 0x00, 0xC0, 0x20, 0x15, 0x6E]);

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
            m.memory.load(guid, &IID_IDIRECT3DHALDEVICE).expect("block was just allocated");
            let strings = DeviceStrings {
                guid,
                description: m
                    .os
                    .string(&mut m.memory, b"Microsoft Direct3D Hardware acceleration through Direct3D HAL"),
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
        &[strings.guid, strings.description, strings.name, hardware, emulation, context],
    );
    m.cpu.regs[4] = saved;
    // There is one device, so the answer (continue or stop) changes nothing.
    answer?;
    Ok(DD_OK)
}

/// `FindDevice`: looks for a device matching a description. With one device
/// the search succeeds or fails as a whole.
fn find_device(m: &mut Machine, a: &[u32]) -> HostResult {
    const D3DFDS_COLORMODEL: u32 = 0x1;
    const D3DFDS_GUID: u32 = 0x2;
    const D3DFDS_HARDWARE: u32 = 0x4;
    const D3DCOLOR_RGB: u32 = 2;
    let (search, result) = (a[1], a[2]);
    if !ddraw::is_live_object(m, a[0]) {
        return Ok(DDERR_INVALIDOBJECT);
    }
    // D3DFINDDEVICESEARCH: size, flags, hardware wanted, colour model, GUID,
    // then capabilities to match.
    let [size, flags, hardware, color_model]: [u32; 4] = m.read_words(search);
    let known = D3DFDS_COLORMODEL | D3DFDS_GUID | D3DFDS_HARDWARE;
    if size < 32 || flags & !known != 0 {
        return Err(crate::Stop::Unimplemented {
            dll: "IDirect3D".into(),
            name: format!("FindDevice with flags {flags:#x}"),
        });
    }
    let matches = (flags & D3DFDS_COLORMODEL == 0 || color_model == D3DCOLOR_RGB)
        && (flags & D3DFDS_HARDWARE == 0 || hardware != 0)
        && (flags & D3DFDS_GUID == 0 || m.read_guid(search + 16) == Some(IID_IDIRECT3DHALDEVICE));
    if !matches {
        return Ok(ddraw::DDERR_NOTFOUND);
    }
    // D3DFINDDEVICERESULT: size, GUID, hardware and software descriptions.
    // Its size tells which version of the description the caller has.
    let result_size = m.memory.read(result, 4).unwrap_or(0);
    let description_words = (result_size.saturating_sub(20) / 8) as usize;
    if result_size < 20 || !(result_size - 20).is_multiple_of(8) || description_words > caps::WORDS {
        return Ok(ddraw::DDERR_INVALIDPARAMS);
    }
    let (mut hardware_caps, mut emulation_caps) = (caps::hardware(), caps::emulation());
    hardware_caps[0] = 4 * description_words as u32;
    emulation_caps[0] = 4 * description_words as u32;
    let ok = m.memory.load(result + 4, &IID_IDIRECT3DHALDEVICE).is_some()
        && m.write_words(result + 20, &hardware_caps[..description_words])
        && m.write_words(result + 20 + 4 * description_words as u32, &emulation_caps[..description_words]);
    Ok(if ok { DD_OK } else { ddraw::DDERR_INVALIDPARAMS })
}

/// `EnumZBufferFormats`: the depth formats usable with a device. There is
/// one: 16 bits, no stencil.
fn enum_z_buffer_formats(m: &mut Machine, a: &[u32]) -> HostResult {
    const DDPF_ZBUFFER: u32 = 0x400;
    let (device, callback, context) = (a[1], a[2], a[3]);
    if !ddraw::is_live_object(m, a[0]) {
        return Ok(DDERR_INVALIDOBJECT);
    }
    if m.read_guid(device) != Some(IID_IDIRECT3DHALDEVICE) {
        return Ok(ddraw::DDERR_NOTFOUND);
    }
    // DDPIXELFORMAT: size, flags, FourCC, depth bits, stencil bits, depth
    // mask, stencil mask, unused.
    let format = [32, DDPF_ZBUFFER, 0, 16, 0, 0xFFFF, 0, 0];
    let saved = m.cpu.regs[4];
    let at = saved - 32;
    m.write_words(at, &format);
    m.cpu.regs[4] = at;
    let answer = m.call_guest(callback, &[at, context]);
    m.cpu.regs[4] = saved;
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
    FindDevice(3) => find_device,
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
    FindDevice(3) => find_device,
    CreateDevice(5),
    CreateVertexBuffer(5),
    EnumZBufferFormats(4) => enum_z_buffer_formats,
    EvictManagedTextures(1),
}
