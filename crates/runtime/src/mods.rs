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
pub const MENU_LOAD_SAVE_WORDING: &str = "menu-load-save-wording";
pub const ONE_TYPEFACE: &str = "one-typeface";
pub const MENU_LOAD_BEFORE_OPTIONS: &str = "menu-load-before-options";
pub const MOUSE_CONTROLS: &str = "mouse-controls";
pub const ORBIT_CAMERA: &str = "orbit-camera";
pub const WALKING_KEYS: &str = "walking-keys";
pub const RENDER_SCALE: &str = "render-scale";
pub const DRAW_DISTANCE: &str = "draw-distance";
pub const FAR_ENEMIES: &str = "far-enemies";

pub static CATALOGUE: &[Modification] = &[
    Modification {
        id: ONE_TYPEFACE,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "Text in the three typefaces (large, small, levels') is drawn by one function with one table from character to \
                  glyph, for every language, from one atlas put together from all the discs at hand, with \
                  every sign any of them has, each once (ADR 0016): the hyphen, the accents and the signs \
                  of each language are there for every language. It also draws the digits and signs the \
                  original's function leaves out though the typeface has them (the stroke among them), \
                  lays out texts with line breaks, and sets a line closer when it is too wide. English \
                  texts come out as in the original, pixel for pixel.",
    },
    Modification {
        id: MENU_LOAD_SAVE_WORDING,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "The main menu's entry for loading and saving reads \"load/save\" (in Italian \
                  \"carica/salva\", in French \"charger/sauvegarder\", in Spanish \"cargar/guardar\"), \
                  where the original has \"load game\" and the other languages \
                  \"carica-salva opzioni\", \"charger-sauvegarder\", \"cargar-guardar\". Needs one-typeface for the stroke. Decided by the owner.",
    },
    Modification {
        id: MENU_ARROWS_BY_TEXT,
        class: Class::Fix,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "What a menu draws beside the entry chosen follows that entry's text, worked out when \
                  it is drawn. The two arrows of the main menu stand beside the text wherever it begins \
                  and ends, and go from one width to the other as they move; the original draws them at \
                  two fixed places that suit its first entry, ten letters long: beside a shorter entry \
                  they stand far off, and on a longer one (in other languages) they fall on the text. \
                  The two pointers of the options menu stand where the original has them, and further \
                  apart where an entry is wider than the original's widest, twenty letters.",
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
        id: MOUSE_CONTROLS,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "The mouse's buttons are controls (ADR 0021): where the game has read its devices and made its \
                  mask of actions, the actions bound to the buttons held are added. By default the left button \
                  fires, the middle one spins, and the right one holds the visor view (first person) for as long \
                  as it is held, where the key for it is a switch. The options have a fifth entry, \"mouse and \
                  camera\" (in the game's language), whose page is made after the graphics page: the mouse's \
                  sensitivity in ten steps, the vertical axis inverted or not, and the right button holding \
                  the visor or spinning. What is set there, and the camera chosen in the pause menu, is kept \
                  in retold.ini in the state directory. The original takes no notice of the mouse and has \
                  four options; with the mouse left alone and the options not opened nothing differs.",
    },
    Modification {
        id: RENDER_SCALE,
        class: Class::Graphics,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "Triangles are drawn at a finer grain than the game's picture (ADR 0014): three by three \
                  texels to each of its pixels unless --scale says otherwise, so 1920 by 1440 for the \
                  game's 640 by 480. Edges lose their steps and no longer jump as the view moves; \
                  textures and everything the game draws in rectangles (menus' pictures, copies, fills) \
                  keep their own pixels. Where the game reads the picture back it gets its own size, \
                  each pixel the mean of its texels. The original draws at the size of the display mode.",
    },
    Modification {
        id: DRAW_DISTANCE,
        class: Class::Graphics,
        affects_simulation: true,
        enabled_by_default: true,
        summary: "Scenery, the objects drawn with it, and coins are drawn from farther away (ADR 0022): \
                  three times the original's distances unless --distance says otherwise, up to the \
                  world's far plane. The original ends its detailed scenery 6500 units from the camera \
                  (12000 at its own high detail) and its coins at about 6650. Only tests that decide \
                  drawing are changed: a coin is still picked up only where the original would have \
                  drawn it. Enemies are far-enemies' matter. Drawing more is more of the game's code run in a frame, and the \
                  machine's clock is its instructions: frames take longer on it, as on a slower computer.",
    },
    Modification {
        id: FAR_ENEMIES,
        class: Class::Graphics,
        affects_simulation: true,
        enabled_by_default: true,
        summary: "Enemies and characters are drawn from farther away than the game sees them (ADR 0022): \
                  within four times their range times the factor of draw-distance, if they are alive, \
                  in a room that was drawn and in view. The picture only: the game does not update one \
                  until its own rule wakes it, so from afar they stand as they were left and do not \
                  move. The original draws an enemy only while it updates it. Like draw-distance it \
                  costs time on the machine's clock. Decided by the owner.",
    },
    Modification {
        id: WALKING_KEYS,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: true,
        summary: "W, A, S and D walk, beside the arrows (ADR 0021): where the game has read its devices, \
                  the directions of those keys held are added to its mask of actions, as its own keys' \
                  are. The arrows and every binding of the game's own stay as they are; the original \
                  has nothing on those four keys. Decided by the owner.",
    },
    Modification {
        id: ORBIT_CAMERA,
        class: Class::Gameplay,
        affects_simulation: true,
        enabled_by_default: true,
        summary: "The camera goes around the character as the mouse moves, sideways and up and down, and stays \
                  where it is put (ADR 0021). The original's camera decides its own angles (it turns behind the \
                  character, swings away from walls, comes back to one elevation); that decision is replaced, and \
                  the game's own code still places the camera from the angles, brings it nearer when something is \
                  in the way and turns it to the character. The camera's two actions turn it as in the original's \
                  \"passive\" camera. The directions the character walks in follow the camera, as in the original. \
                  In the visor (first person) the mouse aims. Cameras the levels script and the pause are \
                  the original's.",
    },
    Modification {
        id: LANGUAGE,
        class: Class::Feature,
        affects_simulation: false,
        enabled_by_default: false,
        summary: "The game in another language (ADR 0016), from the user's own disc of that language \
                  (TS2_LANGUAGE_DIR): Italian, French or Spanish, told by the disc. A language is data and \
                  nothing else: the places of the executable that refer to a text refer to the text that \
                  language's executable has in the same place; speech, movies and textures are read from \
                  that disc. Code, level data and timings stay the reference's. INCOMPLETE: only the \
                  large typeface of the menus is drawn the one way (one-typeface); in the other \
                  typefaces letters with accents are still wrong.",
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
    /// None switched on: the game as its executable is.
    pub fn none() -> Modifications {
        Modifications { enabled: Vec::new() }
    }

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
