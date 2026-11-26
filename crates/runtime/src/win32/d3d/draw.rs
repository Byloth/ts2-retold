//! Drawing: `DrawPrimitive` and `DrawIndexedPrimitive`.
//!
//! The program's vertices and the device's state are turned into what the
//! video backend takes (ADR 0014): a list of triangles in clip space and a
//! [`DrawState`]. A state or a value this cannot express stops the machine
//! and names it; nothing is approximated.

use super::device::{Device, STAGE_STATES};
use super::vertex_buffer::{D3DFVF_XYZRHW, vertex_size};
use super::viewport;
use crate::video::{
    Address, Argument, BlendFactor, Combine, Compare, Cull, DepthState, Draw, DrawState, Filter, Operation,
    Rect, Sampling, Source, Vertex,
};
use crate::win32::HostResult;
use crate::win32::ddraw::{self, DD_OK, DDERR_INVALIDOBJECT, DDERR_INVALIDPARAMS};
use crate::{Machine, Stop};

const D3DERR_INVALIDPRIMITIVETYPE: u32 = ddraw::dd_error(736);
const D3DERR_INVALIDVERTEXFORMAT: u32 = ddraw::dd_error(2048);
const D3DERR_TOOMANYVERTICES: u32 = ddraw::dd_error(2085);
const D3DMAXNUMVERTICES: u32 = 0xFFFF;

// Flexible vertex format bits.
const D3DFVF_POSITION_MASK: u32 = 0x00E;
const D3DFVF_NORMAL: u32 = 0x010;
const D3DFVF_RESERVED1: u32 = 0x020;
const D3DFVF_DIFFUSE: u32 = 0x040;
const D3DFVF_SPECULAR: u32 = 0x080;
const D3DFVF_TEXCOUNT_MASK: u32 = 0xF00;

// `D3DDP_*`: wait for the draw, vertices need no clipping, extents need no
// update, vertices need no lighting. None of them changes what is drawn
// from transformed vertices.
const KNOWN_FLAGS: u32 = 0x01 | 0x04 | 0x08 | 0x10;
const D3DDP_DONOTLIGHT: u32 = 0x10;

/// `D3DRENDERSTATE_*` numbers.
pub(super) mod render_state {
    pub const TEXTUREHANDLE: usize = 1;
    pub const ANTIALIAS: usize = 2;
    pub const TEXTUREADDRESS: usize = 3;
    pub const WRAPU: usize = 5;
    pub const WRAPV: usize = 6;
    pub const ZENABLE: usize = 7;
    pub const FILLMODE: usize = 8;
    pub const SHADEMODE: usize = 9;
    pub const ZWRITEENABLE: usize = 14;
    pub const ALPHATESTENABLE: usize = 15;
    pub const TEXTUREMAG: usize = 17;
    pub const TEXTUREMIN: usize = 18;
    pub const SRCBLEND: usize = 19;
    pub const DESTBLEND: usize = 20;
    pub const TEXTUREMAPBLEND: usize = 21;
    pub const CULLMODE: usize = 22;
    pub const ZFUNC: usize = 23;
    pub const ALPHAREF: usize = 24;
    pub const ALPHAFUNC: usize = 25;
    pub const ALPHABLENDENABLE: usize = 27;
    pub const FOGENABLE: usize = 28;
    pub const SPECULARENABLE: usize = 29;
    pub const STIPPLEDALPHA: usize = 33;
    pub const FOGCOLOR: usize = 34;
    pub const FOGTABLEMODE: usize = 35;
    pub const FOGSTART: usize = 36;
    pub const FOGEND: usize = 37;
    pub const FOGDENSITY: usize = 38;
    pub const RANGEFOGENABLE: usize = 48;
    pub const STIPPLEENABLE: usize = 39;
    pub const EDGEANTIALIAS: usize = 40;
    pub const COLORKEYENABLE: usize = 41;
    pub const TEXTUREADDRESSU: usize = 44;
    pub const TEXTUREADDRESSV: usize = 45;
    pub const ZBIAS: usize = 47;
    pub const STENCILENABLE: usize = 52;
    pub const WRAP0: usize = 128;
}

/// `D3DTSS_*` numbers.
pub(super) mod stage_state {
    pub const COLOROP: usize = 1;
    pub const COLORARG1: usize = 2;
    pub const COLORARG2: usize = 3;
    pub const ALPHAOP: usize = 4;
    pub const ALPHAARG1: usize = 5;
    pub const ALPHAARG2: usize = 6;
    pub const TEXCOORDINDEX: usize = 11;
    pub const ADDRESS: usize = 12;
    pub const ADDRESSU: usize = 13;
    pub const ADDRESSV: usize = 14;
    pub const MAGFILTER: usize = 16;
    pub const MINFILTER: usize = 17;
    pub const MIPFILTER: usize = 18;
}

/// `D3DTOP_*` values.
pub(super) mod operation {
    pub const DISABLE: u32 = 1;
    pub const SELECTARG1: u32 = 2;
    pub const SELECTARG2: u32 = 3;
    pub const MODULATE: u32 = 4;
    pub const MODULATE2X: u32 = 5;
    pub const MODULATE4X: u32 = 6;
    pub const ADD: u32 = 7;
    pub const ADDSIGNED: u32 = 8;
    pub const SUBTRACT: u32 = 10;
    pub const BLENDDIFFUSEALPHA: u32 = 12;
    pub const BLENDTEXTUREALPHA: u32 = 13;
    pub const BLENDCURRENTALPHA: u32 = 16;
}

/// `D3DTA_*` values.
pub(super) mod argument {
    pub const SELECT_MASK: u32 = 0x0F;
    pub const DIFFUSE: u32 = 0;
    pub const CURRENT: u32 = 1;
    pub const TEXTURE: u32 = 2;
    pub const SPECULAR: u32 = 4;
    pub const COMPLEMENT: u32 = 0x10;
    pub const ALPHAREPLICATE: u32 = 0x20;
}

fn unsupported<T>(what: String) -> Result<T, Stop> {
    Err(Stop::Unimplemented { dll: "IDirect3DDevice3".into(), name: format!("drawing with {what}") })
}

fn compare(value: u32, state: &str) -> Result<Compare, Stop> {
    Ok(match value {
        1 => Compare::Never,
        2 => Compare::Less,
        3 => Compare::Equal,
        4 => Compare::LessEqual,
        5 => Compare::Greater,
        6 => Compare::NotEqual,
        7 => Compare::GreaterEqual,
        8 => Compare::Always,
        other => return unsupported(format!("{state} {other}")),
    })
}

fn blend(source: u32, target: u32) -> Result<(BlendFactor, BlendFactor), Stop> {
    let factor = |value: u32, state: &str| {
        Ok(match value {
            1 => BlendFactor::Zero,
            2 => BlendFactor::One,
            3 => BlendFactor::SourceColor,
            4 => BlendFactor::InverseSourceColor,
            5 => BlendFactor::SourceAlpha,
            6 => BlendFactor::InverseSourceAlpha,
            7 => BlendFactor::TargetAlpha,
            8 => BlendFactor::InverseTargetAlpha,
            9 => BlendFactor::TargetColor,
            10 => BlendFactor::InverseTargetColor,
            11 => BlendFactor::SourceAlphaSaturated,
            other => return unsupported(format!("{state} {other}")),
        })
    };
    // Two source values set both factors at once.
    match source {
        12 => Ok((BlendFactor::SourceAlpha, BlendFactor::InverseSourceAlpha)),
        13 => Ok((BlendFactor::InverseSourceAlpha, BlendFactor::SourceAlpha)),
        _ => Ok((factor(source, "source blend")?, factor(target, "destination blend")?)),
    }
}

fn combiner_argument(value: u32) -> Result<Argument, Stop> {
    let source = match value & argument::SELECT_MASK {
        argument::DIFFUSE => Source::Diffuse,
        argument::CURRENT => Source::Current,
        argument::TEXTURE => Source::Texture,
        argument::SPECULAR => Source::Specular,
        other => return unsupported(format!("texture argument {other}")),
    };
    if value & !(argument::SELECT_MASK | argument::COMPLEMENT | argument::ALPHAREPLICATE) != 0 {
        return unsupported(format!("texture argument {value:#x}"));
    }
    Ok(Argument {
        source,
        complement: value & argument::COMPLEMENT != 0,
        alpha_replicate: value & argument::ALPHAREPLICATE != 0,
    })
}

/// The combiner of one channel of stage 0. A disabled stage passes the
/// diffuse colour through.
fn combine(stage: &[u32; STAGE_STATES], op: usize, first: usize, second: usize) -> Result<Combine, Stop> {
    let operation = match stage[op] {
        operation::DISABLE => {
            let current = Argument { source: Source::Current, complement: false, alpha_replicate: false };
            return Ok(Combine { operation: Operation::SelectSecond, first: current, second: current });
        }
        operation::SELECTARG1 => Operation::SelectFirst,
        operation::SELECTARG2 => Operation::SelectSecond,
        operation::MODULATE => Operation::Modulate,
        operation::MODULATE2X => Operation::Modulate2x,
        operation::MODULATE4X => Operation::Modulate4x,
        operation::ADD => Operation::Add,
        operation::ADDSIGNED => Operation::AddSigned,
        operation::SUBTRACT => Operation::Subtract,
        operation::BLENDDIFFUSEALPHA => Operation::BlendDiffuseAlpha,
        operation::BLENDTEXTUREALPHA => Operation::BlendTextureAlpha,
        operation::BLENDCURRENTALPHA => Operation::BlendCurrentAlpha,
        other => return unsupported(format!("texture operation {other}")),
    };
    Ok(Combine {
        operation,
        first: combiner_argument(stage[first])?,
        second: combiner_argument(stage[second])?,
    })
}

fn address(value: u32) -> Result<Address, Stop> {
    Ok(match value {
        1 => Address::Wrap,
        2 => Address::Mirror,
        3 => Address::Clamp,
        other => return unsupported(format!("texture addressing {other}")),
    })
}

fn filter(value: u32, state: &str) -> Result<Filter, Stop> {
    Ok(match value {
        1 => Filter::Nearest,
        2 => Filter::Linear,
        other => return unsupported(format!("{state} {other}")),
    })
}

/// The state of a device as the rasteriser needs it, without its texture.
fn draw_state(device: &Device) -> Result<DrawState, Stop> {
    use render_state::*;
    let states = &device.render_states;
    for (state, name, allowed) in [
        (ANTIALIAS, "antialiasing", &[0u32, 1][..]), // none
        (FILLMODE, "fill mode", &[3]),               // solid
        // Fog by the fog factor in the vertices is drawn; fog computed per
        // pixel from a table is not.
        (FOGTABLEMODE, "table fog", &[0]),
        (STIPPLEDALPHA, "stippled alpha", &[0]),
        (STIPPLEENABLE, "stippling", &[0]),
        (EDGEANTIALIAS, "edge antialiasing", &[0]),
        (STENCILENABLE, "stencil", &[0]),
        (WRAPU, "texture wrapping", &[0]),
        (WRAPV, "texture wrapping", &[0]),
        (WRAP0, "texture wrapping", &[0]),
        (ZENABLE, "depth mode", &[0, 1]),
        (SHADEMODE, "shade mode", &[1, 2]), // flat, Gouraud
    ] {
        if !allowed.contains(&states[state]) {
            return unsupported(format!("{name} {}", states[state]));
        }
    }
    let depth = match (device.depth, states[ZENABLE]) {
        (Some(_), 1) => Some(DepthState {
            compare: compare(states[ZFUNC], "depth function")?,
            write: states[ZWRITEENABLE] != 0,
            // `ZBIAS` runs from 0 to 16. The adapter moves a triangle one
            // step of its 16-bit depth per unit (what cards did with it
            // varied; UNVERIFIED).
            bias: match states[ZBIAS] {
                bias @ 0..=16 => bias as u8,
                other => return unsupported(format!("a depth bias of {other}")),
            },
        }),
        _ => None,
    };
    let blend = match states[ALPHABLENDENABLE] {
        0 => None,
        _ => Some(blend(states[SRCBLEND], states[DESTBLEND])?),
    };
    let alpha_test = match states[ALPHATESTENABLE] {
        0 => None,
        _ => Some((compare(states[ALPHAFUNC], "alpha function")?, states[ALPHAREF] as u8)),
    };
    let cull = match states[CULLMODE] {
        1 => Cull::None,
        2 => Cull::Clockwise,
        3 => Cull::CounterClockwise,
        other => return unsupported(format!("cull mode {other}")),
    };
    // One texture stage: the second must be off.
    if device.stage_states[1][stage_state::COLOROP] != operation::DISABLE {
        return unsupported("a second texture stage".into());
    }
    let stage = &device.stage_states[0];
    if stage[stage_state::TEXCOORDINDEX] != 0 {
        return unsupported(format!("texture coordinate set {}", stage[stage_state::TEXCOORDINDEX]));
    }
    Ok(DrawState {
        depth,
        blend,
        alpha_test,
        cull,
        flat: states[SHADEMODE] == 1,
        specular: states[SPECULARENABLE] != 0,
        fog: (states[FOGENABLE] != 0).then_some(states[FOGCOLOR] & 0x00FF_FFFF),
        color: combine(stage, stage_state::COLOROP, stage_state::COLORARG1, stage_state::COLORARG2)?,
        alpha: combine(stage, stage_state::ALPHAOP, stage_state::ALPHAARG1, stage_state::ALPHAARG2)?,
        texture: None,
    })
}

/// Where the parts of a vertex are, from its flexible vertex format.
#[derive(Debug, Clone, Copy)]
pub(super) struct Layout {
    pub size: u32,
    pub normal: Option<usize>,
    pub diffuse: Option<usize>,
    pub specular: Option<usize>,
    pub uv: Option<usize>,
}

pub(super) fn layout(format: u32) -> Option<Layout> {
    let size = vertex_size(format)?;
    let mut offset = if format & D3DFVF_POSITION_MASK == D3DFVF_XYZRHW { 16 } else { 12 };
    let mut take = |present: bool, bytes: usize| {
        let at = present.then_some(offset);
        offset += if present { bytes } else { 0 };
        at
    };
    let normal = take(format & D3DFVF_NORMAL != 0, 12);
    take(format & D3DFVF_RESERVED1 != 0, 4);
    let diffuse = take(format & D3DFVF_DIFFUSE != 0, 4);
    let specular = take(format & D3DFVF_SPECULAR != 0, 4);
    let uv = take(format & D3DFVF_TEXCOUNT_MASK != 0, 8);
    Some(Layout { size, normal, diffuse, specular, uv })
}

/// Vertices the program transformed itself, as clip-space vertices.
///
/// A screen position (x, y) names the centre of a pixel, and the viewport
/// maps the clip-space square onto pixel edges, hence the half pixel. `rhw`
/// is one over the clip-space w, which corrects textures and colours for
/// perspective. The adapter always corrects, as the cards of the time did:
/// `TEXTUREPERSPECTIVE` is not looked at (ADR 0014).
///
/// A negative `rhw` is a vertex behind the viewer: Direct3D writes it so
/// when it transforms vertices, and multiplying back by the negative w
/// gives the position the vertex had before the division, which the
/// backend clips like any other. An `rhw` of zero, or not a number, is
/// taken as one (UNVERIFIED on hardware).
fn transformed_vertices(bytes: &[u8], layout: &Layout, viewport: Rect) -> Vec<Vertex> {
    let float =
        |vertex: &[u8], at: usize| f32::from_le_bytes(vertex[at..at + 4].try_into().expect("four bytes"));
    let word =
        |vertex: &[u8], at: usize| u32::from_le_bytes(vertex[at..at + 4].try_into().expect("four bytes"));
    bytes
        .chunks_exact(layout.size as usize)
        .map(|vertex| {
            let (x, y, z, rhw) = (float(vertex, 0), float(vertex, 4), float(vertex, 8), float(vertex, 12));
            let w = if rhw.is_finite() && rhw != 0.0 { 1.0 / rhw } else { 1.0 };
            let across = (x + 0.5 - viewport.x as f32) / viewport.width as f32 * 2.0 - 1.0;
            let up = 1.0 - (y + 0.5 - viewport.y as f32) / viewport.height as f32 * 2.0;
            Vertex {
                position: [across * w, up * w, z * w, w],
                diffuse: layout.diffuse.map_or(0xFFFF_FFFF, |at| word(vertex, at)),
                specular: layout.specular.map_or(0, |at| word(vertex, at)),
                uv: layout.uv.map_or([0.0; 2], |at| [float(vertex, at), float(vertex, at + 4)]),
            }
        })
        .collect()
}

/// The ends of the segments of a primitive of lines, two indices each;
/// none for other primitives.
fn segments(primitive: u32, indices: &[u16]) -> Option<Vec<u16>> {
    const LINELIST: u32 = 2;
    const LINESTRIP: u32 = 3;
    match primitive {
        LINELIST => Some(indices[..indices.len() - indices.len() % 2].to_vec()),
        LINESTRIP => Some(indices.windows(2).flatten().copied().collect()),
        _ => None,
    }
}

/// Segments as triangles: each becomes a rectangle one pixel of the
/// viewport wide, lying along it, with the colours and texture
/// coordinates of its two ends. The video backend draws triangles only
/// (ADR 0014), and a rectangle of the picture's own pixels stays one
/// pixel wide at whatever grain the backend draws.
///
/// A segment that goes behind the viewer is cut where it does; one wholly
/// behind is left out. Which pixels Direct3D's own rule for lines lights
/// at a line's ends and along a steep or shallow slope is not modelled:
/// the rectangle covers the pixels whose centres are within half a pixel
/// of the line.
fn line_quads(vertices: &[Vertex], ends: &[u16], viewport: Rect) -> (Vec<Vertex>, Vec<u16>) {
    /// The nearest a point of a line is kept: `w` this small is at the eye.
    const NEAREST: f32 = 1e-6;
    let mix = |a: &Vertex, b: &Vertex, t: f32| {
        let channel = |a: u32, b: u32| {
            let part = |shift: u32| {
                let (a, b) = ((a >> shift & 0xFF) as f32, (b >> shift & 0xFF) as f32);
                ((a + (b - a) * t + 0.5) as u32) << shift
            };
            part(0) | part(8) | part(16) | part(24)
        };
        Vertex {
            position: std::array::from_fn(|n| a.position[n] + (b.position[n] - a.position[n]) * t),
            diffuse: channel(a.diffuse, b.diffuse),
            specular: channel(a.specular, b.specular),
            uv: std::array::from_fn(|n| a.uv[n] + (b.uv[n] - a.uv[n]) * t),
        }
    };
    let (half_width, half_height) = (viewport.width as f32 / 2.0, viewport.height as f32 / 2.0);
    let (mut out, mut indices) = (Vec::new(), Vec::new());
    for ends in ends.chunks_exact(2) {
        let (mut a, mut b) = (vertices[ends[0] as usize], vertices[ends[1] as usize]);
        let (wa, wb) = (a.position[3], b.position[3]);
        if wa < NEAREST && wb < NEAREST {
            continue;
        }
        if wa < NEAREST {
            a = mix(&a, &b, (NEAREST - wa) / (wb - wa));
        } else if wb < NEAREST {
            b = mix(&b, &a, (NEAREST - wb) / (wa - wb));
        }
        // Where the two ends are in the viewport, in pixels.
        let on_screen = |v: &Vertex| {
            [v.position[0] / v.position[3] * half_width, v.position[1] / v.position[3] * half_height]
        };
        let (pa, pb) = (on_screen(&a), on_screen(&b));
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let length = (dx * dx + dy * dy).sqrt();
        // A segment with no length is a point: nothing of a line.
        if length == 0.0 || !length.is_finite() {
            continue;
        }
        // Half a pixel to either side, across the line.
        let across = [-dy / length * 0.5 / half_width, dx / length * 0.5 / half_height];
        let first = out.len() as u16;
        for (end, side) in [(a, 1.0), (a, -1.0), (b, 1.0), (b, -1.0)] {
            let w = end.position[3];
            let mut corner = end;
            corner.position[0] += across[0] * side * w;
            corner.position[1] += across[1] * side * w;
            out.push(corner);
        }
        indices.extend_from_slice(&[first, first + 1, first + 2, first + 2, first + 1, first + 3]);
    }
    (out, indices)
}

/// The triangles of a primitive as a triangle list. The order of the
/// vertices of each triangle keeps its winding and its first vertex, which
/// gives a flat-shaded triangle its colour.
fn triangles(primitive: u32, indices: &[u16]) -> Option<Vec<u16>> {
    const TRIANGLELIST: u32 = 4;
    const TRIANGLESTRIP: u32 = 5;
    const TRIANGLEFAN: u32 = 6;
    let count = indices.len();
    Some(match primitive {
        TRIANGLELIST => indices[..count - count % 3].to_vec(),
        TRIANGLESTRIP => (0..count.saturating_sub(2))
            .flat_map(|n| {
                let odd = n % 2;
                [indices[n], indices[n + 1 + odd], indices[n + 2 - odd]]
            })
            .collect(),
        TRIANGLEFAN => {
            (1..count.saturating_sub(1)).flat_map(|n| [indices[n], indices[n + 1], indices[0]]).collect()
        }
        _ => return None,
    })
}

/// What a draw call is given, whichever method it came through.
struct Primitive {
    /// `D3DPT_*`.
    kind: u32,
    /// Flexible vertex format.
    format: u32,
    vertices: u32,
    vertex_count: u32,
    /// `D3DDP_*`.
    flags: u32,
    /// The vertex buffer the vertices are in and the first one used, when
    /// they come from one.
    buffer: Option<(usize, u32)>,
}

fn draw(m: &mut Machine, this: u32, primitive: Primitive, indices: Option<Vec<u16>>) -> HostResult {
    let Primitive { kind: primitive, format, vertices, vertex_count, flags, buffer } = primitive;
    let Some(device_index) = m.device_index(this) else { return Ok(DDERR_INVALIDOBJECT) };
    if !(1..=6).contains(&primitive) {
        return Ok(D3DERR_INVALIDPRIMITIVETYPE);
    }
    let Some(layout) = layout(format) else { return Ok(D3DERR_INVALIDVERTEXFORMAT) };
    if vertex_count > D3DMAXNUMVERTICES {
        return Ok(D3DERR_TOOMANYVERTICES);
    }
    if flags & !KNOWN_FLAGS != 0 {
        return unsupported(format!("flags {flags:#x}"));
    }
    // Vertices given as they are in the world are transformed here, and
    // lit unless the call says not to, as `ProcessVertices` does for a
    // buffer.
    let untransformed = format & D3DFVF_POSITION_MASK != D3DFVF_XYZRHW;
    if untransformed && format & D3DFVF_POSITION_MASK != super::vertex_buffer::D3DFVF_XYZ {
        return unsupported(format!("vertices of format {format:#x} to transform"));
    }
    let indices = indices.unwrap_or_else(|| (0..vertex_count as u16).collect());
    if indices.iter().any(|&index| index as u32 >= vertex_count) {
        return Ok(DDERR_INVALIDPARAMS);
    }
    // Lines are drawn as the strips of triangles that cover them (see
    // `line_quads`): until then their indices are the segments' ends.
    let segments = segments(primitive, &indices);
    let indices = match (&segments, triangles(primitive, &indices)) {
        (Some(ends), _) => ends.clone(),
        (None, Some(indices)) => indices,
        (None, None) => return unsupported(format!("points (primitive type {primitive})")),
    };
    // Only the vertices the triangles use are read and prepared: a draw
    // from a vertex buffer names a few of the buffer's many.
    let (lowest, vertex_count) = match (indices.iter().min(), indices.iter().max()) {
        (Some(&lowest), Some(&highest)) => (lowest, (highest - lowest) as u32 + 1),
        _ => (0, 0),
    };
    let indices: Vec<u16> = indices.iter().map(|index| index - lowest).collect();
    let vertices = vertices + lowest as u32 * layout.size;
    let buffer = buffer.map(|(buffer, first)| (buffer, first + lowest as u32));
    let Some(bytes) = m.memory.dump(vertices, vertex_count * layout.size) else {
        return Ok(DDERR_INVALIDPARAMS);
    };

    let device = &m.os.ddraw.devices[device_index];
    let Some(viewport) = device.current_viewport else { return unsupported("no current viewport".into()) };
    let Some(viewport) = viewport::area(m, viewport) else {
        return unsupported("a viewport without dimensions".into());
    };
    let mut state = draw_state(device)?;
    let color_key = device.render_states[render_state::COLORKEYENABLE] != 0;
    let (target, depth, texture) = (device.target, device.depth, device.textures[0]);
    let stage = device.stage_states[0];

    if let Some(texture) = texture {
        let (magnify, minify) = (
            filter(stage[stage_state::MAGFILTER], "magnification filter")?,
            filter(stage[stage_state::MINFILTER], "minification filter")?,
        );
        let color_key = match m.surface_color_key(texture).filter(|_| color_key) {
            None => None,
            Some((low, high)) if low != high => {
                return unsupported(format!("a colour key range {low:#x}..{high:#x}"));
            }
            Some(_) if (magnify, minify) != (Filter::Nearest, Filter::Nearest) => {
                return unsupported("a colour key on a filtered texture".into());
            }
            Some((key, _)) => Some(key),
        };
        state.texture = Some(Sampling {
            surface: m.ensure_video(texture),
            magnify,
            minify,
            // Textures have one level, so the filter between levels has
            // nothing to choose from.
            address_u: address(stage[stage_state::ADDRESSU])?,
            address_v: address(stage[stage_state::ADDRESSV])?,
            color_key,
        });
    }
    let mut vertices = if untransformed {
        use super::vertex_buffer::{Missing, Pipeline};
        let pipeline = match Pipeline::prepare(m, device_index, &layout, flags & D3DDP_DONOTLIGHT == 0) {
            Ok(pipeline) => pipeline,
            Err(Missing::Stop(stop)) => return Err(stop),
            Err(Missing::Named(what)) => return unsupported(format!("vertices to transform {what}")),
        };
        let float =
            |vertex: &[u8], at: usize| f32::from_le_bytes(vertex[at..at + 4].try_into().expect("four bytes"));
        bytes
            .chunks_exact(layout.size as usize)
            .map(|vertex| {
                let processed = pipeline.vertex(vertex);
                Vertex {
                    position: processed.clip,
                    diffuse: processed.diffuse,
                    specular: processed.specular,
                    uv: layout.uv.map_or([0.0; 2], |at| [float(vertex, at), float(vertex, at + 4)]),
                }
            })
            .collect()
    } else {
        transformed_vertices(&bytes, &layout, viewport)
    };
    // Vertices Direct3D transformed into a buffer, and the program has not
    // touched since, are drawn from their positions before the division by
    // `w`, so that the backend can clip them (see `VertexBuffer`).
    if let Some((buffer, first)) = buffer {
        let buffer = &m.os.ddraw.vertex_buffers[buffer];
        if buffer.remembered_viewport == Some(viewport) {
            let remembered = &buffer.remembered[first as usize..][..vertices.len()];
            for ((vertex, remembered), written) in
                vertices.iter_mut().zip(remembered).zip(bytes.chunks_exact(layout.size as usize))
            {
                let current: [u32; 4] = std::array::from_fn(|n| {
                    u32::from_le_bytes(written[4 * n..4 * n + 4].try_into().expect("four bytes"))
                });
                if let Some((numbers, clip)) = remembered.filter(|(numbers, _)| *numbers == current) {
                    let _ = numbers;
                    vertex.position = clip;
                }
            }
        }
    }
    let (vertices, indices) = match segments {
        Some(_) => {
            // A line has no face to turn away.
            state.cull = Cull::None;
            line_quads(&vertices, &indices, viewport)
        }
        None => (vertices, indices),
    };
    m.note(|| format!("{} triangles, {state:?}", indices.len() / 3));
    if indices.is_empty() {
        return Ok(DD_OK);
    }
    let draw = Draw {
        target: m.draw_on_video(target),
        // The depth surface changes only if depth is written.
        depth: depth.map(|depth| match state.depth {
            Some(DepthState { write: true, .. }) => m.draw_on_video(depth),
            _ => m.ensure_video(depth),
        }),
        viewport,
        state,
        vertices: &vertices,
        indices: &indices,
    };
    m.video.draw(&draw);
    let statistics = &mut m.os.ddraw.statistics;
    let used = statistics.draw_states.entry(state).or_default();
    used.draws += 1;
    used.triangles += indices.len() as u64 / 3;
    Ok(DD_OK)
}

/// `DrawPrimitive`: primitive type, vertex format, vertices, their count, flags.
pub(super) fn draw_primitive(m: &mut Machine, a: &[u32]) -> HostResult {
    let primitive =
        Primitive { kind: a[1], format: a[2], vertices: a[3], vertex_count: a[4], flags: a[5], buffer: None };
    draw(m, a[0], primitive, None)
}

/// `DrawIndexedPrimitive`: as `DrawPrimitive`, with 16-bit indices and their
/// count before the flags.
pub(super) fn draw_indexed_primitive(m: &mut Machine, a: &[u32]) -> HostResult {
    let (indices, count) = (a[5], a[6]);
    if count > D3DMAXNUMVERTICES {
        return Ok(D3DERR_TOOMANYVERTICES);
    }
    let Some(bytes) = m.memory.dump(indices, 2 * count) else { return Ok(DDERR_INVALIDPARAMS) };
    let indices = bytes.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
    let primitive =
        Primitive { kind: a[1], format: a[2], vertices: a[3], vertex_count: a[4], flags: a[7], buffer: None };
    draw(m, a[0], primitive, Some(indices))
}

/// `DrawPrimitiveVB`: primitive type, vertex buffer, first vertex, count, flags.
pub(super) fn draw_primitive_vb(m: &mut Machine, a: &[u32]) -> HostResult {
    let Some(index) = m.vertex_buffer_index(a[2]) else { return Ok(DDERR_INVALIDOBJECT) };
    let buffer = &m.os.ddraw.vertex_buffers[index];
    let (first, count) = (a[3], a[4]);
    if first.checked_add(count).is_none_or(|end| end > buffer.count) {
        return Ok(DDERR_INVALIDPARAMS);
    }
    let Some(size) = vertex_size(buffer.format) else { return Ok(D3DERR_INVALIDVERTEXFORMAT) };
    let primitive = Primitive {
        kind: a[1],
        format: buffer.format,
        vertices: buffer.memory + first * size,
        vertex_count: count,
        flags: a[5],
        buffer: Some((index, first)),
    };
    draw(m, a[0], primitive, None)
}

/// `DrawIndexedPrimitiveVB`: primitive type, vertex buffer, 16-bit indices
/// into the whole of it, their count, flags.
pub(super) fn draw_indexed_primitive_vb(m: &mut Machine, a: &[u32]) -> HostResult {
    let (indices, count) = (a[3], a[4]);
    let Some(index) = m.vertex_buffer_index(a[2]) else { return Ok(DDERR_INVALIDOBJECT) };
    if count > D3DMAXNUMVERTICES {
        return Ok(D3DERR_TOOMANYVERTICES);
    }
    let Some(bytes) = m.memory.dump(indices, 2 * count) else { return Ok(DDERR_INVALIDPARAMS) };
    let indices = bytes.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
    let buffer = &m.os.ddraw.vertex_buffers[index];
    let primitive = Primitive {
        kind: a[1],
        format: buffer.format,
        vertices: buffer.memory,
        vertex_count: buffer.count,
        flags: a[5],
        buffer: Some((index, 0)),
    };
    draw(m, a[0], primitive, Some(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_become_rectangles_a_pixel_wide() {
        assert_eq!(segments(3, &[4, 5, 6]), Some(vec![4, 5, 5, 6]));
        assert_eq!(segments(2, &[4, 5, 6]), Some(vec![4, 5]));
        assert_eq!(segments(4, &[4, 5, 6]), None);
        let vertex = |x: f32, y: f32, w: f32, diffuse: u32| Vertex {
            position: [x * w, y * w, 0.5 * w, w],
            diffuse,
            specular: 0,
            uv: [0.0; 2],
        };
        // A level line across a viewport of 200 by 100, seen from twice
        // as far at its right end: half a pixel up and down at each end.
        let ends = [vertex(-0.5, 0.0, 1.0, 0xFF00_0000), vertex(0.5, 0.0, 2.0, 0xFF00_00FF)];
        let (quad, indices) = line_quads(&ends, &[0, 1], Rect::whole(200, 100));
        assert_eq!(indices, [0, 1, 2, 2, 1, 3]);
        let heights: Vec<f32> = quad.iter().map(|v| v.position[1] / v.position[3] * 50.0).collect();
        assert_eq!(heights, [0.5, -0.5, 0.5, -0.5]);
        let across: Vec<f32> = quad.iter().map(|v| v.position[0] / v.position[3]).collect();
        assert_eq!(across, [-0.5, -0.5, 0.5, 0.5]);
        // Behind the viewer at one end, it is cut there; wholly behind, gone.
        let ends =
            [vertex(0.0, 0.0, 1.0, 0xFF00_0000), Vertex { position: [1.0, 0.0, 0.0, -1.0], ..ends[1] }];
        let (quad, _) = line_quads(&ends, &[0, 1], Rect::whole(200, 100));
        assert!(quad.len() == 4 && quad.iter().all(|v| v.position[3] > 0.0));
        let behind = [ends[1], ends[1]];
        assert!(line_quads(&behind, &[0, 1], Rect::whole(200, 100)).0.is_empty());
    }

    #[test]
    fn strips_and_fans_become_lists_that_keep_winding_and_first_vertex() {
        assert_eq!(triangles(4, &[0, 1, 2, 3, 4, 5, 6]).unwrap(), [0, 1, 2, 3, 4, 5]);
        // Strip 0 1 2 3 4: triangles (0 1 2), (1 2 3) turned over, (2 3 4).
        assert_eq!(triangles(5, &[0, 1, 2, 3, 4]).unwrap(), [0, 1, 2, 1, 3, 2, 2, 3, 4]);
        // Fan around 0: (0 1 2), (0 2 3), each starting with its own vertex.
        assert_eq!(triangles(6, &[0, 1, 2, 3]).unwrap(), [1, 2, 0, 2, 3, 0]);
        assert!(triangles(5, &[0, 1]).unwrap().is_empty());
        assert_eq!(triangles(2, &[0, 1]), None);
    }

    #[test]
    fn transformed_vertices_map_pixel_centres_into_clip_space() {
        // D3DTLVERTEX: x, y, z, rhw, diffuse, specular, u, v.
        let layout = layout(0x1C4).unwrap();
        assert_eq!(
            (layout.size, layout.diffuse, layout.specular, layout.uv),
            (32, Some(16), Some(20), Some(24))
        );
        let mut bytes = Vec::new();
        for value in [319.5f32, 239.5, 0.25, 0.5] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0x8040_2010u32.to_le_bytes());
        bytes.extend_from_slice(&0xFF00_0001u32.to_le_bytes());
        for value in [0.75f32, 0.125] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let viewport = Rect::whole(640, 480);
        // The pixel centre in the middle of the screen is the middle of clip space.
        let [vertex] = transformed_vertices(&bytes, &layout, viewport)[..] else { panic!("one vertex") };
        assert_eq!(vertex.position, [0.0, 0.0, 0.5, 2.0]);
        assert_eq!((vertex.diffuse, vertex.specular, vertex.uv), (0x8040_2010, 0xFF00_0001, [0.75, 0.125]));
        // An rhw of zero is taken as one. The top-left pixel's centre is
        // half a pixel inside the corner.
        bytes[..8].fill(0);
        bytes[12..16].fill(0);
        let [vertex] = transformed_vertices(&bytes, &layout, viewport)[..] else { panic!("one vertex") };
        assert_eq!(vertex.position, [-1.0 + 1.0 / 640.0, 1.0 - 1.0 / 480.0, 0.25, 1.0]);

        // A vertex behind the viewer has a negative rhw: its position comes
        // back as it was before the division, with a negative w.
        let mut behind = bytes.clone();
        for (at, value) in [(0, 480.0f32), (4, 360.0), (8, 2.0), (12, -0.25)] {
            behind[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let [vertex] = transformed_vertices(&behind, &layout, viewport)[..] else { panic!("one vertex") };
        let [x, y, z, w] = vertex.position;
        assert_eq!((z, w), (-8.0, -4.0));
        assert!((x / w - 0.5015625).abs() < 1e-6 && (y / w + 0.50208336).abs() < 1e-6, "{x} {y}");

        // A format with only a position gets white, no specular, no texture.
        let layout = super::layout(0x004).unwrap();
        let [vertex] = transformed_vertices(&bytes[..16], &layout, viewport)[..] else {
            panic!("one vertex")
        };
        assert_eq!((vertex.diffuse, vertex.specular, vertex.uv), (0xFFFF_FFFF, 0, [0.0, 0.0]));
    }
}
