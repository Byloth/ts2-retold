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

pub const LANGUAGE: &str = "language";
pub const MENU_ARROWS_BY_TEXT: &str = "menu-arrows-by-text";
pub const MENU_LOAD_BEFORE_OPTIONS: &str = "menu-load-before-options";

pub static CATALOGUE: &[Modification] = &[
    Modification {
        id: MENU_ARROWS_BY_TEXT,
        class: Class::Fix,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "The two arrows of the main menu stand beside the text of the entry chosen, wherever it \
                  begins and ends, and go from one width to the other as they move. The original draws \
                  them at two fixed places that suit its first entry, ten letters long: beside a shorter \
                  entry they stand far off, and on a longer one (in other languages) they fall on the text.",
    },
    Modification {
        id: MENU_LOAD_BEFORE_OPTIONS,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "In the main menu the entry for loading and saving comes before the options (the \
                  original has the options second and loading third), in every language. Decided by the \
                  owner.",
    },
    Modification {
        id: LANGUAGE,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: false,
        summary: "The game in another language (ADR 0016), from the user's own disc of that language \
                  (TS2_LANGUAGE_DIR): Italian, French or Spanish, told by the disc. A language is data: \
                  the places of the executable that refer to a text refer to the text that language's \
                  executable has in the same place, with a short list of corrections; speech, movies and \
                  textures are read from that disc. Level data and code stay the reference's. \
                  INCOMPLETE: the reference's code cannot draw letters with accents; until one way of \
                  drawing text serves every language, Italian alone borrows the Italian executable's own \
                  functions for it (to be removed), and French and Spanish show accents wrongly.",
    },
    Modification {
        id: SKIP_MODE_SELECT,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "The start-up \"Screen Mode Select\" window does not exist: the game's routine for it is \
              replaced by one that makes the choice the original proposes when nothing was saved (the \
              first hardware 3D device and the first mode at least 480 lines high: 640x480 at 16 bits, \
              full screen). Choices saved in toy2.cfg are ignored, since they index lists that differ \
              from one machine to another.",
    },
];

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
