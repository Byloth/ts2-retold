//! What the 3D device can do, as the `D3DDEVICEDESC` given to the program.
//!
//! One device is offered: a hardware rasteriser that takes transformed and
//! lit vertices, as accelerators of 1999 did. Transformation, lighting and
//! clipping are the software pipeline's job. The software rasterisers that
//! DirectX also listed ("Ramp", "RGB", "MMX") are not reproduced.
//!
//! Every capability claimed here is a promise the renderer has to keep.

/// `D3DDEVICEDESC` of DirectX 6: 63 words (252 bytes).
pub const WORDS: usize = 63;

// Field positions, in words.
const FLAGS: usize = 1;
pub const COLOR_MODEL: usize = 2;
const DEVICE_CAPS: usize = 3;
const TRANSFORM_CAPS: usize = 4;
const CLIPPING: usize = 6;
const LIGHTING_CAPS: usize = 7;
const LINE_CAPS: usize = 11;
pub const TRIANGLE_CAPS: usize = 25;
const RENDER_BIT_DEPTHS: usize = 39;
pub const Z_BUFFER_BIT_DEPTHS: usize = 40;
const MAX_VERTEX_COUNT: usize = 42;
const MIN_TEXTURE_SIZE: usize = 43;
const MAX_TEXTURE_SIZE: usize = 45;
const VERTEX_FORMAT_CAPS: usize = 60;
const TEXTURE_OPERATION_CAPS: usize = 61;
const TEXTURE_STAGES: usize = 62;

/// `D3DPRIMCAPS`: 14 words. Positions inside it.
const PRIMITIVE_WORDS: usize = 14;
const MISC: usize = 1;
const RASTER: usize = 2;
const Z_COMPARE: usize = 3;
pub const SOURCE_BLEND: usize = 4;
const DESTINATION_BLEND: usize = 5;
const ALPHA_COMPARE: usize = 6;
const SHADE: usize = 7;
pub const TEXTURE: usize = 8;
const TEXTURE_FILTER: usize = 9;
const TEXTURE_BLEND: usize = 10;
const TEXTURE_ADDRESS: usize = 11;

/// All eleven fields up to the vertex count are valid.
const D3DDD_ALL: u32 = 0x7FF;
const D3DCOLOR_RGB: u32 = 2;
const DDBD_16: u32 = 0x400;
const DDBD_32: u32 = 0x100;
pub const LARGEST_TEXTURE: u32 = 2048;

// D3DDEVCAPS: float TL vertices, execute buffers and TL vertices in system
// memory, textures in video memory, DrawPrimitive on TL vertices, rendering
// after a flip, DrawPrimitives2.
const DEVICE: u32 = 0x0001 | 0x0010 | 0x0040 | 0x0200 | 0x0400 | 0x0800 | 0x2000;
const D3DTRANSFORMCAPS_CLIP: u32 = 1;
// Point, spot and directional lights, in the RGB lighting model.
const LIGHTS: u32 = 0x1 | 0x2 | 0x4;
const D3DLIGHTINGMODEL_RGB: u32 = 1;

// D3DPMISCCAPS: z writes can be masked; cull none, clockwise, counter-clockwise.
const MISC_CAPS: u32 = 0x02 | 0x10 | 0x20 | 0x40;
// D3DPRASTERCAPS: dither, z test, subpixel accuracy, vertex fog, table fog.
const RASTER_CAPS: u32 = 0x01 | 0x10 | 0x20 | 0x80 | 0x100;
/// All eight comparison functions.
const COMPARE_CAPS: u32 = 0xFF;
/// Every source blend factor, "both source alpha" and its inverse included.
const SOURCE_BLEND_CAPS: u32 = 0x1FFF;
/// Every destination blend factor (zero .. inverse destination colour, saturate).
const DESTINATION_BLEND_CAPS: u32 = 0x7FF;
// D3DPSHADECAPS: flat and Gouraud colour, specular, alpha blend and fog, RGB.
const SHADE_CAPS: u32 = 0x2 | 0x8 | 0x80 | 0x200 | 0x1000 | 0x4000 | 0x4_0000 | 0x8_0000;
// D3DPTEXTURECAPS: perspective correction, power-of-two sizes, alpha,
// colour-key transparency. Not "square only".
const TEXTURE_CAPS: u32 = 0x1 | 0x2 | 0x4 | 0x8;
/// Nearest, linear and the four mip-mapped filters.
const TEXTURE_FILTER_CAPS: u32 = 0x3F;
// D3DPTBLENDCAPS: decal, modulate, decal alpha, modulate alpha, copy, add.
const TEXTURE_BLEND_CAPS: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x40 | 0x80;
// D3DPTADDRESSCAPS: wrap, mirror, clamp, independent U and V.
const TEXTURE_ADDRESS_CAPS: u32 = 0x1 | 0x2 | 0x4 | 0x10;
// D3DTEXOPCAPS: disable, select either argument, modulate, add.
const TEXTURE_OPERATIONS: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x40;

fn primitive_caps() -> [u32; PRIMITIVE_WORDS] {
    let mut words = [0; PRIMITIVE_WORDS];
    words[0] = 4 * PRIMITIVE_WORDS as u32;
    words[MISC] = MISC_CAPS;
    words[RASTER] = RASTER_CAPS;
    words[Z_COMPARE] = COMPARE_CAPS;
    words[SOURCE_BLEND] = SOURCE_BLEND_CAPS;
    words[DESTINATION_BLEND] = DESTINATION_BLEND_CAPS;
    words[ALPHA_COMPARE] = COMPARE_CAPS;
    words[SHADE] = SHADE_CAPS;
    words[TEXTURE] = TEXTURE_CAPS;
    words[TEXTURE_FILTER] = TEXTURE_FILTER_CAPS;
    words[TEXTURE_BLEND] = TEXTURE_BLEND_CAPS;
    words[TEXTURE_ADDRESS] = TEXTURE_ADDRESS_CAPS;
    words
}

/// Description of the hardware device.
pub fn hardware() -> [u32; WORDS] {
    let mut words = [0; WORDS];
    words[0] = 4 * WORDS as u32;
    words[FLAGS] = D3DDD_ALL;
    words[COLOR_MODEL] = D3DCOLOR_RGB;
    words[DEVICE_CAPS] = DEVICE;
    words[TRANSFORM_CAPS] = 8;
    words[TRANSFORM_CAPS + 1] = D3DTRANSFORMCAPS_CLIP;
    words[CLIPPING] = 1;
    words[LIGHTING_CAPS..LIGHTING_CAPS + 4].copy_from_slice(&[16, LIGHTS, D3DLIGHTINGMODEL_RGB, 0]);
    words[LINE_CAPS..LINE_CAPS + PRIMITIVE_WORDS].copy_from_slice(&primitive_caps());
    words[TRIANGLE_CAPS..TRIANGLE_CAPS + PRIMITIVE_WORDS].copy_from_slice(&primitive_caps());
    words[RENDER_BIT_DEPTHS] = DDBD_16 | DDBD_32;
    words[Z_BUFFER_BIT_DEPTHS] = DDBD_16;
    words[MAX_VERTEX_COUNT] = 0xFFFF;
    words[MIN_TEXTURE_SIZE..MIN_TEXTURE_SIZE + 2].copy_from_slice(&[1, 1]);
    words[MAX_TEXTURE_SIZE..MAX_TEXTURE_SIZE + 2].copy_from_slice(&[LARGEST_TEXTURE, LARGEST_TEXTURE]);
    words[VERTEX_FORMAT_CAPS] = 1; // one set of texture coordinates
    words[TEXTURE_OPERATION_CAPS] = TEXTURE_OPERATIONS;
    words[TEXTURE_STAGES] = 1 | 1 << 16; // one blend stage, one texture at a time
    words
}

/// What the emulation layer adds to the hardware device: nothing. Its colour
/// model is zero, which is how a program tells a hardware device from a
/// software one.
pub fn emulation() -> [u32; WORDS] {
    let mut words = [0; WORDS];
    words[0] = 4 * WORDS as u32;
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_structure() {
        assert_eq!(4 * WORDS, 252);
        assert_eq!(LINE_CAPS + PRIMITIVE_WORDS, TRIANGLE_CAPS);
        assert_eq!(TRIANGLE_CAPS + PRIMITIVE_WORDS, RENDER_BIT_DEPTHS);
        let hardware = hardware();
        // Byte offsets the game reads: texture caps at 0x84, source blend
        // caps at 0x74, z-buffer depths at 0xA0.
        assert_eq!(4 * (TRIANGLE_CAPS + TEXTURE), 0x84);
        assert_eq!(4 * (TRIANGLE_CAPS + SOURCE_BLEND), 0x74);
        assert_eq!(4 * Z_BUFFER_BIT_DEPTHS, 0xA0);
        assert_eq!(hardware[TRIANGLE_CAPS + TEXTURE] & 0x21, 0x01); // perspective, not square-only
        assert_ne!(hardware[TRIANGLE_CAPS + SOURCE_BLEND] & 0x800, 0); // "both source alpha"
        assert_ne!(hardware[COLOR_MODEL], 0);
        assert_eq!(emulation()[COLOR_MODEL], 0);
    }
}
