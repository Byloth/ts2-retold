//! DSOUND: DirectSound.
//!
//! One output device, the default one, which Windows lists first under the
//! name "Primary Sound Driver" with a null identifier.

use super::{HostResult, functions};

pub const DS_OK: u32 = 0;

#[derive(Debug, Default)]
pub struct State {
    /// Guest strings describing the driver, created on first enumeration.
    driver_strings: Option<(u32, u32)>,
}

functions! { "DSOUND.dll";
    DirectSoundEnumerateA(2) => |m, a| -> HostResult {
        let (description, module) = match m.os.dsound.driver_strings {
            Some(strings) => strings,
            None => {
                let strings =
                    (m.os.string(&mut m.memory, b"Primary Sound Driver"), m.os.string(&mut m.memory, b""));
                m.os.dsound.driver_strings = Some(strings);
                strings
            }
        };
        m.call_guest(a[0], &[0, description, module, a[1]])?;
        Ok(DS_OK)
    },
}
