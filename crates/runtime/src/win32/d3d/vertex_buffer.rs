//! Vertex buffers (`IDirect3DVertexBuffer`): arrays of vertices kept by
//! Direct3D, which the program fills through a lock.

use crate::Machine;
use crate::win32::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::win32::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS};

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
    ProcessVertices(7),
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
