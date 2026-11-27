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
/// The instruction before it, `pop ebx`: the last of the function's own.
pub const CONTROLS_MADE: u32 = 0x0041_4E09;
/// The keyboard as the function has just read it, a byte a key.
const KEYS: u32 = 0x0052_9A00;
/// The analogue pair for walking, sideways and forwards: the function
/// sets each to a whole push for a direction held on the keyboard.
const WALK: [u32; 2] = [0x0073_1CD8, 0x0073_1CDC];
const WHOLE_PUSH: u32 = 0x8000;
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
    pub const UP: u32 = 0x0010;
    pub const RIGHT: u32 = 0x0020;
    pub const DOWN: u32 = 0x0040;
    pub const LEFT: u32 = 0x0080;
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
    /// How far it turns in a unit of the game's time step with a joypad's
    /// second stick pushed all the way, in the same 256ths.
    pub stick_turn: [i32; 2],
    /// Whether moving the mouse or the stick up looks down.
    pub inverted: bool,
    /// Whether the camera is the one the player turns, and not one of the
    /// game's two: chosen in the pause menu.
    pub camera_orbits: bool,
    /// The pause menu's name for that camera, in the game's language.
    pub orbit_camera_text: Vec<u8>,
    /// The wordings of the options' entry "mouse and camera" and its
    /// page, in the game's language.
    pub page_wording: andy_recomp::menu::MouseAndCameraWording,
    /// That page while it is open.
    pub(crate) page: crate::options::Page,
    /// The parts of a unit of angle not yet turned, sideways and up.
    pub(crate) turn_left_over: [i32; 2],
    /// The view up and down the player has asked for, as an elevation;
    /// the camera's own elevation as the orbit left it; how far the view
    /// is tilted up beyond the camera's lowest place, and the tilt now in
    /// the camera's rotation.
    pub(crate) view: i32,
    pub(crate) elevation_left: Option<i32>,
    pub(crate) tilt: i32,
    pub(crate) tilt_applied: i32,
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
            // A count is seven eighths of a unit: 4681 counts a turn.
            turn: [DEFAULT_SENSITIVITY * SENSITIVITY_STEP; 2],
            // Twice and once the rate of the camera's two actions.
            stick_turn: [32 * 256, 16 * 256],
            inverted: false,
            camera_orbits: true,
            orbit_camera_text: b"orbit camera".to_vec(),
            page_wording: andy_recomp::menu::mouse_and_camera_wording(None),
            page: crate::options::Page::default(),
            turn_left_over: [0; 2],
            view: 0,
            elevation_left: None,
            tilt: 0,
            tilt_applied: 0,
            visor_held: false,
            visor_asked: false,
        }
    }
}

/// The mouse's sensitivity as the options show it: so many steps, each
/// this much of [`Controls::turn`]; the fourth is what the mouse turns by
/// when nothing was set.
pub const SENSITIVITY_STEPS: i32 = 10;
pub const SENSITIVITY_STEP: i32 = 56;
pub const DEFAULT_SENSITIVITY: i32 = 4;

/// The file of the project's own settings in the state directory, beside
/// the game's saves (ADR 0021): lines of `key=value`.
pub const SETTINGS_FILE: &str = "retold.ini";
/// The keys of the mouse's buttons in that file, in [`Controls::buttons`]'
/// order, and the names of what a button can do.
const BUTTON_KEYS: [&str; crate::win32::dinput::MOUSE_BUTTONS] =
    ["left_button", "right_button", "middle_button", "fourth_button"];
const BINDING_NAMES: [(&str, Binding); 4] = [
    ("nothing", Binding::Nothing),
    ("visor", Binding::VisorWhileHeld),
    ("fire", Binding::Action(action::FIRE)),
    ("spin", Binding::Action(action::SPIN)),
];

impl Controls {
    /// The step of sensitivity nearest to what the mouse turns sideways by.
    pub fn sensitivity(&self) -> i32 {
        ((self.turn[0] + SENSITIVITY_STEP / 2) / SENSITIVITY_STEP).clamp(1, SENSITIVITY_STEPS)
    }

    /// Has the mouse turn by a step of sensitivity, sideways and up alike.
    pub fn set_sensitivity(&mut self, step: i32) {
        self.turn = [step.clamp(1, SENSITIVITY_STEPS) * SENSITIVITY_STEP; 2];
    }

    /// The settings as the settings file holds them.
    pub fn settings_text(&self) -> String {
        let mut text = format!("mouse_sensitivity={}\n", self.sensitivity());
        text += &format!("vertical_inverted={}\n", if self.inverted { "yes" } else { "no" });
        for (key, binding) in BUTTON_KEYS.into_iter().zip(self.buttons) {
            if let Some((name, _)) = BINDING_NAMES.into_iter().find(|&(_, named)| named == binding) {
                text += &format!("{key}={name}\n");
            }
        }
        text += &format!("camera={}\n", if self.camera_orbits { "orbit" } else { "game" });
        text
    }

    /// Takes the settings a settings file's text holds. A line that is not
    /// a known key with one of its values is passed over, and what it
    /// would have set stays as it is; `#` starts a comment.
    pub fn read_settings(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            let yes_or_no = match value {
                "yes" => Some(true),
                "no" => Some(false),
                _ => None,
            };
            match key {
                "mouse_sensitivity" => {
                    let step = value.parse().ok().filter(|step| (1..=SENSITIVITY_STEPS).contains(step));
                    if let Some(step) = step {
                        self.set_sensitivity(step);
                    }
                }
                "vertical_inverted" => self.inverted = yes_or_no.unwrap_or(self.inverted),
                "camera" => match value {
                    "orbit" => self.camera_orbits = true,
                    "game" => self.camera_orbits = false,
                    _ => {}
                },
                _ => {
                    let button = BUTTON_KEYS.iter().position(|&button| button == key);
                    let binding = BINDING_NAMES.into_iter().find(|&(name, _)| name == value);
                    if let (Some(button), Some((_, binding))) = (button, binding) {
                        self.buttons[button] = binding;
                    }
                }
            }
        }
    }
}

impl crate::os::Os {
    /// Takes the settings kept in the state directory, if any were.
    /// Whether there were.
    pub fn read_settings(&mut self) -> bool {
        let Some(bytes) = self.files.read_own(SETTINGS_FILE) else { return false };
        self.controls.read_settings(&String::from_utf8_lossy(&bytes));
        true
    }

    /// Writes the settings down in the state directory, for the next
    /// session. As with the game's own files, a write that fails is lost.
    pub(crate) fn keep_settings(&self) {
        let _ = self.files.write_own(SETTINGS_FILE, self.controls.settings_text().as_bytes());
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

/// The keys that walk beside the game's own (the arrows): W, A, S and D,
/// by their scan codes, as the hand on the keyboard has them when the
/// other is on the mouse.
const WALKING_KEYS: [(u32, u32); 4] =
    [(0x11, action::UP), (0x1E, action::LEFT), (0x1F, action::DOWN), (0x20, action::RIGHT)];

/// In place of the last instruction of the function that reads the
/// devices, `pop ebx`: the directions of the walking keys held join the
/// mask, as the function's own keys do, with the analogue pair at a whole
/// push.
pub fn add_walking_keys(m: &mut Machine) -> Result<(), Stop> {
    let mut added = 0;
    for (key, direction) in WALKING_KEYS {
        if m.memory.read(KEYS + key, 1).unwrap_or(0) != 0 {
            added |= direction;
        }
    }
    if added != 0 {
        let actions = m.game_half(ACTIONS)?;
        m.set_game_half(ACTIONS, actions | added)?;
        for (at, (forth, back)) in
            WALK.into_iter().zip([(action::RIGHT, action::LEFT), (action::UP, action::DOWN)])
        {
            if added & forth != 0 {
                m.set_game_word(at, WHOLE_PUSH)?;
            } else if added & back != 0 {
                m.set_game_word(at, WHOLE_PUSH.wrapping_neg())?;
            }
        }
    }
    // The instruction this stands for.
    let esp = m.cpu.regs[4];
    m.cpu.regs[3] = m.game_word(esp)?;
    m.cpu.regs[4] = esp + 4;
    m.cpu.instructions += 1;
    Ok(())
}

/// The camera's actions held in this frame: left and right.
pub(crate) fn camera_actions(m: &mut Machine) -> Result<(bool, bool), Stop> {
    let pad = m.game_half(PAD)?;
    Ok((pad & action::CAMERA_LEFT != 0, pad & action::CAMERA_RIGHT != 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_file_gives_back_what_was_written() {
        let mut controls = Controls::default();
        assert_eq!(
            controls.settings_text(),
            "mouse_sensitivity=4\nvertical_inverted=no\nleft_button=fire\nright_button=visor\n\
             middle_button=spin\nfourth_button=nothing\ncamera=orbit\n"
        );
        controls.set_sensitivity(9);
        controls.inverted = true;
        controls.buttons[1] = Binding::Action(action::SPIN);
        controls.buttons[3] = Binding::Action(action::FIRE);
        controls.camera_orbits = false;
        let text = controls.settings_text();
        let mut read = Controls::default();
        read.read_settings(&text);
        assert_eq!((read.turn, read.inverted, read.camera_orbits), ([9 * SENSITIVITY_STEP; 2], true, false));
        assert_eq!(read.buttons, controls.buttons);
        assert_eq!(read.settings_text(), text);
    }

    #[test]
    fn what_a_settings_file_does_not_say_stays_as_it_is() {
        let mut controls = Controls::default();
        controls.read_settings("");
        assert_eq!(controls.settings_text(), Controls::default().settings_text());
        // Unknown keys and values, and numbers out of range, are passed over.
        controls.read_settings(
            "# by hand\n mouse_sensitivity = 7 # faster\nvertical_inverted=maybe\ncamera=fixed\n\
             right_button=jump\nfifth_button=fire\nmouse_sensitivity=11\nnonsense\n",
        );
        assert_eq!(controls.sensitivity(), 7);
        let expected = Controls { turn: [7 * SENSITIVITY_STEP; 2], ..Controls::default() };
        assert_eq!(controls.settings_text(), expected.settings_text());
    }

    #[test]
    fn the_default_sensitivity_is_a_step() {
        let controls = Controls::default();
        assert_eq!(controls.turn, [224, 224]);
        assert_eq!(controls.sensitivity(), DEFAULT_SENSITIVITY);
    }

    #[test]
    fn the_settings_are_kept_in_the_state_directory() {
        let directory = std::env::temp_dir().join(format!("andy-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let files = || crate::files::Files::new(None, Some(directory.clone()));
        // No file: the defaults.
        let mut os = crate::os::Os::new(files());
        assert_eq!(os.controls.settings_text(), Controls::default().settings_text());
        os.controls.set_sensitivity(2);
        os.controls.camera_orbits = false;
        os.keep_settings();
        let again = crate::os::Os::new(files());
        assert_eq!((again.controls.sensitivity(), again.controls.camera_orbits), (2, false));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
