//! GDI32: bitmaps, device contexts, copies between them, and text.
//!
//! The game uses GDI for its start-up "Screen Mode Select" window (normally
//! replaced by the `skip-mode-select` modification) and to draw the
//! characters of a font into a bitmap, from which it builds a texture.
//! Text is drawn with a substitute typeface: see [`super::text`].
//!
//! Colours are held as `0x00RRGGBB` at full depth. A real device-dependent
//! bitmap would hold the display's colours (16 bits in the game's modes), so
//! a round trip through one loses nothing here where it would on Windows.

use std::collections::HashMap;
use std::sync::Arc;

use super::text::Font;
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
const DIB_RGB_COLORS: u32 = 0;
const WHITE: u32 = 0x00FF_FFFF;
const FW_DONTCARE: u32 = 0;
const FW_NORMAL: u32 = 400;
/// `tmPitchAndFamily` of the typeface: variable pitch (which is what the
/// `TMPF_FIXED_PITCH` bit means), outlines, TrueType, and the Swiss family.
const PITCH_AND_FAMILY: u8 = 0x01 | 0x02 | 0x04 | 0x20;

/// A `COLORREF` (`0x00BBGGRR`) as `0x00RRGGBB`, and back.
fn swap_red_and_blue(color: u32) -> u32 {
    (color & 0xFF) << 16 | (color & 0xFF00) | (color >> 16 & 0xFF)
}

/// Where a bitmap keeps its pixels.
#[derive(Debug, Clone)]
enum Pixels {
    /// A device-dependent bitmap: pixels held by the runtime, top row first.
    Device(Vec<u32>),
    /// A DIB section: pixels in the program's memory, where the program may
    /// read and write them directly.
    Section(Section),
}

#[derive(Debug, Clone)]
struct Section {
    /// Guest address of the pixels.
    bits: u32,
    /// Bytes from one row to the next.
    stride: u32,
    /// Whether the first stored row is the bottom one.
    bottom_up: bool,
    colors: Colors,
}

/// How the pixels of a DIB section encode colour.
#[derive(Debug, Clone)]
enum Colors {
    /// Three bytes per pixel: blue, green, red.
    Rgb24,
    /// One byte per pixel, an index into a table of `0x00RRGGBB` colours.
    Indexed8(Arc<[u32; 256]>),
}

impl Section {
    /// Rows are padded to 32 bits.
    fn stride(width: u32, depth: u32) -> u32 {
        (width * depth).div_ceil(32) * 4
    }

    fn depth(&self) -> u32 {
        match self.colors {
            Colors::Rgb24 => 24,
            Colors::Indexed8(_) => 8,
        }
    }

    fn address(&self, height: i32, x: i32, y: i32) -> u32 {
        let line = if self.bottom_up { height - 1 - y } else { y } as u32;
        self.bits + line * self.stride + x as u32 * (self.depth() / 8)
    }

    fn read(&self, memory: &mut andy_cpu::memory::Memory, height: i32, x: i32, y: i32) -> u32 {
        let at = self.address(height, x, y);
        match &self.colors {
            Colors::Rgb24 => memory.read(at, 3).unwrap_or(0),
            Colors::Indexed8(table) => table[memory.read_u8(at).unwrap_or(0) as usize],
        }
    }

    fn write(
        &self,
        memory: &mut andy_cpu::memory::Memory,
        height: i32,
        x: i32,
        y: i32,
        color: u32,
    ) -> Option<()> {
        let at = self.address(height, x, y);
        match &self.colors {
            Colors::Rgb24 => memory.load(at, &color.to_le_bytes()[..3]),
            // The table's nearest colour, the first of them if several are
            // equally near.
            Colors::Indexed8(table) => {
                let distance = |entry: &u32| {
                    let channel =
                        |shift: u32| ((entry >> shift & 0xFF) as i32 - (color >> shift & 0xFF) as i32).pow(2);
                    channel(0) + channel(8) + channel(16)
                };
                let index =
                    (0..256).min_by_key(|&index| distance(&table[index])).expect("a table has entries");
                memory.load(at, &[index as u8])
            }
        }
    }
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
    /// A DirectDraw surface, by index. Such a context exists and can be the
    /// model for a compatible one; drawing through it is not implemented.
    Surface(usize),
}

#[derive(Debug, Clone)]
struct DeviceContext {
    target: Target,
    font: u32,
    brush: u32,
    pen: u32,
    text_color: u32,
    background_color: u32,
    background_mode: u32,
}

impl DeviceContext {
    /// A context as Windows hands it out: system font, white brush, black
    /// pen, black text on an opaque white background.
    fn new(target: Target) -> DeviceContext {
        DeviceContext {
            target,
            font: SYSTEM_FONT,
            brush: STOCK_TAG,
            pen: STOCK_TAG | 7,
            text_color: 0,
            background_color: WHITE,
            background_mode: OPAQUE,
        }
    }
}

/// A font object: the substitute font at the size asked for, and whatever
/// was asked that the substitute cannot give.
#[derive(Debug, Clone, Copy)]
struct FontObject {
    font: Font,
    unsupported: Option<&'static str>,
}

#[derive(Debug, Clone)]
enum Object {
    Bitmap(Bitmap),
    Font(FontObject),
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
            Pixels::Section(_) => None,
        }
    }
}

fn not_implemented(what: &str) -> HostResult {
    Err(Stop::Unimplemented { dll: "GDI32.dll".into(), name: what.into() })
}

impl Machine {
    /// Creates a device context for a window's client area.
    pub(crate) fn window_context(&mut self, window: u32) -> u32 {
        self.os.gdi.create(Object::DeviceContext(DeviceContext::new(Target::Window(window))))
    }

    /// Creates a device context for a DirectDraw surface.
    pub(crate) fn surface_context(&mut self, surface: usize) -> u32 {
        self.os.gdi.create(Object::DeviceContext(DeviceContext::new(Target::Surface(surface))))
    }

    /// Stops if a device context draws on a DirectDraw surface.
    fn check_drawable(&mut self, context: u32) -> Result<(), Stop> {
        match self.os.gdi.context(context).map(|context| context.target) {
            Some(Target::Surface(_)) => not_implemented("drawing on a DirectDraw surface").map(|_| ()),
            _ => Ok(()),
        }
    }

    /// The font selected into a device context; `None` if there is no such
    /// context.
    fn selected_font(&mut self, context: u32) -> Result<Option<Font>, Stop> {
        let Some(handle) = self.os.gdi.context(context).map(|context| context.font) else { return Ok(None) };
        match self.os.gdi.object(handle) {
            Some(Object::Font(FontObject { font, unsupported: None })) => Ok(Some(*font)),
            Some(Object::Font(FontObject { unsupported: Some(what), .. })) => {
                let what = format!("text in a font with {what}");
                not_implemented(&what).map(|_| None)
            }
            _ => not_implemented("text in a stock font").map(|_| None),
        }
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
            Target::Bitmap(DEFAULT_BITMAP) | Target::Surface(_) => None,
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
                    Pixels::Section(section) => {
                        section.read(&mut self.memory, bitmap.height, source_x, source_y)
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
            let section = match &bitmap.pixels {
                Pixels::Section(section) => Some(section.clone()),
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
                match &section {
                    None => {
                        if let Some(Bitmap { pixels: Pixels::Device(store), .. }) =
                            self.target_bitmap(context)
                        {
                            store[(target_y * bitmap_width + target_x) as usize] = color;
                        }
                    }
                    Some(section) => {
                        section.write(&mut self.memory, bitmap_height, target_x, target_y, color)?
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
            Pixels::Section(Section { bits, stride, bottom_up, colors: Colors::Rgb24 })
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
    m.check_drawable(target)?;
    m.check_drawable(source)?;
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
        Some(Object::Font(_)) => 1,
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
            Target::Window(_) | Target::Surface(_) => 0,
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
    let (bits, stride, depth) = match &bitmap.pixels {
        Pixels::Section(section) => (section.bits, section.stride, section.depth()),
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
        let header = [
            40,
            bitmap.width as u32,
            bitmap.height as u32,
            1 | depth << 16,
            0,
            stride * bitmap.height as u32,
        ];
        let mut words = [0u32; 21];
        words[..6].copy_from_slice(&basic);
        words[6..12].copy_from_slice(&header);
        return if m.write_words(out, &words) { 84 } else { 0 };
    }
    if capacity >= 24 && m.write_words(out, &basic) { 24 } else { 0 }
}

/// Draws one line of text with its top-left corner at `(x, y)`.
fn text_out(m: &mut Machine, context: u32, x: i32, y: i32, text: u32, count: u32) -> HostResult {
    let Some(font) = m.selected_font(context)? else { return Ok(0) };
    m.check_drawable(context)?;
    let Some(text) = m.memory.dump(text, count) else { return Ok(0) };
    let Some((color, background)) = m.os.gdi.context(context).map(|context| {
        (context.text_color, (context.background_mode == OPAQUE).then_some(context.background_color))
    }) else {
        return Ok(0);
    };
    // The text's box, with room around it for glyphs that reach outside.
    let (cell_width, cell_height, margin) = (font.width(&text), font.height(), font.em);
    let (width, height) = (cell_width + 2 * margin, cell_height + 2 * margin);
    let Some(mut pixels) = m.read_pixels(context, x - margin, y - margin, width, height) else {
        return Ok(0);
    };
    if let Some(background) = background {
        for row in margin..margin + cell_height {
            let start = (row * width + margin) as usize;
            pixels[start..start + cell_width as usize].fill(background);
        }
    }
    font.draw(&text, |glyph_x, glyph_y| {
        let (column, row) = (glyph_x + margin, glyph_y + margin);
        if (0..width).contains(&column) && (0..height).contains(&row) {
            pixels[(row * width + column) as usize] = color;
        }
    });
    Ok(m.write_pixels(context, x - margin, y - margin, width, &pixels).is_some() as u32)
}

/// `CreateDIBSection`: a bitmap whose pixels the program can reach.
fn create_dib_section(m: &mut Machine, a: &[u32]) -> HostResult {
    // Device context, BITMAPINFO, colour usage, where to put the address of
    // the bits, a file mapping and an offset into it.
    let (info, usage, bits_out, mapping) = (a[1], a[2], a[3], a[4]);
    let [size, width, height, planes_and_depth, compression]: [u32; 5] = m.read_words(info);
    let (width, height) = (width as i32, height as i32);
    if size != 40 || width <= 0 || height == 0 || planes_and_depth & 0xFFFF != 1 {
        return Ok(0);
    }
    let depth = planes_and_depth >> 16;
    let colors = match (depth, compression) {
        (24, 0) => Colors::Rgb24,
        (8, 0) if usage == DIB_RGB_COLORS => {
            // The colour table follows the header: as many entries as the
            // header's "colours used" says, or all 256; blue, green, red
            // and a spare byte each.
            let used = match m.memory.read(info + 32, 4) {
                Some(0) => 256,
                Some(count @ 1..=256) => count as usize,
                _ => return Ok(0),
            };
            let mut table = [0; 256];
            for (index, entry) in table.iter_mut().take(used).enumerate() {
                let Some(color) = m.memory.read(info + 40 + 4 * index as u32, 4) else { return Ok(0) };
                *entry = color & 0x00FF_FFFF;
            }
            Colors::Indexed8(Arc::new(table))
        }
        _ => {
            let what = format!(
                "CreateDIBSection of a {width} x {height} bitmap of {depth} bits, compression {compression}, \
                 colour usage {usage}"
            );
            return not_implemented(&what);
        }
    };
    if mapping != 0 {
        return not_implemented("CreateDIBSection over a file mapping");
    }
    let rows = height.unsigned_abs();
    let stride = Section::stride(width as u32, depth);
    let Some(bytes) = stride.checked_mul(rows).filter(|&bytes| bytes <= 1 << 28) else { return Ok(0) };
    let bits = m.os.allocate(&mut m.memory, bytes);
    m.memory.load(bits, &vec![0; bytes as usize]).expect("block was just allocated");
    if bits_out != 0 && m.memory.write(bits_out, 4, bits).is_none() {
        return Ok(0);
    }
    m.note(|| format!("{width} x {rows} at {depth} bits, pixels at {bits:#x}"));
    let pixels = Pixels::Section(Section { bits, stride, bottom_up: height > 0, colors });
    Ok(m.os.gdi.create(Object::Bitmap(Bitmap { width, height: rows as i32, pixels })))
}

/// `GetTextMetricsA`: the measurements of the selected font.
fn get_text_metrics(m: &mut Machine, a: &[u32]) -> HostResult {
    let Some(font) = m.selected_font(a[0])? else { return Ok(0) };
    // TEXTMETRICA: eleven numbers, then nine bytes, padded to 56 bytes.
    let numbers = [
        font.height(),
        font.ascent,
        font.descent,
        font.height() - font.em, // internal leading
        0,                       // external leading
        font.average_width,
        font.max_width,
        FW_NORMAL as i32,
        0,  // overhang
        96, // the display's dots per inch, across and down
        96,
    ];
    // First, last, default and break characters, as Windows reports them
    // for a TrueType font in this code page (UNVERIFIED on Windows 98);
    // not italic, underlined or struck out; pitch and family; ANSI.
    let bytes = [0x20, 0xFF, 0x1F, 0x20, 0, 0, 0, PITCH_AND_FAMILY, 0];
    let mut block = Vec::with_capacity(56);
    numbers.iter().for_each(|number| block.extend_from_slice(&number.to_le_bytes()));
    block.extend_from_slice(&bytes);
    block.resize(56, 0);
    Ok(m.memory.load(a[1], &block).is_some() as u32)
}

functions! { "GDI32.dll";
    GetStockObject(1) => |_, a| if a[0] <= 19 { STOCK_TAG | a[0] } else { 0 },
    CreateCompatibleDC(1) => |m, _| {
        m.os.gdi.create(Object::DeviceContext(DeviceContext::new(Target::Bitmap(DEFAULT_BITMAP))))
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
        // Height, width, escapement, orientation, weight, italic, underline,
        // strike-out, character set, three quality settings, pitch and
        // family, face name. The face is always the substitute one.
        let face = m.text(a[13]);
        let font = Font::with_height(a[0] as i32);
        m.note(|| format!("{face:?}, height {}: {} pixels per em", a[0] as i32, font.em));
        let unsupported = if a[1] != 0 {
            Some("a set width")
        } else if a[2] != 0 || a[3] != 0 {
            Some("rotation")
        } else if a[4] != FW_DONTCARE && a[4] != FW_NORMAL {
            Some("a weight other than normal")
        } else if a[5] & 0xFF != 0 || a[6] & 0xFF != 0 || a[7] & 0xFF != 0 {
            Some("italics, underline or strike-out")
        } else {
            None
        };
        m.os.gdi.create(Object::Font(FontObject { font, unsupported }))
    },
    CreateDIBSection(6) => create_dib_section,
    GetDIBColorTable(4) => |m, a| {
        // Device context, first entry, how many, where to: the colour table
        // of the DIB section selected into the context. Returns how many
        // entries were copied; a bitmap without a table has none.
        let (first, count, out) = (a[1] as usize, a[2] as usize, a[3]);
        let table = match m.target_bitmap(a[0]).map(|bitmap| &bitmap.pixels) {
            Some(Pixels::Section(Section { colors: Colors::Indexed8(table), .. })) => table.clone(),
            _ => return 0,
        };
        let entries = table.get(first..).map_or(&[][..], |rest| &rest[..count.min(rest.len())]);
        if m.write_words(out, entries) { entries.len() as u32 } else { 0 }
    },
    SelectObject(2) => select_object,
    DeleteObject(1) => |m, a| {
        // An object still selected into a device context cannot be deleted.
        let selected = m.os.gdi.objects.iter().flatten().any(|object| match object {
            Object::DeviceContext(c) => c.target == Target::Bitmap(a[0]) || c.font == a[0],
            _ => false,
        });
        match m.os.gdi.slot(a[0]) {
            Some(slot @ Some(Object::Bitmap(_) | Object::Font(_))) if !selected => {
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
        let color = swap_red_and_blue(a[1]);
        m.os.gdi.context(a[0]).map_or(CLR_INVALID, |context| {
            swap_red_and_blue(std::mem::replace(&mut context.text_color, color))
        })
    },
    SetBkColor(2) => |m, a| {
        let color = swap_red_and_blue(a[1]);
        m.os.gdi.context(a[0]).map_or(CLR_INVALID, |context| {
            swap_red_and_blue(std::mem::replace(&mut context.background_color, color))
        })
    },
    GetTextMetricsA(2) => get_text_metrics,
    GetTextExtentPoint32A(4) => |m, a| -> HostResult {
        let Some(font) = m.selected_font(a[0])? else { return Ok(0) };
        let Some(text) = m.memory.dump(a[1], a[2]) else { return Ok(0) };
        Ok(m.write_words(a[3], &[font.width(&text) as u32, font.height() as u32]) as u32)
    },
    TextOutA(5) => |m, a| text_out(m, a[0], a[1] as i32, a[2] as i32, a[3], a[4]),
    ExtTextOutA(8) => |m, a| -> HostResult {
        // Context, position, options, clipping rectangle, text, count, spacing.
        if a[3] != 0 || a[7] != 0 {
            return not_implemented("ExtTextOutA with options or its own spacing");
        }
        text_out(m, a[0], a[1] as i32, a[2] as i32, a[5], a[6])
    },
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

    /// A memory device context drawing on a new 24-bit section of the given
    /// size, with a font of the given height selected; and the section's bits.
    fn text_context(m: &mut Machine, width: u32, height: u32, font_height: u32) -> (u32, u32) {
        let info = m.scratch(40);
        assert!(m.write_words(info, &[40, width, height, 1 | 24 << 16, 0]));
        let bits_out = m.scratch(4);
        let section = m.host("GDI32.dll", "CreateDIBSection", &[0, info, 0, bits_out, 0, 0]);
        assert_ne!(section, 0);
        let context = m.host("GDI32.dll", "CreateCompatibleDC", &[0]);
        assert_eq!(m.host("GDI32.dll", "SelectObject", &[context, section]), DEFAULT_BITMAP);
        let face = m.scratch(8);
        let font =
            m.host("GDI32.dll", "CreateFontA", &[font_height, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, face]);
        assert_eq!(m.host("GDI32.dll", "SelectObject", &[context, font]), SYSTEM_FONT);
        (context, m.memory.read(bits_out, 4).unwrap())
    }

    #[test]
    fn text_is_measured_and_drawn_into_a_section() {
        let mut m = testing::machine();
        let (width, height) = (64u32, 32u32);
        let (context, bits) = text_context(&mut m, width, height, 20);
        // A positive height makes a bottom-up section: its first stored row is the last one.
        let pixel = |m: &mut Machine, x: u32, y: u32| {
            m.memory.read(bits + (height - 1 - y) * width * 3 + 3 * x, 3).unwrap()
        };

        let metrics = m.scratch(56);
        assert_eq!(m.host("GDI32.dll", "GetTextMetricsA", &[context, metrics]), 1);
        let words: [u32; 11] = m.read_words(metrics);
        assert_eq!(words[..5], [20, 16, 4, 2, 0]);
        assert_eq!(words[7..], [400, 0, 96, 96]);
        assert_eq!(
            m.memory.dump(metrics + 44, 12).unwrap(),
            [0x20, 0xFF, 0x1F, 0x20, 0, 0, 0, 0x27, 0, 0, 0, 0]
        );

        let text = m.scratch(4);
        m.memory.load(text, b"Hi").unwrap();
        let size = m.scratch(8);
        assert_eq!(m.host("GDI32.dll", "GetTextExtentPoint32A", &[context, text, 2, size]), 1);
        let [text_width, text_height]: [u32; 2] = m.read_words(size);
        assert_eq!(text_height, 20);
        assert!((10..30).contains(&text_width), "{text_width}");

        // Red text on an opaque blue background (colours are 0x00BBGGRR).
        assert_eq!(m.host("GDI32.dll", "SetTextColor", &[context, 0x0000_00FF]), 0);
        assert_eq!(m.host("GDI32.dll", "SetBkColor", &[context, 0x00FF_0000]), 0x00FF_FFFF);
        assert_eq!(m.host("GDI32.dll", "TextOutA", &[context, 5, 3, text, 2]), 1);
        let (mut red, mut blue, mut other) = (0, 0, 0);
        for y in 0..height {
            for x in 0..width {
                let inside = (5..5 + text_width).contains(&x) && (3..3 + text_height).contains(&y);
                match pixel(&mut m, x, y) {
                    0x00FF_0000 if inside => red += 1,
                    0x0000_00FF if inside => blue += 1,
                    0 if !inside => {}
                    _ => other += 1,
                }
            }
        }
        assert_eq!(other, 0);
        assert_eq!(red + blue, text_width * text_height);
        assert!(red > 30, "{red} pixels of text");

        // A transparent background leaves what was there.
        assert_eq!(m.host("GDI32.dll", "SetBkMode", &[context, TRANSPARENT]), OPAQUE);
        assert_eq!(m.host("GDI32.dll", "SetTextColor", &[context, 0x0000_FF00]), 0x0000_00FF);
        assert_eq!(m.host("GDI32.dll", "TextOutA", &[context, 5, 3, text, 2]), 1);
        let mut green = 0;
        for y in 0..height {
            for x in 0..width {
                match pixel(&mut m, x, y) {
                    0x0000_FF00 => green += 1,
                    color => assert!(color == 0 || color == 0x0000_00FF, "{color:#x} at {x},{y}"),
                }
            }
        }
        assert_eq!(green, red);
    }

    #[test]
    fn text_in_a_font_the_substitute_cannot_give_stops_the_machine() {
        let mut m = testing::machine();
        let context = m.host("GDI32.dll", "CreateCompatibleDC", &[0]);
        let face = m.scratch(8);
        let bold = m.host("GDI32.dll", "CreateFontA", &[20, 0, 0, 0, 700, 0, 0, 0, 0, 0, 0, 0, 0, face]);
        assert_eq!(m.host("GDI32.dll", "SelectObject", &[context, bold]), SYSTEM_FONT);
        let metrics = m.scratch(56);
        let function = crate::win32::lookup("GDI32.dll", "GetTextMetricsA").unwrap();
        assert!(matches!((function.handler)(&mut m, &[context, metrics]), Err(Stop::Unimplemented { .. })));
    }

    #[test]
    fn an_indexed_section_reads_through_its_colour_table_and_writes_the_nearest_entry() {
        let mut m = testing::machine();
        // A 4x2 top-down 8-bit section with three colours: black, red, pale blue.
        let info = m.scratch(40 + 4 * 3);
        assert!(m.write_words(info, &[40, 4, -2i32 as u32, 1 | 8 << 16, 0, 0, 0, 0, 3, 0]));
        assert!(m.write_words(info + 40, &[0, 0x00FF_0000, 0x0080_80FF]));
        let bits_out = m.scratch(4);
        let section = m.host("GDI32.dll", "CreateDIBSection", &[0, info, DIB_RGB_COLORS, bits_out, 0, 0]);
        assert_ne!(section, 0);
        let bits = m.memory.read(bits_out, 4).unwrap();
        m.memory.load(bits, &[0, 1, 2, 1, 2, 2, 0, 0]).unwrap();

        let out = m.scratch(84);
        assert_eq!(m.host("GDI32.dll", "GetObjectA", &[section, 84, out]), 84);
        let words: [u32; 8] = m.read_words(out);
        assert_eq!(words[1..6], [4, 2, 4, 1 | 8 << 16, bits]);

        let from = m.host("GDI32.dll", "CreateCompatibleDC", &[0]);
        let to = m.host("GDI32.dll", "CreateCompatibleDC", &[0]);
        let target = m.host("GDI32.dll", "CreateCompatibleBitmap", &[to, 4, 2]);
        m.host("GDI32.dll", "SelectObject", &[from, section]);
        m.host("GDI32.dll", "SelectObject", &[to, target]);
        assert_eq!(m.host("GDI32.dll", "BitBlt", &[to, 0, 0, 4, 2, from, 0, 0, SRCCOPY]), 1);
        assert_eq!(
            m.read_pixels(to, 0, 0, 4, 2).unwrap(),
            [0, 0x00FF_0000, 0x0080_80FF, 0x00FF_0000, 0x0080_80FF, 0x0080_80FF, 0, 0]
        );

        let table = m.scratch(16);
        assert_eq!(m.host("GDI32.dll", "GetDIBColorTable", &[from, 1, 4, table]), 4);
        assert_eq!(m.read_words::<4>(table), [0x00FF_0000, 0x0080_80FF, 0, 0]);
        assert_eq!(m.host("GDI32.dll", "GetDIBColorTable", &[from, 254, 4, table]), 2);
        assert_eq!(m.host("GDI32.dll", "GetDIBColorTable", &[to, 0, 4, table]), 0);

        // Writing picks the nearest colour of the table.
        m.write_pixels(from, 0, 0, 4, &[0x00F0_1010, 0x0090_90F0, 0x0010_0010, 0x00FF_FFFF]).unwrap();
        assert_eq!(m.memory.dump(bits, 4).unwrap(), [1, 2, 0, 2]);
    }
}
