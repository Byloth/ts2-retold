//! The game's text, drawn one way for every language (ADR 0016).
//!
//! The reference build has a function for each use of each typeface, and
//! each knows only the signs English needs; the builds of other languages
//! have their own, each different. Here is one table from character to
//! glyph and one way of laying a text out, in place of those functions.
//!
//! The three typefaces: the large one of the menus, the small one, and
//! the one of the levels' texts, whose layout stays the game's own. Their
//! tables and atlases are in `andy_recomp::typeface`; the atlases are put
//! together from the discs at hand when the game is started, and written
//! into the texture files as they are read.
//!
//! A text may hold line breaks; and one that is too wide for its room is
//! set closer.

use andy_recomp::typeface::{Face, LARGE, LEVELS, SMALL};

use crate::game::{DRAW_SPRITE, SCREEN_MIDDLE};
use crate::{Machine, Stop};

/// How far apart the reference sets the letters of the large typeface,
/// and its lines (the entries of its menus are this far apart).
pub const LARGE_STEP: i32 = 12;
const LARGE_LINE: i32 = 20;
/// The widest a line of the large typeface may be: what leaves room on a
/// screen 320 wide for the arrows a menu draws beside it.
pub const LARGE_WIDEST: i32 = 240;
/// The same for the small typeface at its usual size: its letters, its
/// lines (the height its cells are drawn at) and the screen's width.
const SMALL_STEP: i32 = 8;
const SMALL_LINE: i32 = 12;
const SMALL_WIDEST: i32 = 320;
/// The size a sprite is drawn at for half its picture's pixels: the
/// typefaces' usual size.
const USUAL_SIZE: i32 = 0x800;
/// The game's function that draws one of four pictures of a sheet as an
/// icon (x, y, sheet, picture), the sheet of the buttons' icons, and the
/// picture drawn for each of the characters 1 to 4 in a centred text of
/// the small typeface.
pub(crate) const DRAW_ICON: u32 = 0x0049_4820;
const BUTTONS_SHEET: u32 = 0x44;
const BUTTONS: [u32; 4] = [2, 3, 1, 0];

/// A centred line: where its first letter goes and how far its letters
/// are apart, in 256ths. As far apart as the reference has them, or
/// closer if the line would be wider than there is room.
fn centred_line(letters: usize, usual_step: i32, widest: i32) -> (i32, i32) {
    let letters = letters as i32;
    let step = if letters * usual_step > widest { 256 * widest / letters } else { 256 * usual_step };
    (SCREEN_MIDDLE - letters * step / 512, step)
}

/// A line of the large typeface, centred.
pub fn large_line(letters: usize) -> (i32, i32) {
    centred_line(letters, LARGE_STEP, LARGE_WIDEST)
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

/// Half the width of the widest line of a text in the small typeface,
/// centred on the screen at its usual size.
pub fn small_half_width(text: &[u8]) -> i32 {
    text.split(|&byte| byte == b'\n')
        .map(|line| SCREEN_MIDDLE - centred_line(line.len(), SMALL_STEP, SMALL_WIDEST).0)
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

/// How a text is drawn.
struct Setting<'a> {
    face: &'a Face,
    /// What the sprite function is given after the picture: three
    /// colours and an opacity.
    look: [u32; 4],
    /// The size the sprites are drawn at.
    size: i32,
    /// How far apart the lines are.
    line: i32,
    /// Whether the characters 1 to 4 are the buttons' icons.
    buttons: bool,
}

/// Draws a text, line by line, around the height `y`: the block of its
/// lines stands where the one line would have. `place` says, for a line
/// of so many letters, where its first goes and how far apart they are
/// (in 256ths).
fn draw(
    m: &mut Machine,
    setting: &Setting,
    text: &[u8],
    y: i32,
    place: impl Fn(usize) -> (i32, i32),
) -> Result<(), Stop> {
    let lines: Vec<&[u8]> = text.split(|&byte| byte == b'\n').collect();
    let top = y - setting.line * (lines.len() as i32 - 1) / 2;
    let size = setting.size as u32;
    for (number, line) in lines.iter().enumerate() {
        let (first, step) = place(line.len());
        let line_y = top + setting.line * number as i32;
        for (at, &character) in line.iter().enumerate() {
            let x = first + at as i32 * step / 256;
            if setting.buttons && (b'1'..=b'4').contains(&character) {
                let picture = BUTTONS[(character - b'1') as usize];
                let arguments = [(x - 4) as u32, (line_y - 3) as u32, BUTTONS_SHEET, picture];
                m.call_guest_c(DRAW_ICON, &arguments)?;
                continue;
            }
            let Some(glyph) = setting.face.glyph(character) else { continue };
            // The letter, and its accent over it.
            let rise = setting.face.accent_rise * setting.size / USUAL_SIZE;
            let accent = glyph.accent.map(|accent| (accent, line_y - rise));
            for (cell, y) in [(glyph.cell, line_y)].into_iter().chain(accent) {
                let [c1, c2, c3, opacity] = setting.look;
                let arguments =
                    [x as u32, y as u32, setting.face.sheet, cell, c1, c2, c3, opacity, size, size];
                m.call_guest_c(DRAW_SPRITE, &arguments)?;
            }
        }
    }
    Ok(())
}

/// The arguments of the function being replaced, as signed numbers.
fn arguments<const N: usize>(m: &mut Machine) -> Result<[i32; N], Stop> {
    let esp = m.cpu.regs[4];
    let mut arguments = [0; N];
    for (number, argument) in arguments.iter_mut().enumerate() {
        *argument = m.game_word(esp + 4 + 4 * number as u32)? as i32;
    }
    Ok(arguments)
}

/// In place of the reference's function at 0x49B580: a text in the large
/// typeface, centred on the screen at a height, in a shade. Its
/// arguments: the height, the text, the shade (0x80 for the usual one).
pub fn draw_large_centred(m: &mut Machine) -> Result<(), Stop> {
    let [y, text, shade] = arguments(m)?;
    let text = text_at(m, text as u32);
    // What the reference passes on for the two kinds of shade.
    let look = if shade == 0x80 {
        [0x80, 0x80, 0x80, 0xFF]
    } else {
        [0xFF, 0xFF, 0xFF, ((shade as u32) << 9).wrapping_add(0x60)]
    };
    let setting = Setting { face: &LARGE, look, size: USUAL_SIZE, line: LARGE_LINE, buttons: false };
    draw(m, &setting, &text, y, large_line)
}

/// What the reference's functions for the small typeface pass on: plain,
/// or see-through if the argument they call a flag is set.
fn small_look(flag: i32) -> [u32; 4] {
    [0x80, 0x80, 0x80, if flag != 0 { 0 } else { 0xFF }]
}

/// In place of the reference's function at 0x49B2A0: a text in the small
/// typeface, centred on the screen at a height. Its arguments: the
/// height, the text, the flag. The characters 1 to 4 are the buttons'
/// icons.
pub fn draw_small_centred(m: &mut Machine) -> Result<(), Stop> {
    let [y, text, flag] = arguments(m)?;
    let text = text_at(m, text as u32);
    let setting =
        Setting { face: &SMALL, look: small_look(flag), size: USUAL_SIZE, line: SMALL_LINE, buttons: true };
    draw(m, &setting, &text, y, |letters| centred_line(letters, SMALL_STEP, SMALL_WIDEST))
}

/// Where a line of the small typeface begins and how far apart its
/// letters are (in 256ths), for the reference's function at 0x49B3B0:
/// from `x` rightwards (mode 0), ending at `x` (mode 2) or centred on it
/// (any other), at a size. The reference's own arithmetic, which rounds
/// towards zero.
pub fn small_line_at(letters: usize, x: i32, mode: i32, size: i32) -> (i32, i32) {
    let width = (letters as i32).wrapping_mul(size).wrapping_mul(16);
    let first = match mode {
        0 => x,
        2 => x + -width / 4096,
        _ => x + -width / 8192,
    };
    (first, 256 * (size.wrapping_mul(16) / 4096))
}

/// In place of the reference's function at 0x49B3B0: a text in the small
/// typeface at a place. Its arguments: x, y, the text, the flag, the
/// mode (see [`small_line_at`]) and the size.
pub fn draw_small_at(m: &mut Machine) -> Result<(), Stop> {
    let [x, y, text, flag, mode, size] = arguments(m)?;
    let text = text_at(m, text as u32);
    let line = SMALL_LINE * size / USUAL_SIZE;
    let setting = Setting { face: &SMALL, look: small_look(flag), size, line, buttons: false };
    draw(m, &setting, &text, y, |letters| small_line_at(letters, x, mode, size))
}

/// The game's other function that draws a sprite, with the same
/// arguments, and its function for one of two pictures of a sheet as an
/// icon; the sheet of the two icons a text of the levels can hold.
const DRAW_SPRITE_OTHERWISE: u32 = 0x0049_42D0;
const DRAW_LEVEL_ICON: u32 = 0x0049_46A0;
const LEVEL_ICONS_SHEET: u32 = 0x26;

/// In place of the reference's function at 0x49B630: one character of
/// the levels' typeface at a place. Its arguments: x, y, the character,
/// three colours, and which of the two sprite functions draws it. The
/// characters `@` and `~` are two icons. The game's own code lays the
/// texts out and calls this for each character.
pub fn draw_level_character(m: &mut Machine) -> Result<(), Stop> {
    let [x, y, character, c1, c2, c3, which] = arguments(m)?;
    let character = character as u8;
    if let Some(picture) = b"~@".iter().position(|&icon| icon == character) {
        m.call_guest_c(DRAW_LEVEL_ICON, &[x as u32, (y - 2) as u32, LEVEL_ICONS_SHEET, picture as u32])?;
        return Ok(());
    }
    let Some(glyph) = LEVELS.glyph(character) else { return Ok(()) };
    let function = if which != 0 { DRAW_SPRITE } else { DRAW_SPRITE_OTHERWISE };
    let accent = glyph.accent.map(|accent| (accent, y - LEVELS.accent_rise));
    for (cell, y) in [(glyph.cell, y)].into_iter().chain(accent) {
        let arguments = [x, y, LEVELS.sheet as i32, cell as i32, c1, c2, c3, 0xFF, USUAL_SIZE, USUAL_SIZE];
        m.call_guest_c(function, &arguments.map(|argument| argument as u32))?;
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

    #[test]
    fn the_small_typeface_is_placed_as_the_reference_places_it() {
        // Centred on the screen: (40 - letters) * 4, eight apart.
        assert_eq!(centred_line(10, SMALL_STEP, SMALL_WIDEST), (120, 256 * 8));
        assert_eq!(centred_line(40, SMALL_STEP, SMALL_WIDEST), (0, 256 * 8));
        // At a place, at the usual size: from it, up to it, around it.
        assert_eq!(small_line_at(5, 100, 0, 0x800), (100, 256 * 8));
        assert_eq!(small_line_at(5, 100, 2, 0x800), (60, 256 * 8));
        assert_eq!(small_line_at(5, 100, 1, 0x800), (80, 256 * 8));
        // At another size the reference rounds towards zero.
        assert_eq!(small_line_at(3, 100, 1, 0x700), (100 - 10, 256 * 7));
        assert_eq!(small_line_at(3, 100, 2, 0x700), (100 - 21, 256 * 7));
    }
}
