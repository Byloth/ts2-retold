//! Vertex buffers (`IDirect3DVertexBuffer`): arrays of vertices kept by
//! Direct3D, which the program fills through a lock.

use super::device::{self, PROJECTION, VIEW, WORLD};
use super::{draw, lighting, viewport};
use crate::video::Rect;
use crate::win32::HostResult;
use crate::win32::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::win32::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS};
use crate::{Machine, Stop};

const IID_IDIRECT3DVERTEXBUFFER: Guid =
    guid(0x7A503555, 0x4A83, 0x11D1, [0xA5, 0xDB, 0x00, 0xA0, 0xC9, 0x03, 0x67, 0xF8]);
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
const D3DERR_VERTEXBUFFERLOCKED: u32 = ddraw::dd_error(2062);
const D3DERR_VERTEXBUFFERUNLOCKFAILED: u32 = ddraw::dd_error(2063);
const D3DMAXNUMVERTICES: u32 = 0xFFFF;

// Flexible vertex format bits.
const D3DFVF_POSITION_MASK: u32 = 0x00E;
const D3DFVF_XYZ: u32 = 0x002;
pub const D3DFVF_XYZRHW: u32 = 0x004;
const D3DFVF_NORMAL: u32 = 0x010;
const D3DFVF_RESERVED1: u32 = 0x020;
const D3DFVF_DIFFUSE: u32 = 0x040;
const D3DFVF_SPECULAR: u32 = 0x080;
const D3DFVF_TEXCOUNT_SHIFT: u32 = 8;
/// A transformed and lit vertex with one set of texture coordinates:
/// `D3DTLVERTEX`, 32 bytes.
pub const D3DFVF_TLVERTEX: u32 =
    D3DFVF_XYZRHW | D3DFVF_DIFFUSE | D3DFVF_SPECULAR | 1 << D3DFVF_TEXCOUNT_SHIFT;

/// Bytes per vertex of a flexible vertex format; `None` if it is not one
/// this runtime knows (blend weights, more than eight texture sets).
pub fn vertex_size(format: u32) -> Option<u32> {
    let position = match format & D3DFVF_POSITION_MASK {
        D3DFVF_XYZ => 12,
        D3DFVF_XYZRHW => 16,
        _ => return None,
    };
    let textures = (format >> D3DFVF_TEXCOUNT_SHIFT) & 0xF;
    let known =
        D3DFVF_POSITION_MASK | D3DFVF_NORMAL | D3DFVF_RESERVED1 | D3DFVF_DIFFUSE | D3DFVF_SPECULAR | 0xF00;
    if format & !known != 0 || textures > 8 {
        return None;
    }
    let flag = |bit: u32, bytes: u32| if format & bit != 0 { bytes } else { 0 };
    Some(
        position
            + flag(D3DFVF_NORMAL, 12)
            + flag(D3DFVF_RESERVED1, 4)
            + flag(D3DFVF_DIFFUSE, 4)
            + flag(D3DFVF_SPECULAR, 4)
            + 8 * textures,
    )
}

#[derive(Debug)]
pub struct VertexBuffer {
    references: u32,
    pointer: u32,
    caps: u32,
    pub format: u32,
    pub count: u32,
    /// Guest address of the vertices.
    pub memory: u32,
    locked: bool,
    /// For each vertex `ProcessVertices` wrote here: the four numbers it
    /// wrote, and the position before the division by `w`, as the video
    /// backend takes it. Direct3D kept the like beside its buffers, so that
    /// triangles reaching behind the viewer could still be clipped when
    /// drawn. A vertex the program has since changed no longer matches the
    /// numbers and is drawn from what the buffer holds.
    pub(super) remembered: Vec<Option<([u32; 4], [f32; 4])>>,
    /// The viewport rectangle the remembered positions are for.
    pub(super) remembered_viewport: Option<Rect>,
}

impl Machine {
    pub(super) fn vertex_buffer_index(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let buffer = self.os.ddraw.vertex_buffers.get(index)?;
        (buffer.references > 0 && buffer.pointer == this).then_some(index)
    }
}

/// `IDirect3D3::CreateVertexBuffer`.
pub(super) fn create_vertex_buffer(m: &mut Machine, a: &[u32]) -> u32 {
    let (description, out, outer) = (a[1], a[2], a[4]);
    if outer != 0 {
        return CLASS_E_NOAGGREGATION;
    }
    if !ddraw::is_live_object(m, a[0]) {
        return DDERR_INVALIDOBJECT;
    }
    // D3DVERTEXBUFFERDESC: size, capabilities, vertex format, vertex count.
    let [size, caps, format, count]: [u32; 4] = m.read_words(description);
    let Some(vertex_size) =
        vertex_size(format).filter(|_| size == 16 && (1..=D3DMAXNUMVERTICES).contains(&count))
    else {
        return DDERR_INVALIDPARAMS;
    };
    m.note(|| format!("{count} vertices of format {format:#x} ({vertex_size} bytes each)"));
    let memory = m.os.allocate(&mut m.memory, vertex_size * count);
    let index = m.os.ddraw.vertex_buffers.len();
    let pointer = m.interface_pointer(&VERTEX_BUFFER, index as u32);
    m.os.ddraw.vertex_buffers.push(VertexBuffer {
        references: 1,
        pointer,
        caps,
        format,
        count,
        memory,
        locked: false,
        remembered: vec![None; count as usize],
        remembered_viewport: None,
    });
    if m.memory.write(out, 4, pointer).is_some() { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return DDERR_INVALIDPARAMS };
    let Some(index) = m.vertex_buffer_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECT3DVERTEXBUFFER {
        m.note(|| format!("interface {} not offered", ddraw::format_guid(&wanted)));
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.ddraw.vertex_buffers[index].references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DDERR_INVALIDPARAMS }
}

fn add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.vertex_buffer_index(a[0]) else { return 0 };
    let buffer = &mut m.os.ddraw.vertex_buffers[index];
    buffer.references += 1;
    buffer.references
}

fn release(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.vertex_buffer_index(a[0]) else { return 0 };
    let buffer = &mut m.os.ddraw.vertex_buffers[index];
    buffer.references -= 1;
    buffer.references
}

fn lock(m: &mut Machine, a: &[u32]) -> u32 {
    let (data_out, size_out) = (a[2], a[3]);
    let Some(index) = m.vertex_buffer_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let buffer = &mut m.os.ddraw.vertex_buffers[index];
    if std::mem::replace(&mut buffer.locked, true) {
        return D3DERR_VERTEXBUFFERLOCKED;
    }
    let (memory, bytes) =
        (buffer.memory, buffer.count * vertex_size(buffer.format).expect("checked at creation"));
    let mut ok = m.memory.write(data_out, 4, memory).is_some();
    if size_out != 0 {
        ok &= m.memory.write(size_out, 4, bytes).is_some();
    }
    if ok { DD_OK } else { DDERR_INVALIDPARAMS }
}

fn unlock(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.vertex_buffer_index(a[0]) else { return DDERR_INVALIDOBJECT };
    if std::mem::replace(&mut m.os.ddraw.vertex_buffers[index].locked, false) {
        DD_OK
    } else {
        D3DERR_VERTEXBUFFERUNLOCKFAILED
    }
}

/// `ProcessVertices`: fills part of this buffer with the transformed
/// vertices of another, as Direct3D transforms them for drawing.
///
/// A position is multiplied by the world, view and projection matrices of
/// the device; the result is divided by its `w` and mapped from the
/// viewport's clip volume to the viewport's rectangle; the fourth number
/// written is `1 / w`. Colours and texture coordinates are copied. The
/// arithmetic is in single precision in the order written here, and
/// follows `process_vertices_strided` of Wine's Direct3D.
///
/// Nothing is clipped: a vertex behind the viewer gets the coordinates the
/// formulas give it, and one with no `w` to divide by keeps its `w` of one.
fn process_vertices(m: &mut Machine, a: &[u32]) -> HostResult {
    const D3DVOP_TRANSFORM: u32 = 1;
    // Asks for vertices outside the view to be marked for clipping. The
    // marks are the remembered positions, which are kept in any case; what
    // Direct3D wrote into the buffer for such a vertex is UNVERIFIED and
    // is here what it writes for any other.
    const D3DVOP_CLIP: u32 = 4;
    const D3DVOP_LIGHT: u32 = 1 << 10;
    let (operation, first, count, source, source_first, device_pointer, flags) =
        (a[1], a[2], a[3], a[4], a[5], a[6], a[7]);
    let unsupported = |what: String| {
        Err(Stop::Unimplemented {
            dll: "IDirect3DVertexBuffer".into(),
            name: format!("ProcessVertices {what}"),
        })
    };
    let (Some(target), Some(source)) = (m.vertex_buffer_index(a[0]), m.vertex_buffer_index(source)) else {
        return Ok(DDERR_INVALIDOBJECT);
    };
    let Some(device) = m.device_index(device_pointer) else { return Ok(DDERR_INVALIDOBJECT) };
    if operation & !(D3DVOP_CLIP | D3DVOP_LIGHT) != D3DVOP_TRANSFORM || flags != 0 {
        return unsupported(format!("with operation {operation:#x} and flags {flags:#x}"));
    }
    let buffers = &m.os.ddraw.vertex_buffers;
    let (to, from) = (&buffers[target], &buffers[source]);
    let in_range =
        |buffer: &VertexBuffer, start: u32| start.checked_add(count).is_some_and(|end| end <= buffer.count);
    if !in_range(to, first) || !in_range(from, source_first) {
        return Ok(DDERR_INVALIDPARAMS);
    }
    let (Some(out), Some(input)) = (draw::layout(to.format), draw::layout(from.format)) else {
        return Ok(DDERR_INVALIDPARAMS);
    };
    if to.format & 0xE != D3DFVF_XYZRHW || from.format & 0xE != D3DFVF_XYZ {
        return unsupported(format!("from format {:#x} to format {:#x}", from.format, to.format));
    }
    let (to_address, from_address) = (to.memory + first * out.size, from.memory + source_first * input.size);

    let state = &m.os.ddraw.devices[device];
    let Some(current) = state.current_viewport else {
        return unsupported("without a current viewport".into());
    };
    let (Some(area), Some([clip_x, clip_y, clip_width, clip_height, min_z, max_z])) =
        (viewport::area(m, current), viewport::clip_volume(m, current))
    else {
        return unsupported("with a viewport without dimensions".into());
    };
    let transforms = &state.transforms;
    let matrix =
        device::multiply(&device::multiply(&transforms[WORLD], &transforms[VIEW]), &transforms[PROJECTION]);

    // Vertices are lit when asked, if they have normals and a material is
    // selected; otherwise their colours are copied.
    let lighting = match input.normal {
        Some(_) if operation & D3DVOP_LIGHT != 0 => lighting::prepare(m, device)?,
        _ => None,
    };
    // Vertex fog: with a fog mode set in the light states, the alpha of the
    // specular colour becomes the fog factor, from the depth in camera
    // space (Wine's `update_fog_factor`).
    let state = &m.os.ddraw.devices[device];
    let fog = match state.light_states[4] {
        0 => None,
        // Linear, between a start and an end.
        3 if state.render_states[draw::render_state::RANGEFOGENABLE] == 0 => Some((
            device::multiply(&state.transforms[WORLD], &state.transforms[VIEW]),
            f32::from_bits(state.render_states[draw::render_state::FOGSTART]),
            f32::from_bits(state.render_states[draw::render_state::FOGEND]),
        )),
        mode => return unsupported(format!("with fog mode {mode} or range fog")),
    };
    let Some(vertices) = m.memory.dump(from_address, count * input.size) else {
        return Ok(DDERR_INVALIDPARAMS);
    };
    let mut result = vec![0u8; (count * out.size) as usize];
    let mut remembered = Vec::with_capacity(count as usize);
    // Half a pixel, in the backend's clip space: a screen coordinate names
    // a pixel's centre, the backend's viewport maps to pixel edges.
    let (half_x, half_y) = (1.0 / area.width as f32, 1.0 / area.height as f32);
    let float =
        |bytes: &[u8], at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    for (vertex, written) in
        vertices.chunks_exact(input.size as usize).zip(result.chunks_exact_mut(out.size as usize))
    {
        let p = [float(vertex, 0), float(vertex, 4), float(vertex, 8)];
        let along = |column: usize| {
            p[0] * matrix[column]
                + p[1] * matrix[4 + column]
                + p[2] * matrix[8 + column]
                + matrix[12 + column]
        };
        let (mut x, mut y, mut z, w) = (along(0), along(1), along(2), along(3));
        let clip = [
            (x - clip_x * w) * 2.0 / clip_width - w + half_x * w,
            w - (clip_y * w - y) * 2.0 / clip_height - half_y * w,
            (z - min_z * w) / (max_z - min_z),
            w,
        ];
        let rhw = if w != 0.0 { 1.0 / w } else { 1.0 };
        x = (x * rhw - clip_x) / clip_width * area.width as f32 + area.x as f32;
        y = (clip_y - y * rhw) / clip_height * area.height as f32 + area.y as f32;
        z = (z * rhw - min_z) / (max_z - min_z);
        for (n, value) in [x, y, z, rhw].into_iter().enumerate() {
            written[4 * n..4 * n + 4].copy_from_slice(&value.to_le_bytes());
        }
        remembered.push(Some(([x, y, z, rhw].map(f32::to_bits), clip)));
        // A colour the source does not have is white for the diffuse one
        // and black for the specular one.
        let word = |at: usize| u32::from_le_bytes(vertex[at..at + 4].try_into().expect("four bytes"));
        let (diffuse, specular) = match (&lighting, input.normal) {
            (Some(lighting), Some(normal)) => {
                let normal = [float(vertex, normal), float(vertex, normal + 4), float(vertex, normal + 8)];
                lighting.light(p, normal, input.diffuse.map(word))
            }
            _ => (input.diffuse.map_or(0xFFFF_FFFF, word), input.specular.map_or(0, word)),
        };
        if let Some(at) = out.diffuse {
            written[at..at + 4].copy_from_slice(&diffuse.to_le_bytes());
        }
        let specular = match &fog {
            Some((to_camera, start, end)) => {
                let along = |column: usize| {
                    p[0] * to_camera[column]
                        + p[1] * to_camera[4 + column]
                        + p[2] * to_camera[8 + column]
                        + to_camera[12 + column]
                };
                let depth = (along(2) / along(3)).abs();
                let factor = ((end - depth) / (end - start)).clamp(0.0, 1.0);
                specular & 0x00FF_FFFF | ((factor * 255.0 + 0.5) as u32) << 24
            }
            None => specular,
        };
        if let Some(at) = out.specular {
            written[at..at + 4].copy_from_slice(&specular.to_le_bytes());
        }
        if let (Some(at), Some(from)) = (out.uv, input.uv) {
            written[at..at + 8].copy_from_slice(&vertex[from..from + 8]);
        }
    }
    let buffer = &mut m.os.ddraw.vertex_buffers[target];
    if buffer.remembered_viewport != Some(area) {
        buffer.remembered.fill(None);
        buffer.remembered_viewport = Some(area);
    }
    buffer.remembered[first as usize..][..count as usize].copy_from_slice(&remembered);
    Ok(if m.memory.load(to_address, &result).is_some() { DD_OK } else { DDERR_INVALIDPARAMS })
}

fn get_description(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.vertex_buffer_index(a[0]) else { return DDERR_INVALIDOBJECT };
    let buffer = &m.os.ddraw.vertex_buffers[index];
    let words = [16, buffer.caps, buffer.format, buffer.count];
    if m.write_words(a[1], &words) { DD_OK } else { DDERR_INVALIDPARAMS }
}

interface! { static VERTEX_BUFFER = "IDirect3DVertexBuffer";
    QueryInterface(3) => query_interface,
    AddRef(1) => add_ref,
    Release(1) => release,
    Lock(4) => lock,
    Unlock(1) => unlock,
    ProcessVertices(8) => process_vertices,
    GetVertexBufferDesc(2) => get_description,
    Optimize(3),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_sizes_follow_the_format_bits() {
        assert_eq!(D3DFVF_TLVERTEX, 0x1C4);
        assert_eq!(vertex_size(D3DFVF_TLVERTEX), Some(32));
        assert_eq!(vertex_size(D3DFVF_XYZ | D3DFVF_NORMAL | 1 << 8), Some(32)); // D3DVERTEX
        assert_eq!(
            vertex_size(D3DFVF_XYZ | D3DFVF_RESERVED1 | D3DFVF_DIFFUSE | D3DFVF_SPECULAR | 1 << 8),
            Some(32)
        );
        assert_eq!(vertex_size(D3DFVF_XYZRHW | D3DFVF_DIFFUSE), Some(20));
        assert_eq!(vertex_size(0x006), None); // blend weights
        assert_eq!(vertex_size(D3DFVF_DIFFUSE), None); // no position
    }
}
