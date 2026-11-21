//! Changes to the menus of the reference build, made in its image.

use crate::pe::Image;

// The main menu (the function at 0x437C40) has five entries at fixed
// heights. Which one was chosen it tells by the height the arrows stand
// at, and answers with a number: 3 for the second entry, the options,
// and 4 for the third, loading and saving. Each entry's text is the
// operand of a `push` further on.

/// Where the numbers answered for the second and the third entry are.
const SECOND_ANSWER: u32 = 0x0043_7D6C;
const THIRD_ANSWER: u32 = 0x0043_7D80;
/// Where the addresses of the second and the third entry's texts are.
const SECOND_TEXT: u32 = 0x0043_7EDB;
const THIRD_TEXT: u32 = 0x0043_7EEF;

fn write(image: &mut Image, at: u32, value: u32) {
    let section = image.sections.iter_mut().find(|section| section.contains(at)).expect("in the image");
    let offset = (at - section.va) as usize;
    section.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Puts the entry for loading and saving before the options in the main
/// menu: the two entries change places, text and answer. The image must
/// be the reference build's, in whatever language its texts are.
pub fn load_before_options(image: &mut Image) -> Result<(), String> {
    let read = |image: &Image, at: u32| image.read_u32(at).ok_or("not the reference build's image");
    if (read(image, SECOND_ANSWER)?, read(image, THIRD_ANSWER)?) != (3, 4) {
        return Err("the main menu is not the reference build's".into());
    }
    let (second, third) = (read(image, SECOND_TEXT)?, read(image, THIRD_TEXT)?);
    write(image, SECOND_ANSWER, 4);
    write(image, THIRD_ANSWER, 3);
    write(image, SECOND_TEXT, third);
    write(image, THIRD_TEXT, second);
    Ok(())
}

/// The project's wording of the main menu's entry for loading and saving,
/// in the language of a build (the reference's, English, for `None`).
/// Decided by the owner: the stroke says both, and the typeface has it.
pub fn load_save_wording(language: Option<&str>) -> &'static [u8] {
    match language {
        Some(crate::builds::ITALIAN) => b"carica/salva",
        Some(crate::builds::FRENCH) => b"charger/sauvegarder",
        Some(crate::builds::SPANISH) => b"cargar/guardar",
        _ => b"load/save",
    }
}

/// The project's name for the camera the mouse turns, as the pause menu
/// lists it beside the game's two (ADR 0021): at most fifteen characters,
/// which is what the menu's box holds, in the game's Latin-1.
pub fn orbit_camera_wording(language: Option<&str>) -> &'static [u8] {
    match language {
        Some(crate::builds::ITALIAN) => b"camera orbitale",
        Some(crate::builds::FRENCH) => b"cam\xE9ra orbitale",
        Some(crate::builds::SPANISH) => b"c\xE1mara orbital",
        _ => b"orbit camera",
    }
}

/// Makes the main menu's entry for loading and saving read `wording`. To
/// be done before [`load_before_options`], which moves the entry.
pub fn word_load_and_save(image: &mut Image, wording: &[u8]) -> Result<(), String> {
    use crate::pe::Section;
    if image.read_u32(THIRD_ANSWER) != Some(4) {
        return Err("the main menu is not the reference build's, or its entries were moved".into());
    }
    let end = image.sections.iter().map(|section| section.va + section.virtual_size).max();
    let va = end.unwrap_or(image.base).next_multiple_of(0x1000);
    let mut data = wording.to_vec();
    data.push(0);
    image.sections.push(Section {
        name: ".menu".into(),
        va,
        virtual_size: data.len() as u32,
        flags: 0x4000_0040, // initialised data, readable
        data,
    });
    write(image, THIRD_TEXT, va);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::Section;

    #[test]
    fn the_two_entries_change_places() {
        let mut data = vec![0u8; 0x400];
        let base = 0x0043_7C00;
        let mut put = |at: u32, value: u32| {
            data[(at - base) as usize..][..4].copy_from_slice(&value.to_le_bytes());
        };
        put(SECOND_ANSWER, 3);
        put(THIRD_ANSWER, 4);
        put(SECOND_TEXT, 0x4F_5AF4);
        put(THIRD_TEXT, 0x4F_67F0);
        let section =
            Section { name: ".text".into(), va: base, virtual_size: 0x400, flags: 0x2000_0020, data };
        let mut image = Image { base: 0x40_0000, sections: vec![section], ..Image::default() };
        // The third entry worded anew, then moved to the second place.
        word_load_and_save(&mut image, load_save_wording(None)).unwrap();
        let worded = image.read_u32(THIRD_TEXT).unwrap();
        assert_eq!(image.c_string(worded).as_deref(), Some("load/save"));
        load_before_options(&mut image).unwrap();
        assert_eq!((image.read_u32(SECOND_ANSWER), image.read_u32(THIRD_ANSWER)), (Some(4), Some(3)));
        assert_eq!(
            (image.read_u32(SECOND_TEXT), image.read_u32(THIRD_TEXT)),
            (Some(worded), Some(0x4F_5AF4))
        );
        // Not twice, and not on something else.
        assert!(load_before_options(&mut image).is_err());
        assert!(word_load_and_save(&mut image, b"x").is_err());
    }
}
