//! A real joypad for the game (ADR 0021): the first one found when the
//! game starts becomes the machine's joystick ([`andy_runtime::win32::dinput::Pad`]),
//! which the game looks for once, at its start.
//!
//! Its controls go to DirectInput's as a pad of the game's time would
//! report them: the left stick as x and y, the right one as the rotations
//! about them, the triggers sharing z; the face buttons first, then the
//! shoulders, the triggers, start and select, and the sticks' own buttons.

use andy_runtime::win32::dinput::Pad;

#[cfg(windows)]
pub struct Pads {
    gilrs: gilrs::Gilrs,
    chosen: gilrs::GamepadId,
}

#[cfg(windows)]
impl Pads {
    /// The first joypad plugged in, if there is one and the system's
    /// service for them can be reached.
    pub fn open() -> Option<Pads> {
        let gilrs = gilrs::Gilrs::new().ok()?;
        let chosen = gilrs.gamepads().next().map(|(id, _)| id)?;
        Some(Pads { gilrs, chosen })
    }

    pub fn name(&self) -> String {
        self.gilrs.gamepad(self.chosen).name().to_string()
    }

    /// What the pad is doing now. Pulled out, it reads as one at rest.
    pub fn read(&mut self) -> Pad {
        use gilrs::{Axis, Button};
        // The library keeps each pad's state up to date as its events are taken.
        while self.gilrs.next_event().is_some() {}
        let pad = self.gilrs.gamepad(self.chosen);
        if !pad.is_connected() {
            return Pad::default();
        }
        let axis = |value: f32| (value.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        let pressed = |button: Button| pad.is_pressed(button);
        let trigger = |button: Button| pad.button_data(button).map_or(0.0, |data| data.value());
        // DirectInput's y grows downwards.
        let axes = [
            axis(pad.value(Axis::LeftStickX)),
            axis(-pad.value(Axis::LeftStickY)),
            axis(trigger(Button::LeftTrigger2) - trigger(Button::RightTrigger2)),
            axis(pad.value(Axis::RightStickX)),
            axis(-pad.value(Axis::RightStickY)),
            0,
        ];
        let order = [
            Button::South,
            Button::East,
            Button::West,
            Button::North,
            Button::LeftTrigger,
            Button::RightTrigger,
            Button::LeftTrigger2,
            Button::RightTrigger2,
            Button::Start,
            Button::Select,
            Button::LeftThumb,
            Button::RightThumb,
        ];
        let mut buttons = [false; andy_runtime::win32::dinput::PAD_BUTTONS];
        for (button, held) in order.into_iter().zip(&mut buttons) {
            *held = pressed(button);
        }
        // Eight directions clockwise from up.
        let (up, right, down, left) = (
            pressed(Button::DPadUp),
            pressed(Button::DPadRight),
            pressed(Button::DPadDown),
            pressed(Button::DPadLeft),
        );
        let hat = match (up, right, down, left) {
            (true, false, _, false) => Some(0),
            (true, true, _, _) => Some(1),
            (false, true, false, _) => Some(2),
            (_, true, true, _) => Some(3),
            (false, false, true, false) => Some(4),
            (_, _, true, true) => Some(5),
            (false, _, false, true) => Some(6),
            (true, _, _, true) => Some(7),
            _ => None,
        };
        Pad { axes, buttons, hat }
    }
}

/// Where no joypad can be read (see the dependency's note in Cargo.toml).
#[cfg(not(windows))]
pub struct Pads;

#[cfg(not(windows))]
impl Pads {
    pub fn open() -> Option<Pads> {
        None
    }

    pub fn name(&self) -> String {
        String::new()
    }

    pub fn read(&mut self) -> Pad {
        Pad::default()
    }
}
