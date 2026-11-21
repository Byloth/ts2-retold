//! The game's typefaces, each with one atlas and one table from character
//! to glyph (ADR 0016).
//!
//! A typeface is a page of 256 by 256 pixels, or a part of one, cut into
//! cells. On every
//! language's disc the first cells hold the same things: the letters, the
//! digits and some signs (the same pictures in the large typeface; in the
//! small one some discs have the digits and a letter drawn anew). After
//! them each disc has what its language added, in its own places and
//! sometimes drawn a little differently: the accents, drawn over a
//! letter, and signs of its own.
//!
//! Here every sign has one place. The atlas is put together, when the
//! game is built, from the pages of all the discs at hand: the common
//! cells are the reference disc's, each other sign is taken from the
//! first disc that has it, and one that no disc at hand has stays empty. The table says, for every character of
//! Latin-1, which cell to draw, and which accent over it: it is the same
//! for every language, so any text can use any sign.
//!
//! What each disc's page holds is written here ([`PAGES`]), page by page,
//! from looking at them. It cannot be asked of the discs' executables:
//! the French one draws its accents from the wrong cells of its own page.

use crate::pe::{Image, Section};

/// The side of a page, in pixels.
pub const PAGE: usize = 256;

/// What a cell beyond the common ones can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    ArrowRight,
    ArrowLeft,
    Grave,
    Acute,
    Circumflex,
    Diaeresis,
    Tilde,
    /// The ring over a letter, and the sign of degrees.
    Ring,
    Hyphen,
    InvertedQuestion,
    InvertedExclamation,
    OSlash,
    Ae,
    Oe,
    CCedilla,
    Pipe,
    Section,
    Asterisk,
    Apostrophe,
    Equals,
    Dollar,
    Exclamation,
    LessThan,
}

use Sign::*;

impl Sign {
    /// Whether it is drawn over a letter.
    pub fn is_accent(self) -> bool {
        matches!(self, Grave | Acute | Circumflex | Diaeresis | Tilde | Ring)
    }
}

/// What is drawn for a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub cell: u32,
    /// The cell of the accent drawn over it, the typeface's rise higher.
    pub accent: Option<u32>,
}

/// The sign a character is, if it stands alone and is not a letter or a
/// digit.
pub fn sign_of(character: u8) -> Option<Sign> {
    Some(match character {
        b'-' => Hyphen,
        // An accent standing alone.
        b'`' => Grave,
        0xB4 => Acute,
        b'^' => Circumflex,
        0xA8 => Diaeresis,
        b'~' => Tilde,
        b'|' => Pipe,
        b'*' => Asterisk,
        b'\'' => Apostrophe,
        b'=' => Equals,
        b'$' => Dollar,
        b'!' => Exclamation,
        b'<' => LessThan,
        0xA1 => InvertedExclamation,
        0xA7 => Section,
        0xB0 => Ring,
        0xBF => InvertedQuestion,
        0xD8 | 0xF8 => OSlash,
        0xC6 | 0xE6 => Ae,
        0x8C | 0x9C => Oe,
        0xC7 | 0xE7 => CCedilla,
        _ => return None,
    })
}

/// The letter and the accent a character of Latin-1 is made of, if it is
/// a letter with an accent.
pub fn accented(character: u8) -> Option<(u8, Sign)> {
    // The capital letters with accents are 0x20 before the small ones.
    let small = if (0xC0..=0xDE).contains(&character) { character + 0x20 } else { character };
    let (letters, first): (&[u8], u8) = match small {
        0xE0..=0xE5 => (b"aaaaaa", 0xE0),
        0xE8..=0xEB => (b"eeee", 0xE8),
        0xEC..=0xEF => (b"iiii", 0xEC),
        0xF1..=0xF6 => (b"nooooo", 0xF1),
        0xF9..=0xFD => (b"uuuuy", 0xF9),
        0xFF => (b"y", 0xFF),
        _ => return None,
    };
    let accent = match small {
        0xE0 | 0xE8 | 0xEC | 0xF2 | 0xF9 => Grave,
        0xE1 | 0xE9 | 0xED | 0xF3 | 0xFA | 0xFD => Acute,
        0xE2 | 0xEA | 0xEE | 0xF4 | 0xFB => Circumflex,
        0xE4 | 0xEB | 0xEF | 0xF6 | 0xFC | 0xFF => Diaeresis,
        0xE3 | 0xF1 | 0xF5 => Tilde,
        _ => Ring,
    };
    Some((letters[(small - first) as usize], accent))
}

/// A page of a typeface cut into its cells, each as its pixels (blue,
/// green, red, rows from the top).
pub type Cells = Vec<Vec<u8>>;

/// A typeface.
pub struct Face {
    pub name: &'static str,
    /// The sheet of sprites the game draws it from, and the number of the
    /// texture that sheet is on.
    pub sheet: u32,
    pub texture: u8,
    /// A cell's width and height in pixels, and how many cells there are
    /// across and down, from the page's corner. What else the page holds
    /// is not the typeface's and is left as it is.
    pub cell: (usize, usize),
    pub across: usize,
    pub rows: usize,
    /// How many cells, from the first, hold the same things on every disc.
    pub common: usize,
    /// The places of the signs in the atlas, after the common cells.
    ///
    /// A sprite is drawn from the very edge of its cell, and the picture
    /// repeats, so a cell whose bottom row of pixels is used shows as a
    /// line over the cell under it. The places are chosen so that under
    /// such a sign is an empty cell; [`Face::bleeding`] checks that no
    /// disc's pictures break this.
    pub places: &'static [Option<Sign>],
    /// How much higher than its letter an accent is drawn, in the game's
    /// screen units (half the picture's pixels): what the builds that
    /// have accents do.
    pub accent_rise: i32,
    /// Where the reference build holds the address of the sheet's record,
    /// and that record's address.
    record_site: u32,
    record: u32,
    /// The name of the section the atlas's record is put in.
    section: &'static str,
    /// The cell of a character among the common ones.
    plain: fn(u8) -> Option<u32>,
}

/// The large typeface of the menus: 64 cells of 32 by 32, 48 of them
/// common.
pub static LARGE: Face = Face {
    name: "large",
    sheet: 0x32,
    texture: 0x1F,
    cell: (32, 32),
    across: 8,
    rows: 8,
    common: 48,
    // The cedilla reaches the bottom of its cell: it stands in the upper
    // row, over the one place left empty. So does the O with a stroke,
    // for which no empty place is left: the atlas has it a pixel higher
    // in its cell ([`Face::atlas`]).
    places: &[
        Some(ArrowRight),
        Some(ArrowLeft),
        Some(CCedilla),
        Some(InvertedQuestion),
        Some(InvertedExclamation),
        Some(OSlash),
        Some(Ae),
        Some(Oe),
        Some(Grave),
        Some(Acute),
        None,
        Some(Circumflex),
        Some(Diaeresis),
        Some(Tilde),
        Some(Ring),
        Some(Hyphen),
    ],
    accent_rise: 4,
    record_site: 0x004F_6B48,
    record: 0x0050_0CC8,
    section: ".tlarge",
    plain: |character| {
        Some(match character {
            b'a'..=b'z' => character - b'a',
            b'A'..=b'Z' => character - b'A',
            b'0'..=b'9' => 26 + character - b'0',
            b'.' => 36,
            b',' => 37,
            b';' => 38,
            b':' => 39,
            b'\\' => 40,
            b'/' => 41,
            b'!' => 43,
            b'?' => 44,
            b'(' => 45,
            b')' => 46,
            // The reference's own function draws this cell for an apostrophe.
            b'\'' => 47,
            _ => return None,
        } as u32)
    },
};

/// The small typeface: cells of 16 by 24, 48 of them common. Its common
/// cells end with three arrows, and the reference draws the four arrows
/// for the capital letters A to D (the names of the arrow keys are
/// written so); that is kept, as the game's own way of writing them.
pub static SMALL: Face = Face {
    name: "small",
    sheet: 0x43,
    texture: 10,
    cell: (16, 24),
    across: 16,
    rows: 10,
    common: 48,
    // The first sixteen as the Spanish page has them; the cedilla and
    // the asterisk reach the bottom of their cells, and under them
    // nothing stands.
    places: &[
        Some(ArrowRight),
        Some(Hyphen),
        Some(Diaeresis),
        Some(Circumflex),
        Some(Grave),
        Some(Acute),
        Some(Tilde),
        Some(InvertedQuestion),
        Some(InvertedExclamation),
        Some(OSlash),
        Some(Ae),
        Some(Oe),
        Some(CCedilla),
        Some(Ring),
        Some(Pipe),
        Some(Section),
        Some(Asterisk),
        Some(Apostrophe),
        Some(Equals),
        Some(Dollar),
        Some(Exclamation),
        Some(LessThan),
    ],
    accent_rise: 3,
    record_site: 0x004F_6B8C,
    record: 0x0050_0C30,
    section: ".tsmall",
    plain: |character| {
        Some(match character {
            b'a'..=b'z' => character - b'a',
            // Up, down, left, right.
            b'A'..=b'D' => 45 + character - b'A',
            b'E'..=b'Z' => character - b'A',
            b',' => 26,
            b'.' => 27,
            b'/' => 28,
            b'\\' => 29,
            b'?' => 30,
            b':' => 31,
            b';' => 32,
            b'(' => 33,
            b')' => 34,
            b'1'..=b'9' => 35 + character - b'1',
            b'0' => 44,
            _ => return None,
        } as u32)
    },
};

/// The typeface of the levels' texts and messages: cells of 16 by 16 in
/// a corner of a page of other sprites, 51 of them common.
pub static LEVELS: Face = Face {
    name: "levels",
    sheet: 0x14,
    texture: 32,
    cell: (16, 16),
    across: 8,
    rows: 9,
    common: 51,
    // As the Spanish and Italian pages have them, and the ring after.
    places: &[
        Some(Diaeresis),
        Some(Circumflex),
        Some(Grave),
        Some(Acute),
        Some(Tilde),
        Some(InvertedQuestion),
        Some(InvertedExclamation),
        Some(OSlash),
        Some(Ae),
        Some(Oe),
        Some(CCedilla),
        Some(Ring),
    ],
    // The French build's, whose ring stands at the very top of its cell
    // and could not be moved higher; the Spanish and Italian accents,
    // drawn a unit lower by their builds, are moved down in their cells.
    accent_rise: 4,
    record_site: 0x004E_C0F4,
    record: 0x0050_0D30,
    section: ".tlevels",
    plain: |character| {
        Some(match character {
            b'a'..=b'z' => character - b'a',
            b'A'..=b'Z' => character - b'A',
            b'0'..=b'9' => 26 + character - b'0',
            b'.' => 36,
            b',' => 37,
            b':' => 38,
            b';' => 39,
            b'?' => 40,
            b'!' => 41,
            b'/' => 42,
            b'\\' => 43,
            b'<' => 44,
            b'>' => 45,
            b'(' => 46,
            b')' => 47,
            b'\'' => 48,
            b'-' => 49,
            // A filled square, which the reference draws for an asterisk.
            b'*' => 50,
            _ => return None,
        } as u32)
    },
};

pub static FACES: [&Face; 3] = [&LARGE, &SMALL, &LEVELS];

/// A page of a typeface as one of the discs has it: the picture, known by
/// the SHA-256 of its pixels, and the signs in its cells after the common
/// ones, as seen by looking at it.
pub struct KnownPage {
    pub face: &'static Face,
    pub sha256: &'static str,
    pub signs: &'static [Option<Sign>],
    /// How much higher than its letter that disc's build draws an accent.
    pub accent_rise: i32,
}

const LARGE_ITALIAN: [Option<Sign>; 15] = [
    Some(ArrowRight),
    Some(ArrowLeft),
    Some(Diaeresis),
    Some(Circumflex),
    Some(Grave),
    Some(Acute),
    Some(Tilde),
    Some(Ring),
    Some(InvertedQuestion),
    Some(InvertedExclamation),
    Some(OSlash),
    Some(Ae),
    Some(Oe),
    Some(CCedilla),
    // The Spanish page has it; the Italian one has an empty cell, and
    // the Italian game shows a gap for its hyphens.
    Some(Hyphen),
];

const SMALL_SPANISH: [Option<Sign>; 17] = [
    Some(ArrowRight),
    Some(Hyphen),
    Some(Diaeresis),
    Some(Circumflex),
    Some(Grave),
    Some(Acute),
    Some(Tilde),
    Some(InvertedQuestion),
    Some(InvertedExclamation),
    Some(OSlash),
    Some(Ae),
    Some(Oe),
    Some(CCedilla),
    Some(Ring),
    Some(Pipe),
    Some(Section),
    // The Spanish page alone.
    Some(Asterisk),
];

/// The pages of the four discs known (ADR 0016).
pub static PAGES: &[KnownPage] = &[
    KnownPage {
        face: &LARGE,
        sha256: "c3f8fe383678515fa93aac25529737384f8c413e0b17ef643403ee562831a31b",
        signs: &[Some(ArrowRight), Some(ArrowLeft)],
        accent_rise: 4,
    },
    // The Spanish page, which the Italian disc has too in three files.
    KnownPage {
        face: &LARGE,
        sha256: "8bbc63537cc3df63b8495f34129ca9872d821da1c22c7468ccd30c6a8aa040c4",
        signs: &LARGE_ITALIAN,
        accent_rise: 4,
    },
    KnownPage {
        face: &LARGE,
        sha256: "27005709e92fa38a025f76c46e336e9021d506788c2d372388916c31b4c20d8b",
        signs: LARGE_ITALIAN.split_at(14).0,
        accent_rise: 4,
    },
    // The French page.
    KnownPage {
        face: &LARGE,
        sha256: "79f15c339454fae9b716151d45a1e8add022412c656b99588ab5cf6766bf9005",
        signs: &[
            Some(ArrowRight),
            Some(ArrowLeft),
            Some(Diaeresis),
            Some(Circumflex),
            Some(Grave),
            Some(Acute),
            Some(Tilde),
            Some(Hyphen),
            Some(InvertedQuestion),
            Some(CCedilla),
            Some(Ring),
            Some(Ae),
        ],
        accent_rise: 4,
    },
    KnownPage {
        face: &SMALL,
        sha256: "39bc7bac5d8939e273dde54c99ee258094fa6d166eaa7c5b82bf3875ac97262c",
        signs: &[Some(ArrowRight)],
        accent_rise: 3,
    },
    KnownPage {
        face: &SMALL,
        sha256: "a45986823e51f14ab6ae1f1f0f24927df63b5b8b364486e1b2832fb131eea1ef",
        signs: &SMALL_SPANISH,
        accent_rise: 3,
    },
    // The Italian page: the Spanish one without the asterisk.
    KnownPage {
        face: &SMALL,
        sha256: "1d1b97fe8d86251b62c4e6c448d62a5efb7cda300041adb2586de87db4729329",
        signs: SMALL_SPANISH.split_at(16).0,
        accent_rise: 3,
    },
    // The French page. Its accents stand a pixel lower in their cells,
    // and the French build draws them a unit higher. (That build takes
    // them from the wrong cells: it draws a circumflex for a grave.)
    // Whether the cell after the ring is the ligature of A and E or of O
    // and E is UNVERIFIED: the French build draws it for the former.
    KnownPage {
        face: &SMALL,
        sha256: "c024eac77cd32a6634eb9a33dadda35b00ae5f90e217463579a0d55f4547e405",
        signs: &[
            Some(ArrowRight),
            Some(Hyphen),
            Some(Apostrophe),
            Some(Diaeresis),
            Some(Circumflex),
            Some(Acute),
            Some(Grave),
            Some(Tilde),
            Some(InvertedQuestion),
            Some(Ring),
            Some(Ae),
            Some(Equals),
            Some(Dollar),
            Some(Asterisk),
            Some(OSlash),
            Some(Exclamation),
            Some(LessThan),
        ],
        accent_rise: 4,
    },
];

const LEVELS_SPANISH: [Option<Sign>; 11] = [
    Some(Diaeresis),
    Some(Circumflex),
    Some(Grave),
    Some(Acute),
    Some(Tilde),
    Some(InvertedQuestion),
    Some(InvertedExclamation),
    Some(OSlash),
    Some(Ae),
    Some(Oe),
    Some(CCedilla),
];

/// The pages of the levels' typeface: the same picture in 36 files of
/// each disc.
pub static LEVELS_PAGES: &[KnownPage] = &[
    KnownPage {
        face: &LEVELS,
        sha256: "632b8e80366989dfc1c9a099cd443dd193a9c071246101954e40bd480e0f5f92",
        signs: &[],
        accent_rise: 3,
    },
    KnownPage {
        face: &LEVELS,
        sha256: "5e217fbf7a33c888bd2be761e7cb1abd2284890633502db142217c8c02950d09",
        signs: &LEVELS_SPANISH,
        accent_rise: 3,
    },
    // The Italian page has the Spanish one's signs. (Both builds draw the
    // ring from the cell after the cedilla, which is empty.)
    KnownPage {
        face: &LEVELS,
        sha256: "eafc137633d21cbe2fe206afb2f8d058209fbe43129b63ba14159486a71c2be9",
        signs: &LEVELS_SPANISH,
        accent_rise: 3,
    },
    // The French page: no cedilla.
    KnownPage {
        face: &LEVELS,
        sha256: "946eb4b94341052dfa3b1d0280f8cf9f310422a7badd7c18489c0b27bb1057d4",
        signs: &[
            Some(Diaeresis),
            Some(Circumflex),
            Some(Grave),
            Some(Acute),
            Some(Tilde),
            Some(Ring),
            Some(Ae),
            Some(Oe),
        ],
        accent_rise: 4,
    },
];

/// The known page with these pixels' SHA-256, if it is one.
pub fn known_page(sha256: &str) -> Option<&'static KnownPage> {
    PAGES.iter().chain(LEVELS_PAGES).find(|page| page.sha256 == sha256)
}

/// A disc's page (its pixels: blue, green, red, rows from the top), with
/// what is known of it.
pub struct Source {
    pub pixels: Vec<u8>,
    pub page: &'static KnownPage,
}

impl Face {
    /// The cells of the typeface on a page.
    pub fn cells(&self) -> usize {
        self.across * self.rows
    }

    /// The sign's cell in the atlas, if the typeface has a place for it.
    pub fn cell_of(&self, sign: Sign) -> Option<u32> {
        let place = self.places.iter().position(|&place| place == Some(sign))?;
        Some((self.common + place) as u32)
    }

    /// What is drawn for a character (Latin-1). `None` for a space and
    /// for what the typeface has nothing for.
    pub fn glyph(&self, character: u8) -> Option<Glyph> {
        if let Some(cell) = (self.plain)(character) {
            return Some(Glyph { cell, accent: None });
        }
        if let Some(sign) = sign_of(character) {
            return self.cell_of(sign).map(|cell| Glyph { cell, accent: None });
        }
        let (letter, accent) = accented(character)?;
        Some(Glyph { cell: (letter - b'a') as u32, accent: self.cell_of(accent) })
    }

    fn corner(&self, cell: usize) -> (usize, usize) {
        ((cell % self.across) * self.cell.0, (cell / self.across) * self.cell.1)
    }

    /// Cuts a page into its cells.
    pub fn cut(&self, pixels: &[u8]) -> Cells {
        assert_eq!(pixels.len(), 3 * PAGE * PAGE, "a page is 256 by 256 pixels");
        (0..self.cells())
            .map(|cell| {
                let (x, y) = self.corner(cell);
                (0..self.cell.1)
                    .flat_map(|row| {
                        let at = 3 * ((y + row) * PAGE + x);
                        pixels[at..at + 3 * self.cell.0].iter().copied()
                    })
                    .collect()
            })
            .collect()
    }

    /// A page with these cells in place of its own; the rest of it is
    /// left as it is.
    pub fn page(&self, page: &[u8], cells: &Cells) -> Vec<u8> {
        let mut pixels = page.to_vec();
        for (cell, picture) in cells.iter().enumerate() {
            let (x, y) = self.corner(cell);
            for row in 0..self.cell.1 {
                let at = 3 * ((y + row) * PAGE + x);
                pixels[at..at + 3 * self.cell.0]
                    .copy_from_slice(&picture[3 * self.cell.0 * row..][..3 * self.cell.0]);
            }
        }
        pixels
    }

    /// The atlas, as a page: the first source's page with, after its
    /// common cells, every sign in its place, each taken from the first
    /// source that has it; a sign none has is an empty cell, and so are
    /// the cells after the last place. Also says which signs are there.
    ///
    /// An accent from a page whose build draws it higher or lower than
    /// this typeface does is moved in its cell by as much (a screen unit
    /// is two pixels of the page), so that it stands where that build
    /// has it.
    pub fn atlas(&self, sources: &[Source]) -> Result<(Vec<u8>, Vec<Sign>), String> {
        let first = sources.first().ok_or("no page of the typeface")?;
        let cut: Vec<Cells> = sources.iter().map(|source| self.cut(&source.pixels)).collect();
        // The colour of nothing: the first pixel of a cell, which no
        // glyph reaches.
        let background = cut[0][0][..3].to_vec();
        let empty: Vec<u8> = background.iter().copied().cycle().take(3 * self.cell.0 * self.cell.1).collect();
        let mut atlas: Cells = cut[0][..self.common].to_vec();
        let mut present = Vec::new();
        for &place in self.places {
            let found = place.and_then(|sign| {
                sources.iter().zip(&cut).find_map(|(source, cells)| {
                    let at = source.page.signs.iter().position(|&held| held == Some(sign))?;
                    Some((sign, &cells[self.common + at], source.page.accent_rise))
                })
            });
            let Some((sign, cell, rise)) = found else {
                atlas.push(empty.clone());
                continue;
            };
            let up = if sign.is_accent() { 2 * (rise - self.accent_rise) } else { 0 };
            atlas.push(self.raised(cell, up, &background).ok_or_else(|| {
                format!("the {} typeface's {sign:?} cannot be moved by {up} in its cell", self.name)
            })?);
            present.push(sign);
        }
        // A sign that would show as a line over the cell under it, and
        // has no empty cell to stand over, is moved up in its cell until
        // its bottom row is free: by a pixel of the page, half a screen
        // unit, in every case seen.
        for cell in self.bleeding(&atlas) {
            let lifted = (1..=2).find_map(|up| {
                let lifted = self.raised(&atlas[cell], up, &background)?;
                let bottom = &lifted[3 * self.cell.0 * (self.cell.1 - 1)..];
                bottom.chunks(3).all(|pixel| pixel == &background[..]).then_some(lifted)
            });
            atlas[cell] = lifted.ok_or_else(|| {
                format!(
                    "in the {} typeface's atlas, cell {cell} would show as a line over the cell under it",
                    self.name
                )
            })?;
        }
        atlas.resize(self.cells(), empty);
        Ok((self.page(&first.pixels, &atlas), present))
    }

    /// A cell's picture moved up by some rows (down, if negative); `None`
    /// if a row that is not empty would leave the cell.
    fn raised(&self, cell: &[u8], up: i32, background: &[u8]) -> Option<Vec<u8>> {
        let (row_bytes, rows) = (3 * self.cell.0, self.cell.1 as i32);
        let is_empty = |row: i32| {
            cell[row as usize * row_bytes..][..row_bytes].chunks(3).all(|pixel| pixel == background)
        };
        if (0..rows).any(|row| !(0..rows).contains(&(row - up)) && !is_empty(row)) {
            return None;
        }
        let mut moved = Vec::with_capacity(cell.len());
        for row in 0..rows {
            let from = row + up;
            if (0..rows).contains(&from) {
                moved.extend_from_slice(&cell[from as usize * row_bytes..][..row_bytes]);
            } else {
                moved.extend(background.iter().copied().cycle().take(row_bytes));
            }
        }
        Some(moved)
    }

    /// The cells after the common ones whose bottom row of pixels is used
    /// while the cell under them is not empty: each would show as a line
    /// over that cell's sprite. Under the last row is the first, where the
    /// typeface fills its page (the picture repeats), and otherwise what
    /// else the page holds, taken as not empty.
    pub fn bleeding(&self, atlas: &Cells) -> Vec<usize> {
        let background = &atlas[0][..3];
        let is_empty =
            |cell: usize| atlas.get(cell).is_none_or(|cell| cell.chunks(3).all(|pixel| pixel == background));
        (self.common..atlas.len())
            .filter(|&cell| {
                let last_row = &atlas[cell][3 * self.cell.0 * (self.cell.1 - 1)..];
                let under = cell + self.across;
                let under_is_empty = if under < self.cells() {
                    is_empty(under)
                } else {
                    self.rows * self.cell.1 == PAGE && is_empty(under % self.cells())
                };
                last_row.chunks(3).any(|pixel| pixel != background) && !under_is_empty
            })
            .collect()
    }

    /// The record of the typeface's sheet of sprites for the atlas: eight
    /// bytes as the reference's begin (the texture's number, the cell's
    /// width and height), then where each cell of the page is.
    pub fn sheet_record(&self) -> Vec<u8> {
        let mut record = vec![self.texture, 0, self.cell.0 as u8, self.cell.1 as u8, 0, 0, 0, 0];
        for cell in 0..self.cells() {
            let (x, y) = self.corner(cell);
            record.extend([x as u8, y as u8]);
        }
        record
    }

    /// Gives the reference build's image the record of the atlas, with
    /// all the page's cells, in place of its own.
    pub fn install_sheet_record(&self, image: &mut Image) -> Result<(), String> {
        if image.read_u32(self.record_site) != Some(self.record) {
            return Err(format!(
                "the {} typeface's sheet is not where the reference build has it",
                self.name
            ));
        }
        let end = image.sections.iter().map(|section| section.va + section.virtual_size).max();
        let va = end.unwrap_or(image.base).next_multiple_of(0x1000);
        let data = self.sheet_record();
        image.sections.push(Section {
            name: self.section.into(),
            va,
            virtual_size: data.len() as u32,
            flags: 0x4000_0040, // initialised data, readable
            data,
        });
        let section = image.sections.iter_mut().find(|section| section.contains(self.record_site));
        let section = section.expect("the site was just read");
        let offset = (self.record_site - section.va) as usize;
        section.data[offset..offset + 4].copy_from_slice(&va.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_character_has_one_glyph_whatever_the_language() {
        let plain = |cell| Some(Glyph { cell, accent: None });
        assert_eq!(LARGE.glyph(b'a'), plain(0));
        assert_eq!(LARGE.glyph(b'Z'), plain(25));
        assert_eq!(LARGE.glyph(b'7'), plain(33));
        assert_eq!(LARGE.glyph(b'/'), plain(41));
        assert_eq!(LARGE.glyph(b' '), None);
        // The signs beyond the common cells, each in its place.
        assert_eq!(LARGE.glyph(b'-'), plain(63));
        assert_eq!(LARGE.glyph(0xBF), plain(51));
        assert_eq!(LARGE.glyph(0xE7), plain(50));
        // Letters with accents, small and capital.
        assert_eq!(LARGE.glyph(0xE0), Some(Glyph { cell: 0, accent: Some(56) }));
        assert_eq!(LARGE.glyph(0xC9), Some(Glyph { cell: 4, accent: Some(57) }));
        assert_eq!(LARGE.glyph(0xF1), Some(Glyph { cell: 13, accent: Some(61) }));
        assert_eq!(LARGE.glyph(0xFC), Some(Glyph { cell: 20, accent: Some(60) }));
        assert_eq!(LARGE.glyph(0xEE), Some(Glyph { cell: 8, accent: Some(59) }));
        assert_eq!(LARGE.glyph(0xE5), Some(Glyph { cell: 0, accent: Some(62) }));
        // The small typeface has its signs elsewhere, the arrows for the
        // capitals A to D, and signs the large one lacks.
        assert_eq!(SMALL.glyph(b'b'), plain(1));
        assert_eq!(
            (SMALL.glyph(b'A'), SMALL.glyph(b'D'), SMALL.glyph(b'E')),
            (plain(45), plain(48), plain(4))
        );
        assert_eq!(
            (SMALL.glyph(b'0'), SMALL.glyph(b'1'), SMALL.glyph(b'9')),
            (plain(44), plain(35), plain(43))
        );
        assert_eq!(SMALL.glyph(b'-'), plain(49));
        assert_eq!(SMALL.glyph(0xE8), Some(Glyph { cell: 4, accent: Some(52) }));
        assert_eq!(
            (SMALL.glyph(b'\''), SMALL.glyph(b'$'), SMALL.glyph(0xA7)),
            (plain(65), plain(67), plain(63))
        );
        assert_eq!(LARGE.glyph(b'$'), None);
        // The levels' typeface: its own signs, and accents too.
        assert_eq!(
            (LEVELS.glyph(b'!'), LEVELS.glyph(b'-'), LEVELS.glyph(b'*')),
            (plain(41), plain(49), plain(50))
        );
        assert_eq!(LEVELS.glyph(0xF1), Some(Glyph { cell: 13, accent: Some(55) }));
        assert_eq!(LEVELS.glyph(0xA1), plain(57));
        for face in FACES {
            // Every sign has one place, and the places fit the page.
            let placed: Vec<Sign> = face.places.iter().flatten().copied().collect();
            assert!((0..placed.len()).all(|one| placed[one + 1..].iter().all(|&other| other != placed[one])));
            assert!(face.common + face.places.len() <= face.cells());
            assert!(face.across * face.cell.0 <= PAGE && face.rows * face.cell.1 <= PAGE);
            // Every known page's signs have a place in the atlas.
            for page in PAGES.iter().chain(LEVELS_PAGES).filter(|page| std::ptr::eq(page.face, face)) {
                assert!(page.signs.iter().flatten().all(|&sign| face.cell_of(sign).is_some()));
                assert!(face.common + page.signs.len() <= face.cells());
            }
        }
        assert!(known_page(PAGES[0].sha256).is_some() && known_page("").is_none());
    }

    /// A page whose every pixel is nothing (zero) but for the typeface's
    /// cells: cell `n` is filled with the value `base + n`, save its
    /// first pixel.
    fn filled(face: &Face, base: u8) -> Vec<u8> {
        let cells: Cells = (0..face.cells())
            .map(|cell| {
                let mut pixels = vec![base.wrapping_add(cell as u8); 3 * face.cell.0 * face.cell.1];
                pixels[..3].fill(0);
                pixels
            })
            .collect();
        face.page(&vec![0; 3 * PAGE * PAGE], &cells)
    }

    #[test]
    fn a_page_is_cut_into_cells_and_put_together_again() {
        for face in FACES {
            let pixels = filled(face, 1);
            let cut = face.cut(&pixels);
            assert_eq!(cut.len(), face.across * face.rows);
            assert!(face.page(&pixels, &cut) == pixels);
            // What the page holds beside the typeface stays.
            let mut other = pixels.clone();
            *other.last_mut().unwrap() = 9;
            assert_eq!(
                face.page(&other, &cut).last(),
                Some(&9).filter(|_| face.rows * face.cell.1 < PAGE).or(pixels.last())
            );
        }
        // The second cell of the second row of the small typeface begins
        // at pixel (16, 24).
        let pixels = filled(&SMALL, 1);
        assert_eq!(pixels[3 * (24 * 256 + 16) + 3], 1 + 17);
        // The levels' typeface is a corner of its page.
        let pixels = filled(&LEVELS, 1);
        assert_eq!((pixels[3 * (16 * 256 + 16) + 3], pixels[3 * (16 * 256 + 128) + 3]), (1 + 9, 0));
    }

    static FIRST: KnownPage =
        KnownPage { face: &LARGE, sha256: "", signs: &[None, Some(Grave), Some(Hyphen)], accent_rise: 4 };
    static SECOND: KnownPage =
        KnownPage { face: &LARGE, sha256: "", signs: &[Some(Hyphen), Some(Tilde)], accent_rise: 5 };

    #[test]
    fn the_atlas_has_every_sign_once_from_whichever_disc_has_it() {
        // Common cells alike; after them each its own. Their first two
        // and last rows are nothing, so that an accent can be moved.
        let page = |base: u8| {
            let mut cells = LARGE.cut(&filled(&LARGE, 1));
            for (number, cell) in cells.iter_mut().enumerate().skip(48) {
                cell.fill(base.wrapping_add(number as u8));
                cell[..3 * 32 * 2].fill(0);
                cell[3 * 32 * 31..].fill(0);
            }
            LARGE.page(&vec![0; 3 * PAGE * PAGE], &cells)
        };
        let sources =
            [Source { pixels: page(100), page: &FIRST }, Source { pixels: page(200), page: &SECOND }];
        let (atlas, present) = LARGE.atlas(&sources).unwrap();
        let atlas = LARGE.cut(&atlas);
        assert_eq!(atlas.len(), 64);
        // The common cells are the first disc's.
        assert!(atlas[..48] == LARGE.cut(&sources[0].pixels)[..48]);
        let cell = |sign| &atlas[LARGE.cell_of(sign).unwrap() as usize];
        // The hyphen from the first disc's cell 50, though the second has it too.
        assert_eq!(cell(Hyphen)[3 * 32 * 2], 150);
        assert_eq!(cell(Grave)[3 * 32 * 2], 149);
        // The tilde from the second disc, whose build draws accents one
        // unit higher: two rows up in its cell.
        assert_eq!((cell(Tilde)[0], cell(Tilde)[3 * 32 * 29 - 3], cell(Tilde)[3 * 32 * 29]), (249, 249, 0));
        // What no disc has is empty.
        assert!(cell(Acute).iter().all(|&byte| byte == 0));
        assert_eq!(present, [Grave, Tilde, Hyphen]);
        // A sign reaching the bottom of its cell over a cell that is not
        // empty is lifted by a pixel; one that cannot be is an error.
        let mut low = LARGE.cut(&page(100));
        low[49][3 * 32 * 31..].fill(7);
        let lowered =
            |cells: &Cells| Source { pixels: LARGE.page(&vec![0; 3 * PAGE * PAGE], cells), page: &FIRST };
        let (atlas, _) = LARGE.atlas(&[lowered(&low)]).unwrap();
        let grave = &LARGE.cut(&atlas)[LARGE.cell_of(Grave).unwrap() as usize];
        assert!(grave[3 * 32 * 31..].iter().all(|&byte| byte == 0) && grave[3 * 32 * 30..][..3] == [7; 3]);
        low[49].fill(7);
        assert!(LARGE.atlas(&[lowered(&low)]).is_err());
    }

    #[test]
    fn a_cell_that_would_show_as_a_line_over_another_is_found() {
        let blank = || -> Cells { vec![vec![0u8; 3 * 32 * 32]; 64] };
        let mut cells = blank();
        // A sign in the lower row reaching its bottom, a letter in the
        // first row under it (the picture repeats).
        cells[62][3 * 32 * 31] = 9;
        cells[6][3] = 9;
        assert_eq!(LARGE.bleeding(&cells), [62]);
        // In the upper row, over an empty place: nothing.
        let mut cells = blank();
        cells[50][3 * 32 * 31] = 9;
        assert!(LARGE.bleeding(&cells).is_empty());
        cells[58][5] = 9;
        assert_eq!(LARGE.bleeding(&cells), [50]);
    }

    #[test]
    fn the_sheet_record_places_all_the_cells() {
        let record = LARGE.sheet_record();
        assert_eq!(record.len(), 8 + 2 * 64);
        assert_eq!(record[..8], [0x1F, 0, 32, 32, 0, 0, 0, 0]);
        assert_eq!(record[8..12], [0, 0, 32, 0]);
        assert_eq!(record[8 + 2 * 9..][..2], [32, 32]);
        assert_eq!(record[8 + 2 * 63..], [224, 224]);
        let record = SMALL.sheet_record();
        assert_eq!(record.len(), 8 + 2 * 160);
        assert_eq!(record[..8], [10, 0, 16, 24, 0, 0, 0, 0]);
        assert_eq!(record[8 + 2 * 17..][..2], [16, 24]);
        assert_eq!(record[8 + 2 * 159..], [240, 216]);
    }
}
