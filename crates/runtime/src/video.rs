//! The runtime's interface to whatever puts pixels on a screen.
//!
//! DirectDraw surfaces have two homes (ADR 0012): memory the game can lock,
//! and a surface of the video backend. The backend is asked to do only what
//! the game's memory cannot: draw triangles, and show a surface. Everything
//! else it offers (uploads, downloads, fills, copies) exists so that pixels
//! need not travel between the two homes more than necessary.
//!
//! [`Headless`] is the backend with no display. It keeps its surfaces in
//! host memory and implements the 2D operations exactly, so the runtime can
//! be built, tested and run anywhere. It draws no triangles.

/// How a surface's pixels are laid out. Channel order is given from the most
/// significant bits down, as DirectDraw's bit masks describe it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// One byte per pixel, an index into a palette.
    Indexed8,
    Rgb565,
    Xrgb1555,
    Argb1555,
    Argb4444,
    Xrgb8888,
    Argb8888,
    /// 16-bit depth values.
    Depth16,
}

const DDPF_ALPHAPIXELS: u32 = 0x01;
const DDPF_PALETTEINDEXED8: u32 = 0x20;
const DDPF_RGB: u32 = 0x40;
const DDPF_ZBUFFER: u32 = 0x400;

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            PixelFormat::Indexed8 => 1,
            PixelFormat::Xrgb8888 | PixelFormat::Argb8888 => 4,
            _ => 2,
        }
    }

    /// The format of a display mode of the given depth.
    pub fn of_display(bits_per_pixel: u32) -> PixelFormat {
        match bits_per_pixel {
            8 => PixelFormat::Indexed8,
            16 => PixelFormat::Rgb565,
            _ => PixelFormat::Xrgb8888,
        }
    }

    /// The raw pixel of this format nearest to a colour given as
    /// `0xAARRGGBB`; channels narrower than eight bits keep their top bits.
    /// Palettes and depth have no colour: the result is zero.
    pub fn from_argb(self, color: u32) -> u32 {
        let [blue, green, red, alpha] = color.to_le_bytes().map(u32::from);
        match self {
            PixelFormat::Rgb565 => (red >> 3) << 11 | (green >> 2) << 5 | blue >> 3,
            PixelFormat::Xrgb1555 => (red >> 3) << 10 | (green >> 3) << 5 | blue >> 3,
            PixelFormat::Argb1555 => (alpha >> 7) << 15 | (red >> 3) << 10 | (green >> 3) << 5 | blue >> 3,
            PixelFormat::Argb4444 => (alpha >> 4) << 12 | (red >> 4) << 8 | (green >> 4) << 4 | blue >> 4,
            PixelFormat::Xrgb8888 => color & 0x00FF_FFFF,
            PixelFormat::Argb8888 => color,
            PixelFormat::Indexed8 | PixelFormat::Depth16 => 0,
        }
    }

    /// The colour, as `0xAARRGGBB`, of a raw pixel of this format. A channel
    /// narrower than eight bits is widened by repeating its bits, so that
    /// its largest value becomes 255; a format without alpha is opaque.
    /// A depth value becomes a grey, far being white. `None` for a palette
    /// index, which has no colour by itself.
    pub fn to_argb(self, raw: u32) -> Option<u32> {
        // The top `bits` bits of a channel found at `shift`, widened to eight.
        let channel = |shift: u32, bits: u32| {
            let value = raw >> shift & ((1 << bits) - 1);
            (0..8u32.div_ceil(bits)).fold(0, |wide, n| wide | (value << 8 >> (bits * (n + 1)))) & 0xFF
        };
        let pack = |alpha: u32, red: u32, green: u32, blue: u32| alpha << 24 | red << 16 | green << 8 | blue;
        Some(match self {
            PixelFormat::Rgb565 => pack(0xFF, channel(11, 5), channel(5, 6), channel(0, 5)),
            PixelFormat::Xrgb1555 => pack(0xFF, channel(10, 5), channel(5, 5), channel(0, 5)),
            PixelFormat::Argb1555 => pack(channel(15, 1), channel(10, 5), channel(5, 5), channel(0, 5)),
            PixelFormat::Argb4444 => pack(channel(12, 4), channel(8, 4), channel(4, 4), channel(0, 4)),
            PixelFormat::Xrgb8888 => raw | 0xFF00_0000,
            PixelFormat::Argb8888 => raw,
            PixelFormat::Depth16 => pack(0xFF, raw >> 8 & 0xFF, raw >> 8 & 0xFF, raw >> 8 & 0xFF),
            PixelFormat::Indexed8 => return None,
        })
    }

    /// The 16-bit depth value for a depth between 0 (near) and 1 (far).
    pub fn depth16(depth: f32) -> u32 {
        (depth.clamp(0.0, 1.0) * 65535.0).round() as u32
    }

    /// The eight words of a `DDPIXELFORMAT`: size, flags, FourCC, bit count,
    /// then the red, green, blue and alpha masks (for a depth format: the
    /// depth bit count, stencil bit count, depth mask and stencil mask).
    pub fn to_words(self) -> [u32; 8] {
        let (flags, bits, masks) = match self {
            PixelFormat::Indexed8 => (DDPF_RGB | DDPF_PALETTEINDEXED8, 8, [0, 0, 0, 0]),
            PixelFormat::Rgb565 => (DDPF_RGB, 16, [0xF800, 0x07E0, 0x001F, 0]),
            PixelFormat::Xrgb1555 => (DDPF_RGB, 16, [0x7C00, 0x03E0, 0x001F, 0]),
            PixelFormat::Argb1555 => (DDPF_RGB | DDPF_ALPHAPIXELS, 16, [0x7C00, 0x03E0, 0x001F, 0x8000]),
            PixelFormat::Argb4444 => (DDPF_RGB | DDPF_ALPHAPIXELS, 16, [0x0F00, 0x00F0, 0x000F, 0xF000]),
            PixelFormat::Xrgb8888 => (DDPF_RGB, 32, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0]),
            PixelFormat::Argb8888 => {
                (DDPF_RGB | DDPF_ALPHAPIXELS, 32, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000])
            }
            PixelFormat::Depth16 => (DDPF_ZBUFFER, 16, [0, 0xFFFF, 0, 0]),
        };
        [32, flags, 0, bits, masks[0], masks[1], masks[2], masks[3]]
    }

    /// Recognises a `DDPIXELFORMAT`. `None` for a layout not offered.
    pub fn from_words(words: &[u32; 8]) -> Option<PixelFormat> {
        const ALL: [PixelFormat; 8] = [
            PixelFormat::Indexed8,
            PixelFormat::Rgb565,
            PixelFormat::Xrgb1555,
            PixelFormat::Argb1555,
            PixelFormat::Argb4444,
            PixelFormat::Xrgb8888,
            PixelFormat::Argb8888,
            PixelFormat::Depth16,
        ];
        // Only the kind of format, its depth and its masks identify it; a
        // caller may leave the size unset or add flags that do not matter.
        let kind = DDPF_RGB | DDPF_ALPHAPIXELS | DDPF_PALETTEINDEXED8 | DDPF_ZBUFFER;
        ALL.into_iter().find(|format| {
            let own = format.to_words();
            own[1] == words[1] & kind && own[3..] == words[3..]
        })
    }
}

/// A rectangle of pixels inside a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn whole(width: u32, height: u32) -> Rect {
        Rect { x: 0, y: 0, width, height }
    }

    /// Whether the rectangle is not empty and lies inside a surface of the given size.
    pub fn fits(&self, width: u32, height: u32) -> bool {
        self.width > 0
            && self.height > 0
            && self.x.checked_add(self.width).is_some_and(|right| right <= width)
            && self.y.checked_add(self.height).is_some_and(|bottom| bottom <= height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u32);

pub trait Video {
    /// Creates a surface whose contents are undefined until written.
    fn create_surface(&mut self, width: u32, height: u32, format: PixelFormat) -> SurfaceId;
    fn destroy_surface(&mut self, surface: SurfaceId);
    /// Replaces a rectangle with pixels in the surface's own format;
    /// `pixels` holds `area.height` rows of `pitch` bytes.
    fn upload(&mut self, surface: SurfaceId, area: Rect, pitch: usize, pixels: &[u8]);
    /// Reads a rectangle back, in the surface's own format.
    fn download(&mut self, surface: SurfaceId, area: Rect, pitch: usize, pixels: &mut [u8]);
    /// Sets every pixel of a rectangle to one raw value.
    fn fill(&mut self, surface: SurfaceId, area: Rect, value: u32);
    /// Copies between surfaces of the same format, scaling to the target
    /// rectangle without filtering. Source pixels whose raw value is inside
    /// `color_key` (inclusive) are not copied.
    fn copy(
        &mut self,
        source: SurfaceId,
        from: Rect,
        target: SurfaceId,
        to: Rect,
        color_key: Option<(u32, u32)>,
    );
    /// Shows a surface on the display.
    fn present(&mut self, surface: SurfaceId);
    /// The backend as its concrete type, for code that knows which one it set up.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Reads the pixel at byte offset `at`.
#[inline]
fn pixel(bytes: &[u8], at: usize, size: usize) -> u32 {
    let mut raw = [0u8; 4];
    raw[..size].copy_from_slice(&bytes[at..at + size]);
    u32::from_le_bytes(raw)
}

/// Fills a rectangle of a pixel buffer whose first byte is the rectangle's
/// top-left pixel.
pub fn fill_pixels(target: &mut [u8], pitch: usize, width: u32, height: u32, size: usize, value: u32) {
    let raw = value.to_le_bytes();
    for row in 0..height as usize {
        for column in 0..width as usize {
            let at = row * pitch + column * size;
            target[at..at + size].copy_from_slice(&raw[..size]);
        }
    }
}

/// Copies a rectangle of `from` pixels onto a rectangle of `to` pixels. Both
/// buffers start at their rectangle's top-left pixel. See [`Video::copy`].
#[allow(clippy::too_many_arguments)]
pub fn copy_pixels(
    source: &[u8],
    source_pitch: usize,
    from: (u32, u32),
    target: &mut [u8],
    target_pitch: usize,
    to: (u32, u32),
    size: usize,
    color_key: Option<(u32, u32)>,
) {
    let same_size = from == to;
    for row in 0..to.1 as usize {
        let source_row = if same_size { row } else { row * from.1 as usize / to.1 as usize };
        if same_size && color_key.is_none() {
            let len = to.0 as usize * size;
            target[row * target_pitch..][..len].copy_from_slice(&source[source_row * source_pitch..][..len]);
            continue;
        }
        for column in 0..to.0 as usize {
            let source_column = if same_size { column } else { column * from.0 as usize / to.0 as usize };
            let at = source_row * source_pitch + source_column * size;
            let value = pixel(source, at, size);
            if color_key.is_some_and(|(low, high)| (low..=high).contains(&value)) {
                continue;
            }
            let out = row * target_pitch + column * size;
            target[out..out + size].copy_from_slice(&source[at..at + size]);
        }
    }
}

struct HeadlessSurface {
    width: u32,
    height: u32,
    format: PixelFormat,
    /// Rows of `width * bytes_per_pixel` bytes, top row first.
    pixels: Vec<u8>,
}

impl HeadlessSurface {
    fn pitch(&self) -> usize {
        (self.width * self.format.bytes_per_pixel()) as usize
    }

    fn offset(&self, area: Rect) -> usize {
        area.y as usize * self.pitch() + (area.x * self.format.bytes_per_pixel()) as usize
    }
}

/// What was last shown on the (absent) display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub pixels: Vec<u8>,
}

/// The video backend without a display.
#[derive(Default)]
pub struct Headless {
    surfaces: Vec<Option<HeadlessSurface>>,
    pub last_frame: Option<Frame>,
    pub frames_presented: u64,
}

impl Headless {
    fn surface(&mut self, id: SurfaceId) -> &mut HeadlessSurface {
        self.surfaces[id.0 as usize].as_mut().expect("the surface exists")
    }
}

impl Video for Headless {
    fn create_surface(&mut self, width: u32, height: u32, format: PixelFormat) -> SurfaceId {
        let pixels = vec![0; (width * height * format.bytes_per_pixel()) as usize];
        let surface = Some(HeadlessSurface { width, height, format, pixels });
        match self.surfaces.iter().position(Option::is_none) {
            Some(index) => {
                self.surfaces[index] = surface;
                SurfaceId(index as u32)
            }
            None => {
                self.surfaces.push(surface);
                SurfaceId(self.surfaces.len() as u32 - 1)
            }
        }
    }

    fn destroy_surface(&mut self, surface: SurfaceId) {
        self.surfaces[surface.0 as usize] = None;
    }

    fn upload(&mut self, surface: SurfaceId, area: Rect, pitch: usize, pixels: &[u8]) {
        let surface = self.surface(surface);
        let (own_pitch, start) = (surface.pitch(), surface.offset(area));
        let size = surface.format.bytes_per_pixel() as usize;
        let (width, height) = (area.width, area.height);
        copy_pixels(
            pixels,
            pitch,
            (width, height),
            &mut surface.pixels[start..],
            own_pitch,
            (width, height),
            size,
            None,
        );
    }

    fn download(&mut self, surface: SurfaceId, area: Rect, pitch: usize, pixels: &mut [u8]) {
        let surface = self.surface(surface);
        let (own_pitch, start) = (surface.pitch(), surface.offset(area));
        let size = surface.format.bytes_per_pixel() as usize;
        let (width, height) = (area.width, area.height);
        copy_pixels(
            &surface.pixels[start..],
            own_pitch,
            (width, height),
            pixels,
            pitch,
            (width, height),
            size,
            None,
        );
    }

    fn fill(&mut self, surface: SurfaceId, area: Rect, value: u32) {
        let surface = self.surface(surface);
        let (pitch, start) = (surface.pitch(), surface.offset(area));
        let size = surface.format.bytes_per_pixel() as usize;
        fill_pixels(&mut surface.pixels[start..], pitch, area.width, area.height, size, value);
    }

    fn copy(
        &mut self,
        source: SurfaceId,
        from: Rect,
        target: SurfaceId,
        to: Rect,
        color_key: Option<(u32, u32)>,
    ) {
        // The source is copied out first: source and target may be one surface.
        let (source_pitch, size, rows) = {
            let surface = self.surface(source);
            let (pitch, start) = (surface.pitch(), surface.offset(from));
            let size = surface.format.bytes_per_pixel() as usize;
            let len = (from.height as usize - 1) * pitch + from.width as usize * size;
            (pitch, size, surface.pixels[start..start + len].to_vec())
        };
        let surface = self.surface(target);
        let (pitch, start) = (surface.pitch(), surface.offset(to));
        copy_pixels(
            &rows,
            source_pitch,
            (from.width, from.height),
            &mut surface.pixels[start..],
            pitch,
            (to.width, to.height),
            size,
            color_key,
        );
    }

    fn present(&mut self, surface: SurfaceId) {
        let surface = self.surface(surface);
        let frame = Frame {
            width: surface.width,
            height: surface.height,
            format: surface.format,
            pixels: surface.pixels.clone(),
        };
        self.last_frame = Some(frame);
        self.frames_presented += 1;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_formats_round_trip_through_their_description() {
        for format in [
            PixelFormat::Indexed8,
            PixelFormat::Rgb565,
            PixelFormat::Xrgb1555,
            PixelFormat::Argb1555,
            PixelFormat::Argb4444,
            PixelFormat::Xrgb8888,
            PixelFormat::Argb8888,
            PixelFormat::Depth16,
        ] {
            assert_eq!(PixelFormat::from_words(&format.to_words()), Some(format));
        }
        assert_eq!(PixelFormat::Rgb565.to_words(), [32, 0x40, 0, 16, 0xF800, 0x07E0, 0x001F, 0]);
        assert_eq!(PixelFormat::Rgb565.from_argb(0xFFFF_8040), 0xFC08);
        assert_eq!(PixelFormat::Argb1555.from_argb(0x80FF_FFFF), 0xFFFF);
        assert_eq!(PixelFormat::Argb4444.from_argb(0x1234_5678), 0x1357);
        assert_eq!(PixelFormat::Xrgb8888.from_argb(0xFF12_3456), 0x0012_3456);
        assert_eq!(PixelFormat::Rgb565.to_argb(0xFFFF), Some(0xFFFF_FFFF));
        assert_eq!(PixelFormat::Rgb565.to_argb(0x8410), Some(0xFF84_8284));
        assert_eq!(PixelFormat::Argb1555.to_argb(0x8000), Some(0xFF00_0000));
        assert_eq!(PixelFormat::Argb1555.to_argb(0x7FFF), Some(0x00FF_FFFF));
        assert_eq!(PixelFormat::Argb4444.to_argb(0x1357), Some(0x1133_5577));
        assert_eq!(PixelFormat::Xrgb8888.to_argb(0x0012_3456), Some(0xFF12_3456));
        assert_eq!(PixelFormat::Depth16.to_argb(0xFFFF), Some(0xFFFF_FFFF));
        assert_eq!(PixelFormat::Indexed8.to_argb(7), None);
        for format in
            [PixelFormat::Rgb565, PixelFormat::Argb1555, PixelFormat::Argb4444, PixelFormat::Argb8888]
        {
            for raw in [0u32, 0x1234, 0x8001, 0xFFFF, 0xABCD] {
                assert_eq!(format.from_argb(format.to_argb(raw).unwrap()), raw, "{format:?} {raw:#x}");
            }
        }
        assert_eq!(
            (PixelFormat::depth16(1.0), PixelFormat::depth16(0.0), PixelFormat::depth16(7.0)),
            (65535, 0, 65535)
        );
        // 24-bit RGB is not offered.
        assert_eq!(PixelFormat::from_words(&[32, 0x40, 0, 24, 0xFF0000, 0xFF00, 0xFF, 0]), None);
    }

    #[test]
    fn rectangles_must_lie_inside_the_surface() {
        assert!(Rect { x: 600, y: 440, width: 40, height: 40 }.fits(640, 480));
        assert!(!Rect { x: 601, y: 440, width: 40, height: 40 }.fits(640, 480));
        assert!(!Rect { x: 0, y: 0, width: 0, height: 4 }.fits(640, 480));
        assert!(!Rect { x: u32::MAX, y: 0, width: 2, height: 1 }.fits(640, 480));
    }

    fn words(pixels: &[u8]) -> Vec<u16> {
        pixels.chunks(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect()
    }

    #[test]
    fn headless_surfaces_fill_copy_scale_and_key() {
        let mut video = Headless::default();
        let a = video.create_surface(4, 2, PixelFormat::Rgb565);
        let b = video.create_surface(4, 2, PixelFormat::Rgb565);
        video.fill(a, Rect::whole(4, 2), 0x1111);
        video.fill(a, Rect { x: 1, y: 0, width: 2, height: 1 }, 0xF800);
        let mut out = [0u8; 16];
        video.download(a, Rect::whole(4, 2), 8, &mut out);
        assert_eq!(words(&out), [0x1111, 0xF800, 0xF800, 0x1111, 0x1111, 0x1111, 0x1111, 0x1111]);

        // A copy with a colour key leaves the keyed pixels of the target alone.
        video.fill(b, Rect::whole(4, 2), 0x2222);
        video.copy(a, Rect::whole(4, 2), b, Rect::whole(4, 2), Some((0x1111, 0x1111)));
        video.download(b, Rect::whole(4, 2), 8, &mut out);
        assert_eq!(words(&out), [0x2222, 0xF800, 0xF800, 0x2222, 0x2222, 0x2222, 0x2222, 0x2222]);

        // Scaling 2x1 to 4x2 repeats each pixel.
        video.copy(a, Rect { x: 0, y: 0, width: 2, height: 1 }, b, Rect::whole(4, 2), None);
        video.download(b, Rect::whole(4, 2), 8, &mut out);
        assert_eq!(words(&out), [0x1111, 0x1111, 0xF800, 0xF800, 0x1111, 0x1111, 0xF800, 0xF800]);

        video.upload(b, Rect { x: 3, y: 1, width: 1, height: 1 }, 2, &[0xCD, 0xAB]);
        video.present(b);
        assert_eq!(video.frames_presented, 1);
        assert_eq!(words(&video.last_frame.as_ref().unwrap().pixels)[7], 0xABCD);
        video.destroy_surface(a);
        assert_eq!(video.create_surface(1, 1, PixelFormat::Indexed8), a);
    }
}
