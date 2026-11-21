//! A fifth entry of the game's options, "mouse and camera", and its page
//! (ADR 0021).
//!
//! The options screen is one function with its own loop (`0x4371B0`): a
//! list of four entries with two pointers beside the one chosen, and for
//! each entry a page, entered with jump and left with jump (accepted) or
//! cancel. Which entry the pointers are at it tells by their height, taken
//! from a table of the entries' heights. Its last page, the graphics'
//! (`0x49CAA0`), is rows of values: up and down choose a row, left and
//! right change its value, two arrows beside the row say which ways it can
//! go.
//!
//! Five short stretches of that loop make the list one of five, and the
//! fifth entry's page is drawn here after the graphics page, with the
//! game's own functions for text, sprites and sounds. What the page shows
//! is a copy of the settings, taken when the page is entered and put back
//! into the settings when it is accepted.
//!
//! Addresses are the reference build's.

use crate::controls::{Binding, Controls, SENSITIVITY_STEPS, action};
use crate::{Machine, Stop};

/// Where the loop, entering a page, starts setting aside what cancelling
/// puts back; the instruction after the first, and the 16 bits the first
/// loads, sign-extended, into `ecx`.
pub const PAGE_ENTERED: u32 = 0x0043_7374;
pub const PAGE_ENTERED_NEXT: u32 = 0x0043_737B;
const SET_ASIDE_FIRST: u32 = 0x0052_F0D2;
/// Where the list, with down held, asks whether the entry chosen is the
/// last (three instructions); where it goes on if it is not, and if it is.
pub const DOWN_ASKED: u32 = 0x0043_7407;
pub const DOWN_TAKEN: u32 = 0x0043_7410;
pub const DOWN_REFUSED: u32 = 0x0043_7480;
/// Where the loop, a page being accepted, starts on the sound for it: a
/// `push 0x50`, and the instruction after it.
pub const PAGE_ACCEPTED: u32 = 0x0043_75BB;
pub const PAGE_ACCEPTED_NEXT: u32 = 0x0043_75BD;
/// Where the list has drawn its four entries and starts on the line under
/// them: a `push 0x600`, and the instruction after it.
pub const ENTRIES_DRAWN: u32 = 0x0043_768C;
pub const ENTRIES_DRAWN_NEXT: u32 = 0x0043_7691;
/// Where the loop, inside a page, starts finding which page by the
/// pointers' height: a load of the first entry's height into `eax`, and
/// the instruction after it; and where the loop goes on after any page.
pub const PAGE_SOUGHT: u32 = 0x0043_773C;
pub const PAGE_SOUGHT_NEXT: u32 = 0x0043_7741;
pub const PAGE_DONE: u32 = 0x0043_76AE;

/// The heights of the four entries, 20 apart. The word after them holds
/// 145, the height a fifth entry has at that step, and the loop's own code
/// reads it as one when the entry chosen is the fifth. (Whether that word
/// was meant as a fifth height is `UNVERIFIED`: the one instruction that
/// names its address, at `0x4397AE` in another function, indexes the
/// table of addresses that follows it.)
const HEIGHTS: u32 = 0x004F_6BD4;
const FIFTH_HEIGHT: u32 = HEIGHTS + 4 * 4;
/// How far under the pointers' height an entry's text is drawn.
const TEXT_UNDER: u32 = 0xE;
/// The loop's own variables, from `esp` as it is in the loop: the entry
/// chosen.
const ENTRY_CHOSEN: u32 = 0x1C;
const LAST_ENTRY: i32 = 4;

/// The game's time step of the frame.
const STEP: u32 = 0x0052_F2D4;
/// The actions held, as the menus read them.
const HELD: u32 = 0x0088_279C;
/// The game's function for a text in the small typeface centred on the
/// screen (height, text, flag) and at a place (x, y, text, flag, mode,
/// size); the second also draws digits and the colon.
const TEXT_CENTRED: u32 = 0x0049_B2A0;
const TEXT_AT: u32 = 0x0049_B3B0;
/// A sound of the menus: which (0 a row chosen or a page accepted, 1 a
/// step), and three numbers the menus always give alike.
const MENU_SOUND: u32 = 0x004A_37E0;
/// Where the graphics page's code holds the addresses of its two lines of
/// help: the keys that choose and change, the keys that accept and cancel.
const HELP_TEXTS: [u32; 2] = [0x0049_CBA0, 0x0049_CBBB];

// The layout of the graphics page, which this page follows.
const TITLE_HEIGHT: u32 = 0x32;
const FIRST_ROW: i32 = 0x4B;
const ROW_STEP: i32 = 25;
const HELP_HEIGHTS: [u32; 2] = [0xB2, 0xBA];
const HELP_SIZE: u32 = 0x600;
const USUAL_SIZE: u32 = 0x800;
/// The two arrows beside the row chosen: how far above the row's height
/// they rest, where each is drawn, their sheet and their pictures.
const ARROWS_ABOVE: i32 = 3;
const ARROWS_X: [u32; 2] = [0x32, 0xFF];
const ARROW_SHEET: u32 = 0x38;
const ARROW_PICTURES: [u32; 2] = [3, 2];
/// How long, in time steps, the page takes no direction after one, and
/// the length of the blink of the row chosen: hidden for its second half.
const DELAY: i32 = 15;
const BLINK: i32 = 20;

const ROWS: usize = 3;

/// What the page's rows show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Values {
    /// The mouse's sensitivity, 1 to [`SENSITIVITY_STEPS`].
    sensitivity: i32,
    /// Whether moving the mouse up looks down.
    inverted: bool,
    /// Whether the right button spins and does not hold the visor.
    right_spins: bool,
}

impl Values {
    fn of(controls: &Controls) -> Values {
        Values {
            sensitivity: controls.sensitivity(),
            inverted: controls.inverted,
            right_spins: controls.buttons[1] == Binding::Action(action::SPIN),
        }
    }

    /// Puts into the settings the values that are not what they were when
    /// the page was entered: a setting no row was moved on stays as it is,
    /// whatever it is.
    fn put(self, entered: Values, controls: &mut Controls) {
        if self.sensitivity != entered.sensitivity {
            controls.set_sensitivity(self.sensitivity);
        }
        if self.inverted != entered.inverted {
            controls.inverted = self.inverted;
        }
        if self.right_spins != entered.right_spins {
            controls.buttons[1] =
                if self.right_spins { Binding::Action(action::SPIN) } else { Binding::VisorWhileHeld };
        }
    }

    /// Whether a row's value can go left, and right.
    fn can_go(self, row: usize) -> [bool; 2] {
        match row {
            0 => [self.sensitivity > 1, self.sensitivity < SENSITIVITY_STEPS],
            // A value of two is turned over by either direction.
            _ => [true, true],
        }
    }

    /// A row's value after left (`towards` 0) or right (1).
    fn moved(mut self, row: usize, towards: usize) -> Values {
        match row {
            0 if self.can_go(0)[towards] => self.sensitivity += if towards == 0 { -1 } else { 1 },
            1 => self.inverted = !self.inverted,
            2 => self.right_spins = !self.right_spins,
            _ => {}
        }
        self
    }
}

/// The page while it is open.
#[derive(Debug, Clone, Default)]
pub struct Page {
    /// The row chosen.
    row: usize,
    /// Time steps left in which no direction is taken.
    delay: i32,
    /// Where the blink of the row chosen is, counting down.
    blink: i32,
    /// The height the two arrows are at: they go after the row chosen.
    arrows_at: i32,
    values: Values,
    /// The values when the page was entered.
    entered: Values,
}

/// Where the arrows are a frame later, on their way to the row chosen: a
/// fifth of the way for each time step, never less than one unit, and not
/// past it. The graphics page's own arithmetic.
fn nearer(at: i32, wanted: i32, step: i32) -> i32 {
    let apart = wanted - at;
    let by = step * apart / 5;
    at + match apart {
        0 => 0,
        1.. => apart.min(by.max(1)),
        _ => apart.max(by.min(-1)),
    }
}

impl Machine {
    /// Whether the options' pointers stand at the fifth entry (`ebp` holds
    /// their height in 256ths).
    fn at_fifth_entry(&mut self) -> Result<bool, Stop> {
        Ok(self.cpu.regs[5] == self.game_word(FIFTH_HEIGHT)? << 8)
    }

    /// The stack takes a word, as a `push` has it do.
    fn push(&mut self, value: u32) -> Result<(), Stop> {
        let esp = self.cpu.regs[4] - 4;
        self.set_game_word(esp, value)?;
        self.cpu.regs[4] = esp;
        Ok(())
    }
}

/// In place of the first instruction of the loop's setting aside, on
/// entering a page: the page here takes its copy of the settings too.
pub fn enter_page(m: &mut Machine) -> Result<(), Stop> {
    let values = Values::of(&m.os.controls);
    m.os.controls.page =
        Page { arrows_at: FIRST_ROW - ARROWS_ABOVE, values, entered: values, ..Page::default() };
    // The instruction this stands for.
    m.cpu.regs[1] = m.game_half(SET_ASIDE_FIRST)? as u16 as i16 as u32;
    m.cpu.instructions += 1;
    Ok(())
}

/// In place of the three instructions that keep down from going past the
/// fourth entry (they leave the entry chosen in `eax`): it stops at the
/// fifth.
pub fn limit_down(m: &mut Machine) -> Result<(), Stop> {
    let entry = m.game_word(m.cpu.regs[4] + ENTRY_CHOSEN)?;
    m.cpu.regs[0] = entry;
    if entry as i32 >= LAST_ENTRY {
        m.resume_at = Some(DOWN_REFUSED);
    }
    m.cpu.instructions += 3;
    Ok(())
}

/// In place of the first instruction of what the loop does when a page is
/// accepted: if the page is the fifth entry's, what it shows becomes the
/// settings, and they are written down.
pub fn accept_page(m: &mut Machine) -> Result<(), Stop> {
    if m.at_fifth_entry()? {
        let Page { values, entered, .. } = m.os.controls.page;
        values.put(entered, &mut m.os.controls);
        m.os.keep_settings();
    }
    // The instruction this stands for.
    m.push(0x50)?;
    m.cpu.instructions += 1;
    Ok(())
}

/// In place of the first instruction after the list's four entries are
/// drawn: the fifth is drawn, as they are, at the next height.
pub fn draw_fifth_entry(m: &mut Machine) -> Result<(), Stop> {
    let height = m.game_word(FIFTH_HEIGHT)?;
    let name = m.os.controls.page_wording.name;
    let name = m.os.kept_string(&mut m.memory, name);
    m.call_guest_c(TEXT_CENTRED, &[height + TEXT_UNDER, name, 0])?;
    // The instruction this stands for.
    m.push(HELP_SIZE)?;
    m.cpu.instructions += 1;
    Ok(())
}

/// In place of the first instruction of the loop's search for the page
/// the pointers are at: at the fifth entry the page is this one, a frame
/// of it is done here, and the loop goes on as after any page.
pub fn run_page(m: &mut Machine) -> Result<(), Stop> {
    if !m.at_fifth_entry()? {
        // The instruction this stands for.
        m.cpu.regs[0] = m.game_word(HEIGHTS)?;
        m.cpu.instructions += 1;
        return Ok(());
    }
    let step = m.game_word(STEP)? as i32;
    let held = m.game_half(HELD)?;
    let wording = m.os.controls.page_wording;
    let mut page = m.os.controls.page.clone();

    // The title and the rows; the row chosen blinks.
    let title = m.os.kept_string(&mut m.memory, wording.name);
    m.call_guest_c(TEXT_CENTRED, &[TITLE_HEIGHT, title, 0])?;
    let mut sensitivity = wording.sensitivity.to_vec();
    sensitivity.extend_from_slice(format!(" {}", page.values.sensitivity).as_bytes());
    let rows: [&[u8]; ROWS] = [
        &sensitivity,
        wording.vertical[usize::from(page.values.inverted)],
        wording.right_button[usize::from(page.values.right_spins)],
    ];
    for (row, text) in rows.into_iter().enumerate() {
        if row == page.row && page.blink >= BLINK / 2 {
            continue;
        }
        let text = m.os.kept_string(&mut m.memory, text);
        let y = (FIRST_ROW + ROW_STEP * row as i32) as u32;
        m.call_guest_c(TEXT_AT, &[crate::game::SCREEN_MIDDLE as u32, y, text, 0, 1, USUAL_SIZE])?;
    }
    // The two lines of help are the graphics page's own.
    for (site, y) in HELP_TEXTS.into_iter().zip(HELP_HEIGHTS) {
        let text = m.game_word(site)?;
        m.call_guest_c(TEXT_AT, &[crate::game::SCREEN_MIDDLE as u32, y, text, 0, 1, HELP_SIZE])?;
    }

    // Up and down choose the row.
    if page.delay <= 0 {
        for (direction, row) in
            [(action::UP, page.row.saturating_sub(1)), (action::DOWN, (page.row + 1).min(ROWS - 1))]
        {
            if held & direction != 0 {
                page.row = row;
                page.delay = DELAY;
                m.call_guest_c(MENU_SOUND, &[0, 0x1200, 0x50, 0x50])?;
            }
        }
    }

    // The arrows go after the row chosen, one for each way its value can go.
    let can_go = page.values.can_go(page.row);
    page.arrows_at = nearer(page.arrows_at, FIRST_ROW - ARROWS_ABOVE + ROW_STEP * page.row as i32, step);
    for towards in 0..2 {
        if can_go[towards] {
            let arguments = [
                ARROWS_X[towards],
                page.arrows_at as u32,
                ARROW_SHEET,
                ARROW_PICTURES[towards],
                0xFF,
                0xFF,
                0xFF,
                0xFF,
                USUAL_SIZE,
                USUAL_SIZE,
            ];
            m.call_guest_c(crate::game::DRAW_SPRITE, &arguments)?;
        }
    }
    page.blink -= step;
    if page.blink < 0 {
        page.blink += BLINK;
    }

    // Left and right change the row's value.
    if page.delay > 0 {
        page.delay -= step;
    } else {
        let towards = [action::LEFT, action::RIGHT].map(|direction| held & direction != 0);
        for way in 0..2 {
            if towards[way] {
                let moved = page.values.moved(page.row, way);
                if moved != page.values {
                    page.values = moved;
                    m.call_guest_c(MENU_SOUND, &[1, 0x1200, 0x50, 0x50])?;
                }
                page.delay = DELAY;
            }
        }
    }
    m.os.controls.page = page;
    m.resume_at = Some(PAGE_DONE);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    #[test]
    fn the_arrows_go_after_the_row_and_stop_at_it() {
        // A fifth of the way a time step, as the graphics page has it.
        assert_eq!(nearer(72, 97, 1), 77);
        assert_eq!(nearer(97, 72, 1), 92);
        assert_eq!(nearer(72, 97, 2), 82);
        // Never less than one unit, and never past the row.
        assert_eq!(nearer(95, 97, 1), 96);
        assert_eq!(nearer(99, 97, 1), 98);
        assert_eq!(nearer(72, 97, 9), 97);
        assert_eq!(nearer(97, 72, 9), 72);
        assert_eq!(nearer(97, 97, 3), 97);
    }

    #[test]
    fn a_row_moves_within_its_values() {
        let values = Values { sensitivity: 1, inverted: false, right_spins: false };
        assert_eq!(values.can_go(0), [false, true]);
        assert_eq!(values.moved(0, 0), values);
        assert_eq!(values.moved(0, 1).sensitivity, 2);
        let top = Values { sensitivity: SENSITIVITY_STEPS, ..values };
        assert_eq!((top.can_go(0), top.moved(0, 1)), ([true, false], top));
        // The two-valued rows turn over either way.
        assert!(values.moved(1, 0).inverted && values.moved(1, 1).inverted);
        assert!(values.moved(2, 0).right_spins && !values.moved(2, 0).moved(2, 1).right_spins);
    }

    #[test]
    fn accepting_changes_only_what_a_row_was_moved_on() {
        // A right button bound by hand to something the page does not show.
        let mut controls = Controls::default();
        controls.buttons[1] = Binding::Action(action::FIRE);
        controls.turn = [300, 100];
        let entered = Values::of(&controls);
        let values = entered.moved(1, 0);
        values.put(entered, &mut controls);
        assert!(controls.inverted);
        assert_eq!(controls.buttons[1], Binding::Action(action::FIRE));
        assert_eq!(controls.turn, [300, 100]);
        // The rows moved on are set: both axes turn alike, the right
        // button spins and the middle one goes on spinning.
        let values = values.moved(0, 1).moved(2, 1);
        values.put(entered, &mut controls);
        assert_eq!(controls.turn, [controls.turn[0]; 2]);
        assert_eq!(controls.sensitivity(), entered.sensitivity + 1);
        assert_eq!(controls.buttons[1], Binding::Action(action::SPIN));
        assert_eq!(controls.buttons[2], Binding::Action(action::SPIN));
        values.moved(2, 0).put(values, &mut controls);
        assert_eq!(controls.buttons[1], Binding::VisorWhileHeld);
    }

    #[test]
    fn down_stops_at_the_fifth_entry() {
        let mut m = testing::machine();
        let stack = m.scratch(0x100);
        m.cpu.regs[4] = stack;
        for (entry, refused) in [(0, false), (3, false), (4, true)] {
            m.memory.write(stack + ENTRY_CHOSEN, 4, entry).unwrap();
            m.resume_at = None;
            limit_down(&mut m).unwrap();
            assert_eq!(m.cpu.regs[0], entry);
            assert_eq!(m.resume_at, refused.then_some(DOWN_REFUSED));
        }
    }
}
