//! The game's text, drawn one way for every language (ADR 0016).
//!
//! The reference build has a function for each use of each typeface, and
//! each knows only the signs English needs; the builds of other languages
//! have their own, each different. Here is one table from character to
//! glyph and one way of laying a text out, in place of those functions.
//!
//! So far: the large typeface of the menus. Its table and its one atlas
//! are in `andy_recomp::typeface`; the atlas is put together from the
//! discs at hand when the game is started, and written into the texture
//! files as they are read.
//!
//! A text may hold line breaks; and one that is too wide for its room is
//! set closer.

use andy_recomp::typeface::{ACCENT_RISE, glyph};

use crate::game::{DRAW_SPRITE, SCREEN_MIDDLE};
use crate::{Machine, Stop};

/// The sheet of sprites of the large typeface.
const LARGE_SHEET: u32 = 0x32;
/// How far apart the reference sets its letters, and its lines (the
/// entries of its menus are this far apart).
pub const LARGE_STEP: i32 = 12;
const LARGE_LINE: i32 = 20;
/// The widest a line of the large typeface may be: what leaves room on a
/// screen 320 wide for the arrows a menu draws beside it.
pub const LARGE_WIDEST: i32 = 240;

/// A line of the large typeface, centred: where its first letter goes and
/// how far its letters are apart, in 256ths. Twelve apart as the reference
/// has them, or closer if the line would be wider than there is room.
pub fn large_line(letters: usize) -> (i32, i32) {
    let letters = letters as i32;
    let step =
        if letters * LARGE_STEP > LARGE_WIDEST { 256 * LARGE_WIDEST / letters } else { 256 * LARGE_STEP };
    (SCREEN_MIDDLE - letters * step / 512, step)
}

/// Half the width of the widest line of a text in the large typeface.
pub fn large_half_width(text: &[u8]) -> i32 {
    text.split(|&byte| byte == b'\n')
        .map(|line| {
            let (first, _) = large_line(line.len());
            SCREEN_MIDDLE - first
        })
        .max()
        .unwrap_or(0)
}

/// The bytes of a text of the game, without its end.
pub fn text_at(m: &mut Machine, mut at: u32) -> Vec<u8> {
    let mut text = Vec::new();
    while let Some(byte) = m.memory.read(at, 1).filter(|&byte| byte != 0) {
        text.push(byte as u8);
        at += 1;
    }
    text
}

/// In place of the reference's function at 0x49B580: a text in the large
/// typeface, centred on the screen at a height, in a shade. Its
/// arguments: the height, the text, the shade (0x80 for the usual one).
///
/// Several lines stand around that height, 20 apart.
pub fn draw_large_centred(m: &mut Machine) -> Result<(), Stop> {
    let esp = m.cpu.regs[4];
    let read = |m: &mut Machine, at: u32| m.game_word(at);
    let (y, text, shade) = (read(m, esp + 4)? as i32, read(m, esp + 8)?, read(m, esp + 12)?);
    let text = text_at(m, text);
    // What the reference passes on for the two kinds of shade.
    let look: [u32; 4] = if shade == 0x80 {
        [0x80, 0x80, 0x80, 0xFF]
    } else {
        [0xFF, 0xFF, 0xFF, (shade << 9).wrapping_add(0x60)]
    };
    let lines: Vec<&[u8]> = text.split(|&byte| byte == b'\n').collect();
    let top = y - LARGE_LINE * (lines.len() as i32 - 1) / 2;
    for (number, line) in lines.iter().enumerate() {
        let (first, step) = large_line(line.len());
        for (place, &character) in line.iter().enumerate() {
            let Some(glyph) = glyph(character) else { continue };
            let x = first + place as i32 * step / 256;
            let line_y = top + LARGE_LINE * number as i32;
            // The letter, and its accent over it.
            let accent = glyph.accent.map(|accent| (accent.cell(), line_y - ACCENT_RISE));
            for (cell, y) in [(glyph.cell, line_y)].into_iter().chain(accent) {
                let arguments =
                    [x as u32, y as u32, LARGE_SHEET, cell, look[0], look[1], look[2], look[3], 0x800, 0x800];
                m.call_guest_c(DRAW_SPRITE, &arguments)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_centred_and_set_closer_if_it_must() {
        // Ten letters: from 100, twelve apart, as the reference does.
        assert_eq!(large_line(10), (100, 256 * 12));
        assert_eq!(large_line(0), (160, 256 * 12));
        // Twenty is the most that fits at twelve apart.
        assert_eq!(large_line(20), (40, 256 * 12));
        let (first, step) = large_line(24);
        assert_eq!((first, step), (40, 2560));
        assert_eq!(first + 23 * step / 256, 270);
        assert_eq!(large_half_width(b"start game"), 60);
        assert_eq!(large_half_width(b"a\nstart game\nab"), 60);
    }
}
