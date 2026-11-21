//! What the runtime knows about the inside of the game executable: the
//! functions its modifications replace and the data those touch.
//!
//! The addresses are those of build `pc-1999-10-29` (see
//! `andy_recomp::builds`), each documented with how it was established.
//! Another build of the game has the same things at other addresses;
//! [`Addresses`] can be translated for it (`andy_recomp::pairing`).

use andy_cpu::interp::Fault;

use crate::mods::SKIP_MODE_SELECT;
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
    /// Performs the function's effects, register results included.
    pub function: fn(&mut Machine) -> Result<(), Stop>,
}

pub static REPLACEMENTS: &[Replacement] = &[Replacement {
    modification: SKIP_MODE_SELECT,
    address: |addresses| addresses.mode_select,
    name: "screen mode select",
    callee_pops: 0,
    function: select_screen_mode,
}];

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
    fn game_word(&mut self, address: u32) -> Result<u32, Stop> {
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
