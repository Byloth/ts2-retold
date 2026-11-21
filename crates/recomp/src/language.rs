//! Bringing another language's texts to the reference build (ADR 0016).
//!
//! The texts of the game are strings in the executable, and each language
//! was a build of its own. With the addresses of the two builds paired
//! ([`crate::pairing`]), every place of the reference build that refers to
//! a text is known together with the place of the other build that stands
//! for it, and so with the text the other language has there. The other
//! language's texts are put in a section of their own after the image, and
//! those places are made to refer to them.
//!
//! This goes place by place, not text by text: one English text can stand
//! where the other language has two different ones.

use std::collections::BTreeMap;

use crate::pairing::Pairing;
use crate::pe::{Image, Section};

/// The name of the section the other language's texts are put in.
pub const SECTION: &str = ".lang";

/// A place of the reference build that now refers to another text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moved {
    /// Where the address is written.
    pub site: u32,
    /// The text it referred to, and the text of the other build it now
    /// stands for (addresses in their own builds).
    pub from: u32,
    pub to: u32,
    /// Where that text is in the new image.
    pub now: u32,
}

/// The reference image with another language's texts.
#[derive(Debug, Clone)]
pub struct Transplant {
    pub image: Image,
    pub moved: Vec<Moved>,
}

/// The bytes of the text at `at`, without its end: a string of printable
/// characters (Latin-1) in the data of an image. Not in the code, not in
/// the table of imported functions (whose entries are numbers that can
/// look like letters), and with no address written anywhere in it.
pub fn text(image: &Image, at: u32) -> Option<&[u8]> {
    let section = image.section_at(at).filter(|section| !section.is_executable())?;
    if image.imports.iter().any(|import| section.contains(import.iat_va)) {
        return None;
    }
    let tail = section.data.get((at - section.va) as usize..)?;
    let bytes = &tail[..tail.iter().position(|&byte| byte == 0)?];
    let printable = |byte: &u8| matches!(byte, 9 | 10 | 13 | 32..=126 | 0xA0..);
    let holds_address = !image.relocs_in(at.saturating_sub(3), at + bytes.len() as u32 + 1).is_empty();
    (!bytes.is_empty() && bytes.iter().all(printable) && !holds_address).then_some(bytes)
}

/// Whether a string is the name of one of the game's source files, which
/// its error messages give. The builds were compiled on different drives;
/// that is no difference of language. (Variables stand right before some
/// of these names without a zero between, so that a reference to the
/// variable reads as a reference into a text: another reason to leave
/// them alone.)
fn is_source_path(text: &[u8]) -> bool {
    text.windows(10).any(|window| window == b":\\projects")
}

/// Where the string that `at` lies in begins: code that copies or compares
/// a text in place refers to its second character too.
fn start_of_text(image: &Image, at: u32) -> u32 {
    let Some(section) = image.section_at(at) else { return at };
    let mut start = at;
    while start > section.va && section.data[(start - 1 - section.va) as usize] != 0 {
        start -= 1;
    }
    start
}

/// Makes every place of `reference` that refers to a text refer to the
/// text `other` has in the same place, where the two differ. Places listed
/// in `keep` are left alone.
pub fn transplant_texts(reference: &Image, other: &Image, pairing: &Pairing, keep: &[u32]) -> Transplant {
    let mut image = reference.clone();
    let end =
        image.sections.iter().map(|section| section.va + section.virtual_size).max().unwrap_or(image.base);
    let section_va = end.next_multiple_of(0x1000);
    // The other build's texts that are wanted, by where they begin there,
    // and where each is put.
    let mut placed: BTreeMap<u32, u32> = BTreeMap::new();
    let mut bytes = Vec::new();
    let mut moved = Vec::new();
    for &(site, other_site) in &pairing.sites {
        let (Some(from), Some(to)) = (reference.read_u32(site), other.read_u32(other_site)) else { continue };
        let (Some(old), Some(new)) = (text(reference, from), text(other, to)) else { continue };
        if old == new || keep.contains(&from) || is_source_path(old) {
            continue;
        }
        let start = start_of_text(other, to);
        let at = *placed.entry(start).or_insert_with(|| {
            let at = section_va + bytes.len() as u32;
            let whole = text(other, start).unwrap_or(new);
            bytes.extend_from_slice(whole);
            bytes.push(0);
            bytes.resize(bytes.len().next_multiple_of(4), 0);
            at
        });
        let now = at + (to - start);
        let section =
            image.sections.iter_mut().find(|section| section.contains(site)).expect("a site is in a section");
        let offset = (site - section.va) as usize;
        section.data[offset..offset + 4].copy_from_slice(&now.to_le_bytes());
        moved.push(Moved { site, from, to, now });
    }
    if !bytes.is_empty() {
        image.sections.push(Section {
            name: SECTION.into(),
            va: section_va,
            virtual_size: bytes.len() as u32,
            flags: 0x4000_0040, // initialised data, readable
            data: bytes,
        });
        // The places still hold addresses, now of the new section.
    }
    Transplant { image, moved }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pairing::pair;

    const CODE: u32 = 0x40_1000;
    const DATA: u32 = 0x40_2000;

    /// An image whose code pushes the addresses of some texts and loads a
    /// variable, and whose data is the texts one after the other and then
    /// a table of them.
    fn image(texts: &[&[u8]], pushed: &[usize], table: &[usize]) -> Image {
        let mut data = Vec::new();
        let mut starts = Vec::new();
        for text in texts {
            starts.push(DATA + data.len() as u32);
            data.extend_from_slice(text);
            data.push(0);
            data.resize(data.len().next_multiple_of(4), 0);
        }
        let mut relocs = Vec::new();
        let mut code = Vec::new();
        for &index in pushed {
            code.push(0x68);
            relocs.push(CODE + code.len() as u32);
            code.extend_from_slice(&starts[index].to_le_bytes());
        }
        code.push(0xC3);
        for &index in table {
            relocs.push(DATA + data.len() as u32);
            data.extend_from_slice(&starts[index].to_le_bytes());
        }
        let section = |name: &str, va: u32, flags: u32, data: Vec<u8>| Section {
            name: name.into(),
            va,
            virtual_size: data.len() as u32,
            flags,
            data,
        };
        Image {
            base: 0x40_0000,
            entry: CODE,
            sections: vec![
                section(".text", CODE, 0x2000_0020, code),
                section(".data", DATA, 0xC000_0040, data),
            ],
            relocs,
            ..Image::default()
        }
    }

    #[test]
    fn texts_are_recognised() {
        let image = image(&[b"hello", b"x\x01", b"", b"citt\xe0"], &[0], &[3]);
        assert_eq!(text(&image, DATA), Some(&b"hello"[..]));
        assert_eq!(text(&image, DATA + 1), Some(&b"ello"[..]));
        assert_eq!(text(&image, DATA + 8), None); // not printable
        assert_eq!(text(&image, DATA + 12), None); // empty
        assert_eq!(text(&image, DATA + 16), Some(&b"citt\xe0"[..]));
        assert_eq!(text(&image, DATA + 24), None); // the table: an address
        assert_eq!(text(&image, CODE), None);
        // Nothing in the section of the imported functions' table is a text.
        let mut importing = image.clone();
        importing.imports.push(crate::pe::Import {
            dll: "KERNEL32.dll".into(),
            name: crate::pe::ImportName::Ordinal(1),
            iat_va: DATA + 4,
        });
        assert_eq!(text(&importing, DATA), None);
        assert_eq!(start_of_text(&image, DATA + 3), DATA);
    }

    #[test]
    fn places_come_to_refer_to_the_other_languages_texts() {
        // The reference: "back" serves as a key's name (in the table) and
        // as a menu's entry (pushed); "file.dat" is no language.
        let reference = image(&[b"start game", b"back", b"C:\\projects\\a.c"], &[0, 1, 2, 0], &[1]);
        // The other build has a longer text and two for "back", and was
        // compiled on another drive.
        let other =
            image(&[b"avvia partita", b"indietro", b"D:\\projects\\a.c", b"back"], &[0, 1, 2, 0], &[3]);
        let pairing = pair(&reference, &other);
        let transplant = transplant_texts(&reference, &other, &pairing, &[]);
        let image = &transplant.image;
        let pointed = |site: u32| text(image, image.read_u32(site).unwrap()).unwrap().to_vec();
        assert_eq!(pointed(CODE + 1), b"avvia partita");
        assert_eq!(pointed(CODE + 6), b"indietro");
        assert_eq!(pointed(CODE + 11), b"C:\\projects\\a.c");
        assert_eq!(pointed(CODE + 16), b"avvia partita");
        // The key keeps its name: the table's place stands for "back" there.
        let table = *reference.relocs.last().unwrap();
        assert_eq!(pointed(table), b"back");
        assert_eq!(image.read_u32(table), reference.read_u32(table));
        // One copy of each text, in a section after the image.
        assert_eq!(image.read_u32(CODE + 1), image.read_u32(CODE + 16));
        let section = image.sections.last().unwrap();
        assert_eq!((section.name.as_str(), section.va), (SECTION, 0x40_3000));
        assert_eq!(section.data, b"avvia partita\0\0\0indietro\0\0\0\0");
        assert_eq!(transplant.moved.len(), 3);
        // The reference itself is untouched, and a kept text stays.
        assert_eq!(text(&reference, reference.read_u32(CODE + 1).unwrap()), Some(&b"start game"[..]));
        let kept = transplant_texts(&reference, &other, &pairing, &[DATA]);
        assert_eq!(kept.moved.len(), 1);
    }
}
