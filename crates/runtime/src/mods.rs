//! The catalogue of modifications (ADR 0004).
//!
//! Everything the port does differently from the original game is listed
//! here, with an identifier and a switch. With every entry off, the port
//! behaves like the original.

/// What kind of change a modification is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Corrects a defect of the original.
    Fix,
    /// Presentation only.
    Graphics,
    /// Changes what happens in the game.
    Gameplay,
    /// Adds or removes something around the game.
    Feature,
}

#[derive(Debug)]
pub struct Modification {
    pub id: &'static str,
    pub class: Class,
    /// Whether results of the simulation can differ with it on.
    pub affects_simulation: bool,
    pub enabled_by_default: bool,
    pub summary: &'static str,
}

pub const SKIP_MODE_SELECT: &str = "skip-mode-select";

pub static CATALOGUE: &[Modification] = &[Modification {
    id: SKIP_MODE_SELECT,
    class: Class::Feature,
    affects_simulation: false,
    enabled_by_default: true,
    summary: "The start-up \"Screen Mode Select\" window does not exist: the game's routine for it is \
              replaced by one that makes the choice the original proposes when nothing was saved (the \
              first hardware 3D device and the first mode at least 480 lines high: 640x480 at 16 bits, \
              full screen). Choices saved in toy2.cfg are ignored, since they index lists that differ \
              from one machine to another.",
}];

/// Which modifications are switched on for this run.
#[derive(Debug, Clone)]
pub struct Modifications {
    enabled: Vec<&'static str>,
}

impl Default for Modifications {
    fn default() -> Self {
        Modifications { enabled: CATALOGUE.iter().filter(|m| m.enabled_by_default).map(|m| m.id).collect() }
    }
}

impl Modifications {
    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.contains(&id)
    }

    /// Switches a modification on or off. Fails for an unknown identifier.
    pub fn set(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let entry =
            CATALOGUE.iter().find(|m| m.id == id).ok_or_else(|| format!("unknown modification {id:?}"))?;
        self.enabled.retain(|&other| other != entry.id);
        if enabled {
            self.enabled.push(entry.id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_unique_and_switchable() {
        for (index, entry) in CATALOGUE.iter().enumerate() {
            assert!(CATALOGUE[..index].iter().all(|other| other.id != entry.id), "{} listed twice", entry.id);
        }
        let mut mods = Modifications::default();
        assert!(mods.is_enabled(SKIP_MODE_SELECT));
        mods.set(SKIP_MODE_SELECT, false).unwrap();
        assert!(!mods.is_enabled(SKIP_MODE_SELECT));
        assert!(mods.set("no-such-thing", true).is_err());
    }
}
