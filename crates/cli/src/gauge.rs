//! The window's gauge: a small panel in the top right corner that says
//! how fast the game is being shown (F9 shows and hides it).
//!
//! It belongs to the window, not to the game (ADR 0018), and is drawn
//! here, by code, in the manner of a space ranger's wrist panel: a white
//! shell with a purple rim, a lime strip, a dark glass with green
//! figures, three oval buttons and a band of red and white stripes. No
//! picture or sign of anyone's is used.

type Rgb = [u8; 3];

const SHELL: Rgb = [0xF2, 0xF3, 0xEE];
const SHELL_SHADE: Rgb = [0xC9, 0xCC, 0xC4];
const RIM: Rgb = [0x5B, 0x35, 0x9A];
const LIME: Rgb = [0x9A, 0xE0, 0x2B];
const GLASS: Rgb = [0x0B, 0x1E, 0x14];
const FIGURE: Rgb = [0x7C, 0xFF, 0x4F];
const FIGURE_DIM: Rgb = [0x16, 0x3A, 0x22];
const RED: Rgb = [0xD8, 0x2B, 0x2B];
const GREEN: Rgb = [0x2F, 0xB5, 0x4A];
const BLUE: Rgb = [0x2E, 0x6F, 0xD6];
const WHITE: Rgb = [0xFF, 0xFF, 0xFF];

/// The panel's size in logical pixels, and how far it stands from the
/// corner of the game's picture.
pub const WIDTH: f64 = 148.0;
pub const HEIGHT: f64 = 66.0;
pub const MARGIN: f64 = 8.0;

/// What the gauge shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reading {
    /// Frames shown in a second.
    pub frames: u32,
    /// The time from one frame to the next, in tenths of a millisecond.
    pub frame_tenths: u32,
    /// The frames a second the machine could make if it never waited.
    pub could: u32,
}

/// A picture with transparency: rows of red, green, blue, alpha bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Picture {
    /// Lays a colour over a pixel, covering it by so much.
    fn blend(&mut self, x: i32, y: i32, color: Rgb, coverage: f64) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || coverage <= 0.0 {
            return;
        }
        let at = 4 * (y as usize * self.width as usize + x as usize);
        let cover = coverage.min(1.0);
        let under = self.rgba[at + 3] as f64 / 255.0;
        let alpha = cover + under * (1.0 - cover);
        for channel in 0..3 {
            let old = self.rgba[at + channel] as f64;
            let mixed = (color[channel] as f64 * cover + old * under * (1.0 - cover)) / alpha;
            self.rgba[at + channel] = mixed.round() as u8;
        }
        self.rgba[at + 3] = (alpha * 255.0).round() as u8;
    }

    /// A rectangle with round corners and smooth edges.
    fn round(&mut self, left: f64, top: f64, right: f64, bottom: f64, radius: f64, color: Rgb) {
        for y in top.floor() as i32..=bottom.ceil() as i32 {
            for x in left.floor() as i32..=right.ceil() as i32 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                // How far the pixel's centre is outside the rectangle
                // drawn in by the radius: inside it by the radius or less.
                let dx = (left + radius - px).max(px - (right - radius)).max(0.0);
                let dy = (top + radius - py).max(py - (bottom - radius)).max(0.0);
                self.blend(x, y, color, radius + 0.5 - (dx * dx + dy * dy).sqrt());
            }
        }
    }

    /// An oval with smooth edges.
    fn oval(&mut self, centre: (f64, f64), radii: (f64, f64), color: Rgb) {
        let (left, right) = ((centre.0 - radii.0).floor() as i32, (centre.0 + radii.0).ceil() as i32);
        let (top, bottom) = ((centre.1 - radii.1).floor() as i32, (centre.1 + radii.1).ceil() as i32);
        for y in top..=bottom {
            for x in left..=right {
                let (px, py) = ((x as f64 + 0.5 - centre.0) / radii.0, (y as f64 + 0.5 - centre.1) / radii.1);
                let distance = (px * px + py * py).sqrt();
                self.blend(x, y, color, (1.0 - distance) * radii.0.min(radii.1) + 0.5);
            }
        }
    }
}

/// Which of its seven strokes a figure lights: top, top right, bottom
/// right, bottom, bottom left, top left, middle.
const STROKES: [[bool; 7]; 10] = [
    [true, true, true, true, true, true, false],
    [false, true, true, false, false, false, false],
    [true, true, false, true, true, false, true],
    [true, true, true, true, false, false, true],
    [false, true, true, false, false, true, true],
    [true, false, true, true, false, true, true],
    [true, false, true, true, true, true, true],
    [true, true, true, false, false, false, false],
    [true, true, true, true, true, true, true],
    [true, true, true, true, false, true, true],
];

/// The letters of the labels, five rows of three dots each.
fn letter(character: char) -> [u8; 5] {
    match character {
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        _ => [0; 5],
    }
}

/// Draws panels at a scale: so many physical pixels to a logical one.
struct Painter {
    picture: Picture,
    scale: f64,
}

impl Painter {
    fn round(&mut self, left: f64, top: f64, right: f64, bottom: f64, radius: f64, color: Rgb) {
        let s = self.scale;
        self.picture.round(left * s, top * s, right * s, bottom * s, radius * s, color);
    }

    /// A figure of seven strokes in a box, the strokes it does not light
    /// drawn dim, as a glass of such figures shows them.
    fn figure(&mut self, digit: Option<u32>, left: f64, top: f64, width: f64, height: f64, thick: f64) {
        let (right, bottom, middle) = (left + width, top + height, top + height / 2.0);
        let gap = thick * 0.6;
        // Each stroke as a rectangle: left, top, right, bottom.
        let strokes = [
            (left + gap, top, right - gap, top + thick),
            (right - thick, top + gap, right, middle - gap / 2.0),
            (right - thick, middle + gap / 2.0, right, bottom - gap),
            (left + gap, bottom - thick, right - gap, bottom),
            (left, middle + gap / 2.0, left + thick, bottom - gap),
            (left, top + gap, left + thick, middle - gap / 2.0),
            (left + gap, middle - thick / 2.0, right - gap, middle + thick / 2.0),
        ];
        for (number, (l, t, r, b)) in strokes.into_iter().enumerate() {
            let lit = digit.is_some_and(|digit| STROKES[digit as usize % 10][number]);
            self.round(l, t, r, b, thick / 2.0, if lit { FIGURE } else { FIGURE_DIM });
        }
    }

    /// A number in figures of seven strokes, ending at `right`: so many
    /// places, the leading ones dark; a point before the last `decimals`.
    #[allow(clippy::too_many_arguments)]
    fn number(&mut self, value: u32, places: u32, decimals: u32, right: f64, top: f64, size: (f64, f64)) {
        let (width, height) = size;
        let (thick, step) = (height / 7.0, width * 1.38);
        let limit = 10u32.pow(places) - 1;
        let value = value.min(limit);
        let mut x = right - width;
        for place in 0..places {
            let digit = value / 10u32.pow(place) % 10;
            let shown = place <= decimals || value >= 10u32.pow(place);
            self.figure(shown.then_some(digit), x, top, width, height, thick);
            if decimals > 0 && place + 1 == decimals {
                // The point stands in room of its own before the decimals.
                let (point, room) = (thick * 1.3, (step - width) * 0.6);
                let left = x - room - point;
                self.round(left, top + height - point, left + point, top + height, point / 2.0, FIGURE);
                x = left - room - width + step;
            }
            x -= step;
        }
    }

    /// A label in dots.
    fn label(&mut self, text: &str, left: f64, top: f64, dot: f64, color: Rgb) {
        for (place, character) in text.chars().enumerate() {
            for (row, bits) in letter(character).into_iter().enumerate() {
                for column in 0..3 {
                    if bits & (0b100 >> column) != 0 {
                        let (x, y) =
                            (left + (place as f64 * 4.0 + column as f64) * dot, top + row as f64 * dot);
                        self.round(x, y, x + dot, y + dot, 0.0, color);
                    }
                }
            }
        }
    }
}

/// The gauge's picture, `scale` physical pixels to a logical one.
pub fn render(reading: Reading, scale: f64) -> Picture {
    let (width, height) = ((WIDTH * scale).ceil() as u32, (HEIGHT * scale).ceil() as u32);
    let picture = Picture { width, height, rgba: vec![0; 4 * width as usize * height as usize] };
    let mut painter = Painter { picture, scale };
    // The shell: a purple rim, white inside, a shade along the bottom.
    painter.round(0.0, 0.0, WIDTH, HEIGHT, 9.0, RIM);
    painter.round(2.0, 2.0, WIDTH - 2.0, HEIGHT - 2.0, 7.0, SHELL_SHADE);
    painter.round(2.0, 2.0, WIDTH - 2.0, HEIGHT - 4.0, 7.0, SHELL);
    // The lime strip down the left, with the three buttons on it.
    painter.round(6.0, 6.0, 30.0, HEIGHT - 16.0, 5.0, LIME);
    for (number, color) in [RED, GREEN, BLUE].into_iter().enumerate() {
        let centre = ((18.0) * scale, (14.0 + 11.0 * number as f64) * scale);
        painter.picture.oval(centre, (8.0 * scale, 3.6 * scale), RIM);
        painter.picture.oval(centre, (6.6 * scale, 2.5 * scale), color);
    }
    // The band of red and white stripes along the bottom, leaning.
    let (band_top, band_bottom) = (HEIGHT - 12.0, HEIGHT - 6.0);
    painter.round(6.0, band_top, WIDTH - 6.0, band_bottom, 3.0, WHITE);
    let stripe = 6.0 * scale;
    for y in (band_top * scale) as i32..(band_bottom * scale) as i32 {
        for x in (9.0 * scale) as i32..((WIDTH - 9.0) * scale) as i32 {
            let along = (x as f64 + (y as f64 - band_top * scale)) / stripe;
            if along.rem_euclid(2.0) < 1.0 {
                painter.picture.blend(x, y, RED, 1.0);
            }
        }
    }
    // The glass, with the frames a second large and, beside them, the
    // time of a frame and the frames a second there could be.
    painter.round(34.0, 6.0, WIDTH - 6.0, HEIGHT - 16.0, 4.0, RIM);
    painter.round(35.5, 7.5, WIDTH - 7.5, HEIGHT - 17.5, 3.0, GLASS);
    painter.label("FPS", 39.0, 11.0, 1.5, LIME);
    painter.number(reading.frames, 4, 0, 96.0, 20.0, (10.0, 24.0));
    painter.label("MS", 101.0, 11.0, 1.0, LIME);
    painter.number(reading.frame_tenths, 4, 1, WIDTH - 11.0, 18.0, (5.2, 10.0));
    painter.label("MAX", 101.0, 31.0, 1.0, LIME);
    painter.number(reading.could, 5, 0, WIDTH - 11.0, 37.0, (5.2, 10.0));
    painter.picture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gauge_is_a_panel_with_a_clear_corner_and_lit_figures() {
        let reading = Reading { frames: 60, frame_tenths: 166, could: 240 };
        let picture = render(reading, 2.0);
        assert_eq!((picture.width, picture.height), (296, 132));
        let pixel = |x: usize, y: usize| &picture.rgba[4 * (y * 296 + x)..][..4];
        // The corner is clear, the rim is not.
        assert_eq!(pixel(0, 0)[3], 0);
        assert_eq!(pixel(148, 1), [RIM[0], RIM[1], RIM[2], 0xFF]);
        // The glass is dark and has lit figures on it.
        assert_eq!(pixel(96, 30), [GLASS[0], GLASS[1], GLASS[2], 0xFF]);
        let lit = picture.rgba.chunks(4).filter(|pixel| pixel[..3] == FIGURE).count();
        assert!(lit > 400, "{lit}");
        // Another reading is another picture, and the same the same.
        assert!(render(Reading { frames: 61, ..reading }, 2.0) != picture);
        assert!(render(reading, 2.0) == picture);
    }

    /// Writes the gauge to `out/gauge.png`, to look at:
    /// `cargo test -p andy gauge -- --ignored`.
    #[test]
    #[ignore]
    fn the_gauge_as_a_picture() {
        let picture = render(Reading { frames: 60, frame_tenths: 166, could: 1240 }, 3.0);
        std::fs::create_dir_all("../../out").unwrap();
        let file = std::fs::File::create("../../out/gauge.png").unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), picture.width, picture.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.write_header().unwrap().write_image_data(&picture.rgba).unwrap();
    }

    #[test]
    fn a_colour_laid_over_nothing_is_that_colour() {
        let mut picture = Picture { width: 1, height: 1, rgba: vec![0; 4] };
        picture.blend(0, 0, [200, 100, 50], 0.5);
        assert_eq!(picture.rgba, [200, 100, 50, 128]);
        picture.blend(0, 0, [0, 0, 0], 1.0);
        assert_eq!(picture.rgba, [0, 0, 0, 255]);
    }
}
