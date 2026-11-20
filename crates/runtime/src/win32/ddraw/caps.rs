//! What the display adapter can do, as reported by `IDirectDraw::GetCaps`.
//!
//! The adapter is a generic 3D accelerator of 1999 with 32 MB of memory. Only
//! capabilities the runtime actually implements are claimed, so the program
//! never chooses a path that does not exist here.

/// `DDCAPS` of DirectX 6: 95 words. Earlier versions are prefixes of it.
pub const WORDS: usize = 95;
/// Sizes a caller may state: DirectX 3, DirectX 5 and DirectX 6.
pub const SIZES: [usize; 3] = [316, 364, 380];
/// Total and free video memory.
pub const VIDEO_MEMORY: u32 = 32 << 20;

// Positions of the fields used here.
const CAPS: usize = 1;
const COLOR_KEY_CAPS: usize = 3;
const EFFECT_CAPS: usize = 4;
const PALETTE_CAPS: usize = 6;
const Z_BUFFER_DEPTHS: usize = 14;
const VIDEO_MEMORY_TOTAL: usize = 15;
const VIDEO_MEMORY_FREE: usize = 16;
const RASTER_OPERATIONS: usize = 25;
const OLD_SURFACE_CAPS: usize = 33;
/// Capabilities for copies from system memory to video memory.
const SYSTEM_TO_VIDEO_CAPS: usize = 43;
const SYSTEM_TO_VIDEO_COLOR_KEY_CAPS: usize = 44;
const SYSTEM_TO_VIDEO_EFFECT_CAPS: usize = 45;
const SYSTEM_TO_VIDEO_RASTER_OPERATIONS: usize = 46;
const SURFACE_CAPS: usize = 91;

const DDCAPS_3D: u32 = 0x0000_0001;
const DDCAPS_BLT: u32 = 0x0000_0040;
const DDCAPS_BLTSTRETCH: u32 = 0x0000_0200;
const DDCAPS_GDI: u32 = 0x0000_0400;
const DDCAPS_PALETTE: u32 = 0x0000_8000;
const DDCAPS_COLORKEY: u32 = 0x0040_0000;
const DDCAPS_BLTCOLORFILL: u32 = 0x0400_0000;
const DDCAPS_BLTDEPTHFILL: u32 = 0x1000_0000;
const DDCAPS_CANCLIP: u32 = 0x2000_0000;
const DDCAPS_CANBLTSYSMEM: u32 = 0x8000_0000;
const DDCKEYCAPS_SRCBLT: u32 = 0x0000_0200;
const DDFXCAPS_STRETCH_AND_SHRINK: u32 = 0x0400 | 0x1000 | 0x4000 | 0x0001_0000;
const DDPCAPS_8BIT: u32 = 0x04;
const DDPCAPS_PRIMARYSURFACE: u32 = 0x10;
const DDPCAPS_ALLOW256: u32 = 0x40;
const DDBD_16: u32 = 0x0400;
/// `SRCCOPY`, the one raster operation offered, as bit 0xCC of the 256-bit set.
const SRCCOPY_WORD: usize = 0xCC / 32;
const SRCCOPY_BIT: u32 = 1 << (0xCC % 32);

const BLIT_CAPS: u32 =
    DDCAPS_BLT | DDCAPS_BLTSTRETCH | DDCAPS_COLORKEY | DDCAPS_BLTCOLORFILL | DDCAPS_CANCLIP;
/// Back buffer, complex, flip, front buffer, off-screen plain, palette,
/// primary, system memory, texture, video memory and mip-map surfaces.
const BASE_SURFACES: u32 =
    0x0004 | 0x0008 | 0x0010 | 0x0020 | 0x0040 | 0x0100 | 0x0200 | 0x0800 | 0x1000 | 0x4000 | 0x0040_0000;
/// 3D device and z-buffer surfaces.
const SURFACES_3D: u32 = 0x2000 | 0x0002_0000;

fn common(blits: u32, surfaces: u32) -> [u32; WORDS] {
    let mut words = [0; WORDS];
    words[0] = 4 * WORDS as u32;
    words[CAPS] = blits | DDCAPS_GDI | DDCAPS_PALETTE | DDCAPS_CANBLTSYSMEM;
    words[COLOR_KEY_CAPS] = DDCKEYCAPS_SRCBLT;
    words[EFFECT_CAPS] = DDFXCAPS_STRETCH_AND_SHRINK;
    words[PALETTE_CAPS] = DDPCAPS_8BIT | DDPCAPS_PRIMARYSURFACE | DDPCAPS_ALLOW256;
    words[RASTER_OPERATIONS + SRCCOPY_WORD] = SRCCOPY_BIT;
    words[OLD_SURFACE_CAPS] = surfaces;
    words[SYSTEM_TO_VIDEO_CAPS] = blits | DDCAPS_CANBLTSYSMEM;
    words[SYSTEM_TO_VIDEO_COLOR_KEY_CAPS] = DDCKEYCAPS_SRCBLT;
    words[SYSTEM_TO_VIDEO_EFFECT_CAPS] = DDFXCAPS_STRETCH_AND_SHRINK;
    words[SYSTEM_TO_VIDEO_RASTER_OPERATIONS + SRCCOPY_WORD] = SRCCOPY_BIT;
    words[SURFACE_CAPS] = surfaces;
    words
}

/// Capabilities of the hardware.
pub fn driver() -> [u32; WORDS] {
    let mut words = common(
        BLIT_CAPS | DDCAPS_3D | DDCAPS_BLTDEPTHFILL,
        BASE_SURFACES | SURFACES_3D,
    );
    words[Z_BUFFER_DEPTHS] = DDBD_16;
    words[VIDEO_MEMORY_TOTAL] = VIDEO_MEMORY;
    words[VIDEO_MEMORY_FREE] = VIDEO_MEMORY;
    words
}

/// Capabilities of the software emulation layer: the same 2D, no 3D.
pub fn emulation() -> [u32; WORDS] {
    common(BLIT_CAPS, BASE_SURFACES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_structure() {
        assert_eq!(4 * WORDS, 380);
        let driver = driver();
        assert_eq!(driver[0], 380);
        assert_eq!(driver[CAPS] & DDCAPS_3D, DDCAPS_3D);
        assert_eq!(driver[SYSTEM_TO_VIDEO_CAPS] >> 31, 1);
        assert_eq!(emulation()[CAPS] & DDCAPS_3D, 0);
        // The raster-operation arrays are eight words each and do not overlap
        // the fields that follow them.
        assert_eq!(RASTER_OPERATIONS + 8, OLD_SURFACE_CAPS);
        assert_eq!(SYSTEM_TO_VIDEO_RASTER_OPERATIONS + 8, 54);
        assert_eq!(SURFACE_CAPS + 4, WORDS);
    }
}
