//! Which address of one build of the game is which address of another.
//!
//! The builds of the game for different languages were compiled from
//! almost the same source: nearly all of their code is the same once the
//! addresses in it are set aside, and what moved (every variable and
//! function after the first difference) moved together. Both keep their
//! relocation table, which lists every place where an absolute address
//! is written. So:
//!
//! - the code of each build is cut at its relocation sites. The piece
//!   before a site is the code between the site before and this one,
//!   with the distances of relative calls and jumps blanked;
//! - the two sequences of pieces are aligned: where the code is the same,
//!   the pieces are;
//! - each pair of aligned sites holds the same address in both builds: a
//!   pair of addresses;
//! - relocation sites outside the code are the pointers in tables of the
//!   data. Their sequences are aligned by how far apart they stand, which
//!   pairs the addresses that only tables refer to.
//!
//! An address is usually referred to from many places, and all of them
//! must give the same answer. Where they do not, the answer most of them
//! give is taken and the address is reported ([`Pairing::conflicts`]).
//!
//! This is what a language is brought to the reference build with
//! (ADR 0016).

use std::collections::{BTreeMap, HashMap};

use crate::pe::Image;

/// Sites further apart than this are all "far apart" to the alignment of
/// the data.
const FAR: u32 = 64;

/// The addresses of one build in another.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Pairing {
    /// An address of the first build and the same thing's address in the
    /// second.
    pub pairs: BTreeMap<u32, u32>,
    /// Addresses of the first build that were given more than one answer:
    /// each answer with the number of sites that gave it, the most given
    /// first (that one is in `pairs`).
    pub conflicts: Vec<(u32, Vec<(u32, usize)>)>,
    /// Relocation sites in the code of the first build, of the second, and
    /// how many were aligned.
    pub code_sites: (usize, usize, usize),
    /// The same for the sites outside the code.
    pub data_sites: (usize, usize, usize),
    /// Addresses the first build refers to that have no pair.
    pub unpaired: Vec<u32>,
}

/// The pairs of indices at which two sequences agree, in a longest common
/// subsequence of them (Myers' algorithm: time and memory grow with the
/// number of differences, which is small between two builds).
pub fn align<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    // The furthest x reached on each diagonal, for each number of
    // differences so far.
    let mut furthest = vec![0isize; 2 * max + 2];
    let mut history: Vec<Vec<isize>> = Vec::new();
    let offset = max as isize;
    let mut found = None;
    'search: for d in 0..=max as isize {
        history.push(furthest[(offset - d) as usize..=(offset + d) as usize].to_vec());
        for k in (-d..=d).step_by(2) {
            let at = (offset + k) as usize;
            let mut x = if k == -d || (k != d && furthest[at - 1] < furthest[at + 1]) {
                furthest[at + 1]
            } else {
                furthest[at - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            furthest[at] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'search;
            }
        }
    }
    // Back along the path, collecting the stretches where the two agree.
    let mut matches = Vec::new();
    let (mut x, mut y) = (n, m);
    let mut d = found.expect("the search ends when both sequences are used up");
    while d > 0 {
        let k = x - y;
        // `history[d]` holds the state before step d: diagonals -d..=d
        // of the step before, shifted so that diagonal -d is index 0.
        let before = &history[d as usize];
        let get = |diagonal: isize| before[(diagonal + d) as usize];
        let down = k == -d || (k != d && get(k - 1) < get(k + 1));
        let previous_k = if down { k + 1 } else { k - 1 };
        let previous_x = get(previous_k);
        let previous_y = previous_x - previous_k;
        let (start_x, start_y) =
            if down { (previous_x, previous_y + 1) } else { (previous_x + 1, previous_y) };
        while x > start_x && y > start_y {
            x -= 1;
            y -= 1;
            matches.push((x as usize, y as usize));
        }
        (x, y) = (previous_x, previous_y);
        d -= 1;
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        matches.push((x as usize, y as usize));
    }
    matches.reverse();
    matches
}

/// A stretch of code with the distances of relative calls and jumps
/// blanked: they differ wherever the function called has moved. (An `E8`
/// or `E9` that is not an instruction's first byte is blanked like one;
/// that happens alike in both builds.)
fn without_distances(code: &[u8]) -> Vec<u8> {
    let mut out = code.to_vec();
    let mut at = 0;
    while at < out.len() {
        if matches!(out[at], 0xE8 | 0xE9) && at + 4 < out.len() {
            out[at + 1..at + 5].fill(0);
            at += 5;
        } else {
            at += 1;
        }
    }
    out
}

/// The relocation sites in the code of an image and, for each, the piece
/// of code before it.
fn code_pieces(image: &Image) -> (Vec<u32>, Vec<Vec<u8>>) {
    let Some(code) = image.code_section() else { return (Vec::new(), Vec::new()) };
    let end = code.va + code.data.len() as u32;
    let sites = image.relocs_in(code.va, end.saturating_sub(3)).to_vec();
    let mut pieces = Vec::with_capacity(sites.len());
    let mut previous = code.va;
    for &site in &sites {
        // Two fixups never overlap; a table of them has empty pieces.
        let from = previous.min(site);
        pieces.push(without_distances(&code.data[(from - code.va) as usize..(site - code.va) as usize]));
        previous = site + 4;
    }
    (sites, pieces)
}

/// The relocation sites outside the code and, for each, how far it is
/// from the one before.
fn data_gaps(image: &Image) -> (Vec<u32>, Vec<u32>) {
    let code = image.code_section().map_or(0..0, |code| code.va..code.va + code.data.len() as u32);
    let sites: Vec<u32> = image.relocs.iter().copied().filter(|site| !code.contains(site)).collect();
    let gaps = sites
        .iter()
        .enumerate()
        .map(|(index, &site)| if index == 0 { 4 } else { (site - sites[index - 1]).min(FAR) })
        .collect();
    (sites, gaps)
}

/// Gives each piece a number, the same for equal pieces.
fn numbered<'p>(numbers: &mut HashMap<&'p [u8], u32>, pieces: &'p [Vec<u8>]) -> Vec<u32> {
    pieces
        .iter()
        .map(|piece| {
            let next = numbers.len() as u32;
            *numbers.entry(piece.as_slice()).or_insert(next)
        })
        .collect()
}

/// Pairs the addresses of `first` with those of `second`.
pub fn pair(first: &Image, second: &Image) -> Pairing {
    // Every answer an address was given, with the number of sites.
    let mut answers: BTreeMap<u32, HashMap<u32, usize>> = BTreeMap::new();
    let mut vote = |sites: (&[u32], &[u32]), matches: &[(usize, usize)]| {
        for &(a, b) in matches {
            let (Some(from), Some(to)) = (first.read_u32(sites.0[a]), second.read_u32(sites.1[b])) else {
                continue;
            };
            *answers.entry(from).or_default().entry(to).or_default() += 1;
        }
    };

    let ((sites_a, pieces_a), (sites_b, pieces_b)) = (code_pieces(first), code_pieces(second));
    // Pieces are compared as numbers: equal pieces get the same number.
    let mut numbers: HashMap<&[u8], u32> = HashMap::new();
    let numbered_a = numbered(&mut numbers, &pieces_a);
    let numbered_b = numbered(&mut numbers, &pieces_b);
    let code_matches = align(&numbered_a, &numbered_b);
    vote((&sites_a, &sites_b), &code_matches);
    let code_sites = (sites_a.len(), sites_b.len(), code_matches.len());

    let ((sites_a, gaps_a), (sites_b, gaps_b)) = (data_gaps(first), data_gaps(second));
    let data_matches = align(&gaps_a, &gaps_b);
    vote((&sites_a, &sites_b), &data_matches);
    let data_sites = (sites_a.len(), sites_b.len(), data_matches.len());

    let mut pairing = Pairing { code_sites, data_sites, ..Pairing::default() };
    for (from, given) in answers {
        let mut given: Vec<(u32, usize)> = given.into_iter().collect();
        given.sort_by_key(|&(to, count)| (std::cmp::Reverse(count), to));
        pairing.pairs.insert(from, given[0].0);
        if given.len() > 1 {
            pairing.conflicts.push((from, given));
        }
    }
    let mut targets: Vec<u32> = first.relocs.iter().filter_map(|&site| first.read_u32(site)).collect();
    targets.sort_unstable();
    targets.dedup();
    pairing.unpaired = targets.into_iter().filter(|target| !pairing.pairs.contains_key(target)).collect();
    pairing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::Section;

    #[test]
    fn sequences_are_aligned_where_they_agree() {
        let pairs = |a: &str, b: &str| align(a.as_bytes(), b.as_bytes());
        assert_eq!(pairs("abc", "abc"), [(0, 0), (1, 1), (2, 2)]);
        assert_eq!(pairs("", "abc"), []);
        assert_eq!(pairs("abc", ""), []);
        // One left out, one put in, one changed.
        assert_eq!(pairs("abcd", "acd"), [(0, 0), (2, 1), (3, 2)]);
        assert_eq!(pairs("acd", "abcd"), [(0, 0), (1, 2), (2, 3)]);
        assert_eq!(pairs("abcd", "axcd"), [(0, 0), (2, 2), (3, 3)]);
        // As many as can be, in order.
        let matched = pairs("the quick brown fox", "a quick red fox jumps");
        assert_eq!(matched.len(), " quick r fox".len());
        assert!(matched.windows(2).all(|pair| pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1));
        assert!(
            matched
                .iter()
                .all(|&(a, b)| "the quick brown fox".as_bytes()[a] == "a quick red fox jumps".as_bytes()[b])
        );
    }

    const CODE: u32 = 0x40_1000;
    const DATA: u32 = 0x40_2000;

    /// An image with code and data; `code` is given as pieces, a number
    /// standing for a four-byte address with a relocation.
    enum Piece<'a> {
        Bytes(&'a [u8]),
        Address(u32),
    }
    use Piece::{Address, Bytes};

    fn image(code: &[Piece], data: &[Piece]) -> Image {
        let mut relocs = Vec::new();
        let mut section = |name: &str, va: u32, pieces: &[Piece]| {
            let mut bytes = Vec::new();
            for piece in pieces {
                match piece {
                    Bytes(more) => bytes.extend_from_slice(more),
                    Address(address) => {
                        relocs.push(va + bytes.len() as u32);
                        bytes.extend_from_slice(&address.to_le_bytes());
                    }
                }
            }
            Section {
                name: name.into(),
                va,
                virtual_size: bytes.len() as u32,
                flags: if name == ".text" { 0x2000_0000 } else { 0 },
                data: bytes,
            }
        };
        let sections = vec![section(".text", CODE, code), section(".data", DATA, data)];
        Image { base: 0x40_0000, entry: CODE, sections, relocs, ..Image::default() }
    }

    #[test]
    fn addresses_are_paired_through_the_code_and_the_tables() {
        // mov eax, [x]; call f; push offset text; mov ecx, [table + eax*4]; ret
        let first = image(
            &[
                Bytes(&[0xA1]),
                Address(0x50_0000),
                Bytes(&[0xE8, 0x10, 0, 0, 0, 0x68]),
                Address(DATA + 0x100),
                Bytes(&[0x8B, 0x0C, 0x85]),
                Address(DATA),
                Bytes(&[0xC3]),
            ],
            // The table: two texts, a number, a third text.
            &[Address(DATA + 0x200), Address(DATA + 0x210), Bytes(&[7, 0, 0, 0]), Address(DATA + 0x220)],
        );
        // The second build: everything further on, the function called
        // elsewhere, and an instruction more at the start.
        let second = image(
            &[
                Bytes(&[0x90, 0x68]),
                Address(0x66_0000),
                Bytes(&[0xA1]),
                Address(0x50_3000),
                Bytes(&[0xE8, 0x44, 0, 0, 0, 0x68]),
                Address(DATA + 0x180),
                Bytes(&[0x8B, 0x0C, 0x85]),
                Address(DATA + 0x40),
                Bytes(&[0xC3]),
            ],
            &[Address(DATA + 0x300), Address(DATA + 0x318), Bytes(&[7, 0, 0, 0]), Address(DATA + 0x330)],
        );
        let pairing = pair(&first, &second);
        assert_eq!(pairing.code_sites, (3, 4, 3));
        assert_eq!(pairing.data_sites, (3, 3, 3));
        assert_eq!(
            pairing.pairs.iter().map(|(&from, &to)| (from, to)).collect::<Vec<_>>(),
            [
                (DATA, DATA + 0x40),
                (DATA + 0x100, DATA + 0x180),
                (DATA + 0x200, DATA + 0x300),
                (DATA + 0x210, DATA + 0x318),
                (DATA + 0x220, DATA + 0x330),
                (0x50_0000, 0x50_3000),
            ]
        );
        assert!(pairing.conflicts.is_empty() && pairing.unpaired.is_empty());
    }

    #[test]
    fn an_address_given_two_answers_is_reported() {
        let load = |address: u32| [Bytes(&[0xA1]), Address(address)];
        let first =
            image(&[load(0x50_0000), load(0x50_0000), load(0x50_0000), load(0x50_0004)].concat_pieces(), &[]);
        let second =
            image(&[load(0x60_0000), load(0x60_0000), load(0x61_0000), load(0x60_0004)].concat_pieces(), &[]);
        let pairing = pair(&first, &second);
        assert_eq!(pairing.pairs[&0x50_0000], 0x60_0000);
        assert_eq!(pairing.conflicts, [(0x50_0000, vec![(0x60_0000, 2), (0x61_0000, 1)])]);
    }

    trait ConcatPieces<'a> {
        fn concat_pieces(self) -> Vec<Piece<'a>>;
    }
    impl<'a, const N: usize> ConcatPieces<'a> for [[Piece<'a>; 2]; N] {
        fn concat_pieces(self) -> Vec<Piece<'a>> {
            self.into_iter().flatten().collect()
        }
    }
}
