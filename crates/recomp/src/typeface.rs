//! The one atlas of the large typeface, and the one table from character
//! to glyph (ADR 0016).
//!
//! The large typeface is a page of 64 cells of 32 by 32 pixels. On every
//! language's disc the first 48 cells are the same picture: the letters,
//! the digits and twelve signs. The last 16 hold what each language added,
//! each in its own places and sometimes drawn a little differently: the
//! accents, drawn over a letter, and a few signs of their own.
//!
//! Here every sign has one place. The atlas is put together, when the game
//! is built, from the pages of all the discs at hand; a sign that no disc
//! at hand has stays empty. The table says, for every character of
//! Latin-1, which cell to draw, and which accent over it: it is the same
//! for every language, so any text can use any sign.

/// The cells of a page, across and down, and a cell's side in pixels.
pub const CELLS_ACROSS: usize = 8;
pub const CELLS: usize = 64;
pub const CELL: usize = 32;
/// The cells that are the same on every disc.
pub const COMMON_CELLS: usize = 48;

/// What a cell beyond the common ones holds. Their order is their place in
/// the atlas: the first is cell 48.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sign {
    /// The two arrows every disc has in its cells 48 and 49.
    ArrowRight,
    ArrowLeft,
    Grave,
    Acute,
    Circumflex,
    Diaeresis,
    Tilde,
    Ring,
    Hyphen,
    InvertedQuestion,
    InvertedExclamation,
    OSlash,
    Ae,
    Oe,
    CCedilla,
}

/// The places of the signs in the atlas, from cell 48 on: two rows of
/// eight. A sprite is drawn from the very edge of its cell, and the
/// picture repeats, so a cell whose bottom row of pixels is used shows as
/// a line over the cell under it (under the last row is the first: the
/// letters A to H). The cedilla reaches the bottom of its cell: it stands
/// in the upper row, over the one place left empty. [`bleeding`] checks
/// that no disc's pictures break this.
pub const PLACES: [Option<Sign>; 16] = [
    Some(Sign::ArrowRight),
    Some(Sign::ArrowLeft),
    Some(Sign::CCedilla),
    Some(Sign::InvertedQuestion),
    Some(Sign::InvertedExclamation),
    Some(Sign::OSlash),
    Some(Sign::Ae),
    Some(Sign::Oe),
    Some(Sign::Grave),
    Some(Sign::Acute),
    None,
    Some(Sign::Circumflex),
    Some(Sign::Diaeresis),
    Some(Sign::Tilde),
    Some(Sign::Ring),
    Some(Sign::Hyphen),
];

impl Sign {
    /// The sign's cell in the atlas.
    pub fn cell(self) -> u32 {
        let place = PLACES.iter().position(|&place| place == Some(self)).expect("every sign has a place");
        (COMMON_CELLS + place) as u32
    }

    /// Whether it is drawn over a letter.
    pub fn is_accent(self) -> bool {
        matches!(
            self,
            Sign::Grave | Sign::Acute | Sign::Circumflex | Sign::Diaeresis | Sign::Tilde | Sign::Ring
        )
    }
}

/// How much higher than its letter an accent is drawn, in the game's
/// screen units: what the builds that have accents do.
pub const ACCENT_RISE: i32 = 4;

/// What is drawn for a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub cell: u32,
    /// The accent drawn over it, [`ACCENT_RISE`] higher.
    pub accent: Option<Sign>,
}

/// The sign a character is, if it is one of the signs beyond the common
/// cells and stands alone.
pub fn sign_of(character: u8) -> Option<Sign> {
    Some(match character {
        b'-' => Sign::Hyphen,
        0xBF => Sign::InvertedQuestion,
        0xA1 => Sign::InvertedExclamation,
        0xD8 | 0xF8 => Sign::OSlash,
        0xC6 | 0xE6 => Sign::Ae,
        0x8C | 0x9C => Sign::Oe,
        0xC7 | 0xE7 => Sign::CCedilla,
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
        0xE0 | 0xE8 | 0xEC | 0xF2 | 0xF9 => Sign::Grave,
        0xE1 | 0xE9 | 0xED | 0xF3 | 0xFA | 0xFD => Sign::Acute,
        0xE2 | 0xEA | 0xEE | 0xF4 | 0xFB => Sign::Circumflex,
        0xE4 | 0xEB | 0xEF | 0xF6 | 0xFC | 0xFF => Sign::Diaeresis,
        0xE3 | 0xF1 | 0xF5 => Sign::Tilde,
        _ => Sign::Ring,
    };
    Some((letters[(small - first) as usize], accent))
}

/// What is drawn for a character (Latin-1). `None` for a space and for
/// what the typeface has nothing for.
pub fn glyph(character: u8) -> Option<Glyph> {
    let plain = |cell: u8| Some(Glyph { cell: cell as u32, accent: None });
    match character {
        b'a'..=b'z' => plain(character - b'a'),
        b'A'..=b'Z' => plain(character - b'A'),
        b'0'..=b'9' => plain(26 + character - b'0'),
        b'.' => plain(36),
        b',' => plain(37),
        b';' => plain(38),
        b':' => plain(39),
        b'\\' => plain(40),
        b'/' => plain(41),
        b'!' => plain(43),
        b'?' => plain(44),
        b'(' => plain(45),
        b')' => plain(46),
        // The reference's own function draws this cell for an apostrophe.
        b'\'' => plain(47),
        _ => match (sign_of(character), accented(character)) {
            (Some(sign), _) => Some(Glyph { cell: sign.cell(), accent: None }),
            (None, Some((letter, accent))) => {
                Some(Glyph { cell: (letter - b'a') as u32, accent: Some(accent) })
            }
            (None, None) => None,
        },
    }
}

/// A page of the typeface: its cells, each as its pixels (blue, green,
/// red, rows from the top).
pub type Cells = Vec<Vec<u8>>;

/// Cuts a page of 256 by 256 pixels into its cells.
pub fn cells(pixels: &[u8]) -> Cells {
    let side = CELLS_ACROSS * CELL;
    assert_eq!(pixels.len(), 3 * side * side, "a page is 256 by 256 pixels");
    (0..CELLS)
        .map(|cell| {
            let (across, down) = (cell % CELLS_ACROSS, cell / CELLS_ACROSS);
            (0..CELL)
                .flat_map(|row| {
                    let at = 3 * ((down * CELL + row) * side + across * CELL);
                    pixels[at..at + 3 * CELL].iter().copied()
                })
                .collect()
        })
        .collect()
}

/// Puts cells together into a page.
pub fn page(cells: &Cells) -> Vec<u8> {
    let side = CELLS_ACROSS * CELL;
    let mut pixels = vec![0u8; 3 * side * side];
    for (cell, picture) in cells.iter().enumerate() {
        let (across, down) = (cell % CELLS_ACROSS, cell / CELLS_ACROSS);
        for row in 0..CELL {
            let at = 3 * ((down * CELL + row) * side + across * CELL);
            pixels[at..at + 3 * CELL].copy_from_slice(&picture[3 * CELL * row..][..3 * CELL]);
        }
    }
    pixels
}

/// Whether a picture of 256 by 256 is a page of the large typeface: its
/// common cells are those of the reference's page.
pub fn is_page(pixels: &[u8], reference: &Cells) -> bool {
    pixels.len() == 3 * (CELLS_ACROSS * CELL).pow(2)
        && cells(pixels)[..COMMON_CELLS] == reference[..COMMON_CELLS]
}

/// A disc's page with what its build draws from it: for each sign, the
/// cell of this page that holds it.
pub struct Source {
    pub cells: Cells,
    pub signs: Vec<(Sign, usize)>,
}

/// The atlas: the reference's common cells, then every sign in its place,
/// each taken from the first source that has it; a sign none has is an
/// empty cell (the reference's last one, which is empty on every disc).
/// Also says which signs are there.
pub fn atlas(reference: &Cells, sources: &[Source]) -> (Cells, Vec<Sign>) {
    let empty = reference[CELLS - 1].clone();
    let mut atlas: Cells = reference[..COMMON_CELLS].to_vec();
    let mut present = Vec::new();
    for place in PLACES {
        let found = place.and_then(|sign| {
            let cell = sources.iter().find_map(|source| {
                source.signs.iter().find(|&&(held, _)| held == sign).map(|&(_, cell)| &source.cells[cell])
            })?;
            present.push(sign);
            Some(cell.clone())
        });
        atlas.push(found.unwrap_or_else(|| empty.clone()));
    }
    (atlas, present)
}

/// The cells of the two last rows whose bottom row of pixels is used
/// while the cell under them (the first row's, under the last) is not
/// empty: each would show as a line over that cell's sprite.
pub fn bleeding(atlas: &Cells) -> Vec<usize> {
    // The colour of nothing: the first pixel of any cell, which no glyph
    // reaches.
    let background = &atlas[0][..3];
    (COMMON_CELLS..CELLS)
        .filter(|&cell| {
            let last_row = &atlas[cell][3 * CELL * (CELL - 1)..];
            let under = &atlas[(cell + CELLS_ACROSS) % CELLS];
            last_row.chunks(3).any(|pixel| pixel != background)
                && under.chunks(3).any(|pixel| pixel != background)
        })
        .collect()
}

/// The record of the typeface's sheet of sprites for the atlas: eight
/// bytes as the reference's begin (the texture's number, the cell's
/// width and height), then where each of the 64 cells is.
pub fn sheet_record(texture: u8) -> Vec<u8> {
    let mut record = vec![texture, 0, CELL as u8, CELL as u8, 0, 0, 0, 0];
    for cell in 0..CELLS {
        record.push(((cell % CELLS_ACROSS) * CELL) as u8);
        record.push(((cell / CELLS_ACROSS) * CELL) as u8);
    }
    record
}

/// Where the reference build's code holds the address of the record of
/// the large typeface's sheet, and that record's address: 8 bytes and 48
/// places, for a page of which it uses 48 cells.
const SHEET_RECORD_SITE: u32 = 0x004F_6B48;
const SHEET_RECORD: u32 = 0x0050_0CC8;
/// The texture the sheet is drawn from.
pub const SHEET_TEXTURE: u8 = 0x1F;

/// Gives the reference build's image the record of the atlas, with all 64
/// cells, in place of its own.
pub fn install_sheet_record(image: &mut crate::pe::Image) -> Result<(), String> {
    use crate::pe::Section;
    if image.read_u32(SHEET_RECORD_SITE) != Some(SHEET_RECORD) {
        return Err("the large typeface's sheet is not where the reference build has it".into());
    }
    let end = image.sections.iter().map(|section| section.va + section.virtual_size).max();
    let va = end.unwrap_or(image.base).next_multiple_of(0x1000);
    let data = sheet_record(SHEET_TEXTURE);
    image.sections.push(Section {
        name: ".type".into(),
        va,
        virtual_size: data.len() as u32,
        flags: 0x4000_0040, // initialised data, readable
        data,
    });
    let section = image.sections.iter_mut().find(|section| section.contains(SHEET_RECORD_SITE));
    let section = section.expect("the site was just read");
    let offset = (SHEET_RECORD_SITE - section.va) as usize;
    section.data[offset..offset + 4].copy_from_slice(&va.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_character_has_one_glyph_whatever_the_language() {
        assert_eq!(glyph(b'a'), Some(Glyph { cell: 0, accent: None }));
        assert_eq!(glyph(b'Z').unwrap().cell, 25);
        assert_eq!(glyph(b'7').unwrap().cell, 33);
        assert_eq!(glyph(b'/').unwrap().cell, 41);
        assert_eq!(glyph(b' '), None);
        // The signs beyond the common cells, each in its place.
        assert_eq!((Sign::ArrowRight.cell(), Sign::CCedilla.cell(), Sign::Hyphen.cell()), (48, 50, 63));
        assert_eq!(glyph(b'-'), Some(Glyph { cell: Sign::Hyphen.cell(), accent: None }));
        assert_eq!(glyph(0xBF).unwrap().cell, Sign::InvertedQuestion.cell());
        // Letters with accents, small and capital.
        assert_eq!(glyph(0xE0), Some(Glyph { cell: 0, accent: Some(Sign::Grave) }));
        assert_eq!(glyph(0xC9), Some(Glyph { cell: 4, accent: Some(Sign::Acute) }));
        assert_eq!(glyph(0xF1), Some(Glyph { cell: 13, accent: Some(Sign::Tilde) }));
        assert_eq!(glyph(0xFC), Some(Glyph { cell: 20, accent: Some(Sign::Diaeresis) }));
        assert_eq!(glyph(0xEE), Some(Glyph { cell: 8, accent: Some(Sign::Circumflex) }));
        assert_eq!(glyph(0xE5), Some(Glyph { cell: 0, accent: Some(Sign::Ring) }));
        assert_eq!(glyph(0xE7).unwrap(), Glyph { cell: Sign::CCedilla.cell(), accent: None });
        // Every sign has one place, and the place under the cedilla is empty.
        let placed: Vec<Sign> = PLACES.iter().flatten().copied().collect();
        assert_eq!(placed.len(), 15);
        assert!((0..placed.len()).all(|one| placed[one + 1..].iter().all(|&other| other != placed[one])));
        assert_eq!(PLACES[(Sign::CCedilla.cell() as usize - COMMON_CELLS) + CELLS_ACROSS], None);
    }

    /// A page whose cell `n` is filled with the value `base + n`.
    fn filled(base: u8) -> Cells {
        (0..CELLS).map(|cell| vec![base.wrapping_add(cell as u8); 3 * CELL * CELL]).collect()
    }

    #[test]
    fn a_page_is_cut_into_cells_and_put_together_again() {
        let cut = filled(0);
        let pixels = page(&cut);
        assert_eq!(pixels.len(), 3 * 256 * 256);
        // The second cell of the second row begins at pixel (32, 32).
        assert_eq!(pixels[3 * (32 * 256 + 32)], 9);
        assert_eq!(cells(&pixels), cut);
        assert!(is_page(&pixels, &cut));
        let mut other = cut.clone();
        other[3][0] ^= 1;
        assert!(!is_page(&page(&other), &cut));
        // What differs beyond the common cells does not matter.
        other = cut.clone();
        other[50][0] ^= 1;
        assert!(is_page(&page(&other), &cut));
    }

    #[test]
    fn the_atlas_has_every_sign_once_from_whichever_disc_has_it() {
        let reference = filled(0);
        // One disc has the hyphen in its cell 62 and the grave accent in
        // 50; another has the hyphen too, elsewhere, and the tilde.
        let first = Source { cells: filled(100), signs: vec![(Sign::Hyphen, 62), (Sign::Grave, 50)] };
        let second = Source { cells: filled(200), signs: vec![(Sign::Hyphen, 55), (Sign::Tilde, 51)] };
        let (atlas, present) = atlas(&reference, &[first, second]);
        assert_eq!(atlas.len(), CELLS);
        assert!(atlas[..COMMON_CELLS] == reference[..COMMON_CELLS]);
        assert_eq!(atlas[Sign::Hyphen.cell() as usize][0], 100 + 62);
        assert_eq!(atlas[Sign::Grave.cell() as usize][0], 100 + 50);
        assert_eq!(atlas[Sign::Tilde.cell() as usize][0], 200 + 51);
        // What no disc has is empty, like the reference's last cell.
        assert!(atlas[Sign::Acute.cell() as usize] == reference[CELLS - 1]);
        assert!(atlas[58] == reference[CELLS - 1], "the place left empty");
        assert_eq!(present, [Sign::Grave, Sign::Tilde, Sign::Hyphen]);
    }

    #[test]
    fn a_cell_that_would_show_as_a_line_over_another_is_found() {
        // Background everywhere but where a test puts something.
        let blank = || -> Cells { vec![vec![0u8; 3 * CELL * CELL]; CELLS] };
        let mut cells = blank();
        // A sign in the lower row reaching its bottom, a letter in the
        // first row under it (the picture repeats).
        cells[62][3 * CELL * (CELL - 1)] = 9;
        cells[6][0] = 9;
        assert_eq!(bleeding(&cells), [62]);
        // In the upper row, over an empty place: nothing.
        let mut cells = blank();
        cells[50][3 * CELL * (CELL - 1)] = 9;
        assert!(bleeding(&cells).is_empty());
        cells[58][5] = 9;
        assert_eq!(bleeding(&cells), [50]);
    }

    #[test]
    fn the_sheet_record_places_all_the_cells() {
        let record = sheet_record(0x1F);
        assert_eq!(record.len(), 8 + 2 * 64);
        assert_eq!(record[..8], [0x1F, 0, 32, 32, 0, 0, 0, 0]);
        assert_eq!(record[8..12], [0, 0, 32, 0]);
        assert_eq!(record[8 + 2 * 9..][..2], [32, 32]);
        assert_eq!(record[8 + 2 * 63..], [224, 224]);
    }
}
