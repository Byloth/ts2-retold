//! Recorded input: what the keyboard does, and when.
//!
//! Time in a recording is counted in **reads of the keyboard**. The game
//! reads the keyboard's state once per frame, and nothing else it sees
//! depends on when a key went down, so "at the n-th read the state changes
//! like this" replays a session exactly, whatever the speed of the machine
//! it is replayed on (ADR 0017).
//!
//! As text, one change per line: the read at which it happens (the first
//! read is 1), `press` or `release`, and the key's DirectInput name without
//! `DIK_`. `#` starts a comment.
//!
//! ```text
//! 3600 press SPACE
//! 3605 release SPACE
//! ```

use crate::keys;
use crate::win32::dinput::Input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The read of the keyboard from which the change is seen.
    pub read: u64,
    /// The key's scan code.
    pub key: u8,
    pub down: bool,
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
            let [read, action, key] = line.split_whitespace().collect::<Vec<_>>()[..] else {
                return Err(wrong("expected a read number, press or release, and a key"));
            };
            let read =
                read.parse().ok().filter(|&read| read > 0).ok_or_else(|| wrong("not a read number"))?;
            let down = match action {
                "press" => true,
                "release" => false,
                _ => return Err(wrong("neither press nor release")),
            };
            let key = keys::code(key).ok_or_else(|| wrong("not a key name"))?;
            events.push(Event { read, key, down });
        }
        Ok(Recording::new(events))
    }

    /// The text form of the recording.
    pub fn to_text(&self) -> String {
        let line = |event: &Event| {
            let action = if event.down { "press" } else { "release" };
            format!("{} {action} {}\n", event.read, keys::name(event.key).unwrap_or("?"))
        };
        self.events.iter().map(line).collect()
    }

    /// Brings the keyboard to its state at the given read.
    pub(crate) fn apply(&mut self, read: u64, input: &mut Input) {
        while let Some(event) = self.events.get(self.next).filter(|event| event.read <= read) {
            input.keys[event.key as usize] = event.down;
            self.next += 1;
        }
    }

    /// Whether every change has been applied.
    pub fn finished(&self) -> bool {
        self.next == self.events.len()
    }
}

/// A recording being made: what the game sees of the keyboard, read by
/// read. Written where the game reads, and not where the keys arrive, a
/// recording holds exactly what the game observed and nothing else.
#[derive(Debug, Clone)]
pub struct Recorder {
    /// The keyboard as the game last read it.
    seen: [bool; 256],
    events: Vec<Event>,
}

impl Default for Recorder {
    fn default() -> Recorder {
        Recorder { seen: [false; 256], events: Vec::new() }
    }
}

impl Recorder {
    /// Notes how the keyboard at this read differs from the one before.
    pub(crate) fn observe(&mut self, read: u64, input: &Input) {
        for (key, (&down, seen)) in input.keys.iter().zip(&mut self.seen).enumerate() {
            if down != *seen {
                *seen = down;
                self.events.push(Event { read, key: key as u8, down });
            }
        }
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
                Event { read: 3600, key: 0x39, down: true },
                Event { read: 3605, key: 0x39, down: false },
                Event { read: 3605, key: 0xC8, down: true },
            ]
        );
        assert_eq!(recording.to_text(), "3600 press SPACE\n3605 release SPACE\n3605 press UP\n");
        assert_eq!(Recording::parse(&recording.to_text()), Ok(recording));
        for wrong in ["press SPACE", "0 press SPACE", "5 hold SPACE", "5 press SPACEBAR", "5 press SPACE now"]
        {
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
