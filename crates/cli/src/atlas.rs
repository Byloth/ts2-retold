//! Putting the one atlas of each typeface together from the discs at
//! hand (ADR 0016), when the game is started; and asking a disc's
//! executable what it draws for a character, as a tool for finding out
//! how the builds differ.

use std::path::{Path, PathBuf};

use andy_recomp::pe::Image;
use andy_recomp::typeface::{self, Face, PAGE, Sign, Source};
use andy_recomp::{analysis, builds, pairing, textures};
use andy_runtime::Machine;
use andy_runtime::files::Files;
use andy_runtime::mods::Modifications;
use andy_runtime::os::Os;

/// In the reference build: the function that draws a text in the large
/// typeface and the function that draws a sprite.
const DRAW_TEXT: u32 = 0x0049_B580;
const DRAW_SPRITE: u32 = 0x0049_3F40;

/// The texture files under a directory.
fn texture_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            texture_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("ngn")) {
            found.push(path);
        }
    }
}

/// Whether a picture could be a page of a typeface, by its size and name:
/// only these are looked at closer.
fn is_candidate(picture: &textures::Picture) -> bool {
    (picture.width, picture.height) == (PAGE as u32, PAGE as u32)
        && typeface::FACES.iter().any(|face| picture.name == format!("tex{:02}", face.texture))
}

/// The atlas of one typeface: its pixels and the signs it has.
pub struct Atlas {
    pub face: &'static Face,
    pub pixels: Vec<u8>,
    pub signs: Vec<Sign>,
}

/// The atlases of the typefaces.
pub struct Atlases(pub Vec<Atlas>);

/// Puts the atlases together from the discs at hand, each given as its
/// game directory, the reference first: every known page of a typeface
/// found in their texture files is a source of signs.
pub fn build(game_dirs: &[PathBuf]) -> Result<Atlases, String> {
    let mut sources: Vec<(&'static str, Source)> = Vec::new();
    for dir in game_dirs {
        let mut files = Vec::new();
        texture_files(dir, &mut files);
        for path in files {
            let Ok(file) = std::fs::read(&path) else { continue };
            let Ok(pictures) = textures::pictures(&file) else { continue };
            for picture in pictures.into_iter().filter(is_candidate) {
                let Some(page) = typeface::known_page(&builds::sha256_hex(&picture.pixels)) else { continue };
                if !sources.iter().any(|(sha256, _)| *sha256 == page.sha256) {
                    sources.push((page.sha256, Source { cells: page.face.cut(&picture.pixels), page }));
                }
            }
        }
    }
    let mut atlases = Vec::new();
    for face in typeface::FACES {
        let (of_face, rest): (Vec<_>, Vec<_>) =
            sources.into_iter().partition(|(_, source)| std::ptr::eq(source.page.face, face));
        sources = rest;
        let of_face: Vec<Source> = of_face.into_iter().map(|(_, source)| source).collect();
        if of_face.is_empty() {
            return Err(format!("no known page of the {} typeface on the discs at hand", face.name));
        }
        let (atlas, signs) = face.atlas(&of_face)?;
        let bleeding = face.bleeding(&atlas);
        if !bleeding.is_empty() {
            return Err(format!(
                "in the {} typeface's atlas, cells {bleeding:?} would show as a line over the cells under them",
                face.name
            ));
        }
        atlases.push(Atlas { face, pixels: face.page(&atlas), signs });
    }
    Ok(Atlases(atlases))
}

impl Atlases {
    /// A texture file with every known page of a typeface in it replaced
    /// by that typeface's atlas; `None` if it has none.
    pub fn in_texture_file(&self, file: &[u8]) -> Option<Vec<u8>> {
        let pictures = textures::pictures(file).ok()?;
        let mut changed = None;
        for picture in pictures.into_iter().filter(is_candidate) {
            let Some(page) = typeface::known_page(&builds::sha256_hex(&picture.pixels)) else { continue };
            if let Some(atlas) = self.0.iter().find(|atlas| std::ptr::eq(atlas.face, page.face)) {
                let atlas = textures::Picture { pixels: atlas.pixels.clone(), ..picture };
                changed = textures::with_picture(changed.as_deref().unwrap_or(file), &atlas).ok();
            }
        }
        changed
    }
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

/// A text function of the reference build, for asking a build what it
/// draws for a character: its address there and how it is called.
#[derive(Clone, Copy)]
pub struct TextFunction {
    pub name: &'static str,
    pub address: u32,
    /// The arguments for a text (its address) or, for the function that
    /// draws one character, for that character.
    pub arguments: fn(u32) -> Vec<u32>,
    /// Whether it is given one character instead of a text.
    pub by_character: bool,
}

/// The reference's text functions.
pub const TEXT_FUNCTIONS: [TextFunction; 4] = [
    TextFunction {
        name: "small, centred",
        address: 0x0049_B2A0,
        arguments: |text| vec![100, text, 0],
        by_character: false,
    },
    TextFunction {
        name: "small, at a place",
        address: 0x0049_B3B0,
        arguments: |text| vec![100, 100, text, 0, 0, 0x800],
        by_character: false,
    },
    TextFunction {
        name: "large, centred",
        address: DRAW_TEXT,
        arguments: |text| vec![100, text, 0x80],
        by_character: false,
    },
    TextFunction {
        name: "levels, one character",
        address: 0x0049_B630,
        arguments: |character| vec![100, 100, character, 0, 0x80, 0x80, 0x80, 0xFF],
        by_character: true,
    },
];

/// The reference's functions that draw a sprite: the general one and the
/// two the text functions use for their icons.
pub const SPRITE_FUNCTIONS: [u32; 3] = [DRAW_SPRITE, 0x0049_4820, 0x0049_46A0];

/// A sprite a text function drew: the number of the sprite function (as
/// in [`SPRITE_FUNCTIONS`]) and its first four arguments (x, y, sheet,
/// picture).
pub type Drawn = (usize, [u32; 4]);

/// What a build draws for each character through one of its text
/// functions: for every character that draws anything, the sprites, each
/// as the number of the sprite function (as in [`SPRITE_FUNCTIONS`]) and
/// its first four arguments (x, y, sheet, picture).
pub fn probe(image: &Image, function: &TextFunction, at: u32, sprites: &[u32]) -> Vec<(u8, Vec<Drawn>)> {
    let mut os = Os::new(Files::new(None, None));
    os.mods = Modifications::none();
    let mut machine = Machine::load(image, os);
    for &sprite in sprites {
        machine.record_calls_to(sprite);
    }
    let mut drawn = Vec::new();
    for character in 0x21..=0xFFu8 {
        let argument = if function.by_character {
            character as u32
        } else {
            machine.os.string(&mut machine.memory, &[character])
        };
        machine.recorded_calls.clear();
        if machine.call_guest_c(at, &(function.arguments)(argument)).is_err() {
            continue;
        }
        let calls: Vec<Drawn> = machine
            .recorded_calls
            .iter()
            .filter_map(|(address, arguments)| {
                let which = sprites.iter().position(|sprite| sprite == address)?;
                Some((which, [arguments[0], arguments[1], arguments[2], arguments[3]]))
            })
            .collect();
        if !calls.is_empty() {
            drawn.push((character, calls));
        }
    }
    drawn
}

/// Prints what each disc's build draws for each character through each of
/// its text functions.
pub fn print_glyphs(reference: &Image, others: &[(PathBuf, Image)]) -> Result<(), String> {
    let reference_starts = function_starts(reference)?;
    let mut builds: Vec<(String, &Image, Vec<u32>)> =
        vec![("reference".into(), reference, reference_starts.clone())];
    for (dir, image) in others {
        builds.push((dir.display().to_string(), image, function_starts(image)?));
    }
    for (name, image, starts) in &builds {
        let there = |function: u32| {
            if std::ptr::eq(*image, reference) {
                return Ok(function);
            }
            counterpart((reference, &reference_starts), (image, starts), function)
                .ok_or_else(|| format!("{name}: no counterpart of {function:#x}"))
        };
        let sprites = SPRITE_FUNCTIONS.iter().map(|&sprite| there(sprite)).collect::<Result<Vec<_>, _>>()?;
        for function in &TEXT_FUNCTIONS {
            let at = there(function.address)?;
            println!("# {name}: {} ({:#x} there {at:#x})", function.name, function.address);
            for (character, calls) in probe(image, function, at, &sprites) {
                let calls: Vec<String> = calls
                    .iter()
                    .map(|(which, a)| format!("{which}:{},{},{:#x},{}", a[0] as i32, a[1] as i32, a[2], a[3]))
                    .collect();
                println!("{character:#04x} {}", calls.join(" "));
            }
        }
    }
    Ok(())
}
