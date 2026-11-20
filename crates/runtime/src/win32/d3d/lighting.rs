//! Direct3D's lighting of vertices, as DirectX 6 computed it on the CPU.
//!
//! The model is the one of Wine's Direct3D for devices older than DirectX 7
//! (`compute_light` and its helpers in `dlls/wined3d/device.c`, with
//! "legacy lighting"), which was fitted to tests on Windows: lighting in
//! camera space, an attenuation that falls off with the light's range, and
//! specular highlights only when the material has a power. Everything is
//! single precision, in the order written here.

use super::device::{Matrix, VIEW, WORLD, multiply};
use crate::{Machine, Stop};

/// Light state numbers (`D3DLIGHTSTATE_*`).
const MATERIAL: usize = 1;
const AMBIENT: usize = 2;
const COLORVERTEX: usize = 8;
const SPECULARENABLE: usize = 29;
// Light types and flags.
const D3DLIGHT_POINT: u32 = 1;
const D3DLIGHT_DIRECTIONAL: u32 = 3;
const D3DLIGHT_ACTIVE: u32 = 1;
const D3DLIGHT_NO_SPECULAR: u32 = 2;

type Vector = [f32; 3];
type Color = [f32; 4];

fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A vector of length one; a vector of no length is left as it is.
fn normalise(v: Vector) -> Vector {
    let length = dot(v, v).sqrt();
    if length == 0.0 { v } else { v.map(|component| component / length) }
}

/// A point (`w` of one) or a direction (`w` of zero) through a matrix.
fn transform(v: Vector, w: f32, matrix: &Matrix) -> [f32; 4] {
    std::array::from_fn(|column| {
        v[0] * matrix[column]
            + v[1] * matrix[4 + column]
            + v[2] * matrix[8 + column]
            + w * matrix[12 + column]
    })
}

fn color(argb: u32) -> Color {
    let [blue, green, red, alpha] = argb.to_le_bytes().map(|channel| channel as f32 / 255.0);
    [red, green, blue, alpha]
}

fn floats(words: &[u32]) -> [f32; 4] {
    std::array::from_fn(|n| f32::from_bits(words[n]))
}

/// A colour of floats as `0xAARRGGBB`, each channel clamped and rounded.
fn pack(color: Color) -> u32 {
    let [red, green, blue, alpha] = color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0 + 0.5) as u32);
    alpha << 24 | red << 16 | green << 8 | blue
}

enum Kind {
    /// The direction towards the light, in camera space.
    Directional(Vector),
    /// Its position in camera space, its range and its three attenuation factors.
    Point(Vector, f32, [f32; 3]),
}

struct Light {
    kind: Kind,
    color: Color,
    specular: bool,
}

/// What is needed to light vertices, gathered once for a batch.
pub(super) struct Lighting {
    /// World then view.
    to_camera: Matrix,
    /// Takes a normal to camera space: the inverse of the upper 3x3 of
    /// `to_camera`, transposed; or that 3x3 itself if it has no inverse.
    normal: [f32; 9],
    ambient: Color,
    lights: Vec<Light>,
    diffuse: Color,
    material_ambient: Color,
    specular: Color,
    emissive: Color,
    /// The material's specular power, or zero while specular is off.
    power: f32,
    /// The vertex's own diffuse colour stands in for the material's.
    color_vertex: bool,
}

fn inverse_transpose(m: &Matrix) -> [f32; 9] {
    let a = |row: usize, column: usize| m[4 * row + column];
    let cofactor = |row: usize, column: usize| {
        let (r1, r2) = ((row + 1) % 3, (row + 2) % 3);
        let (c1, c2) = ((column + 1) % 3, (column + 2) % 3);
        a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1)
    };
    let determinant = a(0, 0) * cofactor(0, 0) + a(0, 1) * cofactor(0, 1) + a(0, 2) * cofactor(0, 2);
    if determinant == 0.0 {
        return std::array::from_fn(|n| a(n / 3, n % 3));
    }
    // The inverse is the transposed cofactors over the determinant, so its
    // transpose is the cofactors themselves over it.
    std::array::from_fn(|n| cofactor(n / 3, n % 3) / determinant)
}

/// The lighting set up on a device; `None` when no material is selected,
/// in which case vertices are not lit.
pub(super) fn prepare(m: &Machine, device: usize) -> Result<Option<Lighting>, Stop> {
    let unsupported = |what: String| Stop::Unimplemented { dll: "Direct3D lighting".into(), name: what };
    let state = &m.os.ddraw.devices[device];
    let Some(material) =
        state.light_states[MATERIAL].checked_sub(1).and_then(|n| m.os.ddraw.materials.get(n as usize))
    else {
        return Ok(None);
    };
    let to_camera = multiply(&state.transforms[WORLD], &state.transforms[VIEW]);
    let view = &state.transforms[VIEW];
    let mut lights = Vec::new();
    for &index in state.current_viewport.map_or(&[][..], |viewport| &m.os.ddraw.viewports[viewport].lights) {
        let data = &m.os.ddraw.lights[index].data;
        let flags = data[19];
        if flags & D3DLIGHT_ACTIVE == 0 {
            continue;
        }
        let vector = |at: usize| -> Vector { std::array::from_fn(|n| f32::from_bits(data[at + n])) };
        let [x, y, z, _] = match data[1] {
            // The structure holds the direction the light travels in.
            D3DLIGHT_DIRECTIONAL => transform(vector(9).map(|component| -component), 0.0, view),
            D3DLIGHT_POINT => transform(vector(6), 1.0, view),
            other => return Err(unsupported(format!("a light of type {other}"))),
        };
        let kind = match data[1] {
            D3DLIGHT_DIRECTIONAL => Kind::Directional(normalise([x, y, z])),
            _ => Kind::Point([x, y, z], f32::from_bits(data[12]), vector(14)),
        };
        lights.push(Light { kind, color: floats(&data[2..6]), specular: flags & D3DLIGHT_NO_SPECULAR == 0 });
    }
    let specular_on = state.render_states[SPECULARENABLE] != 0;
    Ok(Some(Lighting {
        normal: inverse_transpose(&to_camera),
        to_camera,
        ambient: color(state.light_states[AMBIENT]),
        lights,
        diffuse: floats(&material.data[1..5]),
        material_ambient: floats(&material.data[5..9]),
        specular: floats(&material.data[9..13]),
        emissive: floats(&material.data[13..17]),
        power: if specular_on { f32::from_bits(material.data[17]) } else { 0.0 },
        color_vertex: state.light_states[COLORVERTEX] != 0,
    }))
}

impl Lighting {
    /// The diffuse and specular colours of a vertex, as `0xAARRGGBB`.
    pub(super) fn light(&self, position: Vector, normal: Vector, vertex_diffuse: Option<u32>) -> (u32, u32) {
        let [x, y, z, w] = transform(position, 1.0, &self.to_camera);
        let position = [x / w, y / w, z / w];
        let n = &self.normal;
        let normal = normalise(std::array::from_fn(|column| {
            normal[0] * n[column] + normal[1] * n[3 + column] + normal[2] * n[6 + column]
        }));
        let (mut diffuse, mut specular) = ([0.0f32; 3], [0.0f32; 3]);
        for light in &self.lights {
            let (towards, attenuation) = match light.kind {
                Kind::Directional(towards) => (towards, 1.0),
                Kind::Point(at, range, [constant, linear, quadratic]) => {
                    let towards = [at[0] - position[0], at[1] - position[1], at[2] - position[2]];
                    // How far inside the light's range the vertex is, from
                    // one at the light to zero at the range.
                    let inside = (range - dot(towards, towards).sqrt()) / range;
                    if inside.is_nan() || inside <= 0.0 {
                        continue;
                    }
                    (normalise(towards), constant + inside * linear + inside * inside * quadratic)
                }
            };
            let facing = dot(towards, normal);
            let amount = facing.clamp(0.0, 1.0) * attenuation;
            for channel in 0..3 {
                diffuse[channel] += light.color[channel] * amount;
            }
            // Highlights, for a viewer far away along the camera's axis.
            if light.specular && self.power > 0.0 && facing > 0.0 {
                let halfway = dot(normal, normalise([towards[0], towards[1], towards[2] - 1.0]));
                if halfway > 0.0 {
                    let amount = attenuation * halfway.powf(self.power);
                    for channel in 0..3 {
                        specular[channel] += light.color[channel] * amount;
                    }
                }
            }
        }
        let material = match vertex_diffuse {
            Some(argb) if self.color_vertex => color(argb),
            _ => self.diffuse,
        };
        let lit: Color = std::array::from_fn(|channel| match channel {
            3 => material[3],
            _ => {
                self.ambient[channel] * self.material_ambient[channel]
                    + diffuse[channel] * material[channel]
                    + self.emissive[channel]
            }
        });
        let highlight: Color =
            std::array::from_fn(
                |channel| if channel == 3 { 0.0 } else { specular[channel] * self.specular[channel] },
            );
        (pack(lit), pack(highlight))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::d3d::device::IDENTITY;

    fn lighting(lights: Vec<Light>) -> Lighting {
        Lighting {
            to_camera: IDENTITY,
            normal: inverse_transpose(&IDENTITY),
            ambient: [0.25, 0.25, 0.25, 1.0],
            lights,
            diffuse: [1.0, 0.5, 0.0, 0.5],
            material_ambient: [1.0, 1.0, 1.0, 1.0],
            specular: [1.0, 1.0, 1.0, 1.0],
            emissive: [0.0, 0.0, 0.125, 0.0],
            power: 0.0,
            color_vertex: false,
        }
    }

    #[test]
    fn a_directional_light_shades_by_the_angle_to_the_normal() {
        let white = [1.0, 1.0, 1.0, 1.0];
        let towards_viewer =
            Light { kind: Kind::Directional([0.0, 0.0, -1.0]), color: white, specular: true };
        let l = lighting(vec![towards_viewer]);
        // Facing the light: ambient plus the full diffuse colour, then the emissive blue.
        assert_eq!(l.light([0.0; 3], [0.0, 0.0, -1.0], None), (0x80FF_BF60, 0));
        // Edge on and facing away: ambient and emissive only.
        assert_eq!(l.light([0.0; 3], [1.0, 0.0, 0.0], None), (0x8040_4060, 0));
        assert_eq!(l.light([0.0; 3], [0.0, 0.0, 1.0], None), (0x8040_4060, 0));
        // At sixty degrees: half; the normal's length does not matter.
        let tilted = [0.0, 3.0f32.sqrt(), -1.0];
        assert_eq!(l.light([0.0; 3], tilted, None), (0x80BF_8060, 0));
    }

    #[test]
    fn the_vertex_colour_can_stand_in_for_the_material_and_highlights_need_a_power() {
        let white = [1.0, 1.0, 1.0, 1.0];
        let light = || Light { kind: Kind::Directional([0.0, 0.0, -1.0]), color: white, specular: true };
        let mut l = lighting(vec![light()]);
        l.color_vertex = true;
        assert_eq!(l.light([0.0; 3], [0.0, 0.0, -1.0], Some(0x2000_8000)).0, 0x2040_C060);
        assert_eq!(l.light([0.0; 3], [0.0, 0.0, -1.0], None).0, 0x80FF_BF60);
        l.power = 8.0;
        assert_eq!(l.light([0.0; 3], [0.0, 0.0, -1.0], None).1, 0x00FF_FFFF);
    }

    #[test]
    fn a_point_light_fades_towards_its_range() {
        let white = [1.0, 1.0, 1.0, 1.0];
        let point = Light {
            kind: Kind::Point([0.0, 0.0, -10.0], 20.0, [0.0, 1.0, 0.0]),
            color: white,
            specular: false,
        };
        let mut l = lighting(vec![point]);
        (l.ambient, l.emissive, l.diffuse) = ([0.0; 4], [0.0; 4], white);
        let facing = [0.0, 0.0, -1.0];
        // Half the range away: half the light. At the range and beyond: none.
        assert_eq!(l.light([0.0; 3], facing, None).0, 0xFF80_8080);
        assert_eq!(l.light([0.0, 0.0, 10.0], facing, None).0, 0xFF00_0000);
        assert_eq!(l.light([0.0, 0.0, 30.0], facing, None).0, 0xFF00_0000);
    }

    #[test]
    fn normals_follow_a_scaled_and_turned_model() {
        // Twice as wide, and turned a quarter about the vertical axis.
        let mut matrix = IDENTITY;
        (matrix[0], matrix[2], matrix[8], matrix[10]) = (0.0, 2.0, -1.0, 0.0);
        let n = inverse_transpose(&matrix);
        let through = |v: Vector| -> Vector {
            std::array::from_fn(|c| v[0] * n[c] + v[1] * n[3 + c] + v[2] * n[6 + c])
        };
        assert_eq!(through([1.0, 0.0, 0.0]), [0.0, 0.0, 0.5]);
        assert_eq!(through([0.0, 0.0, 1.0]), [-1.0, 0.0, 0.0]);
        assert_eq!(through([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
        // A matrix without an inverse is used as it is.
        let mut flat = IDENTITY;
        flat[5] = 0.0;
        assert_eq!(inverse_transpose(&flat), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    }
}
