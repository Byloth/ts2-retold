//! Putting the one atlas of the large typeface together from the discs at
//! hand (ADR 0016), when the game is started.
//!
//! Each disc's page has the common cells and, after them, that language's
//! own signs in its own places. Which sign a cell holds is asked of the
//! disc's own executable: its function that draws text in the large
//! typeface is run, in a machine of its own, on every character that
//! could be such a sign, and what it draws is written down.

use std::path::{Path, PathBuf};

use andy_recomp::pe::Image;
use andy_recomp::typeface::{self, ACCENT_RISE, COMMON_CELLS, Cells, Sign, Source};
use andy_recomp::{analysis, pairing, textures};
use andy_runtime::Machine;
use andy_runtime::files::Files;
use andy_runtime::mods::Modifications;
use andy_runtime::os::Os;

/// In the reference build: the function that draws a text in the large
/// typeface, the function that draws a sprite, and the sheet of the
/// typeface.
const DRAW_TEXT: u32 = 0x0049_B580;
const DRAW_SPRITE: u32 = 0x0049_3F40;
const SHEET: u32 = 0x32;
/// The texture file and the picture every disc has its page in.
const PAGE_FILE: [&str; 3] = ["data", "level00", "levelt1.ngn"];
const PAGE_PICTURE: &str = "tex31";

/// A file under a directory, by the parts of its path, whatever the case.
fn find(root: &Path, parts: &[&str]) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for part in parts {
        let entry = std::fs::read_dir(&path)
            .ok()?
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        path = entry.path();
    }
    Some(path)
}

/// The page of the large typeface on a disc, as cells.
fn page_of(game_dir: &Path) -> Result<Cells, String> {
    let path =
        find(game_dir, &PAGE_FILE).ok_or_else(|| format!("{}: no {}", game_dir.display(), PAGE_FILE[2]))?;
    let file = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let pictures = textures::pictures(&file).map_err(|error| format!("{}: {error:?}", path.display()))?;
    let picture = pictures
        .iter()
        .find(|picture| picture.name == PAGE_PICTURE && (picture.width, picture.height) == (256, 256))
        .ok_or_else(|| format!("{}: no page of the large typeface", path.display()))?;
    Ok(typeface::cells(&picture.pixels))
}

fn function_starts(image: &Image) -> Result<Vec<u32>, String> {
    let analysis = analysis::analyze(image).ok_or("an executable without code")?;
    let mut starts: Vec<u32> = analysis.functions.iter().map(|function| function.start).collect();
    starts.sort_unstable();
    starts.dedup();
    Ok(starts)
}

/// Where a function of the reference build is in another build: its pair
/// if the two builds have it in common, or, among functions that differ,
/// the one standing in the same place between the same two common ones.
fn counterpart(reference: (&Image, &[u32]), other: (&Image, &[u32]), function: u32) -> Option<u32> {
    let common = pairing::common_functions(reference.0, other.0, (reference.1, other.1));
    if let Some(&(_, there, _)) = common.iter().find(|&&(here, _, _)| here == function) {
        return Some(there);
    }
    let &(before, before_there, _) = common.iter().rev().find(|&&(here, _, _)| here < function)?;
    let &(after, after_there, _) = common.iter().find(|&&(here, _, _)| here > function)?;
    let between = |starts: &[u32], from: u32, to: u32| -> Vec<u32> {
        starts.iter().copied().filter(|&start| start > from && start < to).collect()
    };
    let (here, there) = (between(reference.1, before, after), between(other.1, before_there, after_there));
    if here.len() != there.len() {
        return None;
    }
    Some(there[here.iter().position(|&start| start == function)?])
}

/// Which signs a build draws from which cells of its page: its text
/// function run on every character that is a sign or a letter with an
/// accent.
fn signs_of(image: &Image, draw_text: u32, draw_sprite: u32) -> Vec<(Sign, usize)> {
    let mut os = Os::new(Files::new(None, None));
    os.mods = Modifications::none();
    let mut machine = Machine::load(image, os);
    machine.record_calls_to(draw_sprite);
    let mut signs: Vec<(Sign, usize)> = Vec::new();
    for character in 0x21..=0xFFu8 {
        let (alone, with_letter) = (typeface::sign_of(character), typeface::accented(character));
        if alone.is_none() && with_letter.is_none() {
            continue;
        }
        let text = machine.os.string(&mut machine.memory, &[character]);
        machine.recorded_calls.clear();
        // The height, the text, the usual shade.
        if machine.call_guest_c(draw_text, &[100, text, 0x80]).is_err() {
            continue;
        }
        // What was drawn from the typeface's sheet: the cell and the height.
        let drawn: Vec<(usize, i32)> = machine
            .recorded_calls
            .iter()
            .filter(|(_, arguments)| arguments[2] == SHEET)
            .map(|(_, arguments)| (arguments[3] as usize, arguments[1] as i32))
            .collect();
        let extra = |cell: usize| (COMMON_CELLS..typeface::CELLS).contains(&cell);
        let found = match (alone, with_letter, drawn.as_slice()) {
            // A sign of its own: one cell beyond the common ones.
            (Some(sign), _, &[(cell, _)]) if extra(cell) => Some((sign, cell)),
            // A letter with an accent: the letter, and over it the accent.
            (_, Some((letter, accent)), &[(base, height), (cell, accent_height)])
                if base == (letter - b'a') as usize
                    && extra(cell)
                    && height - accent_height == ACCENT_RISE =>
            {
                Some((accent, cell))
            }
            // The build has nothing for this character.
            _ => None,
        };
        if let Some((sign, cell)) = found
            && !signs.iter().any(|&(held, _)| held == sign)
        {
            signs.push((sign, cell));
        }
    }
    signs
}

/// The atlas as the pixels of a page, with the reference's page as cells
/// (by which a disc's page is known when a texture file is read) and the
/// signs the atlas has.
pub struct Atlas {
    pub reference: Cells,
    pub pixels: Vec<u8>,
    pub signs: Vec<Sign>,
}

/// Puts the atlas together from the reference disc and the other discs at
/// hand: each as its game directory and its executable's image.
pub fn build(reference: (&Path, &Image), others: &[(PathBuf, Image)]) -> Result<Atlas, String> {
    let reference_cells = page_of(reference.0)?;
    // The reference's own: the two arrows.
    let mut sources = vec![Source {
        cells: reference_cells.clone(),
        signs: vec![(Sign::ArrowRight, COMMON_CELLS), (Sign::ArrowLeft, COMMON_CELLS + 1)],
    }];
    let reference_starts = function_starts(reference.1)?;
    for (game_dir, image) in others {
        let cells = page_of(game_dir)?;
        if cells[..COMMON_CELLS] != reference_cells[..COMMON_CELLS] {
            return Err(format!("{}: its large typeface is not the reference's", game_dir.display()));
        }
        let starts = function_starts(image)?;
        let there = |function: u32| {
            counterpart((reference.1, &reference_starts), (image, &starts), function).ok_or_else(|| {
                format!("{}: a text function was not found in its executable", game_dir.display())
            })
        };
        let signs = signs_of(image, there(DRAW_TEXT)?, there(DRAW_SPRITE)?);
        sources.push(Source { cells, signs });
    }
    let (atlas, signs) = typeface::atlas(&reference_cells, &sources);
    let bleeding = typeface::bleeding(&atlas);
    if !bleeding.is_empty() {
        return Err(format!(
            "in the atlas, cells {bleeding:?} would show as a line over the cells under them"
        ));
    }
    Ok(Atlas { reference: reference_cells, pixels: typeface::page(&atlas), signs })
}

impl Atlas {
    /// A texture file with every page of the large typeface in it replaced
    /// by the atlas; `None` if it has none.
    pub fn in_texture_file(&self, file: &[u8]) -> Option<Vec<u8>> {
        let pictures = textures::pictures(file).ok()?;
        let mut changed = None;
        for picture in pictures {
            if (picture.width, picture.height) == (256, 256)
                && typeface::is_page(&picture.pixels, &self.reference)
            {
                let atlas = textures::Picture { pixels: self.pixels.clone(), ..picture };
                changed = textures::with_picture(changed.as_deref().unwrap_or(file), &atlas).ok();
            }
        }
        changed
    }
}
