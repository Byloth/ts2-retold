//! Recorded input: what the keyboard and the mouse do, and when.
//!
//! Time in a recording is counted in **reads of the keyboard**. The game
//! reads the keyboard's state once per frame, and nothing else it sees
//! depends on when a key went down, so "at the n-th read the state changes
//! like this" replays a session exactly, whatever the speed of the machine
//! it is replayed on (ADR 0017). The mouse goes by the same clock: its
//! buttons change at a read, and its movement is handed over read by read
//! ([`Input::frame_motion`]).
//!
//! As text, one change per line, the read at which it happens first (the
//! first read is 1); `#` starts a comment:
//!
//! ```text
//! 3600 press SPACE          a key, by its DirectInput name without DIK_
//! 3605 release SPACE
//! 3610 press MOUSE_RIGHT    a mouse button: MOUSE_LEFT, _RIGHT, _MIDDLE, _4
//! 3611 move -12 3 0         the mouse's movement in that frame: x, y, wheel
//! 0 plug PAD                a joypad is there (read 0: before the game starts)
//! 3620 axis X -20000        an axis of it: X, Y, Z, RX, RY, RZ; -32768 to 32767
//! 3620 press PAD_1          a button of it, PAD_1 to PAD_16
//! 3625 hat 2                its direction pad: 0 (up) to 7 clockwise, or `none`
//! 3700 unplug PAD
//! ```

use crate::keys;
use crate::win32::dinput::{Input, MOUSE_AXES, MOUSE_BUTTONS, PAD_AXES, PAD_BUTTONS, Pad};

/// The names of the mouse's buttons in a recording, in DirectInput's order.
const BUTTON_NAMES: [&str; MOUSE_BUTTONS] = ["MOUSE_LEFT", "MOUSE_RIGHT", "MOUSE_MIDDLE", "MOUSE_4"];
/// The names of a joypad's axes, in DirectInput's order.
const AXIS_NAMES: [&str; PAD_AXES] = ["X", "Y", "Z", "RX", "RY", "RZ"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A key, by its scan code, goes down or up.
    Key {
        key: u8,
        down: bool,
    },
    /// A button of the mouse goes down or up.
    Button {
        button: u8,
        down: bool,
    },
    /// The mouse's movement in the frame: x, y and the wheel.
    Motion([i32; MOUSE_AXES]),
    /// A joypad is plugged in, at rest, or pulled out.
    Plugged(bool),
    PadAxis {
        axis: u8,
        value: i16,
    },
    PadButton {
        button: u8,
        down: bool,
    },
    PadHat(Option<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The read of the keyboard from which the change is seen.
    pub read: u64,
    pub change: Change,
}

/// A recording being played back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    /// In the order they happen.
    events: Vec<Event>,
    /// The first event not yet applied.
    next: usize,
}

impl Recording {
    pub fn new(mut events: Vec<Event>) -> Recording {
        // A stable sort: changes at the same read keep the order given.
        events.sort_by_key(|event| event.read);
        Recording { events, next: 0 }
    }

    /// Reads a recording from its text form.
    pub fn parse(text: &str) -> Result<Recording, String> {
        let mut events = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let wrong = |what: &str| format!("line {}: {what}: {line:?}", number + 1);
            let words: Vec<&str> = line.split_whitespace().collect();
            // Read 0 is before the game starts.
            let read = words[0].parse().map_err(|_| wrong("not a read number"))?;
            let change = match words[1..] {
                ["move", x, y, wheel] => {
                    let axis = |text: &str| text.parse().map_err(|_| wrong("not a movement"));
                    Change::Motion([axis(x)?, axis(y)?, axis(wheel)?])
                }
                [action @ ("plug" | "unplug"), "PAD"] => Change::Plugged(action == "plug"),
                ["axis", name, value] => {
                    let axis = AXIS_NAMES.iter().position(|axis| axis.eq_ignore_ascii_case(name));
                    let axis = axis.ok_or_else(|| wrong("not an axis"))? as u8;
                    Change::PadAxis { axis, value: value.parse().map_err(|_| wrong("not an axis's value"))? }
                }
                ["hat", "none"] => Change::PadHat(None),
                ["hat", direction] => {
                    let direction = direction.parse().ok().filter(|&direction: &u8| direction < 8);
                    Change::PadHat(Some(direction.ok_or_else(|| wrong("not a direction"))?))
                }
                [action @ ("press" | "release"), name] => {
                    let down = action == "press";
                    let button = BUTTON_NAMES.iter().position(|button| button.eq_ignore_ascii_case(name));
                    let pad_button = name
                        .strip_prefix("PAD_")
                        .and_then(|number| number.parse().ok())
                        .filter(|number| (1..=PAD_BUTTONS).contains(number));
                    match (button, pad_button) {
                        (Some(button), _) => Change::Button { button: button as u8, down },
                        (_, Some(number)) => Change::PadButton { button: number as u8 - 1, down },
                        _ => {
                            let key = keys::code(name).ok_or_else(|| wrong("neither a key nor a button"))?;
                            Change::Key { key, down }
                        }
                    }
                }
                _ => return Err(wrong("not a change of the keyboard, the mouse or a joypad")),
            };
            events.push(Event { read, change });
        }
        Ok(Recording::new(events))
    }

    /// The text form of the recording.
    pub fn to_text(&self) -> String {
        let action = |down: bool| if down { "press" } else { "release" };
        let line = |event: &Event| match event.change {
            Change::Key { key, down } => {
                format!("{} {} {}\n", event.read, action(down), keys::name(key).unwrap_or("?"))
            }
            Change::Button { button, down } => {
                format!("{} {} {}\n", event.read, action(down), BUTTON_NAMES[button as usize])
            }
            Change::Motion([x, y, wheel]) => format!("{} move {x} {y} {wheel}\n", event.read),
            Change::Plugged(plugged) => {
                format!("{} {} PAD\n", event.read, if plugged { "plug" } else { "unplug" })
            }
            Change::PadAxis { axis, value } => {
                format!("{} axis {} {value}\n", event.read, AXIS_NAMES[axis as usize])
            }
            Change::PadButton { button, down } => {
                format!("{} {} PAD_{}\n", event.read, action(down), button + 1)
            }
            Change::PadHat(None) => format!("{} hat none\n", event.read),
            Change::PadHat(Some(direction)) => format!("{} hat {direction}\n", event.read),
        };
        self.events.iter().map(line).collect()
    }

    /// Brings the keyboard and the mouse to their state at the given read.
    /// Movement is added to what the mouse has moved since the read before.
    pub fn apply(&mut self, read: u64, input: &mut Input) {
        while let Some(event) = self.events.get(self.next).filter(|event| event.read <= read) {
            match event.change {
                Change::Key { key, down } => input.keys[key as usize] = down,
                Change::Button { button, down } => input.mouse_buttons[button as usize] = down,
                Change::Motion(motion) => {
                    for (moved, by) in input.mouse_motion.iter_mut().zip(motion) {
                        *moved += by;
                    }
                }
                Change::Plugged(plugged) => input.pad = plugged.then(Pad::default),
                // What a pad that is not there does is nothing.
                Change::PadAxis { axis, value } => {
                    if let Some(pad) = &mut input.pad {
                        pad.axes[axis as usize] = value;
                    }
                }
                Change::PadButton { button, down } => {
                    if let Some(pad) = &mut input.pad {
                        pad.buttons[button as usize] = down;
                    }
                }
                Change::PadHat(direction) => {
                    if let Some(pad) = &mut input.pad {
                        pad.hat = direction;
                    }
                }
            }
            self.next += 1;
        }
    }

    /// Whether every change has been applied.
    pub fn finished(&self) -> bool {
        self.next == self.events.len()
    }
}

/// A recording being made: what the game sees of the keyboard and the
/// mouse, read by read. Written where the game reads, and not where the
/// keys arrive, a recording holds exactly what the game observed and
/// nothing else.
#[derive(Debug, Clone)]
pub struct Recorder {
    /// The keyboard and the buttons as the game last read them.
    keys: [bool; 256],
    buttons: [bool; MOUSE_BUTTONS],
    pad: Option<Pad>,
    events: Vec<Event>,
}

impl Default for Recorder {
    fn default() -> Recorder {
        Recorder { keys: [false; 256], buttons: [false; MOUSE_BUTTONS], pad: None, events: Vec::new() }
    }
}

impl Recorder {
    /// Notes how the input at this read differs from that of the read
    /// before, and the movement of the frame.
    pub(crate) fn observe(&mut self, read: u64, input: &Input) {
        for (key, (&down, seen)) in input.keys.iter().zip(&mut self.keys).enumerate() {
            if down != *seen {
                *seen = down;
                self.events.push(Event { read, change: Change::Key { key: key as u8, down } });
            }
        }
        for (button, (&down, seen)) in input.mouse_buttons.iter().zip(&mut self.buttons).enumerate() {
            if down != *seen {
                *seen = down;
                self.events.push(Event { read, change: Change::Button { button: button as u8, down } });
            }
        }
        if input.frame_motion != [0; MOUSE_AXES] {
            self.events.push(Event { read, change: Change::Motion(input.frame_motion) });
        }
        if input.pad.is_some() != self.pad.is_some() {
            self.pad = input.pad.map(|_| Pad::default());
            self.events.push(Event { read, change: Change::Plugged(input.pad.is_some()) });
        }
        if let (Some(now), Some(seen)) = (input.pad, &mut self.pad) {
            for axis in 0..PAD_AXES {
                if now.axes[axis] != seen.axes[axis] {
                    let change = Change::PadAxis { axis: axis as u8, value: now.axes[axis] };
                    self.events.push(Event { read, change });
                }
            }
            for button in 0..PAD_BUTTONS {
                if now.buttons[button] != seen.buttons[button] {
                    let change = Change::PadButton { button: button as u8, down: now.buttons[button] };
                    self.events.push(Event { read, change });
                }
            }
            if now.hat != seen.hat {
                self.events.push(Event { read, change: Change::PadHat(now.hat) });
            }
            *seen = now;
        }
    }

    /// A recorder for a game about to start with this input: what is
    /// there before the game's first read is of read 0.
    pub fn starting_with(input: &Input) -> Recorder {
        let mut recorder = Recorder::default();
        recorder.observe(0, input);
        recorder
    }

    /// What has been recorded so far.
    pub fn recording(&self) -> Recording {
        Recording::new(self.events.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_reads_and_writes_its_text_form() {
        let text = "# jump at the title\n3600 press SPACE\n3605 release space  # let go\n\n3605 press up\n";
        let recording = Recording::parse(text).unwrap();
        assert_eq!(
            recording.events,
            [
                Event { read: 3600, change: Change::Key { key: 0x39, down: true } },
                Event { read: 3605, change: Change::Key { key: 0x39, down: false } },
                Event { read: 3605, change: Change::Key { key: 0xC8, down: true } },
            ]
        );
        assert_eq!(recording.to_text(), "3600 press SPACE\n3605 release SPACE\n3605 press UP\n");
        assert_eq!(Recording::parse(&recording.to_text()), Ok(recording));
        for wrong in [
            "press SPACE",
            "5 hold SPACE",
            "5 press SPACEBAR",
            "5 press SPACE now",
            "5 move 1 2",
            "5 move a 2 0",
        ] {
            assert!(Recording::parse(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn what_is_recorded_plays_back_the_same() {
        let mut recorder = Recorder::default();
        let mut input = Input::default();
        let mut seen = Vec::new();
        for read in 1..=6u64 {
            input.keys[0x39] = (3..5).contains(&read);
            input.keys[0x01] = read >= 5;
            recorder.observe(read, &input);
            seen.push(input.keys);
        }
        let mut recording = recorder.recording();
        assert_eq!(recording.to_text(), "3 press SPACE\n5 press ESCAPE\n5 release SPACE\n");
        let mut input = Input::default();
        for read in 1..=6u64 {
            recording.apply(read, &mut input);
            assert_eq!(input.keys, seen[read as usize - 1], "read {read}");
        }
    }

    #[test]
    fn the_mouse_is_recorded_and_played_back_frame_by_frame() {
        let mut recorder = Recorder::default();
        let mut input = Input::default();
        let mut seen = Vec::new();
        for read in 1..=4u64 {
            input.mouse_buttons[1] = read == 2 || read == 3;
            // What arrived since the read before becomes the frame's.
            input.mouse_motion = if read == 3 { [-12, 3, 0] } else { [0; 3] };
            input.take_frame_motion();
            recorder.observe(read, &input);
            seen.push((input.mouse_buttons, input.frame_motion));
        }
        let mut recording = recorder.recording();
        let text = "2 press MOUSE_RIGHT\n3 move -12 3 0\n4 release MOUSE_RIGHT\n";
        assert_eq!(recording.to_text(), text);
        assert_eq!(Recording::parse(text), Ok(recording.clone()));
        let mut input = Input::default();
        for read in 1..=4u64 {
            recording.apply(read, &mut input);
            input.take_frame_motion();
            assert_eq!((input.mouse_buttons, input.frame_motion), seen[read as usize - 1], "read {read}");
        }
    }

    #[test]
    fn a_joypad_is_recorded_and_played_back() {
        let mut input = Input { pad: Some(Pad::default()), ..Input::default() };
        let mut recorder = Recorder::starting_with(&input);
        let mut seen = Vec::new();
        for read in 1..=4u64 {
            let pad = input.pad.as_mut().unwrap();
            pad.axes[0] = if read >= 2 { -20000 } else { 0 };
            pad.buttons[2] = read == 3;
            pad.hat = (read == 4).then_some(6);
            recorder.observe(read, &input);
            seen.push(input.pad);
        }
        let mut recording = recorder.recording();
        let text = "0 plug PAD\n2 axis X -20000\n3 press PAD_3\n4 release PAD_3\n4 hat 6\n";
        assert_eq!(recording.to_text(), text);
        assert_eq!(Recording::parse(text), Ok(recording.clone()));
        let mut input = Input::default();
        recording.apply(0, &mut input);
        assert_eq!(input.pad, Some(Pad::default()));
        for read in 1..=4u64 {
            recording.apply(read, &mut input);
            assert_eq!(input.pad, seen[read as usize - 1], "read {read}");
        }
        for wrong in ["1 axis W 5", "1 axis X much", "1 hat 8", "1 press PAD_17", "1 plug MOUSE"] {
            assert!(Recording::parse(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn changes_are_seen_from_their_read_on() {
        let mut recording = Recording::parse("3 press SPACE\n5 release SPACE\n5 press ESCAPE\n").unwrap();
        let mut input = Input::default();
        let mut seen = Vec::new();
        for read in 1..=6 {
            recording.apply(read, &mut input);
            seen.push((input.keys[0x39], input.keys[0x01]));
        }
        assert_eq!(
            seen,
            [(false, false), (false, false), (true, false), (true, false), (false, true), (false, true)]
        );
        assert!(recording.finished());
    }
}
