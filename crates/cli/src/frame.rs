//! The window's frame, drawn by the program: a title bar with the title and
//! three buttons, and a thin border.
//!
//! Where the window system draws no frame (Wayland without server-side
//! decorations) a program has to bring its own. The usual library for it
//! puts the frame in sub-surfaces around the window, which WSLg's
//! compositor shows wrongly and sometimes dies of (`docs/BACKLOG.md`); this
//! frame is part of the window's own picture instead.
//!
//! This module knows the frame's geometry and draws the title bar as
//! pixels; the window loop turns what the pointer does on it into moving,
//! resizing, minimising, maximising and closing.

use std::sync::LazyLock;

/// Sizes in logical pixels (multiplied by the display's scale factor).
const BAR_HEIGHT: f64 = 36.0;
const BUTTON_WIDTH: f64 = 46.0;
/// How far inside the window's edge the pointer resizes it.
const EDGE: f64 = 6.0;
const TITLE_SIZE: f64 = 14.0;
const TITLE_LEFT: f64 = 14.0;
const ICON: f64 = 10.0;
const STROKE: f64 = 1.2;
/// The room each cloud has along the bar.
const CLOUD_SPACING: f64 = 84.0;

/// Colours as red, green, blue.
type Rgb = [u8; 3];

/// The colours of the frame: a clear sky with white clouds, a drawing of
/// this project's own. With the keyboard elsewhere the sky is overcast.
struct Palette {
    sky_top: Rgb,
    sky_bottom: Rgb,
    /// The line under the bar, and the border.
    edge: Rgb,
    cloud: Rgb,
    /// How opaque the clouds are at most.
    cloud_cover: f64,
    text: Rgb,
    shadow: Rgb,
}
const ACTIVE: Palette = Palette {
    sky_top: [0x4C, 0xA4, 0xEE],
    sky_bottom: [0x2A, 0x7B, 0xD6],
    edge: [0x1B, 0x56, 0xA3],
    cloud: [0xFF, 0xFF, 0xFF],
    cloud_cover: 0.92,
    text: [0xFF, 0xFF, 0xFF],
    shadow: [0x12, 0x3E, 0x7A],
};
const INACTIVE: Palette = Palette {
    sky_top: [0x86, 0xA4, 0xC0],
    sky_bottom: [0x6C, 0x8C, 0xAB],
    edge: [0x4E, 0x6A, 0x86],
    cloud: [0xF2, 0xF5, 0xF8],
    cloud_cover: 0.55,
    text: [0xE6, 0xEC, 0xF2],
    shadow: [0x3F, 0x58, 0x72],
};
const CLOSE: Rgb = [0xE5, 0x34, 0x3A];
const CLOSE_PRESSED: Rgb = [0xB8, 0x26, 0x2C];
const WHITE: Rgb = [0xFF, 0xFF, 0xFF];

static TYPEFACE: LazyLock<fontdue::Font> = LazyLock::new(|| {
    fontdue::Font::from_bytes(andy_runtime::win32::text::TYPEFACE_FILE, fontdue::FontSettings::default())
        .expect("the bundled font is valid")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Minimise,
    Maximise,
    Close,
}

/// An edge or corner of the window, by which it is resized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

/// What a point of the window belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The picture.
    Content,
    /// The title bar, by which the window is moved.
    Bar,
    Button(Button),
    Edge(Edge),
}

/// The frame of a window of a given size.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub title: String,
    /// The window's size in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Physical pixels per logical pixel.
    pub scale: f64,
    /// Whether the window has the keyboard.
    pub active: bool,
    pub maximized: bool,
    /// The button the pointer is over, and the one held down.
    pub hovered: Option<Button>,
    pub pressed: Option<Button>,
}

/// A picture of the title bar: rows of red, green, blue, alpha bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Bar {
    fn blend(&mut self, x: i32, y: i32, color: Rgb, coverage: f64) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || coverage <= 0.0 {
            return;
        }
        let at = 4 * (y as usize * self.width as usize + x as usize);
        let coverage = coverage.min(1.0);
        for channel in 0..3 {
            let old = self.rgba[at + channel] as f64;
            self.rgba[at + channel] = (old + (color[channel] as f64 - old) * coverage).round() as u8;
        }
    }

    /// A line of the given thickness with smooth edges: each pixel is
    /// covered by how far its centre is inside the line.
    fn stroke(&mut self, from: (f64, f64), to: (f64, f64), thickness: f64, color: Rgb) {
        let reach = thickness / 2.0 + 1.0;
        let (left, right) =
            ((from.0.min(to.0) - reach).floor() as i32, (from.0.max(to.0) + reach).ceil() as i32);
        let (top, bottom) =
            ((from.1.min(to.1) - reach).floor() as i32, (from.1.max(to.1) + reach).ceil() as i32);
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length_squared = dx * dx + dy * dy;
        for y in top..=bottom {
            for x in left..=right {
                let (px, py) = (x as f64 + 0.5 - from.0, y as f64 + 0.5 - from.1);
                let along = if length_squared == 0.0 {
                    0.0
                } else {
                    ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
                };
                let distance = ((px - along * dx).powi(2) + (py - along * dy).powi(2)).sqrt();
                self.blend(x, y, color, thickness / 2.0 + 0.5 - distance);
            }
        }
    }

    fn outline(&mut self, left: f64, top: f64, right: f64, bottom: f64, thickness: f64, color: Rgb) {
        for (from, to) in [
            ((left, top), (right, top)),
            ((right, top), (right, bottom)),
            ((right, bottom), (left, bottom)),
            ((left, bottom), (left, top)),
        ] {
            self.stroke(from, to, thickness, color);
        }
    }

    /// A cloud resting on the line `base`, `centre` along it, of the size
    /// `unit` (about a third of its width): round puffs over a flat bottom.
    fn cloud(&mut self, centre: f64, base: f64, unit: f64, color: Rgb, cover: f64) {
        // Each puff: where its centre is and its radius, in units.
        const PUFFS: [(f64, f64, f64); 6] = [
            (-1.12, -0.24, 0.24),
            (-0.72, -0.42, 0.42),
            (-0.18, -0.66, 0.62),
            (0.50, -0.52, 0.50),
            (0.98, -0.32, 0.32),
            (1.22, -0.20, 0.20),
        ];
        let (left, right) = ((centre - 1.5 * unit).floor() as i32, (centre + 1.5 * unit).ceil() as i32);
        let top = (base - 1.4 * unit).floor() as i32;
        for y in top..=base.ceil() as i32 {
            for x in left..=right {
                let (px, py) = ((x as f64 + 0.5 - centre) / unit, (y as f64 + 0.5 - base) / unit);
                // Inside the shape by this many pixels, at the least.
                let mut inside = f64::MIN;
                for (cx, cy, radius) in PUFFS {
                    inside = inside.max((radius - ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()) * unit);
                }
                // The body between the outer puffs, under their centres.
                if (-1.12..=1.22).contains(&px) && py >= -0.24 {
                    inside = inside.max(1.0);
                }
                let above_base = -py * unit;
                self.blend(x, y, color, (inside + 0.5).min(above_base + 0.5).clamp(0.0, 1.0) * cover);
            }
        }
    }

    fn fill(&mut self, left: i32, top: i32, right: i32, bottom: i32, color: Rgb, coverage: f64) {
        for y in top..bottom {
            for x in left..right {
                self.blend(x, y, color, coverage);
            }
        }
    }
}

impl Frame {
    pub fn new(title: &str, width: u32, height: u32, scale: f64) -> Frame {
        Frame {
            title: title.into(),
            width,
            height,
            scale,
            active: true,
            maximized: false,
            hovered: None,
            pressed: None,
        }
    }

    fn pixels(&self, logical: f64) -> f64 {
        (logical * self.scale).round()
    }

    /// The height of the title bar in physical pixels.
    pub fn bar_height(&self) -> u32 {
        self.pixels(BAR_HEIGHT) as u32
    }

    /// The thickness of the border in physical pixels: none while the
    /// window fills the screen.
    pub fn border(&self) -> u32 {
        if self.maximized { 0 } else { self.pixels(1.0).max(1.0) as u32 }
    }

    fn palette(&self) -> &'static Palette {
        if self.active { &ACTIVE } else { &INACTIVE }
    }

    pub fn border_color(&self) -> Rgb {
        self.palette().edge
    }

    /// Where a button is: its left and right edges.
    fn button_span(&self, button: Button) -> (f64, f64) {
        let width = self.pixels(BUTTON_WIDTH);
        let from_right = match button {
            Button::Close => 1.0,
            Button::Maximise => 2.0,
            Button::Minimise => 3.0,
        };
        let right = self.width as f64 - (from_right - 1.0) * width;
        (right - width, right)
    }

    /// What the point (x, y) of the window, in physical pixels, belongs to.
    pub fn hit(&self, x: f64, y: f64) -> Hit {
        let (width, height) = (self.width as f64, self.height as f64);
        if !self.maximized {
            let edge = self.pixels(EDGE);
            // Corners reach further along the edges, as they are small.
            let corner = 2.0 * edge;
            let (west, east) = (x < edge, x >= width - edge);
            let (north, south) = (y < edge, y >= height - edge);
            let (near_west, near_east) = (x < corner, x >= width - corner);
            let (near_north, near_south) = (y < corner, y >= height - corner);
            let found = match () {
                _ if (north && near_west) || (west && near_north) => Some(Edge::NorthWest),
                _ if (north && near_east) || (east && near_north) => Some(Edge::NorthEast),
                _ if (south && near_west) || (west && near_south) => Some(Edge::SouthWest),
                _ if (south && near_east) || (east && near_south) => Some(Edge::SouthEast),
                _ if north => Some(Edge::North),
                _ if south => Some(Edge::South),
                _ if west => Some(Edge::West),
                _ if east => Some(Edge::East),
                _ => None,
            };
            if let Some(edge) = found {
                return Hit::Edge(edge);
            }
        }
        if y >= self.bar_height() as f64 {
            return Hit::Content;
        }
        for button in [Button::Close, Button::Maximise, Button::Minimise] {
            let (left, right) = self.button_span(button);
            if (left..right).contains(&x) {
                return Hit::Button(button);
            }
        }
        Hit::Bar
    }

    /// The title bar as pixels, with the border along its top and sides.
    pub fn render(&self) -> Bar {
        let palette = self.palette();
        let (width, height) = (self.width.max(1), self.bar_height().max(1));
        let line = self.pixels(1.0).max(1.0);
        let mut bar = Bar { width, height, rgba: vec![0xFF; 4 * (width * height) as usize] };
        // The sky: lighter at the top.
        for y in 0..height {
            let along = y as f64 / (height - 1).max(1) as f64;
            let color: Rgb = std::array::from_fn(|channel| {
                let (top, bottom) = (palette.sky_top[channel] as f64, palette.sky_bottom[channel] as f64);
                (top + (bottom - top) * along).round() as u8
            });
            for x in 0..width as usize {
                bar.rgba[4 * (y as usize * width as usize + x)..][..3].copy_from_slice(&color);
            }
        }
        let middle = (height as f64 - line) / 2.0;

        // The title, at the left, over a soft shadow.
        let size = (TITLE_SIZE * self.scale) as f32;
        let glyphs: Vec<_> =
            self.title.chars().map(|character| TYPEFACE.rasterize(character, size)).collect();
        let buttons = self.button_span(Button::Minimise).0;
        let room = buttons - self.pixels(12.0);
        let metrics = TYPEFACE.horizontal_line_metrics(size).expect("the bundled font has line metrics");
        let baseline = (middle + (metrics.ascent + metrics.descent) as f64 / 2.0).round() as i32;
        let mut title_end = self.pixels(TITLE_LEFT);
        for (color, drop) in [(palette.shadow, line as i32), (palette.text, 0)] {
            let mut pen = self.pixels(TITLE_LEFT);
            for (glyph, coverage) in &glyphs {
                if pen + glyph.advance_width as f64 > room {
                    break;
                }
                let (left, top) =
                    (pen.round() as i32 + glyph.xmin, baseline - glyph.ymin - glyph.height as i32);
                for row in 0..glyph.height {
                    for column in 0..glyph.width {
                        let amount = coverage[row * glyph.width + column] as f64 / 255.0;
                        let amount = if drop == 0 { amount } else { amount * 0.55 };
                        bar.blend(left + column as i32, top + row as i32 + drop, color, amount);
                    }
                }
                pen += glyph.advance_width as f64;
            }
            title_end = pen;
        }

        // Clouds drift between the title and the buttons. Each has a place
        // of its own along the bar, counted from the left, so that they
        // stay where they are while the window changes width; the ones
        // that would touch the title or the buttons are left out.
        let spacing = self.pixels(CLOUD_SPACING);
        for slot in 0..(width as f64 / spacing).ceil() as u32 {
            // Three numbers from 0 to 1 that depend on the slot alone.
            let [along, down, large] = [0x9E37_79B9u32, 0x85EB_CA6B, 0xC2B2_AE35].map(|multiplier| {
                let mixed = (slot + 1).wrapping_mul(multiplier);
                ((mixed ^ (mixed >> 15)).wrapping_mul(0x2C1B_3C6D) >> 8) as f64 / (1u32 << 24) as f64
            });
            let unit = (5.5 + 4.0 * large) * self.scale;
            let centre = (slot as f64 + 0.25 + 0.5 * along) * spacing;
            let base = (height as f64 - line) * (0.58 + 0.34 * down);
            let (left, right) = (centre - 1.6 * unit, centre + 1.6 * unit);
            if left < title_end + self.pixels(14.0) || right > buttons - self.pixels(8.0) {
                continue;
            }
            bar.cloud(centre, base.round(), unit, palette.cloud, palette.cloud_cover * (0.72 + 0.28 * large));
        }

        // The buttons, each lit when the pointer is over it.
        for button in [Button::Minimise, Button::Maximise, Button::Close] {
            let (left, right) = self.button_span(button);
            let (hovered, pressed) = (self.hovered == Some(button), self.pressed == Some(button));
            if hovered || pressed {
                let (left, right, bottom) = (left as i32, right as i32, height as i32 - line as i32);
                match button {
                    Button::Close => {
                        bar.fill(left, 0, right, bottom, if pressed { CLOSE_PRESSED } else { CLOSE }, 1.0)
                    }
                    _ => bar.fill(left, 0, right, bottom, WHITE, if pressed { 0.34 } else { 0.20 }),
                }
            }
            let icon = if hovered || pressed { WHITE } else { palette.text };
            let (centre, half, stroke) = ((left + right) / 2.0, self.pixels(ICON) / 2.0, STROKE * self.scale);
            match button {
                Button::Minimise => {
                    bar.stroke((centre - half, middle), (centre + half, middle), stroke, icon)
                }
                Button::Maximise if self.maximized => {
                    // Two squares, one behind the other.
                    let shift = (2.0 * self.scale).round();
                    let (l, t, r, b) =
                        (centre - half, middle - half + shift, centre + half - shift, middle + half);
                    bar.stroke((l + shift, t - shift), (r + shift, t - shift), stroke, icon);
                    bar.stroke((r + shift, t - shift), (r + shift, b - shift), stroke, icon);
                    bar.stroke((l + shift, t - shift), (l + shift, t), stroke, icon);
                    bar.stroke((r, b - shift), (r + shift, b - shift), stroke, icon);
                    bar.outline(l, t, r, b, stroke, icon);
                }
                Button::Maximise => {
                    bar.outline(centre - half, middle - half, centre + half, middle + half, stroke, icon)
                }
                Button::Close => {
                    bar.stroke((centre - half, middle - half), (centre + half, middle + half), stroke, icon);
                    bar.stroke((centre - half, middle + half), (centre + half, middle - half), stroke, icon);
                }
            }
        }

        // The line under the bar, and the border over its other sides.
        let (line, border) = (line as i32, self.border() as i32);
        let (right, bottom) = (width as i32, height as i32);
        bar.fill(0, bottom - line, right, bottom, palette.edge, 1.0);
        bar.fill(0, 0, right, border, palette.edge, 1.0);
        bar.fill(0, 0, border, bottom, palette.edge, 1.0);
        bar.fill(right - border, 0, right, bottom, palette.edge, 1.0);
        bar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame::new("Toy Story 2: Retold", 962, 757, 1.0)
    }

    #[test]
    fn points_belong_to_the_bar_its_buttons_the_edges_or_the_picture() {
        let frame = frame();
        assert_eq!(frame.bar_height(), 36);
        assert_eq!(frame.hit(480.0, 17.0), Hit::Bar);
        assert_eq!(frame.hit(480.0, 36.0), Hit::Content);
        assert_eq!(frame.hit(940.0, 17.0), Hit::Button(Button::Close));
        assert_eq!(frame.hit(890.0, 17.0), Hit::Button(Button::Maximise));
        assert_eq!(frame.hit(845.0, 17.0), Hit::Button(Button::Minimise));
        assert_eq!(frame.hit(820.0, 17.0), Hit::Bar);
        // Edges and corners come before everything else.
        assert_eq!(frame.hit(480.0, 2.0), Hit::Edge(Edge::North));
        assert_eq!(frame.hit(2.0, 400.0), Hit::Edge(Edge::West));
        assert_eq!(frame.hit(957.0, 400.0), Hit::Edge(Edge::East));
        assert_eq!(frame.hit(480.0, 752.0), Hit::Edge(Edge::South));
        assert_eq!(frame.hit(8.0, 2.0), Hit::Edge(Edge::NorthWest));
        assert_eq!(frame.hit(957.0, 8.0), Hit::Edge(Edge::NorthEast));
        assert_eq!(frame.hit(2.0, 745.0), Hit::Edge(Edge::SouthWest));
        assert_eq!(frame.hit(950.0, 752.0), Hit::Edge(Edge::SouthEast));
        // A window that fills the screen has no edges to pull, nor a border.
        let maximized = Frame { maximized: true, ..frame.clone() };
        assert_eq!(maximized.hit(958.0, 2.0), Hit::Button(Button::Close));
        assert_eq!(maximized.hit(2.0, 400.0), Hit::Content);
        assert_eq!((frame.border(), maximized.border()), (1, 0));
    }

    #[test]
    fn sizes_follow_the_scale_of_the_display() {
        let frame = Frame::new("x", 1920, 1508, 2.0);
        assert_eq!(frame.bar_height(), 72);
        assert_eq!(frame.hit(1900.0, 30.0), Hit::Button(Button::Close));
        assert_eq!(frame.hit(1800.0, 30.0), Hit::Button(Button::Maximise));
        assert_eq!(frame.border(), 2);
    }

    fn pixel(bar: &Bar, x: usize, y: usize) -> Rgb {
        bar.rgba[4 * (y * bar.width as usize + x)..][..3].try_into().unwrap()
    }

    /// How many pixels of the two pictures differ, in a span of columns.
    fn differing(one: &Bar, other: &Bar, columns: std::ops::Range<usize>) -> usize {
        columns
            .flat_map(|x| (0..one.height as usize).map(move |y| (x, y)))
            .filter(|&(x, y)| pixel(one, x, y) != pixel(other, x, y))
            .count()
    }

    #[test]
    fn the_bar_is_a_sky_with_the_title_clouds_and_lit_buttons() {
        let frame = frame();
        let bar = frame.render();
        assert_eq!((bar.width, bar.height, bar.rgba.len()), (962, 36, 4 * 962 * 36));
        assert!(bar.rgba.chunks(4).all(|pixel| pixel[3] == 0xFF));
        // The border along the top and the sides, the line underneath.
        for (x, y) in [(0, 0), (500, 0), (961, 20), (0, 20), (500, 35)] {
            assert_eq!(pixel(&bar, x, y), ACTIVE.edge, "{x}, {y}");
        }
        assert_eq!(pixel(&bar, 5, 1), [0x4B, 0xA3, 0xED]);
        // The title is at the left, and only there.
        let untitled = Frame { title: String::new(), ..frame.clone() }.render();
        assert!(differing(&bar, &untitled, 14..150) > 300);
        assert_eq!(differing(&bar, &untitled, 0..14), 0);
        // Clouds: white pixels between the title and the buttons.
        let white = |bar: &Bar, columns: std::ops::Range<usize>| {
            columns
                .flat_map(|x| (1..35).map(move |y| (x, y)))
                .filter(|&(x, y)| pixel(bar, x, y)[0] > 0xA0)
                .count()
        };
        assert!(white(&bar, 180..800) > 400);
        // They stay where they are when the window gets wider.
        let wider = Frame { width: 1200, ..frame.clone() }.render();
        assert_eq!(differing(&bar, &wider, 0..700), 0);
        // The close button turns red under the pointer, with a white cross.
        let hovered = Frame { hovered: Some(Button::Close), ..frame.clone() }.render();
        assert_eq!(pixel(&hovered, 925, 5), CLOSE);
        assert!(white(&hovered, 927..952) > 10);
        assert_eq!(differing(&bar, &hovered, 0..914), 0);
        // Without the keyboard the sky is overcast.
        let inactive = Frame { active: false, ..frame.clone() }.render();
        assert_eq!(pixel(&inactive, 0, 0), INACTIVE.edge);
        assert!(pixel(&inactive, 5, 1)[2] < pixel(&bar, 5, 1)[2]);
    }

    /// Writes the bar in its states as pictures, to be looked at:
    /// `ANDY_FRAME_PICTURES=dir cargo test -p andy frame -- --ignored`.
    #[test]
    #[ignore]
    fn pictures_of_the_bar() {
        let directory = std::env::var("ANDY_FRAME_PICTURES").expect("ANDY_FRAME_PICTURES names a directory");
        let frame = frame();
        for (name, frame) in [
            ("active", frame.clone()),
            ("inactive", Frame { active: false, ..frame.clone() }),
            ("close", Frame { hovered: Some(Button::Close), ..frame.clone() }),
            ("maximise", Frame { hovered: Some(Button::Maximise), maximized: true, ..frame.clone() }),
            ("double", Frame::new("Toy Story 2: Retold", 1920, 1512, 2.0)),
        ] {
            let bar = frame.render();
            let file = std::fs::File::create(format!("{directory}/{name}.png")).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), bar.width, bar.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.write_header().unwrap().write_image_data(&bar.rgba).unwrap();
        }
    }
}
