//! Viewports (`IDirect3DViewport3`): the rectangle of the render target a
//! device draws into, and the way that rectangle is cleared.

use super::{device, light};
use crate::video::{PixelFormat, Rect};
use crate::win32::HostResult;
use crate::win32::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::win32::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS};
use crate::{Machine, Stop};

const IID_IDIRECT3DVIEWPORT3: Guid =
    guid(0xB0AB3B61, 0x33D7, 0x11D1, [0xA9, 0x81, 0x00, 0xC0, 0x4F, 0xD7, 0xB1, 0x74]);
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
/// The viewport has not been given its dimensions yet.
const D3DERR_VIEWPORTDATANOTSET: u32 = ddraw::dd_error(773);
const D3DERR_VIEWPORTHASNODEVICE: u32 = ddraw::dd_error(774);
const D3DCLEAR_TARGET: u32 = 1;
const D3DCLEAR_ZBUFFER: u32 = 2;
/// `D3DVIEWPORT2`: size, x, y, width, height, then the clip volume as six floats.
const VIEWPORT2_WORDS: usize = 11;

#[derive(Debug)]
pub struct Viewport {
    references: u32,
    pointer: u32,
    /// The `D3DVIEWPORT2` last set.
    data: Option<[u32; VIEWPORT2_WORDS]>,
    /// The device it was added to.
    pub(super) device: Option<usize>,
    /// The lights added to it, in order.
    pub(super) lights: Vec<usize>,
}

impl Viewport {
    /// The rectangle of the render target it covers.
    pub fn area(&self) -> Option<Rect> {
        self.data.map(|words| Rect { x: words[1], y: words[2], width: words[3], height: words[4] })
    }
}

impl Machine {
    pub(super) fn viewport_index(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let viewport = self.os.ddraw.viewports.get(index)?;
        (viewport.references > 0 && viewport.pointer == this).then_some(index)
    }
}

/// The rectangle a viewport covers, once it has been given one.
pub(super) fn area(m: &Machine, viewport: usize) -> Option<Rect> {
    m.os.ddraw.viewports[viewport].area()
}

/// `IDirect3D3::CreateViewport`.
pub(super) fn create_viewport(m: &mut Machine, a: &[u32]) -> u32 {
    if a[2] != 0 {
        return CLASS_E_NOAGGREGATION;
    }
    if !ddraw::is_live_object(m, a[0]) {
        return DDERR_INVALIDOBJECT;
    }
    let index = m.os.ddraw.viewports.len();
    let pointer = m.interface_pointer(&VIEWPORT_3, index as u32);
    m.os.ddraw.viewports.push(Viewport {
        references: 1,
        pointer,
        data: None,
        device: None,
        lights: Vec::new(),
    });
    if m.memory.write(a[1], 4, pointer).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DDERR_INVALIDPARAMS };
    let Some(index) = m.viewport_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECT3DVIEWPORT3 {
        m.note(|| format!("interface {} not offered", ddraw::format_guid(&wanted)));
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.ddraw.viewports[index].references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DDERR_INVALIDPARAMS }
}

fn add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.viewport_index(a[0]) else { return 0 };
    let viewport = &mut m.os.ddraw.viewports[index];
    viewport.references += 1;
    viewport.references
}

fn release(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.viewport_index(a[0]) else { return 0 };
    let viewport = &mut m.os.ddraw.viewports[index];
    viewport.references -= 1;
    viewport.references
}

fn set_viewport2(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.viewport_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let words: [u32; VIEWPORT2_WORDS] = m.read_words(a[1]);
    if words[0] != 4 * VIEWPORT2_WORDS as u32 || words[3] == 0 || words[4] == 0 {
        return DDERR_INVALIDPARAMS;
    }
    m.note(|| format!("{}x{} at {},{}", words[3], words[4], words[1], words[2]));
    m.os.ddraw.viewports[index].data = Some(words);
    DD_OK
}

fn get_viewport2(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.viewport_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let Some(words) = m.os.ddraw.viewports[index].data else { return D3DERR_VIEWPORTDATANOTSET };
    if m.memory.read(a[1], 4) != Some(words[0]) {
        return DDERR_INVALIDPARAMS;
    }
    if m.write_words(a[1], &words) { DD_OK } else { DDERR_INVALIDPARAMS }
}

/// `Clear2`: sets rectangles of the render target to a colour and of the
/// z-buffer to a depth.
fn clear2(m: &mut Machine, a: &[u32]) -> HostResult {
    let (count, rects, flags, color, depth) = (a[1], a[2], a[3], a[4], f32::from_bits(a[5]));
    let Some(index) = m.viewport_index(a[0]) else { return Ok(DDERR_INVALIDOBJECT) };
    let viewport = &m.os.ddraw.viewports[index];
    let Some(device) = viewport.device else { return Ok(D3DERR_VIEWPORTHASNODEVICE) };
    let Some(bounds) = viewport.area() else { return Ok(D3DERR_VIEWPORTDATANOTSET) };
    if flags & !(D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER) != 0 {
        return Err(Stop::Unimplemented {
            dll: "IDirect3DViewport3".into(),
            name: format!("Clear2 flags {flags:#x}"),
        });
    }
    let (target, depth_surface) = device::targets(m, device);
    for n in 0..count {
        // D3DRECT: x1, y1, x2, y2, clipped to the viewport.
        let [x1, y1, x2, y2]: [u32; 4] = m.read_words(rects + 16 * n);
        let (left, top) = (x1.max(bounds.x), y1.max(bounds.y));
        let (right, bottom) = (x2.min(bounds.x + bounds.width), y2.min(bounds.y + bounds.height));
        if right <= left || bottom <= top {
            continue;
        }
        let area = Rect { x: left, y: top, width: right - left, height: bottom - top };
        if flags & D3DCLEAR_TARGET != 0 {
            let format = m.surface_format(target);
            m.fill_surface(target, area, format.from_argb(color));
        }
        if let Some(depth_surface) = depth_surface.filter(|_| flags & D3DCLEAR_ZBUFFER != 0) {
            m.fill_surface(depth_surface, area, PixelFormat::depth16(depth));
        }
    }
    Ok(DD_OK)
}

interface! { static VIEWPORT_3 = "IDirect3DViewport3";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    Initialize(2),
    GetViewport(2),
    SetViewport(2),
    TransformVertices(5),
    LightElements(3),
    SetBackground(2),
    GetBackground(3),
    SetBackgroundDepth(2),
    GetBackgroundDepth(3),
    Clear(4),
    AddLight(2) => |m, a| match m.viewport_index(a[0]) {
        Some(index) => light::add_to_viewport(m, index, a[1]),
        None => DDERR_INVALIDOBJECT,
    },
    DeleteLight(2) => |m, a| match m.viewport_index(a[0]) {
        Some(index) => light::remove_from_viewport(m, index, a[1]),
        None => DDERR_INVALIDOBJECT,
    },
    NextLight(4),
    GetViewport2(2) => get_viewport2,
    SetViewport2(2) => set_viewport2,
    SetBackgroundDepth2(2),
    GetBackgroundDepth2(3),
    Clear2(7) => clear2,
}
