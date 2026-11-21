//! Controls the original does not have: the mouse among the game's
//! actions, and a camera turned by it (ADR 0021).
//!
//! The game reads its devices in one function a frame and leaves a mask of
//! actions (jump, fire, the directions...) for everything else to read.
//! The mouse comes in exactly there: where that function returns, the
//! actions bound to the mouse's buttons are added to its mask. No key is
//! made up, and the game's own bindings of keyboard and joystick stay as
//! they are.
//!
//! Addresses are the reference build's; how they were found is in
//! `docs/CODE-DISCOVERY.md`.

use andy_cpu::interp::Fault;

use crate::{Machine, Stop};

/// The `ret` of the function that reads the devices (`0x414AF0`): its
/// mask of actions is complete there.
pub const CONTROLS_READ: u32 = 0x0041_4E0A;
/// The mask of actions held, 16 bits.
const ACTIONS: u32 = 0x0052_9B00;
/// The pad word the game's own code reads while playing, a frame's.
const PAD: u32 = 0x0052_AD88;
/// 0 the camera follows, 3 going into the visor, 4 the visor (first
/// person), 5 leaving it.
const VIEW: u32 = 0x0050_A13C;
const VIEW_FOLLOWS: u32 = 0;
const VIEW_VISOR: u32 = 4;

/// Bits of the mask of actions, from the table of the game's controls
/// screen (`0x500E58`).
pub mod action {
    pub const CAMERA_LEFT: u32 = 0x0100;
    pub const CAMERA_RIGHT: u32 = 0x0200;
    pub const VISOR: u32 = 0x0400;
    pub const SPIN: u32 = 0x2000;
    pub const FIRE: u32 = 0x8000;
}

/// What a button of the mouse does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Nothing,
    /// The action is held while the button is.
    Action(u32),
    /// The visor view is on while the button is held, as aiming is in a
    /// first-person game. (On a key the game's visor is a switch: pressed
    /// once it goes on, pressed again it goes off.)
    VisorWhileHeld,
}

/// The settings of the controls the project adds, and what they remember
/// from one frame to the next.
#[derive(Debug, Clone)]
pub struct Controls {
    /// By the mouse's buttons in DirectInput's order: left, right, middle,
    /// fourth.
    pub buttons: [Binding; crate::win32::dinput::MOUSE_BUTTONS],
    /// How far the camera turns for one count of the mouse, sideways and
    /// up and down, in 256ths of the game's unit of angle (4096 a turn).
    pub turn: [i32; 2],
    /// Whether moving the mouse up looks down.
    pub inverted: bool,
    /// The parts of a unit of angle not yet turned, sideways and up.
    pub(crate) turn_left_over: [i32; 2],
    /// Whether the visor is on because a button is held.
    visor_held: bool,
    /// Whether the last frame's mask had the visor's action from here: the
    /// game acts on the action going down, so it is given every other frame.
    visor_asked: bool,
}

impl Default for Controls {
    fn default() -> Controls {
        Controls {
            buttons: [
                Binding::Action(action::FIRE),
                Binding::VisorWhileHeld,
                Binding::Action(action::SPIN),
                Binding::Nothing,
            ],
            // A count is 1.5 units: 2731 counts a turn.
            turn: [384, 384],
            inverted: false,
            turn_left_over: [0; 2],
            visor_held: false,
            visor_asked: false,
        }
    }
}

impl Machine {
    pub(crate) fn game_half(&mut self, address: u32) -> Result<u32, Stop> {
        self.memory.read(address, 2).ok_or(Stop::Fault(Fault::Memory { address, write: false }))
    }

    pub(crate) fn set_game_half(&mut self, address: u32, value: u32) -> Result<(), Stop> {
        self.memory.write(address, 2, value).ok_or(Stop::Fault(Fault::Memory { address, write: true }))
    }
}

/// In place of the `ret` of the function that reads the devices: the
/// actions of the mouse's buttons join the mask, and the function returns.
pub fn add_the_mouse(m: &mut Machine) -> Result<(), Stop> {
    // The `ret` this stands for is one instruction of the machine's clock.
    m.cpu.instructions += 1;
    let view = m.game_word(VIEW)?;
    let held = m.os.input.mouse_buttons;
    let controls = &mut m.os.controls;
    let mut added = 0;
    let mut visor_wanted = false;
    for (binding, held) in controls.buttons.into_iter().zip(held) {
        match binding {
            Binding::Action(action) if held => added |= action,
            Binding::VisorWhileHeld if held => visor_wanted = true,
            _ => {}
        }
    }
    // The visor follows the button: asked for while the button is held
    // and the camera still follows, and put away when the button is let
    // go — if it is on because of the button, and not because of a key.
    if view == VIEW_FOLLOWS && !visor_wanted {
        controls.visor_held = false;
    }
    let change = (visor_wanted && view == VIEW_FOLLOWS)
        || (!visor_wanted && controls.visor_held && view == VIEW_VISOR);
    controls.visor_asked = change && !controls.visor_asked;
    if controls.visor_asked {
        added |= action::VISOR;
        controls.visor_held = visor_wanted;
    }
    if added != 0 {
        let actions = m.game_half(ACTIONS)?;
        m.set_game_half(ACTIONS, actions | added)?;
    }
    Ok(())
}

/// The camera's actions held in this frame: left and right.
pub(crate) fn camera_actions(m: &mut Machine) -> Result<(bool, bool), Stop> {
    let pad = m.game_half(PAD)?;
    Ok((pad & action::CAMERA_LEFT != 0, pad & action::CAMERA_RIGHT != 0))
}
