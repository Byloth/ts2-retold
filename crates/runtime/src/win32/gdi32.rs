//! GDI32: bitmaps, device contexts and copies between them.
//!
//! The game uses GDI for its start-up "Screen Mode Select" window and for
//! debug text. Bitmaps and copies are implemented; **text is not drawn**:
//! it needs the fonts of Windows, which cannot be shipped, so text output
//! stops the machine. The window that needs it is normally replaced by the
//! `skip-mode-select` modification.
//!
//! Colours are held as `0x00RRGGBB` at full depth. A real device-dependent
//! bitmap would hold the display's colours (16 bits in the game's modes), so
//! a round trip through one loses nothing here where it would on Windows.

use std::collections::HashMap;

use super::user32::Window;
use super::{HostResult, functions};
use crate::files::Disposition;
use crate::{Machine, Stop};

/// Handles of stock objects: this tag with the stock index in the low 12 bits.
const STOCK_TAG: u32 = 0x0180_0000;
const SYSTEM_FONT: u32 = STOCK_TAG | 13;
/// The 1x1 bitmap every memory device context starts with.
const DEFAULT_BITMAP: u32 = STOCK_TAG | 0x100;
const FIRST_OBJECT: u32 = 0x0A00_0000;

const SRCCOPY: u32 = 0x00CC_0020;
const TRANSPARENT: u32 = 1;
const OPAQUE: u32 = 2;
const CLR_INVALID: u32 = u32::MAX;
const CBM_INIT: u32 = 4;

/// Where a bitmap keeps its pixels.
#[derive(Debug, Clone)]
enum Pixels {
    /// A device-dependent bitmap: pixels held by the runtime, top row first.
    Device(Vec<u32>),
    /// A DIB section: 24-bit pixels in the program's memory, where the
    /// program may read and write them directly.
    Section { bits: u32, stride: u32, bottom_up: bool },
}

#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: i32,
    pub height: i32,
    pixels: Pixels,
}

impl Bitmap {
    fn device(width: i32, height: i32) -> Bitmap {
        let (width, height) = (width.max(1), height.max(1));
        Bitmap { width, height, pixels: Pixels::Device(vec![0; width as usize * height as usize]) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The client area of a window.
    Window(u32),
    /// The bitmap selected into a memory device context.
    Bitmap(u32),
}

#[derive(Debug, Clone)]
struct DeviceContext {
    target: Target,
    font: u32,
    brush: u32,
    pen: u32,
    text_color: u32,
    background_mode: u32,
}

#[derive(Debug, Clone)]
enum Object {
    Bitmap(Bitmap),
    /// A font. Nothing about it is kept, since text is not drawn.
    Font,
    DeviceContext(DeviceContext),
}

#[derive(Debug, Default)]
pub struct Gdi {
    objects: Vec<Option<Object>>,
    /// What each window shows in its client area, created when first drawn to.
    window_surfaces: HashMap<u32, Bitmap>,
}

impl Gdi {
    fn create(&mut self, object: Object) -> u32 {
        let index = self.objects.iter().position(Option::is_none).unwrap_or_else(|| {
            self.objects.push(None);
            self.objects.len() - 1
        });
        self.objects[index] = Some(object);
        FIRST_OBJECT + 4 * index as u32
    }

    fn slot(&mut self, handle: u32) -> Option<&mut Option<Object>> {
        let offset = handle.checked_sub(FIRST_OBJECT)?;
        self.objects.get_mut((offset / 4) as usize).filter(|_| offset % 4 == 0)
    }

    fn object(&mut self, handle: u32) -> Option<&mut Object> {
        self.slot(handle)?.as_mut()
    }

    fn context(&mut self, handle: u32) -> Option<&mut DeviceContext> {
        match self.object(handle)? {
            Object::DeviceContext(context) => Some(context),
            _ => None,
        }
    }

    fn bitmap(&mut self, handle: u32) -> Option<&mut Bitmap> {
        match self.object(handle)? {
            Object::Bitmap(bitmap) => Some(bitmap),
            _ => None,
        }
    }

    /// The pixels a window shows, if anything was drawn to it.
    pub fn window_surface(&self, window: u32) -> Option<(&[u32], i32, i32)> {
        let bitmap = self.window_surfaces.get(&window)?;
        match &bitmap.pixels {
            Pixels::Device(pixels) => Some((pixels, bitmap.width, bitmap.height)),
            Pixels::Section { .. } => None,
        }
    }
}

fn not_implemented(what: &str) -> HostResult {
    Err(Stop::Unimplemented { dll: "GDI32.dll".into(), name: what.into() })
}

impl Machine {
    /// Creates a device context for a window's client area.
    pub(crate) fn window_context(&mut self, window: u32) -> u32 {
        self.os.gdi.create(Object::DeviceContext(DeviceContext {
            target: Target::Window(window),
            font: SYSTEM_FONT,
            brush: STOCK_TAG,
            pen: STOCK_TAG | 7,
            text_color: 0,
            background_mode: OPAQUE,
        }))
    }

    pub(crate) fn delete_context(&mut self, handle: u32) -> bool {
        match self.os.gdi.slot(handle) {
            Some(slot @ Some(Object::DeviceContext(_))) => {
                *slot = None;
                true
            }
            _ => false,
        }
    }

    /// The bitmap a device context draws on. A window's surface is created
    /// at the size of its client area the first time it is needed.
    fn target_bitmap(&mut self, context: u32) -> Option<&mut Bitmap> {
        match self.os.gdi.context(context)?.target {
            Target::Bitmap(DEFAULT_BITMAP) => None,
            Target::Bitmap(handle) => self.os.gdi.bitmap(handle),
            Target::Window(window) => {
                let (width, height) = self.os.user.window(window).map(Window::client_size)?;
                let surface = self
                    .os
                    .gdi
                    .window_surfaces
                    .entry(window)
                    .or_insert_with(|| Bitmap::device(width, height));
                if (surface.width, surface.height) != (width.max(1), height.max(1)) {
                    *surface = Bitmap::device(width, height);
                }
                Some(surface)
            }
        }
    }

    /// Reads a rectangle of a device context's bitmap as rows of `0x00RRGGBB`.
    /// Pixels outside the bitmap read as black.
    fn read_pixels(&mut self, context: u32, x: i32, y: i32, width: i32, height: i32) -> Option<Vec<u32>> {
        let bitmap = self.target_bitmap(context)?.clone();
        let mut out = vec![0; width as usize * height as usize];
        for row in 0..height {
            let source_y = y + row;
            if source_y < 0 || source_y >= bitmap.height {
                continue;
            }
            for column in 0..width {
                let source_x = x + column;
                if source_x < 0 || source_x >= bitmap.width {
                    continue;
                }
                out[(row * width + column) as usize] = match &bitmap.pixels {
                    Pixels::Device(pixels) => pixels[(source_y * bitmap.width + source_x) as usize],
                    Pixels::Section { bits, stride, bottom_up } => {
                        let line = if *bottom_up { bitmap.height - 1 - source_y } else { source_y } as u32;
                        let at = bits + line * stride + 3 * source_x as u32;
                        // Stored blue, green, red.
                        self.memory.read(at, 3).unwrap_or(0)
                    }
                };
            }
        }
        Some(out)
    }

    /// Writes rows of `0x00RRGGBB` to a device context's bitmap, clipped to it.
    fn write_pixels(&mut self, context: u32, x: i32, y: i32, width: i32, pixels: &[u32]) -> Option<()> {
        let (bitmap_width, bitmap_height, section) = {
            let bitmap = self.target_bitmap(context)?;
            let section = match bitmap.pixels {
                Pixels::Section { bits, stride, bottom_up } => Some((bits, stride, bottom_up)),
                Pixels::Device(_) => None,
            };
            (bitmap.width, bitmap.height, section)
        };
        for (row, line) in pixels.chunks(width as usize).enumerate() {
            let target_y = y + row as i32;
            if target_y < 0 || target_y >= bitmap_height {
                continue;
            }
            for (column, &color) in line.iter().enumerate() {
                let target_x = x + column as i32;
                if target_x < 0 || target_x >= bitmap_width {
                    continue;
                }
                match section {
                    None => {
                        if let Some(Bitmap { pixels: Pixels::Device(store), .. }) =
                            self.target_bitmap(context)
                        {
                            store[(target_y * bitmap_width + target_x) as usize] = color;
                        }
                    }
                    Some((bits, stride, bottom_up)) => {
                        let line = if bottom_up { bitmap_height - 1 - target_y } else { target_y } as u32;
                        let at = bits + line * stride + 3 * target_x as u32;
                        for byte in 0..3 {
                            self.memory.write(at + byte, 1, color >> (8 * byte))?;
                        }
                    }
                }
            }
        }
        Some(())
    }

    /// Loads a `.bmp` file as a bitmap object.
    pub(crate) fn load_bitmap_file(&mut self, path: &str, as_section: bool) -> HostResult {
        let Some(file) = self.os.files.open(path, false, Disposition::OpenExisting) else {
            return Ok(0);
        };
        let data = self.os.files.read(file, u32::MAX).map(<[u8]>::to_vec).unwrap_or_default();
        self.os.files.close(file);
        let word = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        // File header (14 bytes), then a 40-byte information header.
        let header = (data.get(..2) == Some(b"BM"))
            .then(|| (word(10), word(14), word(18), word(22), word(26), word(30)));
        let Some((Some(offset), Some(40), Some(width), Some(height), Some(planes_bits), Some(compression))) =
            header
        else {
            return Ok(0);
        };
        let (width, height) = (width as i32, height as i32);
        if planes_bits >> 16 != 24 || compression != 0 || width <= 0 || height == 0 {
            return not_implemented("LoadImageA of a bitmap that is not 24-bit uncompressed");
        }
        let (bottom_up, rows) = (height > 0, height.unsigned_abs());
        let stride = (width as u32 * 3 + 3) & !3;
        let Some(source) = data.get(offset as usize..(offset + stride * rows) as usize) else {
            return Ok(0);
        };
        let pixels = if as_section {
            let bits = self.os.allocate(&mut self.memory, stride * rows);
            self.memory.load(bits, source).expect("block was just allocated");
            Pixels::Section { bits, stride, bottom_up }
        } else {
            let mut pixels = Vec::with_capacity((width as u32 * rows) as usize);
            for row in 0..rows {
                let line = if bottom_up { rows - 1 - row } else { row };
                let start = (line * stride) as usize;
                pixels.extend(
                    source[start..start + 3 * width as usize]
                        .chunks(3)
                        .map(|bgr| u32::from_le_bytes([bgr[0], bgr[1], bgr[2], 0])),
                );
            }
            Pixels::Device(pixels)
        };
        Ok(self.os.gdi.create(Object::Bitmap(Bitmap { width, height: rows as i32, pixels })))
    }
}

fn copy(m: &mut Machine, a: &[u32], source_size: Option<(i32, i32)>) -> HostResult {
    let (target, x, y, width, height) = (a[0], a[1] as i32, a[2] as i32, a[3] as i32, a[4] as i32);
    let (source, source_x, source_y) = (a[5], a[6] as i32, a[7] as i32);
    let operation = a[a.len() - 1];
    if operation != SRCCOPY {
        return not_implemented("a raster operation other than SRCCOPY");
    }
    let (source_width, source_height) = source_size.unwrap_or((width, height));
    if width <= 0 || height <= 0 || source_width <= 0 || source_height <= 0 {
        return not_implemented("a mirrored or empty copy");
    }
    let Some(pixels) = m.read_pixels(source, source_x, source_y, source_width, source_height) else {
        return Ok(0);
    };
    let scaled = if (source_width, source_height) == (width, height) {
        pixels
    } else {
        stretch(&pixels, (source_width, source_height), (width, height))
    };
    Ok(m.write_pixels(target, x, y, width, &scaled).is_some() as u32)
}

/// Scales with GDI's default mode ("black on white"): enlarging repeats
/// pixels; shrinking combines the pixels that fall together with AND.
fn stretch(
    pixels: &[u32],
    (source_width, source_height): (i32, i32),
    (width, height): (i32, i32),
) -> Vec<u32> {
    let span = |index: i32, from: i32, to: i32| {
        // Source positions covered by target position `index`.
        let start = (index as i64 * from as i64 / to as i64) as i32;
        let end = (((index + 1) as i64 * from as i64 + to as i64 - 1) / to as i64) as i32;
        start..end.clamp(start + 1, from)
    };
    let mut out = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            let mut color = 0x00FF_FFFF;
            for source_y in span(y, source_height, height) {
                for source_x in span(x, source_width, width) {
                    color &= pixels[(source_y * source_width + source_x) as usize];
                }
            }
            out.push(color);
        }
    }
    out
}

fn select_object(m: &mut Machine, a: &[u32]) -> u32 {
    let (context_handle, object) = (a[0], a[1]);
    let is_stock = object & !0xFFF == STOCK_TAG;
    let kind = match m.os.gdi.object(object) {
        Some(Object::Bitmap(_)) => 0,
        Some(Object::Font) => 1,
        None if object == DEFAULT_BITMAP => 0,
        // Stock objects 10 to 17 are fonts; 0 to 5 brushes; 6 to 8 pens.
        None if is_stock && (10..=17).contains(&(object & 0xFF)) => 1,
        None if is_stock && (object & 0xFF) <= 5 => 2,
        None if is_stock && (6..=8).contains(&(object & 0xFF)) => 3,
        _ => return 0,
    };
    let Some(context) = m.os.gdi.context(context_handle) else {
        return 0;
    };
    match kind {
        0 => match context.target {
            // Only a memory device context holds a bitmap.
            Target::Bitmap(previous) => {
                context.target = Target::Bitmap(object);
                previous
            }
            Target::Window(_) => 0,
        },
        1 => std::mem::replace(&mut context.font, object),
        2 => std::mem::replace(&mut context.brush, object),
        _ => std::mem::replace(&mut context.pen, object),
    }
}

fn get_object(m: &mut Machine, a: &[u32]) -> u32 {
    let (handle, capacity, out) = (a[0], a[1], a[2]);
    let Some(bitmap) = m.os.gdi.bitmap(handle).map(|b| b.clone()) else {
        return 0;
    };
    let (bits, stride, depth) = match bitmap.pixels {
        Pixels::Section { bits, stride, .. } => (bits, stride, 24),
        // Rows of a device-dependent bitmap are padded to 16 bits.
        Pixels::Device(_) => {
            let depth = m.os.user.display.bits_per_pixel;
            (0, (bitmap.width as u32 * depth).div_ceil(16) * 2, depth)
        }
    };
    // BITMAP: type, width, height, bytes per row, planes and depth (16 bits each), bits.
    let basic = [0, bitmap.width as u32, bitmap.height as u32, stride, 1 | depth << 16, bits];
    if out == 0 {
        return 24;
    }
    if capacity >= 84 && bits != 0 {
        // DIBSECTION: the BITMAP, a BITMAPINFOHEADER, three colour masks, a
        // section handle and an offset.
        let header =
            [40, bitmap.width as u32, bitmap.height as u32, 1 | 24 << 16, 0, stride * bitmap.height as u32];
        let mut words = [0u32; 21];
        words[..6].copy_from_slice(&basic);
        words[6..12].copy_from_slice(&header);
        return if m.write_words(out, &words) { 84 } else { 0 };
    }
    if capacity >= 24 && m.write_words(out, &basic) { 24 } else { 0 }
}

fn text_out(m: &mut Machine, context: u32) -> HostResult {
    if m.os.gdi.context(context).is_none() {
        return Ok(0);
    }
    not_implemented("drawing text")
}

functions! { "GDI32.dll";
    GetStockObject(1) => |_, a| if a[0] <= 19 { STOCK_TAG | a[0] } else { 0 },
    CreateCompatibleDC(1) => |m, _| {
        m.os.gdi.create(Object::DeviceContext(DeviceContext {
            target: Target::Bitmap(DEFAULT_BITMAP),
            font: SYSTEM_FONT,
            brush: STOCK_TAG,
            pen: STOCK_TAG | 7,
            text_color: 0,
            background_mode: OPAQUE,
        }))
    },
    DeleteDC(1) => |m, a| m.delete_context(a[0]) as u32,
    CreateCompatibleBitmap(3) => |m, a| {
        if m.os.gdi.context(a[0]).is_none() {
            return 0;
        }
        m.os.gdi.create(Object::Bitmap(Bitmap::device(a[1] as i32, a[2] as i32)))
    },
    CreateDIBitmap(6) => |m, a| -> HostResult {
        // Device context, BITMAPINFOHEADER, initialisation flag, bits, their
        // BITMAPINFO, colour usage. Without initialisation only the size counts.
        if a[2] & CBM_INIT != 0 {
            return not_implemented("CreateDIBitmap with initial bits");
        }
        let [size, width, height]: [u32; 3] = m.read_words(a[1]);
        if m.os.gdi.context(a[0]).is_none() || size != 40 {
            return Ok(0);
        }
        Ok(m.os.gdi.create(Object::Bitmap(Bitmap::device(width as i32, (height as i32).abs()))))
    },
    CreateFontA(14) => |m, a| {
        let face = m.text(a[13]);
        m.note(|| format!("{face:?}, height {}", a[0] as i32));
        m.os.gdi.create(Object::Font)
    },
    SelectObject(2) => select_object,
    DeleteObject(1) => |m, a| {
        // An object still selected into a device context cannot be deleted.
        let selected = m.os.gdi.objects.iter().flatten().any(|object| match object {
            Object::DeviceContext(c) => c.target == Target::Bitmap(a[0]) || c.font == a[0],
            _ => false,
        });
        match m.os.gdi.slot(a[0]) {
            Some(slot @ Some(Object::Bitmap(_) | Object::Font)) if !selected => {
                *slot = None;
                1
            }
            _ => 0,
        }
    },
    GetObjectA(3) => get_object,
    BitBlt(9) => |m, a| copy(m, a, None),
    StretchBlt(11) => |m, a| copy(m, a, Some((a[8] as i32, a[9] as i32))),
    SetBkMode(2) => |m, a| {
        if a[1] != TRANSPARENT && a[1] != OPAQUE {
            return 0;
        }
        m.os.gdi.context(a[0]).map_or(0, |context| std::mem::replace(&mut context.background_mode, a[1]))
    },
    SetTextColor(2) => |m, a| {
        // COLORREF is 0x00BBGGRR.
        let color = (a[1] & 0xFF) << 16 | (a[1] & 0xFF00) | (a[1] >> 16 & 0xFF);
        m.os.gdi.context(a[0]).map_or(CLR_INVALID, |context| {
            let previous = std::mem::replace(&mut context.text_color, color);
            (previous & 0xFF) << 16 | (previous & 0xFF00) | (previous >> 16 & 0xFF)
        })
    },
    TextOutA(5) => |m, a| text_out(m, a[0]),
    ExtTextOutA(8) => |m, a| text_out(m, a[0]),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    #[test]
    fn stretching_repeats_when_enlarging_and_ands_when_shrinking() {
        let source = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0x00FF_FFFF];
        assert_eq!(
            stretch(&source, (4, 1), (8, 1)),
            [
                0x00FF_0000,
                0x00FF_0000,
                0x0000_FF00,
                0x0000_FF00,
                0x0000_00FF,
                0x0000_00FF,
                0x00FF_FFFF,
                0x00FF_FFFF
            ]
        );
        assert_eq!(stretch(&source, (4, 1), (2, 1)), [0, 0x0000_00FF]);
        assert_eq!(stretch(&[1, 2, 3, 4], (2, 2), (2, 2)), [1, 2, 3, 4]);
    }

    /// A 2x2 bottom-up 24-bit bitmap file: red, green on top; blue, white below.
    fn bitmap_file() -> Vec<u8> {
        let mut file = vec![b'B', b'M'];
        file.extend_from_slice(&[0; 8]);
        file.extend_from_slice(&54u32.to_le_bytes());
        for word in [40u32, 2, 2, 1 | 24 << 16, 0, 16, 0, 0, 0, 0] {
            file.extend_from_slice(&word.to_le_bytes());
        }
        file.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]); // bottom row: blue, white, padding
        file.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]); // top row: red, green, padding
        file
    }

    #[test]
    fn loads_a_bitmap_and_copies_it_between_contexts() {
        let mut m = testing::machine();
        m.os.files.inject("test.bmp", bitmap_file());
        let section = m.load_bitmap_file("test.bmp", true).unwrap();
        assert_ne!(section, 0);

        // The DIB section describes itself and exposes its bits.
        let out = m.scratch(84);
        assert_eq!(m.host("GDI32.dll", "GetObjectA", &[section, 84, out]), 84);
        let words: [u32; 8] = m.read_words(out);
        assert_eq!(words[1..5], [2, 2, 8, 1 | 24 << 16]);
        assert_eq!(m.memory.read(words[5], 3), Some(0x0000_00FF)); // first stored pixel: blue
        assert_eq!(words[6], 40);

        let from = m.host("GDI32.dll", "CreateCompatibleDC", &[0]);
        let to = m.host("GDI32.dll", "CreateCompatibleDC", &[0]);
        let target = m.host("GDI32.dll", "CreateCompatibleBitmap", &[to, 4, 2]);
        assert_eq!(m.host("GDI32.dll", "SelectObject", &[from, section]), DEFAULT_BITMAP);
        assert_eq!(m.host("GDI32.dll", "SelectObject", &[to, target]), DEFAULT_BITMAP);
        assert_eq!(m.host("GDI32.dll", "DeleteObject", &[target]), 0);
        assert_eq!(m.host("GDI32.dll", "StretchBlt", &[to, 0, 0, 4, 2, from, 0, 0, 2, 2, SRCCOPY]), 1);
        assert_eq!(
            m.read_pixels(to, 0, 0, 4, 2).unwrap(),
            [
                0x00FF_0000,
                0x00FF_0000,
                0x0000_FF00,
                0x0000_FF00,
                0x0000_00FF,
                0x0000_00FF,
                0x00FF_FFFF,
                0x00FF_FFFF
            ]
        );
        // Writing through a context reaches the section's memory.
        assert_eq!(m.host("GDI32.dll", "BitBlt", &[from, 0, 0, 1, 1, to, 3, 1, SRCCOPY]), 1);
        assert_eq!(m.memory.read(words[5] + 8, 3), Some(0x00FF_FFFF)); // top-left, second stored row

        assert_eq!(m.host("GDI32.dll", "SelectObject", &[to, DEFAULT_BITMAP]), target);
        assert_eq!(m.host("GDI32.dll", "DeleteObject", &[target]), 1);
        assert_eq!(m.host("GDI32.dll", "DeleteDC", &[to]), 1);
        assert_eq!(m.host("GDI32.dll", "DeleteDC", &[to]), 0);
    }
}
