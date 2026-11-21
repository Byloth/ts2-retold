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

/// What was done with the texts that stand, in one build or the other,
/// between two places that were aligned (see [`Hybrid::texts_between`]).
/// Places are given by where the address is written, each in its build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Between {
    /// A place of the reference was given the other build's texts, in
    /// their order: one, or several as the lines of one text.
    Given { site: u32, texts: Vec<u32> },
    /// Texts the other build draws right after an aligned text were made
    /// further lines of it.
    Continued { site: u32, texts: Vec<u32> },
    /// Nothing was done: the two builds have these places, which could
    /// not be told apart. The reference's stay as they are.
    Left { reference: Vec<u32>, other: Vec<u32> },
}

/// How far after a place of the other build a text may be referred to and
/// still be taken for a further line of that place's text: two calls of a
/// text function with their arguments.
const CONTINUATION_REACH: u32 = 48;

/// The most lines the other build's texts are made into for one place of
/// the reference: it draws in two lines what the reference draws in one.
/// More than that for one place is not one text cut in pieces.
const MOST_LINES: usize = 2;

/// Whether a text is words, and not a sign or a filler: only such are
/// told apart by where they stand.
fn is_words(text: &[u8]) -> bool {
    text.len() >= 2 && text.iter().any(u8::is_ascii_lowercase)
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

    /// Puts a text of the project's making beside the other build's.
    fn place_bytes(&mut self, bytes: &[u8]) -> u32 {
        let at = self.section_va + self.texts.len() as u32;
        self.texts.extend_from_slice(bytes);
        self.texts.push(0);
        self.texts.resize(self.texts.len().next_multiple_of(4), 0);
        at
    }

    /// The other build's texts referred to from these places of it, as
    /// the lines of one text.
    fn lines(&self, sites: &[u32]) -> Vec<u8> {
        let texts = sites.iter().filter_map(|&site| text(self.other, self.other.read_u32(site)?));
        let lines: Vec<&[u8]> = texts.map(|line| line.trim_ascii_end()).collect();
        lines.join(&b'\n')
    }

    /// Deals with the texts [`Hybrid::texts`] cannot reach: those referred
    /// to from places of the code that were not aligned, because the code
    /// around them differs between the builds. To be called after it.
    ///
    /// Between two aligned places the two builds refer to texts in the
    /// same order. Texts that are the same in both (the names of keys,
    /// signs) are taken as the same places, and divide the stretch. In
    /// what is left between them:
    ///
    /// - as many texts in the other build as in the reference, or twice
    ///   as many: each place of the reference gets its
    ///   one, or its several as the lines of one text (the other build
    ///   draws in two lines what the reference draws in one);
    /// - texts in the other build only, right after an aligned place whose
    ///   text was brought over: further lines of that text;
    /// - anything else is left alone and reported.
    pub fn texts_between(&mut self) -> Vec<Between> {
        let (reference, other) = (self.reference, self.other);
        let in_code = |image: &Image, site: u32| image.section_at(site).is_some_and(Section::is_executable);
        let mut aligned: Vec<(u32, u32)> = self.pairing.sites.clone();
        aligned.retain(|&(here, there)| in_code(reference, here) && in_code(other, there));
        aligned.sort_unstable();
        // The places of a build's code that refer to a text and were not aligned.
        let loose = |image: &Image, taken: Vec<u32>| -> Vec<u32> {
            let refers_to_text = |site: u32| {
                let text = image.read_u32(site).and_then(|at| text(image, at));
                text.is_some_and(|text| !is_source_path(text))
            };
            let mut sites: Vec<u32> = image.relocs.clone();
            sites.retain(|&site| {
                in_code(image, site) && taken.binary_search(&site).is_err() && refers_to_text(site)
            });
            sites
        };
        let mut taken_there: Vec<u32> = aligned.iter().map(|&(_, there)| there).collect();
        taken_there.sort_unstable();
        let here = loose(reference, aligned.iter().map(|&(here, _)| here).collect());
        let there = loose(other, taken_there);
        let content = |image: &'a Image, site: u32| image.read_u32(site).and_then(|at| text(image, at));

        let mut done = Vec::new();
        let ends = aligned.iter().copied().chain([(u32::MAX, u32::MAX)]);
        let mut from = (0u32, 0u32);
        for to in ends {
            let within = |sites: &[u32], low: u32, high: u32| -> Vec<u32> {
                sites.iter().copied().filter(|&site| site > low && site < high).collect()
            };
            let (ours, theirs) = (within(&here, from.0, to.0), within(&there, from.1, to.1));
            let head = Some(from).filter(|&(here, _)| here != 0);
            from = to;
            if ours.is_empty() && theirs.is_empty() {
                continue;
            }
            // Texts that are the same in both, in order, divide the stretch.
            let (mut pieces, mut piece, mut next) = (Vec::new(), (Vec::new(), head), 0);
            for &site in &ours {
                let same =
                    (next..theirs.len()).find(|&at| content(other, theirs[at]) == content(reference, site));
                match same {
                    Some(at) => {
                        let (ours, head) = std::mem::replace(&mut piece, (Vec::new(), None));
                        pieces.push((ours, theirs[next..at].to_vec(), head));
                        next = at + 1;
                    }
                    None => piece.0.push(site),
                }
            }
            pieces.push((piece.0, theirs[next..].to_vec(), piece.1));
            for (ours, theirs, head) in pieces {
                let words = |image: &'a Image, sites: Vec<u32>| -> Vec<u32> {
                    sites.into_iter().filter(|&site| content(image, site).is_some_and(is_words)).collect()
                };
                let (ours, theirs) = (words(reference, ours), words(other, theirs));
                if theirs.is_empty() {
                    if !ours.is_empty() {
                        done.push(Between::Left { reference: ours, other: theirs });
                    }
                } else if ours.is_empty() {
                    // Further lines of the aligned text before them.
                    let moved = head.and_then(|(here, there)| {
                        let near = theirs
                            .iter()
                            .zip([there].iter().chain(&theirs))
                            .all(|(&site, &before)| site - before <= CONTINUATION_REACH);
                        self.moved.iter().position(|moved| moved.site == here && near)
                    });
                    match moved {
                        Some(moved) => {
                            let site = self.moved[moved].site;
                            let first = self.pairing.sites.iter().find(|&&(here, _)| here == site);
                            let mut sites = vec![first.expect("it was moved by its pair").1];
                            sites.extend(&theirs);
                            let now = self.place_bytes(&self.lines(&sites));
                            self.write(site, now);
                            self.moved[moved].now = now;
                            done.push(Between::Continued { site, texts: theirs });
                        }
                        None => done.push(Between::Left { reference: ours, other: theirs }),
                    }
                } else if theirs.len() % ours.len() == 0 && theirs.len() / ours.len() <= MOST_LINES {
                    let each = theirs.len() / ours.len();
                    for (&site, theirs) in ours.iter().zip(theirs.chunks(each)) {
                        let (from, to) = (reference.read_u32(site), other.read_u32(theirs[0]));
                        let (from, to) =
                            (from.expect("it refers to a text"), to.expect("it refers to a text"));
                        let now = if each == 1 {
                            self.place_text(to)
                        } else {
                            self.place_bytes(&self.lines(theirs))
                        };
                        self.write(site, now);
                        self.moved.push(Moved { site, from, to, now });
                        done.push(Between::Given { site, texts: theirs.to_vec() });
                    }
                } else {
                    done.push(Between::Left { reference: ours, other: theirs });
                }
            }
        }
        done
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

    /// The text a place of an image refers to.
    fn pointed(image: &Image, site: u32) -> Vec<u8> {
        text(image, image.read_u32(site).unwrap()).unwrap().to_vec()
    }

    #[test]
    fn texts_between_aligned_places_are_told_apart_by_their_order() {
        // The reference draws: a title, two entries, a key's name, a hint.
        let reference = image(
            &[b"pause menu", b"continue", b"exit level", b"???", b"jump to select"],
            &[0, 1, 2, 3, 4],
            &[],
        );
        // The other build: its title and after it a second line of it; its
        // two entries; the same key's name; its hint in two lines.
        let texts: [&[u8]; 7] = [
            b"menu di pausa",
            b"del gioco ",
            b"continua",
            b"esci dal livello",
            b"???",
            b"premi salto",
            b"per scegliere",
        ];
        let other = image(&texts, &[0, 1, 2, 3, 4, 5, 6], &[]);
        // Only the titles were aligned: the code around the rest differs.
        let mut pairing = pair(&reference, &other);
        pairing.sites = vec![(CODE + 1, CODE + 1)];
        let mut hybrid = Hybrid::new(&reference, &other, &pairing);
        hybrid.texts(&[]);
        let done = hybrid.texts_between();
        let site = |push: u32| CODE + 1 + 5 * push;
        assert_eq!(
            done,
            [
                // The key's name divides what follows the title in two.
                // Before it: two for two. (The title's second line is
                // among them, so nothing says it is one: three for two.)
                Between::Left { reference: vec![site(1), site(2)], other: vec![site(1), site(2), site(3)] },
                Between::Given { site: site(4), texts: vec![site(5), site(6)] },
            ]
        );
        let image = hybrid.finish().image;
        assert_eq!(pointed(&image, site(0)), b"menu di pausa");
        assert_eq!(pointed(&image, site(1)), b"continue");
        assert_eq!(pointed(&image, site(4)), b"premi salto\nper scegliere");

        // Without the second line of the title: two for two, in order.
        let other = image_of(&[texts[0], texts[2], texts[3], texts[4], texts[5]]);
        let mut pairing = pair(&reference, &other);
        pairing.sites = vec![(CODE + 1, CODE + 1)];
        let mut hybrid = Hybrid::new(&reference, &other, &pairing);
        hybrid.texts(&[]);
        let done = hybrid.texts_between();
        assert_eq!(done.len(), 3);
        let image = hybrid.finish().image;
        assert_eq!(pointed(&image, site(1)), b"continua");
        assert_eq!(pointed(&image, site(2)), b"esci dal livello");
        assert_eq!(pointed(&image, site(4)), b"premi salto");

        // A text of the other build alone, right after an aligned text:
        // a further line of it. Further away it is not.
        let reference = image_of(&[b"select slot", b"file.dat"]);
        let other = image_of(&[b"scegli la casella ", b"da cui caricare", b"file.dat"]);
        let mut pairing = pair(&reference, &other);
        pairing.sites = vec![(CODE + 1, CODE + 1), (CODE + 6, CODE + 11)];
        let mut hybrid = Hybrid::new(&reference, &other, &pairing);
        hybrid.texts(&[]);
        assert_eq!(hybrid.texts_between(), [Between::Continued { site: CODE + 1, texts: vec![CODE + 6] }]);
        let image = hybrid.finish().image;
        assert_eq!(pointed(&image, CODE + 1), b"scegli la casella\nda cui caricare");
    }

    /// An image that pushes each of its texts once, in order.
    fn image_of(texts: &[&[u8]]) -> Image {
        let pushed: Vec<usize> = (0..texts.len()).collect();
        image(texts, &pushed, &[])
    }
}
