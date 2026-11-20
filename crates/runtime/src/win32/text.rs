//! Text for GDI: fonts, their measurements, and their pixels.
//!
//! Windows' own fonts cannot be shipped, so every font a program asks for is
//! substituted. There is one typeface, Liberation Sans, which was drawn to
//! have the character widths and line metrics of Arial. Layout that a
//! program derives by measuring text therefore comes out close to what Arial
//! gives; it is not identical, because Windows used Arial's hand-tuned
//! widths at each pixel size and those belong to Arial. The letter shapes
//! are similar, not the same pixels. Which font Windows 98 itself picked for
//! a face name it did not have (the game asks for "ariel") is `UNVERIFIED`.
//!
//! As in Windows, a font is scaled to a whole number of pixels per em.
//! Glyphs are drawn without smoothing, the Windows 98 default: a pixel is
//! set when the glyph covers at least half of it.

use std::sync::LazyLock;

use ttf_parser::Tag;

struct Typeface {
    raster: fontdue::Font,
    units_per_em: i32,
    /// `usWinAscent` and `usWinDescent` of the OS/2 table: the line metrics
    /// GDI uses.
    ascent: i32,
    descent: i32,
    /// `xAvgCharWidth` of the OS/2 table.
    average_width: i32,
    /// `advanceWidthMax` of the horizontal header.
    max_width: i32,
}

static TYPEFACE: LazyLock<Typeface> = LazyLock::new(|| {
    let data = include_bytes!("../../assets/fonts/LiberationSans-Regular.ttf") as &[u8];
    let face = ttf_parser::Face::parse(data, 0).expect("the bundled font is valid");
    let field = |table: &[u8; 4], offset: usize| {
        let bytes = face.raw_face().table(Tag::from_bytes(table)).and_then(|t| t.get(offset..offset + 2));
        u16::from_be_bytes(bytes.expect("the bundled font has this table").try_into().expect("two bytes"))
    };
    Typeface {
        units_per_em: face.units_per_em() as i32,
        ascent: field(b"OS/2", 74) as i32,
        descent: field(b"OS/2", 76) as i32,
        average_width: field(b"OS/2", 2) as i16 as i32,
        max_width: field(b"hhea", 10) as i32,
        raster: fontdue::Font::from_bytes(data, fontdue::FontSettings::default())
            .expect("the bundled font is valid"),
    }
});

/// Em size of the font Windows picks when asked for "any size": 12 points
/// at 96 dots per inch.
const DEFAULT_EM: i32 = 16;
const HALF_COVERAGE: u8 = 128;

/// `a * b / c`, rounded to nearest: Windows' `MulDiv`, for positive values.
fn scale(a: i32, b: i32, c: i32) -> i32 {
    (a * b + c / 2) / c
}

/// A font at one size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Font {
    /// Size of the em square in pixels.
    pub em: i32,
    /// Pixels above and below the baseline in a line of text.
    pub ascent: i32,
    pub descent: i32,
    pub average_width: i32,
    pub max_width: i32,
}

impl Font {
    /// The font for a `LOGFONT` height: positive is the height of a line,
    /// negative the height of the em square, zero a default.
    pub fn with_height(height: i32) -> Font {
        let face = &*TYPEFACE;
        let em = match height {
            0 => DEFAULT_EM,
            line if line > 0 => scale(line, face.units_per_em, face.ascent + face.descent),
            negative => negative.saturating_neg(),
        }
        .clamp(1, 4096);
        Font {
            em,
            ascent: scale(face.ascent, em, face.units_per_em),
            descent: scale(face.descent, em, face.units_per_em),
            average_width: scale(face.average_width, em, face.units_per_em),
            max_width: scale(face.max_width, em, face.units_per_em),
        }
    }

    /// Height of a line of text.
    pub fn height(&self) -> i32 {
        self.ascent + self.descent
    }

    fn advance(&self, character: char) -> i32 {
        TYPEFACE.raster.metrics(character, self.em as f32).advance_width.round() as i32
    }

    /// Width of a run of text in the Windows-1252 code page.
    pub fn width(&self, text: &[u8]) -> i32 {
        text.iter().map(|&byte| self.advance(decode(byte))).sum()
    }

    /// Calls `plot(x, y)` for every pixel the text sets, relative to the
    /// top-left corner of its line. A glyph may reach a little outside the
    /// line's box (never by more than an em).
    pub fn draw(&self, text: &[u8], mut plot: impl FnMut(i32, i32)) {
        let mut pen = 0;
        for &byte in text {
            let (metrics, coverage) = TYPEFACE.raster.rasterize(decode(byte), self.em as f32);
            // The bitmap's bottom row is `ymin` pixels above the baseline.
            let top = self.ascent - (metrics.ymin + metrics.height as i32);
            for row in 0..metrics.height {
                for column in 0..metrics.width {
                    if coverage[row * metrics.width + column] >= HALF_COVERAGE {
                        plot(pen + metrics.xmin + column as i32, top + row as i32);
                    }
                }
            }
            pen += metrics.advance_width.round() as i32;
        }
    }
}

/// The character a byte stands for in Windows-1252, the code page of the
/// game's text.
pub fn decode(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
        '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}', '\u{90}',
        '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{2DC}',
        '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
    ];
    match byte {
        0x80..=0x9F => HIGH[byte as usize - 0x80],
        other => other as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_typeface_has_the_line_metrics_of_arial() {
        let face = &*TYPEFACE;
        assert_eq!((face.units_per_em, face.ascent, face.descent), (2048, 1854, 434));
        assert_eq!((face.average_width, face.max_width), (1187, 2740));
    }

    #[test]
    fn a_height_selects_a_whole_em() {
        // A line 20 pixels high is an 18-pixel em: 20 * 2048 / 2288 = 17.9.
        let font = Font::with_height(20);
        assert_eq!((font.em, font.ascent, font.descent, font.height()), (18, 16, 4, 20));
        // A negative height is the em itself; the line is taller.
        let font = Font::with_height(-20);
        assert_eq!((font.em, font.ascent, font.descent), (20, 18, 4));
        assert_eq!(Font::with_height(0), Font::with_height(-16));
        for height in 1..200 {
            let font = Font::with_height(height);
            assert!((font.height() - height).abs() <= 1, "{height}: {font:?}");
        }
    }

    #[test]
    fn widths_add_up_and_follow_the_typeface() {
        let font = Font::with_height(20);
        assert_eq!(font.width(b""), 0);
        assert_eq!(font.width(b"toy story"), font.width(b"toy") + font.width(b" ") + font.width(b"story"));
        assert!(font.width(b"W") > font.width(b"i"));
        // Digits share one width, as in Arial.
        assert_eq!(font.width(b"1"), font.width(b"8"));
        assert!(font.width(b"W") <= font.max_width);
    }

    #[test]
    fn text_is_drawn_on_its_baseline() {
        let font = Font::with_height(20);
        let text = b"Buzz";
        let (mut pixels, mut left, mut right, mut top, mut bottom) =
            (0, i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        font.draw(text, |x, y| {
            pixels += 1;
            (left, right, top, bottom) = (left.min(x), right.max(x), top.min(y), bottom.max(y));
        });
        assert!(pixels > 50, "{pixels} pixels");
        assert!(left >= -font.em && right < font.width(text) + font.em, "{left}..{right}");
        assert!(top >= 0, "{top}");
        // Letters without descenders stand on the baseline; a space sets nothing.
        assert_eq!(bottom, font.ascent - 1);
        font.draw(b" ", |_, _| panic!("a space has no pixels"));
    }

    #[test]
    fn the_code_page_is_windows_1252() {
        assert_eq!(decode(b'A'), 'A');
        assert_eq!(decode(0xE9), 'é');
        assert_eq!(decode(0x80), '€');
        assert_eq!(decode(0x99), '™');
    }
}
