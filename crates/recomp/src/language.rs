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

/// The name of the section the other build's functions are put in.
pub const CODE_SECTION: &str = ".graft";
/// How far after the texts' section the functions' section begins.
const CODE_SECTION_AFTER: u32 = 0x10_0000;

/// The functions the Italian build has in another form because of the
/// language: where each begins in the reference build (0 for a function
/// the reference build does not have) and in the Italian one
/// (`builds::REFERENCE`, `builds::ITALIAN`). What they are is known from
/// the texts they use and from where they stand; none has been read
/// through (`UNVERIFIED` beyond that).
pub const ITALIAN_FUNCTIONS: &[(u32, u32)] = &[
    // The text shown while nothing is pressed ("press jump to continue").
    (0x0040_1C30, 0x0040_1C30),
    // The front end: the options menu, the main menu, two functions of its
    // drawing and one they call that the reference build lacks, the level
    // map, the credits and the movie viewer.
    (0x0043_71B0, 0x0043_71C0),
    (0x0043_7C40, 0x0043_7C90),
    (0, 0x0043_8000),
    (0x0043_81F0, 0x0043_8250),
    (0x0043_8520, 0x0043_8580),
    (0x0043_8A50, 0x0043_8AD0),
    (0x0043_98B0, 0x0043_9960),
    (0x0043_A600, 0x0043_A6D0),
    // The four that draw a line of text in the small typeface: the Italian
    // ones draw a letter with an accent as the letter and the accent, know
    // more signs, and set the letters 7 pixels apart instead of 8.
    (0x0049_B2A0, 0x0049_B390),
    (0x0049_B3B0, 0x0049_B8B0),
    (0x0049_B580, 0x0049_BDE0),
    (0x0049_B630, 0x0049_C260),
    // Loading and saving, the controls, the graphics settings.
    (0x0049_B9E0, 0x0049_C970),
    (0x0049_C420, 0x0049_D410),
    (0x0049_CAA0, 0x0049_DB20),
    // Two more of the text functions, and the pause menu with the
    // messages of the game.
    (0x0049_D390, 0x0049_E450),
    (0x0049_D910, 0x0049_ED60),
    (0x0049_FD40, 0x004A_1190),
];

/// Functions that differ between the two builds for other reasons than
/// language (the Italian build is a later revision: fixes in the 3D
/// engine and the sound, sizes of tables, line numbers in messages). They
/// stay the reference's; an Italian function that calls one of them calls
/// the reference's. In the reference build and in the Italian one.
pub const ITALIAN_COUNTERPARTS: &[(u32, u32)] = &[
    (0x0043_2AD0, 0x0043_2AE0),
    (0x0045_2260, 0x0045_2340),
    (0x0045_2FC0, 0x0045_30A0),
    (0x0047_E420, 0x0047_E500),
    (0x0048_9C30, 0x0048_9D20),
    (0x0049_09E0, 0x0049_0AD0),
    (0x0049_8480, 0x0049_8570),
    (0x0049_8550, 0x0049_8640),
    (0x004B_2ED0, 0x004B_4370),
    (0x004B_3110, 0x004B_45B0),
    (0x004B_37F0, 0x004B_4CB0),
    (0x004C_3BE0, 0x004C_50A0),
    (0x004C_4080, 0x004C_5540),
    (0x004C_4220, 0x004C_56E0),
    (0x004C_BC90, 0x004C_D150),
];

/// A function of the reference build whose place is taken by the other
/// build's function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Graft {
    /// Where the function begins in the reference build, if it has it.
    pub reference: Option<u32>,
    /// The other build's function: where it begins and where it ends,
    /// with the tables that follow its code.
    pub other: (u32, u32),
}

/// Why a function could not be taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// It refers to this address of the other build's data, which has no
    /// counterpart in the reference build (the place is given first).
    Data(u32, u32),
    /// It refers to, calls or jumps to this address of the other build's
    /// code, which has no counterpart.
    Code(u32, u32),
    /// A short jump at this place leaves the function.
    ShortJump(u32),
}

/// The reference image being given another language.
pub struct Hybrid<'a> {
    reference: &'a Image,
    other: &'a Image,
    pairing: &'a Pairing,
    image: Image,
    section_va: u32,
    /// The other build's texts that are wanted, by where they begin
    /// there, and where each is put.
    placed: BTreeMap<u32, u32>,
    texts: Vec<u8>,
    moved: Vec<Moved>,
    code: Vec<u8>,
    code_sites: Vec<u32>,
    /// Addresses taken from their neighbours for lack of a pair of their
    /// own: the other build's and the reference's.
    pub inferred: Vec<(u32, u32)>,
    /// Variables only the other build has, given room beside the texts:
    /// the other build's address, the new one and the size.
    pub added: Vec<(u32, u32, u32)>,
    /// The functions the two builds have in common: where each begins in
    /// the other build and in the reference, and its length; by the other
    /// build's address ([`crate::pairing::common_functions`]).
    pub functions: Vec<(u32, u32, u32)>,
}

impl<'a> Hybrid<'a> {
    pub fn new(reference: &'a Image, other: &'a Image, pairing: &'a Pairing) -> Hybrid<'a> {
        let end = reference.sections.iter().map(|section| section.va + section.virtual_size).max();
        Hybrid {
            reference,
            other,
            pairing,
            image: reference.clone(),
            section_va: end.unwrap_or(reference.base).next_multiple_of(0x1000),
            placed: BTreeMap::new(),
            texts: Vec::new(),
            moved: Vec::new(),
            code: Vec::new(),
            code_sites: Vec::new(),
            inferred: Vec::new(),
            added: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// Where the other build's text at `to` is in the new image; it is
    /// put there if it was not yet.
    fn place_text(&mut self, to: u32) -> u32 {
        let start = start_of_text(self.other, to);
        let (section_va, other, texts) = (self.section_va, self.other, &mut self.texts);
        let at = *self.placed.entry(start).or_insert_with(|| {
            let at = section_va + texts.len() as u32;
            texts.extend_from_slice(text(other, start).unwrap_or_default());
            texts.push(0);
            texts.resize(texts.len().next_multiple_of(4), 0);
            at
        });
        at + (to - start)
    }

    fn write(&mut self, site: u32, value: u32) {
        let section = self.image.sections.iter_mut().find(|section| section.contains(site));
        let section = section.expect("a site is in a section");
        let offset = (site - section.va) as usize;
        section.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// Makes every place of the reference that refers to a text refer to
    /// the text the other build has in the same place, where the two
    /// differ. Texts at the addresses in `keep` are left alone.
    pub fn texts(&mut self, keep: &[u32]) {
        for &(site, other_site) in &self.pairing.sites {
            let (reference, other) = (self.reference, self.other);
            let (Some(from), Some(to)) = (reference.read_u32(site), other.read_u32(other_site)) else {
                continue;
            };
            let (Some(old), Some(new)) = (text(reference, from), text(other, to)) else { continue };
            if old == new || keep.contains(&from) || is_source_path(old) {
                continue;
            }
            let now = self.place_text(to);
            self.write(site, now);
            self.moved.push(Moved { site, from, to, now });
        }
    }

    /// The address in the reference build of a place in the other build's
    /// code, where the code around it is the same in both: between two
    /// relocation sites that follow each other in both builds and were
    /// aligned, the code is the same byte for byte.
    fn code_in_reference(&self, at: u32) -> Option<u32> {
        // In a function both builds have: the same place in it.
        let index = self.functions.partition_point(|&(other, _, _)| other <= at);
        if let Some(&(other, reference, length)) =
            index.checked_sub(1).and_then(|index| self.functions.get(index))
        {
            if at - other < length {
                return Some(reference + (at - other));
            }
        }
        let code = self.other.code_section()?;
        let sites = &self.pairing.sites;
        // The aligned sites of the code come first and in order.
        let count = sites.partition_point(|&(_, other)| code.contains(other) && other <= at);
        let &(before, other_before) = sites.get(count.checked_sub(1)?)?;
        let &(after, other_after) = sites.get(count).filter(|&&(_, other)| code.contains(other))?;
        let consecutive = self.other.relocs_in(other_before + 1, other_after).is_empty()
            && self.reference.relocs_in(before + 1, after).is_empty()
            && other_after - other_before == after - before;
        consecutive.then(|| before + (at - other_before))
    }

    /// The address in the reference build of something in the other
    /// build's data: its pair, or, for something between two things that
    /// moved by the same amount, the address that amount away. Something
    /// between two things that moved by different amounts is taken for a
    /// variable only the other build has, as large as the room up to the
    /// next thing, and is given room of its own: right if nothing of the
    /// reference build that is kept uses the variable, which is for
    /// whoever lists the functions to check ([`Hybrid::added`]).
    fn data_in_reference(&mut self, at: u32, backward: &BTreeMap<u32, u32>) -> Option<u32> {
        if let Some(&paired) = backward.get(&at) {
            return Some(paired);
        }
        if let Some(&(_, now, _)) =
            self.added.iter().find(|&&(other, _, size)| (other..other + size).contains(&at))
        {
            let other = self.added.iter().find(|&&(other, _, size)| (other..other + size).contains(&at))?.0;
            return Some(now + (at - other));
        }
        let (&below, &below_paired) = backward.range(..at).next_back()?;
        let (&above, &above_paired) = backward.range(at..).next()?;
        let same_section = self.other.section_at(below).map(|section| section.va)
            == self.other.section_at(above).map(|section| section.va);
        if !same_section {
            return None;
        }
        if below_paired.wrapping_sub(below) == above_paired.wrapping_sub(above) {
            let inferred = at.wrapping_add(below_paired.wrapping_sub(below));
            self.inferred.push((at, inferred));
            return Some(inferred);
        }
        // Room for it beside the texts, with what it holds at the start.
        let size = above - at;
        let section = self.other.section_at(at)?;
        let offset = (at - section.va) as usize;
        let now = self.section_va + self.texts.len() as u32;
        let initial = section.data.get(offset..).unwrap_or_default();
        let initial = &initial[..initial.len().min(size as usize)];
        self.texts.extend_from_slice(initial);
        self.texts.resize((self.texts.len() - initial.len() + size as usize).next_multiple_of(4), 0);
        self.added.push((at, now, size));
        Some(now)
    }

    /// Puts the other build's functions beside the image, with every
    /// address in them translated, and makes the reference build's
    /// functions lead to them. `instructions` are the addresses of the
    /// other build's instructions (from its analysis). Either all the
    /// functions are taken over or none.
    pub fn graft(
        &mut self,
        grafts: &[Graft],
        counterparts: &[(u32, u32)],
        instructions: &[u32],
    ) -> Result<(), Vec<Problem>> {
        use iced_x86::{Decoder, DecoderOptions, OpKind};
        let other = self.other;
        let other_code = other.code_section().expect("the other build has code");
        let code_va = self.section_va + CODE_SECTION_AFTER;
        // Where each function goes.
        let mut starts = Vec::new();
        let mut size = self.code.len() as u32;
        for graft in grafts {
            size = size.next_multiple_of(16);
            starts.push(code_va + size);
            size += graft.other.1 - graft.other.0;
        }
        // A place in one of the functions taken over, or the start of a
        // function that stays the reference's.
        let grafted = |at: u32| {
            grafts
                .iter()
                .zip(&starts)
                .find(|(graft, _)| (graft.other.0..graft.other.1).contains(&at))
                .map(|(graft, &start)| start + (at - graft.other.0))
                .or_else(|| {
                    counterparts.iter().find(|&&(_, other)| other == at).map(|&(reference, _)| reference)
                })
        };
        let backward: BTreeMap<u32, u32> = self.pairing.pairs.iter().map(|(&from, &to)| (to, from)).collect();
        let mut problems = Vec::new();
        let mut code = self.code.clone();
        let mut sites = Vec::new();
        for (graft, &start) in grafts.iter().zip(&starts) {
            let (from, to) = graft.other;
            let offset = (from - other_code.va) as usize;
            let mut bytes = other_code.data[offset..offset + (to - from) as usize].to_vec();
            // The addresses written in it.
            for &site in other.relocs_in(from, to) {
                let held = other.read_u32(site).expect("a relocation site holds an address");
                let now = if let Some(now) = grafted(held) {
                    Some(now)
                } else if other_code.contains(held) {
                    self.code_in_reference(held)
                } else {
                    // A text the reference has differently, or not at
                    // all, is the other build's; everything else is the
                    // reference's own.
                    let paired = backward.get(&held).copied();
                    let same_text =
                        paired.is_some_and(|paired| text(self.reference, paired) == text(other, held));
                    if text(other, held).is_some() && !same_text {
                        Some(self.place_text(held))
                    } else {
                        self.data_in_reference(held, &backward)
                    }
                };
                match now {
                    Some(now) => {
                        let at = (site - from) as usize;
                        bytes[at..at + 4].copy_from_slice(&now.to_le_bytes());
                        sites.push(start + (site - from));
                    }
                    None if other_code.contains(held) => problems.push(Problem::Code(site, held)),
                    None => problems.push(Problem::Data(site, held)),
                }
            }
            // The calls and jumps that leave it.
            let first = instructions.partition_point(|&at| at < from);
            for &at in instructions[first..].iter().take_while(|&&at| at < to) {
                let tail = &other_code.data[(at - other_code.va) as usize..];
                let instruction = Decoder::with_ip(32, tail, at as u64, DecoderOptions::NONE).decode();
                if instruction.op_count() == 0 || instruction.op0_kind() != OpKind::NearBranch32 {
                    continue;
                }
                let target = instruction.near_branch_target() as u32;
                if (from..to).contains(&target) {
                    continue;
                }
                let length = instruction.len() as u32;
                // A distance of four bytes ends the instruction.
                if length < 5 {
                    problems.push(Problem::ShortJump(at));
                    continue;
                }
                let Some(now) = grafted(target).or_else(|| self.code_in_reference(target)) else {
                    problems.push(Problem::Code(at, target));
                    continue;
                };
                let next = start + (at - from) + length;
                let distance = now.wrapping_sub(next);
                let place = (at - from + length - 4) as usize;
                bytes[place..place + 4].copy_from_slice(&distance.to_le_bytes());
            }
            code.resize((start - code_va) as usize, 0xCC);
            code.extend_from_slice(&bytes);
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        self.code = code;
        self.code_sites.extend(sites);
        // The reference's functions begin with a jump to their successors.
        for (graft, &start) in grafts.iter().zip(&starts) {
            let Some(reference) = graft.reference else { continue };
            let section = self.image.sections.iter_mut().find(|section| section.contains(reference));
            let section = section.expect("the function is in a section");
            let at = (reference - section.va) as usize;
            section.data[at] = 0xE9;
            section.data[at + 1..at + 5].copy_from_slice(&start.wrapping_sub(reference + 5).to_le_bytes());
            // An address that was written under the jump is no more.
            self.image.relocs.retain(|&site| site + 3 < reference || site >= reference + 5);
        }
        Ok(())
    }

    /// The image with its new sections.
    pub fn finish(mut self) -> Transplant {
        if !self.texts.is_empty() {
            self.image.sections.push(Section {
                name: SECTION.into(),
                va: self.section_va,
                virtual_size: self.texts.len() as u32,
                flags: 0xC000_0040, // initialised data, readable and writable
                data: self.texts,
            });
        }
        if !self.code.is_empty() {
            self.image.sections.push(Section {
                name: CODE_SECTION.into(),
                va: self.section_va + CODE_SECTION_AFTER,
                virtual_size: self.code.len() as u32,
                flags: 0x6000_0020, // code, executable, readable
                data: self.code,
            });
            self.image.relocs.extend(self.code_sites);
            self.image.relocs.sort_unstable();
        }
        Transplant { image: self.image, moved: self.moved }
    }
}

/// Makes every place of `reference` that refers to a text refer to the
/// text `other` has in the same place, where the two differ. Places listed
/// in `keep` are left alone.
pub fn transplant_texts(reference: &Image, other: &Image, pairing: &Pairing, keep: &[u32]) -> Transplant {
    let mut hybrid = Hybrid::new(reference, other, pairing);
    hybrid.texts(keep);
    hybrid.finish()
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

    #[test]
    fn a_function_is_taken_over_with_its_addresses_translated() {
        let section = |name: &str, va: u32, flags: u32, data: Vec<u8>| Section {
            name: name.into(),
            va,
            virtual_size: data.len() as u32,
            flags,
            data,
        };
        // The reference: f pushes a text and calls g; g loads a variable.
        let (text_at, variable) = (DATA, DATA + 8);
        let mut code = vec![0x68];
        code.extend(text_at.to_le_bytes());
        code.extend([0xE8, 6, 0, 0, 0, 0xC3, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC]); // call g; ret
        let g = CODE + code.len() as u32;
        code.push(0xA1);
        code.extend(variable.to_le_bytes());
        code.push(0xC3);
        let reference = Image {
            base: 0x40_0000,
            entry: CODE,
            sections: vec![
                section(".text", CODE, 0x2000_0020, code),
                section(".data", DATA, 0xC000_0040, b"one\0\0\0\0\0\x07\0\0\0".to_vec()),
            ],
            relocs: vec![CODE + 1, g + 1],
            ..Image::default()
        };
        // The other build: f also loads the variable and begins otherwise;
        // its text is longer, so the variable is further on; g is the same.
        let (other_text, other_variable) = (DATA, DATA + 12);
        let mut code = vec![0x90, 0x90, 0x68];
        code.extend(other_text.to_le_bytes());
        code.push(0xA1);
        code.extend(other_variable.to_le_bytes());
        code.extend([0xE8, 6, 0, 0, 0, 0xC3, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC]);
        let other_g = CODE + code.len() as u32;
        code.push(0xA1);
        code.extend(other_variable.to_le_bytes());
        code.push(0xC3);
        let other = Image {
            base: 0x40_0000,
            entry: CODE,
            sections: vec![
                section(".text", CODE, 0x2000_0020, code),
                section(".data", DATA, 0xC000_0040, b"uno e due\0\0\0\x07\0\0\0".to_vec()),
            ],
            relocs: vec![CODE + 3, CODE + 8, other_g + 1],
            ..Image::default()
        };
        let pairing = pair(&reference, &other);
        assert_eq!(pairing.pairs.get(&variable), Some(&other_variable));
        let mut hybrid = Hybrid::new(&reference, &other, &pairing);
        hybrid.functions = vec![(other_g, g, 6)];
        let graft = Graft { reference: Some(CODE), other: (CODE, other_g) };
        // The instructions of the other build's f.
        let instructions = [CODE, CODE + 1, CODE + 2, CODE + 7, CODE + 12, CODE + 17];
        hybrid.graft(&[graft], &[], &instructions).unwrap();
        let image = hybrid.finish().image;
        let (texts, grafted) = (&image.sections[2], &image.sections[3]);
        assert_eq!((texts.name.as_str(), texts.va), (SECTION, 0x40_3000));
        assert_eq!(texts.data, b"uno e due\0\0\0");
        assert_eq!((grafted.name.as_str(), grafted.va), (CODE_SECTION, 0x50_3000));
        // nop nop; push the Italian text; load the reference's variable;
        // call the reference's g.
        let mut expected = vec![0x90, 0x90, 0x68];
        expected.extend(texts.va.to_le_bytes());
        expected.push(0xA1);
        expected.extend(variable.to_le_bytes());
        expected.push(0xE8);
        expected.extend(g.wrapping_sub(grafted.va + 17).to_le_bytes());
        expected.extend([0xC3, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC]);
        assert_eq!(grafted.data, expected);
        // The reference's f leads there; the address it pushed is gone
        // from the list of addresses, the new ones are on it.
        let code = &image.sections[0].data;
        assert_eq!(code[0], 0xE9);
        assert_eq!(code[1..5], grafted.va.wrapping_sub(CODE + 5).to_le_bytes());
        assert_eq!(image.relocs, [g + 1, grafted.va + 3, grafted.va + 8]);

        // What cannot be translated is said, and nothing is changed.
        let mut hybrid = Hybrid::new(&reference, &other, &pairing);
        let problems = hybrid.graft(&[graft], &[], &instructions).unwrap_err();
        assert_eq!(problems, [Problem::Code(CODE + 12, other_g)]);
        assert_eq!(hybrid.finish().image.sections[0].data, reference.sections[0].data);
    }
}
