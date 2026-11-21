//! What the runtime knows about the inside of the game executable: the
//! functions its modifications replace and the data those touch.
//!
//! The addresses are those of build `pc-1999-10-29` (see
//! `andy_recomp::builds`), each documented with how it was established.
//! Another build of the game has the same things at other addresses;
//! [`Addresses`] can be translated for it (`andy_recomp::pairing`).

use andy_cpu::interp::Fault;

use crate::mods::{MENU_ARROWS_BY_TEXT, ONE_TYPEFACE, SKIP_MODE_SELECT};
use crate::{Machine, Stop};

/// A function of the game that a modification replaces with native code.
///
/// The original is never executed: when control reaches its address the
/// native function runs instead and returns to the caller. A recompiled
/// build simply leaves the original out.
pub struct Replacement {
    /// The modification this replacement belongs to (see `crate::mods`).
    pub modification: &'static str,
    /// Where the function is, among the addresses of the build.
    pub address: fn(&Addresses) -> u32,
    pub name: &'static str,
    /// Bytes of arguments the original removes from the stack on return.
    pub callee_pops: u32,
    /// For a stretch of code inside a function instead of a whole
    /// function: where the game goes on after it. The native code leaves
    /// the registers and the stack as the stretch would.
    pub resume: Option<u32>,
    /// Whether it is written against the reference build's code and data
    /// and so cannot serve another build run as it is.
    pub reference_only: bool,
    /// Performs the function's effects, register results included.
    pub function: fn(&mut Machine) -> Result<(), Stop>,
}

pub static REPLACEMENTS: &[Replacement] = &[
    Replacement {
        modification: SKIP_MODE_SELECT,
        address: |addresses| addresses.mode_select,
        name: "screen mode select",
        callee_pops: 0,
        resume: None,
        reference_only: false,
        function: select_screen_mode,
    },
    Replacement {
        modification: MENU_ARROWS_BY_TEXT,
        address: |_| MAIN_MENU_ARROWS,
        name: "main menu arrows",
        callee_pops: 0,
        resume: Some(MAIN_MENU_AFTER_ARROWS),
        reference_only: true,
        function: draw_main_menu_arrows,
    },
    Replacement {
        modification: ONE_TYPEFACE,
        address: |_| 0x0049_B580,
        name: "text in the large typeface, centred",
        callee_pops: 0,
        resume: None,
        reference_only: true,
        function: crate::typeface::draw_large_centred,
    },
    Replacement {
        modification: ONE_TYPEFACE,
        address: |_| 0x0049_B2A0,
        name: "text in the small typeface, centred",
        callee_pops: 0,
        resume: None,
        reference_only: true,
        function: crate::typeface::draw_small_centred,
    },
    Replacement {
        modification: ONE_TYPEFACE,
        address: |_| 0x0049_B3B0,
        name: "text in the small typeface, at a place",
        callee_pops: 0,
        resume: None,
        reference_only: true,
        function: crate::typeface::draw_small_at,
    },
];

// The main menu (the function at 0x437C40) draws, every frame, two arrows
// beside the entry chosen and then its five entries, each centred on the
// screen's middle (x = 160 of 320) by the function at 0x49B580, which sets
// the letters of the large typeface 12 apart. The arrows are drawn at
// x = 64 and x = 240 whatever the entry: right for "start game", which
// is ten letters, and for nothing else.

/// Where the stretch that draws the two arrows begins, and where the code
/// goes on after the two calls (it then removes their twenty arguments).
const MAIN_MENU_ARROWS: u32 = 0x0043_7E61;
const MAIN_MENU_AFTER_ARROWS: u32 = 0x0043_7EAE;
/// The function that draws a sprite: x, y, sheet, picture, three colour
/// values, opacity and two scales.
pub(crate) const DRAW_SPRITE: u32 = 0x0049_3F40;
/// The sheet of the arrows; its pictures 0 and 1 point right and left.
const ARROW_SHEET: u32 = 0x3E;
/// Not zero when there is a game to continue: the first entry then reads
/// otherwise.
const GAME_TO_CONTINUE: u32 = 0x0083_0D58;
/// Where the code holds the address of each entry's text: the first
/// entry's two wordings, then the other four.
const MAIN_MENU_TEXTS: [u32; 6] =
    [0x0043_7EC7, 0x0043_7EC0, 0x0043_7EDB, 0x0043_7EEF, 0x0043_7F03, 0x0043_7F17];
/// The first entry's height on the screen and the step to the next.
const MAIN_MENU_TOP: u32 = 0x78;
const MAIN_MENU_STEP: u32 = 0x14;
/// The middle of the screen, on which the menus' texts are centred.
pub(crate) const SCREEN_MIDDLE: i32 = 160;
/// How far left of a text's first letter the left arrow is drawn, and how
/// far right of its last letter's place the right one: what the original
/// has for its ten-letter entry (100 - 64 and 240 - 220).
const ARROW_BEFORE: i32 = 36;
const ARROW_AFTER: i32 = 20;

/// Replaces the stretch of the main menu that draws the two arrows: they
/// are drawn beside the text of the entry they are at, wherever that text
/// begins and ends, instead of at two fixed places. While they move from
/// one entry to the next, they go from the one width to the other.
///
/// At the stretch's start `eax` holds the arrows' brightness step, `ebx`
/// zero, `edi` the arrows' height and `ebp` the height of the entry
/// chosen; it leaves four times `eax` in `esi` and the arguments of its
/// two calls on the stack.
fn draw_main_menu_arrows(m: &mut Machine) -> Result<(), Stop> {
    let (step, y) = (m.cpu.regs[0], m.cpu.regs[7]);
    let brightness = step * 4;
    m.cpu.regs[6] = brightness;
    // Half the width of each entry's text, as the text function lays it
    // out. (With the text function switched off, the reference's own
    // lays it out twelve apart whatever its length: the same up to
    // twenty letters.)
    let continuing = m.game_word(GAME_TO_CONTINUE)? != 0;
    let mut half_widths = [0i32; 5];
    for (entry, half_width) in half_widths.iter_mut().enumerate() {
        let site = match entry {
            0 => MAIN_MENU_TEXTS[usize::from(continuing)],
            other => MAIN_MENU_TEXTS[other + 1],
        };
        let text = m.game_word(site)?;
        let text = crate::typeface::text_at(m, text);
        *half_width = crate::typeface::large_half_width(&text);
    }
    // Where the arrows are between two entries, in 256ths of a step.
    let along = (y.saturating_sub(MAIN_MENU_TOP) as i32 * 256 / MAIN_MENU_STEP as i32).clamp(0, 4 * 256);
    let (from, part) = ((along / 256).min(3) as usize, along - (along / 256).min(3) * 256);
    let half_width = (half_widths[from] * (256 - part) + half_widths[from + 1] * part) / 256;
    let (left, right) = (SCREEN_MIDDLE - half_width - ARROW_BEFORE, SCREEN_MIDDLE + half_width + ARROW_AFTER);
    let esp = m.cpu.regs[4];
    for (x, picture) in [(left, 0), (right, 1)] {
        let arguments = [x as u32, y, ARROW_SHEET, picture, brightness, brightness, 0x80, 0xFF, 0x800, 0x800];
        m.call_guest_c(DRAW_SPRITE, &arguments)?;
    }
    // The stretch's two calls leave their arguments for the code after it.
    m.cpu.regs[4] = esp - 4 * 20;
    Ok(())
}

// The game keeps what its device enumeration found (code at 0x4AC4D0, which
// follows the DirectX 6 SDK's D3DFrame) in three linked lists.

/// The addresses in the executable that the runtime knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Addresses {
    /// The function that shows the "Screen Mode Select" window.
    pub mode_select: u32,
    /// Head of the list of DirectDraw drivers (returned by the function
    /// at `0x4AD000`).
    pub driver_list: u32,
    /// The driver the renderer will use.
    pub selected_driver: u32,
    /// Indices of the chosen driver, device and mode, saved to `toy2.cfg`.
    pub saved_driver_index: u32,
    pub saved_device_index: u32,
    pub saved_mode_index: u32,
}

impl Addresses {
    /// Those of the reference build.
    pub const REFERENCE: Addresses = Addresses {
        mode_select: 0x0043_2020,
        driver_list: 0x0088_4028,
        selected_driver: 0x0088_4030,
        saved_driver_index: 0x0050_8D7C,
        saved_device_index: 0x0050_8D80,
        saved_mode_index: 0x0050_8D84,
    };

    /// Those of another build, given where each address of the reference
    /// build is there. `None` if one of them is not known there.
    pub fn translated(translate: impl Fn(u32) -> Option<u32>) -> Option<Addresses> {
        let reference = Addresses::REFERENCE;
        Some(Addresses {
            mode_select: translate(reference.mode_select)?,
            driver_list: translate(reference.driver_list)?,
            selected_driver: translate(reference.selected_driver)?,
            saved_driver_index: translate(reference.saved_driver_index)?,
            saved_device_index: translate(reference.saved_device_index)?,
            saved_mode_index: translate(reference.saved_mode_index)?,
        })
    }
}

impl Default for Addresses {
    fn default() -> Addresses {
        Addresses::REFERENCE
    }
}

// Fields of a driver record.
const DRIVER_SELECTED_DEVICE: u32 = 0x378;
const DRIVER_DEVICES: u32 = 0x37C;
// Fields of a 3D device record.
const DEVICE_IS_HARDWARE: u32 = 0x138;
const DEVICE_WINDOWED: u32 = 0x140;
const DEVICE_SELECTED_MODE: u32 = 0x144;
const DEVICE_MODES: u32 = 0x148;
const DEVICE_NEXT: u32 = 0x14C;
// Fields of a display mode record: it starts with a surface description,
// whose third word is the height.
const MODE_HEIGHT: u32 = 0x08;
const MODE_NEXT: u32 = 0xA4;

impl Machine {
    pub(crate) fn game_word(&mut self, address: u32) -> Result<u32, Stop> {
        self.memory.read(address, 4).ok_or(Stop::Fault(Fault::Memory { address, write: false }))
    }

    fn set_game_word(&mut self, address: u32, value: u32) -> Result<(), Stop> {
        self.memory.write(address, 4, value).ok_or(Stop::Fault(Fault::Memory { address, write: true }))
    }
}

/// Replaces the "Screen Mode Select" window (the function at `0x432020`).
///
/// The original shows a window, lets the user pick a display driver, a
/// render method and a resolution with the keyboard, and leaves the choice in
/// the records and variables written below. This makes the choice the
/// original proposes when nothing was saved (its path at `0x432330`, through
/// the helpers at `0x431CA0`, `0x431D40` and `0x431E00`): the first driver,
/// its first hardware device, and that device's first mode at least 480
/// lines high. Full screen, as the original always leaves it.
fn select_screen_mode(m: &mut Machine) -> Result<(), Stop> {
    let addresses = m.os.addresses;
    let driver = m.game_word(addresses.driver_list)?;

    let first_device = m.game_word(driver + DRIVER_DEVICES)?;
    let (mut device, mut device_index) = (first_device, 0);
    while device != 0 && m.game_word(device + DEVICE_IS_HARDWARE)? == 0 {
        device = m.game_word(device + DEVICE_NEXT)?;
        device_index += 1;
    }
    if device == 0 {
        // No hardware device: the first one, whatever it is.
        (device, device_index) = (first_device, 0);
    }
    m.set_game_word(driver + DRIVER_SELECTED_DEVICE, device)?;

    let (mut mode, mut mode_index) = (m.game_word(device + DEVICE_MODES)?, 0);
    loop {
        let next = m.game_word(mode + MODE_NEXT)?;
        if next == 0 || m.game_word(mode + MODE_HEIGHT)? >= 480 {
            break;
        }
        (mode, mode_index) = (next, mode_index + 1);
    }
    m.set_game_word(device + DEVICE_SELECTED_MODE, mode)?;
    m.set_game_word(device + DEVICE_WINDOWED, 0)?;

    m.set_game_word(addresses.selected_driver, driver)?;
    m.set_game_word(addresses.saved_driver_index, 0)?;
    m.set_game_word(addresses.saved_device_index, device_index)?;
    m.set_game_word(addresses.saved_mode_index, mode_index)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    #[test]
    fn mode_selection_takes_the_first_hardware_device_and_the_first_480_line_mode() {
        let mut m = testing::machine();
        m.memory.map(Addresses::REFERENCE.saved_driver_index & !0xFFF, 0x1000);
        m.memory.map(Addresses::REFERENCE.driver_list & !0xFFF, 0x1000);
        let driver = m.scratch(0x400);
        let [software, hardware] = [m.scratch(0x200), m.scratch(0x200)];
        let modes: Vec<u32> = (0..4).map(|_| m.scratch(0x100)).collect();
        m.memory.write(Addresses::REFERENCE.driver_list, 4, driver).unwrap();
        m.memory.write(driver + DRIVER_DEVICES, 4, software).unwrap();
        m.memory.write(software + DEVICE_NEXT, 4, hardware).unwrap();
        m.memory.write(hardware + DEVICE_IS_HARDWARE, 4, 1).unwrap();
        m.memory.write(hardware + DEVICE_WINDOWED, 4, 1).unwrap();
        m.memory.write(hardware + DEVICE_MODES, 4, modes[0]).unwrap();
        for (index, height) in [200, 240, 480, 600].into_iter().enumerate() {
            m.memory.write(modes[index] + MODE_HEIGHT, 4, height).unwrap();
            m.memory.write(modes[index] + MODE_NEXT, 4, modes.get(index + 1).copied().unwrap_or(0)).unwrap();
        }
        m.memory.write(Addresses::REFERENCE.saved_mode_index, 4, 46).unwrap(); // a stale saved choice

        select_screen_mode(&mut m).unwrap();
        let word = |m: &mut Machine, at: u32| m.memory.read(at, 4).unwrap();
        assert_eq!(word(&mut m, Addresses::REFERENCE.selected_driver), driver);
        assert_eq!(word(&mut m, driver + DRIVER_SELECTED_DEVICE), hardware);
        assert_eq!(word(&mut m, hardware + DEVICE_SELECTED_MODE), modes[2]);
        assert_eq!(word(&mut m, hardware + DEVICE_WINDOWED), 0);
        assert_eq!(
            [
                Addresses::REFERENCE.saved_driver_index,
                Addresses::REFERENCE.saved_device_index,
                Addresses::REFERENCE.saved_mode_index
            ]
            .map(|at| word(&mut m, at)),
            [0, 1, 2]
        );

        // With only low modes it settles on the last one, as the original does.
        m.memory.write(modes[1] + MODE_NEXT, 4, 0).unwrap();
        select_screen_mode(&mut m).unwrap();
        assert_eq!(word(&mut m, hardware + DEVICE_SELECTED_MODE), modes[1]);
    }
}
