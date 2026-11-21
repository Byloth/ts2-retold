//! Command-line front end of the recompiler.

mod atlas;
mod capture;
mod frame;
mod gauge;
mod play;
mod speaker;
mod wav;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use andy_recomp::analysis::{self, Analysis, EntryKind};
use andy_recomp::pe::{Image, ResourceId, resource_type};
use andy_recomp::{builds, difftest};
use andy_runtime::files::Files;
use andy_runtime::mods::{CATALOGUE, Modifications};
use andy_runtime::os::Os;
use andy_runtime::{Machine, Stop};

const USAGE: &str = "\
andy - recompiler tools

USAGE:
    andy info    [EXE]              Print the layout of the executable
    andy analyze [EXE] [--json OUT] Discover code and functions
    andy boot    [EXE] [--trace] [--limit N] [--seconds N] [--args TEXT]
                 [--stop-at ADDR] [--dump ADDR:LEN]... [--surfaces DIR]
                 [--gpu] [--frames DIR [--frame-step N]] [--input FILE]
                 [--sound FILE] [--draws FILE]
                                    Run the game in the interpreter until it
                                    stops, and report how far it got. --args
                                    is the game's own command line: -log
                                    makes it write its log, shown at the end.
                                    --stop-at halts before an instruction;
                                    --dump prints guest memory at the end;
                                    --surfaces writes every DirectDraw
                                    surface as a PNG file into DIR; --gpu
                                    draws on the graphics adapter (without
                                    it triangles are counted, not drawn);
                                    --frames writes every Nth frame the
                                    game shows as a PNG file into DIR;
                                    --input plays back recorded input (one
                                    line per change: READ press|release KEY);
                                    --draws writes the triangles of the last
                                    frame shown, batch by batch, as text
                                    (without --gpu);
                                    --sound writes what the game plays as
                                    a WAVE file;
                                    --limit stops after N instructions,
                                    --seconds after N seconds of the
                                    machine's clock (which also passes
                                    while the game waits)
                                    --with/--without ID switch a modification
    andy play    [EXE] [--args TEXT] [--seconds N] [--gauge] [--with/--without ID]
                                    Run the game in a window, with the
                                    keyboard and the sound device;
                                    --seconds closes the window after N
                                    seconds of the machine's clock
    andy bench [boot's options]     Run as boot does and report, for every
                                    500 frames, how fast they were made
    andy atlas --out DIRECTORY      Put the one atlas of each typeface
                                    together from the discs at hand
                                    (TS2_LANGUAGE_DIR, TS2_OTHER_DISCS) and
                                    write them as pictures (derived from the
                                    game: keep them out of git)
    andy glyphs                     What each disc's executable draws for
                                    each character through each of its text
                                    functions
    andy textures FILE              List the pictures of one of the game's
                                    texture files (*.ngn)
    andy pair FIRST SECOND [--json OUT]
                                    Pair the addresses of two builds of the
                                    game (two language versions) through
                                    their code, and report how well (ADR 0016);
                                    --json lists the places whose text would
                                    change, with both texts (derived from the
                                    game: keep it out of git)
    andy mods                       List the modifications (docs/MODIFICATIONS.md)
    andy movie FILE [--yuv OUT [--pictures N]] [--pcm OUT]
                                    Decode a movie of the game and write its
                                    pictures as planar YUV 4:2:0, or its
                                    sound as two channels of 32-bit floats,
                                    to compare with another decoder
    andy difftest [EXE] --out FILE [--rounds N]
                                    Write differential test cases for every
                                    distinct instruction in the executable

EXE defaults to $TS2_GAME_DIR/toy2.exe. The executable is only ever read.
Output written with --json or --out is derived from the game: keep it out of git.";

/// Runs the tool on the command line's arguments. `blocks` is the game's
/// code as compiled Rust, when the program is one `andy recompile` wrote:
/// the game is then run from them, and otherwise interpreted.
pub fn main_with(blocks: Option<&'static andy_runtime::blocks::Blocks>) -> ExitCode {
    match run(env::args().skip(1).collect(), blocks) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>, compiled: Option<&'static andy_runtime::blocks::Blocks>) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut json = None;
    let mut out = None;
    let mut rounds = 3u32;
    let mut trace = false;
    let mut mods = Modifications::default();
    let mut stop_at = None;
    let mut dumps = Vec::new();
    let mut surfaces = None;
    let mut draws = None;
    let mut sound = None;
    let mut gpu = false;
    let mut gauge = false;
    let (mut check_blocks, mut interpret) = (false, false);
    let mut frames = None;
    let mut frame_step = 1u64;
    let mut input = None;
    let mut yuv = None;
    let mut pcm = None;
    let mut pictures = u64::MAX;
    let mut game_args = String::new();
    let mut limit = 2_000_000_000u64;
    let mut seconds = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--json" => json = Some(PathBuf::from(args.next().ok_or("--json needs a path")?)),
            "--out" => out = Some(PathBuf::from(args.next().ok_or("--out needs a path")?)),
            "--trace" => trace = true,
            "--with" | "--without" => {
                let id = args.next().ok_or("--with and --without need a modification identifier")?;
                mods.set(&id, arg == "--with")?;
            }
            "--stop-at" => {
                let text = args.next().ok_or("--stop-at needs an address")?;
                stop_at = Some(parse_hex(&text).ok_or("--stop-at needs a hexadecimal address")?);
            }
            "--dump" => {
                let text = args.next().ok_or("--dump needs ADDR:LEN")?;
                let range = text.split_once(':').and_then(|(a, l)| Some((parse_hex(a)?, parse_hex(l)?)));
                dumps.push(range.ok_or("--dump needs ADDR:LEN in hexadecimal")?);
            }
            "--gpu" => gpu = true,
            "--gauge" => gauge = true,
            "--check-blocks" => check_blocks = true,
            "--interpret" => interpret = true,
            "--input" => input = Some(PathBuf::from(args.next().ok_or("--input needs a file")?)),
            "--frames" => frames = Some(PathBuf::from(args.next().ok_or("--frames needs a directory")?)),
            "--yuv" => yuv = Some(PathBuf::from(args.next().ok_or("--yuv needs a path")?)),
            "--pcm" => pcm = Some(PathBuf::from(args.next().ok_or("--pcm needs a path")?)),
            "--pictures" => {
                pictures = args.next().and_then(|n| n.parse().ok()).ok_or("--pictures needs a number")?;
            }
            "--frame-step" => {
                frame_step = args.next().and_then(|n| n.parse().ok()).ok_or("--frame-step needs a number")?;
            }
            "--draws" => draws = Some(PathBuf::from(args.next().ok_or("--draws needs a file")?)),
            "--sound" => sound = Some(PathBuf::from(args.next().ok_or("--sound needs a file")?)),
            "--surfaces" => {
                surfaces = Some(PathBuf::from(args.next().ok_or("--surfaces needs a directory")?));
            }
            "--args" => game_args = args.next().ok_or("--args needs the command line text")?,
            "--limit" => {
                limit = args.next().and_then(|n| n.parse().ok()).ok_or("--limit needs a number")?;
            }
            "--seconds" => {
                seconds = Some(args.next().and_then(|n| n.parse().ok()).ok_or("--seconds needs a number")?);
            }
            "--rounds" => {
                rounds = args.next().and_then(|n| n.parse().ok()).ok_or("--rounds needs a number")?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n\n{USAGE}"));
            }
            _ => positional.push(arg),
        }
    }
    let Some(command) = positional.first() else {
        return Err(USAGE.into());
    };
    if command == "mods" {
        list_modifications();
        return Ok(());
    }
    if command == "textures" {
        let file = positional.get(1).ok_or("textures needs a file")?;
        let bytes = fs::read(file).map_err(|error| format!("{file}: {error}"))?;
        let pictures =
            andy_recomp::textures::pictures(&bytes).map_err(|error| format!("{file}: {error:?}"))?;
        for picture in &pictures {
            println!(
                "{:8} {:4} x {:<4} {}",
                picture.name,
                picture.width,
                picture.height,
                builds::sha256_hex(&picture.pixels)
            );
        }
        // Putting each picture back must give the file again; the ones
        // with a palette cannot be put back and are left out.
        let (mut again, mut written) = (bytes.clone(), 0);
        for picture in &pictures {
            if let Ok(file) = andy_recomp::textures::with_picture(&again, picture) {
                (again, written) = (file, written + 1);
            }
        }
        let verdict = if again == bytes { "the same" } else { "DIFFERENT" };
        println!("{} pictures; with {written} of them written back, the file is {verdict}", pictures.len());
        return Ok(());
    }
    if command == "pair" {
        let (first, second) = (positional.get(1), positional.get(2));
        let (Some(first), Some(second)) = (first, second) else {
            return Err("pair needs two executables".into());
        };
        return pair(Path::new(first), Path::new(second), json.as_deref());
    }
    if command == "movie" {
        let file = positional.get(1).ok_or("movie needs a file")?;
        if yuv.is_none() && pcm.is_none() {
            return Err("movie needs --yuv OUT or --pcm OUT".into());
        }
        return movie(Path::new(file), yuv.as_deref(), pcm.as_deref(), pictures);
    }
    let exe = match positional.get(1) {
        Some(path) => PathBuf::from(path),
        None => {
            let dir = env::var_os("TS2_GAME_DIR").ok_or("no EXE given and TS2_GAME_DIR is not set")?;
            Path::new(&dir).join("toy2.exe")
        }
    };
    let bytes = fs::read(&exe).map_err(|e| format!("cannot read {}: {e}", exe.display()))?;
    let mut image = Image::parse(&bytes).map_err(|e| format!("{}: {e}", exe.display()))?;

    println!("file      {} ({} bytes)", exe.display(), bytes.len());
    println!("sha256    {}", builds::sha256_hex(&bytes));
    match builds::identify(&bytes) {
        Some(build) => {
            let status = if build.confirmed_retail { "the reference copy" } else { "not the reference copy" };
            println!("build     {} - {} ({status})", build.id, build.description);
        }
        None => println!("build     UNKNOWN - addresses and patches are not valid for this file"),
    }

    if command == "glyphs" {
        return atlas::print_glyphs(&image, &other_discs()?);
    }
    let is_reference = builds::identify(&bytes).map(|build| build.id) == Some(builds::REFERENCE);
    let mut language = None;
    if mods.is_enabled(andy_runtime::mods::LANGUAGE) {
        if !is_reference {
            return Err("another language can only be brought to the reference build".into());
        }
        let (in_language, build) = in_another_language(image)?;
        (image, language) = (in_language, Some(build));
    }
    // The typefaces' one atlas, for the reference build's code.
    let one_typeface = is_reference && mods.is_enabled(andy_runtime::mods::ONE_TYPEFACE);
    let atlas = match (one_typeface, exe.parent()) {
        (true, Some(game_dir))
            if matches!(command.as_str(), "boot" | "bench" | "play" | "atlas" | "recompile") =>
        {
            let atlases = build_atlases(game_dir)?;
            for atlas in &atlases.0 {
                atlas.face.install_sheet_record(&mut image)?;
            }
            Some(std::rc::Rc::new(atlases))
        }
        _ => None,
    };
    if one_typeface && mods.is_enabled(andy_runtime::mods::MENU_LOAD_SAVE_WORDING) {
        let wording = andy_recomp::menu::load_save_wording(language);
        andy_recomp::menu::word_load_and_save(&mut image, wording)?;
    }
    let filter_textures = |files: &mut Files| {
        if let Some(atlas) = atlas.clone() {
            let filter =
                andy_runtime::files::TextureFilter(Box::new(move |file| atlas.in_texture_file(file)));
            files.texture_filter = Some(filter);
        }
    };
    if is_reference && mods.is_enabled(andy_runtime::mods::MENU_LOAD_BEFORE_OPTIONS) {
        andy_recomp::menu::load_before_options(&mut image)?;
    }
    // Another build of the game than the reference has the things the
    // runtime knows at other addresses.
    let addresses = match builds::identify(&bytes).map(|build| build.id) {
        Some(builds::ITALIAN | builds::FRENCH | builds::SPANISH) => addresses_in(&image)?,
        _ => andy_runtime::game::Addresses::REFERENCE,
    };

    // The compiled blocks belong to one code: the code section as it now
    // is, with what the language and the modifications chosen changed.
    // They are also cut where the runtime's replacements start and resume:
    // those places are part of what they belong to.
    let code = image.sections.iter().find(|section| section.is_executable());
    let mut cut_from = code.map(|section| section.data.clone()).unwrap_or_default();
    for point in andy_runtime::game::replacement_points() {
        cut_from.extend(point.to_le_bytes());
    }
    let code_sha256 = builds::sha256_hex(&cut_from);
    let blocks = match compiled.filter(|_| !interpret) {
        Some(blocks) if blocks.code_sha256 != code_sha256 => {
            return Err("this program's compiled code was written from other code than this: another \
                        build, language or set of modifications. Run `andy recompile` with the same \
                        options and build it again, or pass --interpret"
                .into());
        }
        blocks => blocks,
    };

    match command.as_str() {
        "recompile" => {
            let out = out.unwrap_or_else(|| PathBuf::from("out/recomp"));
            return recompile(&image, &code_sha256, &out);
        }
        "atlas" => {
            let (atlases, out) =
                (atlas.ok_or("no atlas without one-typeface")?, out.ok_or("atlas needs --out DIRECTORY")?);
            fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
            for atlas in &atlases.0 {
                let path = out.join(format!("{}.png", atlas.face.name));
                // Blue, green, red in the page; red, green, blue in the file.
                let rgb: Vec<u8> =
                    atlas.pixels.chunks(3).flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]).collect();
                let file = fs::File::create(&path).map_err(|error| format!("{}: {error}", path.display()))?;
                let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 256, 256);
                encoder.set_color(png::ColorType::Rgb);
                let written = encoder.write_header().and_then(|mut writer| writer.write_image_data(&rgb));
                written.map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        "info" => info(&image),
        "analyze" => {
            let analysis = analysis::analyze(&image).ok_or("no executable section at the entry point")?;
            report(&analysis);
            if let Some(path) = json {
                let text = serde_json::to_string_pretty(&analysis).map_err(|e| e.to_string())?;
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                }
                fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                println!("\nfull analysis written to {}", path.display());
            }
        }
        "boot" | "bench" => boot(
            &image,
            BootOptions {
                atlas: atlas.clone(),
                mods,
                addresses,
                game_dir: exe.parent().map(Path::to_path_buf),
                trace,
                blocks,
                check_blocks,
                bench: command == "bench",
                limit,
                seconds,
                game_args,
                stop_at,
                dumps,
                surfaces,
                draws,
                sound,
                gpu,
                input,
                frames: frames.map(|directory| (directory, frame_step)),
            },
        ),
        "play" => {
            // The program reads the game directory and writes only to `out/run`.
            let mut files = Files::new(exe.parent().map(Path::to_path_buf), Some(PathBuf::from("out/run")));
            if let Some(dir) = language_dir(&mods) {
                files.set_language_dir(dir);
            }
            filter_textures(&mut files);
            let mut os = Os::new(files);
            os.command_line_arguments = game_args;
            os.mods = mods;
            os.addresses = addresses;
            let mut machine = Machine::load(&image, os);
            if let Some(blocks) = blocks {
                machine.use_blocks(blocks);
                machine.check_blocks = check_blocks;
            }
            let (machine, stop) = play::play(machine, seconds, gauge)?;
            println!("frames shown           {}", machine.os.ddraw.frames);
            match stop {
                Some(stop) => println!("stopped at eip {:#010x}: {}", machine.cpu.eip, describe(&stop)),
                None => println!("window closed"),
            }
        }
        "difftest" => {
            let path = out.ok_or("difftest needs --out FILE")?;
            let analysis = analysis::analyze(&image).ok_or("no executable section at the entry point")?;
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            let file =
                fs::File::create(&path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            let mut writer = std::io::BufWriter::new(file);
            let summary = difftest::generate(&image, &analysis, rounds, &mut writer)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            println!("\ndistinct encodings  {}", summary.distinct_encodings);
            println!("cases written       {} to {}", summary.cases, path.display());
            println!("skipped             {} (cpuid, rdtsc)", summary.skipped);
            for (mnemonic, (count, first)) in &summary.unsupported {
                println!("UNSUPPORTED         {mnemonic} ({count} encodings, first at {first:#x})");
            }
        }
        other => return Err(format!("unknown command {other}\n\n{USAGE}")),
    }
    Ok(())
}

/// How many instructions the machine runs between two looks at the clock
/// when it is measured, and how many frames a line of the report covers.
const BENCH_SLICE: u64 = 1_000_000;
const BENCH_FRAMES: u64 = 500;

/// Runs the machine as fast as it goes and reports, for every stretch of
/// frames, how fast they were made: frames a second of the computer's
/// time, the instructions a frame took, and how much of the time went
/// into host functions (drawing, sound, files) and not into running the
/// game's code.
fn measure(machine: &mut Machine, budget: u64) -> Stop {
    machine.host_nanos = Some(0);
    let end = machine.cpu.instructions.saturating_add(budget);
    let started = std::time::Instant::now();
    // At the start of the stretch: frames, instructions, time, host time.
    let mut mark = (machine.os.ddraw.frames, machine.cpu.instructions, 0.0f64, 0u64);
    println!("\n       frames   frames/s  ms/frame  instructions/frame  ns/instruction  in host functions");
    let report = |machine: &Machine, mark: &mut (u64, u64, f64, u64)| {
        let now = (
            machine.os.ddraw.frames,
            machine.cpu.instructions,
            started.elapsed().as_secs_f64(),
            machine.host_nanos.unwrap_or(0),
        );
        let (frames, instructions) = ((now.0 - mark.0) as f64, (now.1 - mark.1) as f64);
        let (seconds, host) = (now.2 - mark.2, (now.3 - mark.3) as f64 / 1e9);
        if frames > 0.0 && seconds > 0.0 {
            println!(
                "{:>6}-{:<6} {:>9.0} {:>9.2} {:>19.0} {:>15.1} {:>17.0}%",
                mark.0,
                now.0,
                frames / seconds,
                1000.0 * seconds / frames,
                instructions / frames,
                1e9 * (seconds - host).max(0.0) / instructions.max(1.0),
                100.0 * host / seconds
            );
        }
        *mark = now;
    };
    let stop = loop {
        let left = end.saturating_sub(machine.cpu.instructions);
        if left == 0 {
            break Stop::Budget;
        }
        match machine.run(left.min(BENCH_SLICE)) {
            Stop::Budget => {}
            stop => break stop,
        }
        if machine.os.ddraw.frames - mark.0 >= BENCH_FRAMES {
            report(machine, &mut mark);
        }
    };
    report(machine, &mut mark);
    // Where the time in host functions went.
    let mut times = machine.host_times();
    times.sort_by_key(|&(_, nanos)| std::cmp::Reverse(nanos));
    let counts: std::collections::HashMap<&str, u64> = machine.call_counts().into_iter().collect();
    println!("\n   seconds        calls  ns a call  host function");
    for (name, nanos) in times.iter().take(14) {
        let calls = counts.get(name).copied().unwrap_or(0).max(1);
        println!("{:>10.3} {calls:>12} {:>10.0}  {name}", *nanos as f64 / 1e9, *nanos as f64 / calls as f64);
    }
    stop
}

/// Writes the game's code out as a Rust crate (`andy_recomp::blocks`).
fn recompile(image: &Image, code_sha256: &str, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let analysis = analysis::analyze(image).ok_or("the executable has no code")?;
    let extra = andy_runtime::game::replacement_points();
    let built = andy_recomp::blocks::build(image, &analysis.instruction_starts, &extra);
    let instructions: u64 = built.blocks.iter().map(|block| block.instructions as u64).sum();
    let live: u32 = built.live.iter().map(|live| live.count_ones()).sum();
    println!(
        "blocks    {} of {} instructions in all ({} instructions found); {} flags kept after them, {:.2} a block",
        built.blocks.len(),
        instructions,
        analysis.instruction_starts.len(),
        live,
        live as f64 / built.blocks.len().max(1) as f64
    );
    println!(
        "          {} places control can arrive at have no block and are interpreted",
        built.unwritten.len()
    );
    for address in built.unwritten.iter().take(12) {
        println!("            {address:#010x}");
    }
    // The crate depends on this project's crates where they are.
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repository =
        repository.canonicalize().map_err(|error| format!("{}: {error}", repository.display()))?;
    let files = built.crate_files(&repository.display().to_string(), code_sha256);
    let _ = fs::remove_dir_all(out.join("src"));
    let mut bytes = 0;
    for (name, text) in &files {
        let path = out.join(name);
        let parent = path.parent().ok_or("a file has a directory")?;
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
        bytes += text.len();
    }
    println!(
        "written   {} files, {:.1} MB, to {} in {:.1} s",
        files.len(),
        bytes as f64 / 1e6,
        out.display(),
        started.elapsed().as_secs_f64()
    );
    println!("build it  cargo build --release --manifest-path {}/Cargo.toml", out.display());
    Ok(())
}

struct BootOptions {
    /// The typefaces' one atlas, put into the texture files as they are read.
    atlas: Option<std::rc::Rc<atlas::Atlases>>,
    mods: Modifications,
    addresses: andy_runtime::game::Addresses,
    game_dir: Option<PathBuf>,
    trace: bool,
    /// The game's code as compiled blocks, and whether each is checked
    /// against the interpreter as it runs.
    blocks: Option<&'static andy_runtime::blocks::Blocks>,
    check_blocks: bool,
    /// Whether the run is measured: how fast frames are made, stretch by
    /// stretch.
    bench: bool,
    limit: u64,
    /// How far the machine's clock may go.
    seconds: Option<u64>,
    game_args: String,
    stop_at: Option<u32>,
    dumps: Vec<(u32, u32)>,
    surfaces: Option<PathBuf>,
    /// The file the batches of the last frame are written to.
    draws: Option<PathBuf>,
    /// The file the machine's sound is written to.
    sound: Option<PathBuf>,
    gpu: bool,
    /// A recording of input to play back.
    input: Option<PathBuf>,
    /// Where to write presented frames, and every how many.
    frames: Option<(PathBuf, u64)>,
}

fn list_modifications() {
    for entry in CATALOGUE {
        let default = if entry.enabled_by_default { "on" } else { "off" };
        let simulation = if entry.affects_simulation {
            "affects the simulation"
        } else {
            "does not affect the simulation"
        };
        println!("{} ({:?}, {default} by default, {simulation})", entry.id, entry.class);
        println!("    {}", entry.summary);
    }
}

fn parse_hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

fn boot(image: &Image, options: BootOptions) {
    let BootOptions {
        atlas,
        mods,
        addresses,
        game_dir,
        trace,
        blocks,
        check_blocks,
        bench,
        limit,
        seconds,
        game_args,
        stop_at,
        dumps,
        surfaces,
        draws: draws_file,
        sound,
        gpu,
        frames,
        input,
    } = options;
    // The program reads the game directory and writes only to `out/run`.
    let write_dir = PathBuf::from("out/run");
    let log_path = write_dir.join("toy2.log");
    let _ = fs::remove_file(&log_path);
    let mut files = Files::new(game_dir, Some(write_dir));
    if let Some(dir) = language_dir(&mods) {
        files.set_language_dir(dir);
    }
    if let Some(atlas) = atlas {
        let filter = andy_runtime::files::TextureFilter(Box::new(move |file| atlas.in_texture_file(file)));
        files.texture_filter = Some(filter);
    }
    let mut os = Os::new(files);
    os.command_line_arguments = game_args;
    os.mods = mods;
    os.addresses = addresses;
    let mut machine = Machine::load(image, os);
    if let Some(blocks) = blocks {
        machine.use_blocks(blocks);
        machine.check_blocks = check_blocks;
    }
    machine.breakpoint = stop_at;
    if let Some(path) = sound {
        match wav::WaveFile::create(&path) {
            Ok(file) => machine.audio = Some(Box::new(file)),
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                return;
            }
        }
    }
    if let Some(path) = input {
        let recording = fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| andy_runtime::input::Recording::parse(&text));
        match recording {
            Ok(recording) => machine.os.recording = Some(recording),
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                return;
            }
        }
    }
    if gpu {
        match andy_gpu::Gpu::new() {
            Ok(gpu) => {
                println!("graphics adapter       {}", gpu.adapter_name());
                machine.video = Box::new(gpu);
            }
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        }
    }
    if let Some((directory, step)) = frames {
        let inner = std::mem::replace(&mut machine.video, Box::new(andy_runtime::video::Headless::default()));
        match capture::Capture::new(inner, directory.clone(), step) {
            Ok(capture) => machine.video = Box::new(capture),
            Err(error) => {
                eprintln!("{}: {error}", directory.display());
                return;
            }
        }
    }
    if trace {
        machine.trace = Some(Vec::new());
    }
    if draws_file.is_some() {
        match machine.video.as_any_mut().downcast_mut::<andy_runtime::video::Headless>() {
            Some(headless) => headless.keep_draws = true,
            None => {
                eprintln!("--draws works without --gpu");
                return;
            }
        }
    }
    let started = std::time::Instant::now();
    // The clock passes with the instructions run and while every thread
    // waits: the first is bounded by the budget, the second by the limit.
    let clock_limit = seconds.map(|seconds| seconds * 1000);
    machine.clock_limit = clock_limit;
    let budget = clock_limit.map_or(limit, |clock| {
        limit.min(clock.saturating_sub(machine.clock()) * andy_runtime::INSTRUCTIONS_PER_MS)
    });
    let stop = if bench { measure(&mut machine, budget) } else { machine.run(budget) };
    let stop = match stop {
        Stop::Budget if budget < limit => Stop::Idle,
        stop => stop,
    };
    let elapsed = started.elapsed().as_secs_f64();

    if let Some(calls) = machine.trace.take() {
        println!();
        if machine.trace_dropped > 0 {
            println!("  ({} earlier host calls not shown)", machine.trace_dropped);
        }
        for call in &calls {
            let args: Vec<String> = call.args.iter().map(|a| format!("{a:#x}")).collect();
            let note = call.note.as_deref().map(|n| format!("   ; {n}")).unwrap_or_default();
            let indent = "  ".repeat(call.depth as usize);
            let thread = if call.thread == 0 { String::new() } else { format!("[thread {}] ", call.thread) };
            println!(
                "  {:#010x}  {indent}{thread}{}({}) = {:#x}{note}",
                call.caller,
                call.name,
                args.join(", "),
                call.result
            );
        }
    }
    let mut counts = machine.call_counts();
    counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
    let total: u64 = counts.iter().map(|(_, count)| count).sum();
    println!("\ninstructions executed  {}", machine.cpu.instructions);
    println!("speed                  {:.1} million/s", machine.cpu.instructions as f64 / elapsed / 1e6);
    if blocks.is_some() {
        println!(
            "run as compiled code   {} instructions, {:.1}% ({})",
            machine.block_instructions,
            100.0 * machine.block_instructions as f64 / machine.cpu.instructions.max(1) as f64,
            if check_blocks { "each block checked against the interpreter" } else { "unchecked" }
        );
    }
    println!("host calls             {total} ({} distinct)", counts.len());
    for (name, count) in counts.iter().take(12) {
        println!("  {count:>8}  {name}");
    }
    for line in &machine.os.debug_output {
        println!("debug output           {:?}", line.trim_end());
    }
    println!("x87 control word       {:#06x}", machine.cpu.fpu.control);
    match machine.first_inexact {
        Some(address) => println!("x87 model inexact      first at {address:#010x}"),
        None => println!("x87 model inexact      never"),
    }
    let video = &machine.os.ddraw;
    println!("frames shown           {}", video.frames);
    println!("keyboard reads         {}", machine.os.keyboard_reads);
    println!("surface transfers      {} to video, {} back", video.uploads, video.downloads);
    let mut states: Vec<_> = video.statistics.draw_states.iter().collect();
    states.sort_by_key(|(_, used)| std::cmp::Reverse(used.draws));
    let (draws, triangles) = states
        .iter()
        .fold((0, 0), |(draws, triangles), (_, used)| (draws + used.draws, triangles + used.triangles));
    println!("drawn                  {triangles} triangles in {draws} batches, {} draw states", states.len());
    if trace {
        for (state, used) in &states {
            println!("  {:>8} batches {:>8} triangles  {state:?}", used.draws, used.triangles);
        }
    }
    let mut backend = machine.video.as_ref();
    if let Some(capture) = backend.as_any().downcast_ref::<capture::Capture>() {
        match &capture.error {
            Some(error) => println!("frames written         {}, then failed: {error}", capture.written),
            None => println!("frames written         {}", capture.written),
        }
        backend = capture.inner();
    }
    if let Some(gpu) = backend.as_any().downcast_ref::<andy_gpu::Gpu>() {
        let statistics = gpu.statistics;
        println!(
            "on the graphics adapter {} triangles in {} batches, {} render passes",
            statistics.triangles, statistics.batches, statistics.passes
        );
    }
    if let Some(path) = draws_file {
        use std::fmt::Write;
        let headless = machine.video.as_any().downcast_ref::<andy_runtime::video::Headless>();
        let mut text = String::new();
        for (number, draw) in
            headless.map(|headless| &headless.draws_of_last_frame).into_iter().flatten().enumerate()
        {
            let _ = writeln!(text, "batch {number}: target {:?}, {:?}", draw.target, draw.state);
            let _ = writeln!(text, "  indices {:?}", draw.indices);
            for vertex in &draw.vertices {
                let _ = writeln!(text, "  {vertex:?}");
            }
        }
        if let Err(error) = fs::write(&path, text) {
            eprintln!("{}: {error}", path.display());
        }
    }
    let missing = machine.os.files.log.iter().filter(|(_, found)| !found).count();
    println!("files opened           {} ({missing} not found)", machine.os.files.log.len());
    if trace {
        let mut names: Vec<&str> = machine.os.files.log.iter().map(|(name, _)| name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        for name in names {
            println!("  file {name}");
        }
    }
    if let Ok(log) = fs::read_to_string(&log_path) {
        println!("\nthe game's own log (toy2.log)");
        for line in log.lines() {
            println!("  | {line}");
        }
    }
    println!("\nstopped at eip {:#010x}: {}", machine.cpu.eip, describe(&stop));
    for (address, len) in dumps {
        println!("\nmemory at {address:#010x}");
        match machine.memory.dump(address, len) {
            Some(bytes) => {
                for (row, chunk) in bytes.chunks(16).enumerate() {
                    let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                    println!("  {:08x}  {}", address + 16 * row as u32, hex.join(" "));
                }
            }
            None => println!("  not mapped"),
        }
    }
    if let Some(directory) = surfaces {
        match write_surfaces(&mut machine, &directory) {
            Ok(count) => println!("\n{count} surfaces written to {}", directory.display()),
            Err(error) => println!("\nsurfaces not written to {}: {error}", directory.display()),
        }
    }
    if matches!(stop, Stop::Fault(_) | Stop::Breakpoint) {
        let r = &machine.cpu.regs;
        println!(
            "  eax {:08x} ecx {:08x} edx {:08x} ebx {:08x} esp {:08x} ebp {:08x} esi {:08x} edi {:08x}",
            r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]
        );
    }
}

/// Writes every DirectDraw surface as a PNG file. The files are derived
/// from the game: they belong under `out/`, never in the repository.
fn write_surfaces(machine: &mut Machine, directory: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    fs::create_dir_all(directory)?;
    let images = machine.surface_images();
    for image in &images {
        let name = format!(
            "surface-{:03}-{}x{}-{:?}-caps{:08x}.png",
            image.index, image.width, image.height, image.format, image.caps
        );
        capture::write_png(&directory.join(name), image.width, image.height, &image.rgba)?;
    }
    Ok(images.len())
}

/// Decodes a movie and writes its pictures one after another as planar
/// YUV 4:2:0, the form other decoders can write too.
/// The addresses the runtime knows, in another build of the game than
/// the reference: found by pairing the reference executable (in
/// `TS2_GAME_DIR`) with it. This is how another language's executable is
/// run as it is, to compare the reference build in that language with.
fn addresses_in(other: &Image) -> Result<andy_runtime::game::Addresses, String> {
    let dir =
        env::var_os("TS2_GAME_DIR").ok_or("running another build needs the reference one in TS2_GAME_DIR")?;
    let path = Path::new(&dir).join("toy2.exe");
    let bytes = fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if builds::identify(&bytes).map(|build| build.id) != Some(builds::REFERENCE) {
        return Err(format!("{} is not the reference build", path.display()));
    }
    let reference = Image::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    let pairing = andy_recomp::pairing::pair(&reference, other);
    let starts = |image: &Image| -> Result<Vec<u32>, String> {
        let analysis = analysis::analyze(image).ok_or("an executable without code")?;
        let mut starts: Vec<u32> = analysis.functions.iter().map(|function| function.start).collect();
        starts.sort_unstable();
        starts.dedup();
        Ok(starts)
    };
    let (reference_starts, other_starts) = (starts(&reference)?, starts(other)?);
    let functions =
        andy_recomp::pairing::common_functions(&reference, other, (&reference_starts, &other_starts));
    let code = reference.code_section().ok_or("an executable without code")?;
    let translate = |address: u32| {
        if code.contains(address) {
            let function =
                functions.iter().find(|&&(start, _, length)| (start..start + length).contains(&address))?;
            Some(function.1 + (address - function.0))
        } else {
            pairing.pairs.get(&address).copied()
        }
    };
    andy_runtime::game::Addresses::translated(translate)
        .ok_or_else(|| "an address the runtime knows was not found in this build".into())
}

/// The directory of the game in the other language, if the game is to
/// be in one.
fn language_dir(mods: &Modifications) -> Option<PathBuf> {
    let dir = env::var_os("TS2_LANGUAGE_DIR").filter(|_| mods.is_enabled(andy_runtime::mods::LANGUAGE))?;
    Some(PathBuf::from(dir))
}

/// The reference image in another language, taken from the user's disc
/// of that language (ADR 0016), and which build that disc is. A language
/// is its texts (and its files, see `language_dir`): nothing else.
fn in_another_language(reference: Image) -> Result<(Image, &'static str), String> {
    let dir = env::var_os("TS2_LANGUAGE_DIR")
        .ok_or("language needs TS2_LANGUAGE_DIR, the game in that language")?;
    let (build, other) = language_build(Path::new(&dir))?;
    let pairing = andy_recomp::pairing::pair(&reference, &other);
    let mut hybrid = andy_recomp::language::Hybrid::new(&reference, &other, &pairing);
    hybrid.texts(&[]);
    let between = hybrid.texts_between();
    let transplant = hybrid.finish();
    println!("language  {build}: {} places refer to its texts", transplant.moved.len());
    // What could not be told apart stays in the reference's language: said
    // every time, so that it is not taken for done.
    let latin = |image: &Image, site: &u32| -> String {
        let text = image.read_u32(*site).and_then(|at| andy_recomp::language::text(image, at));
        format!(
            "{site:#x} {:?}",
            text.unwrap_or_default().iter().map(|&byte| byte as char).collect::<String>()
        )
    };
    for done in &between {
        use andy_recomp::language::Between;
        match done {
            Between::Left { reference: ours, other: theirs }
                if !ours.is_empty() || env::var_os("ANDY_TEXTS").is_some() =>
            {
                let ours: Vec<String> = ours.iter().map(|site| latin(&reference, site)).collect();
                let theirs: Vec<String> = theirs.iter().map(|site| latin(&other, site)).collect();
                println!("          left as they are: {} (there: {})", ours.join(", "), theirs.join(", "));
            }
            Between::Given { site, texts } | Between::Continued { site, texts }
                if env::var_os("ANDY_TEXTS").is_some() =>
            {
                let theirs: Vec<String> = texts.iter().map(|site| latin(&other, site)).collect();
                let kind = if matches!(done, Between::Given { .. }) { "given" } else { "continued with" };
                println!("          {} {kind} {}", latin(&reference, site), theirs.join(", "));
            }
            _ => {}
        }
    }
    Ok((transplant.image, build))
}

/// The executable of a disc of another language: which build it is, and
/// its image.
fn language_build(game_dir: &Path) -> Result<(&'static str, Image), String> {
    let path = game_dir.join("toy2.exe");
    let bytes = fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let build = builds::identify(&bytes)
        .map(|build| build.id)
        .filter(|id| [builds::ITALIAN, builds::FRENCH, builds::SPANISH].contains(id))
        .ok_or_else(|| format!("{} is not a build of the game whose language is known", path.display()))?;
    let image = Image::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok((build, image))
}

/// The one atlas of each typeface, from the reference disc and every
/// other disc at hand: the language's (`TS2_LANGUAGE_DIR`) and those
/// listed in `TS2_OTHER_DISCS`, game directories with `:` between them.
/// The atlases have what all of them have, whatever the language played.
fn build_atlases(reference_dir: &Path) -> Result<atlas::Atlases, String> {
    let mut dirs = vec![reference_dir.to_path_buf()];
    dirs.extend(other_discs()?.into_iter().map(|(dir, _)| dir));
    let atlases = atlas::build(&dirs)?;
    for atlas in &atlases.0 {
        println!(
            "typeface  {}: one atlas from {} discs, with {:?}",
            atlas.face.name,
            dirs.len(),
            atlas.signs
        );
    }
    Ok(atlases)
}

/// The other discs at hand, each as its game directory and its
/// executable's image.
fn other_discs() -> Result<Vec<(PathBuf, Image)>, String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for variable in ["TS2_LANGUAGE_DIR", "TS2_OTHER_DISCS"] {
        if let Some(value) = env::var_os(variable) {
            dirs.extend(env::split_paths(&value).filter(|dir| !dir.as_os_str().is_empty()));
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs.into_iter().map(|dir| language_build(&dir).map(|(_, image)| (dir, image))).collect()
}

/// Pairs the addresses of two builds of the game and reports how well.
fn pair(first: &Path, second: &Path, texts: Option<&Path>) -> Result<(), String> {
    let load = |path: &Path| -> Result<Image, String> {
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        println!("{}  {}", builds::sha256_hex(&bytes), path.display());
        Image::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))
    };
    let (first, second) = (load(first)?, load(second)?);
    let pairing = andy_recomp::pairing::pair(&first, &second);
    let (a, b, aligned) = pairing.code_sites;
    println!("relocation sites in the code   {a} and {b}, {aligned} aligned");
    let (a, b, aligned) = pairing.data_sites;
    println!("relocation sites elsewhere     {a} and {b}, {aligned} aligned");
    println!("addresses paired               {}", pairing.pairs.len());
    println!("  given more than one answer   {}", pairing.conflicts.len());
    for (from, answers) in &pairing.conflicts {
        let answers: Vec<String> =
            answers.iter().map(|(to, count)| format!("{to:#010x} by {count}")).collect();
        println!("    {from:#010x}: {}", answers.join(", "));
    }
    println!("addresses without a pair       {}", pairing.unpaired.len());
    // Texts: pairs of different strings of printable characters.
    let text = |image: &Image, at: u32| {
        let section = image.section_at(at)?;
        let tail = section.data.get((at - section.va) as usize..)?;
        let bytes = &tail[..tail.iter().position(|&byte| byte == 0)?];
        let printable = |byte: &u8| matches!(byte, 9 | 10 | 13 | 32..=126 | 0xA0..);
        (bytes.len() >= 2 && bytes.iter().all(printable) && !image.is_reloc(at)).then(|| bytes.to_vec())
    };
    let differing = pairing
        .pairs
        .iter()
        .filter(
            |&(&from, &to)| matches!((text(&first, from), text(&second, to)), (Some(a), Some(b)) if a != b),
        )
        .count();
    println!("pairs of different strings     {differing}");
    let transplant = andy_recomp::language::transplant_texts(&first, &second, &pairing, &[]);
    println!("places that would change text  {}", transplant.moved.len());
    // Texts the second build refers to from places the first has no place
    // for: a text one build has as one and the other as two is among them.
    let aligned: std::collections::BTreeMap<u32, u32> =
        pairing.sites.iter().map(|&(here, there)| (there, here)).collect();
    let extra: Vec<u32> = second
        .relocs
        .iter()
        .copied()
        .filter(|site| second.section_at(*site).is_some_and(|section| section.is_executable()))
        .filter(|site| !aligned.contains_key(site))
        .filter(|&site| {
            second.read_u32(site).is_some_and(|to| andy_recomp::language::text(&second, to).is_some())
        })
        .collect();
    let here: std::collections::BTreeSet<u32> = pairing.sites.iter().map(|&(here, _)| here).collect();
    let left: Vec<u32> = first
        .relocs
        .iter()
        .copied()
        .filter(|site| first.section_at(*site).is_some_and(|section| section.is_executable()))
        .filter(|site| !here.contains(site))
        .filter(|&site| {
            first.read_u32(site).is_some_and(|to| andy_recomp::language::text(&first, to).is_some())
        })
        .collect();
    println!("texts only the first refers to  {}", left.len());
    for site in left {
        let text = first.read_u32(site).and_then(|at| andy_recomp::language::text(&first, at));
        println!(
            "    {site:#010x} {:?}",
            text.unwrap_or_default().iter().map(|&byte| byte as char).collect::<String>()
        );
    }
    println!("texts only the second refers to {}", extra.len());
    let latin = |bytes: &[u8]| -> String { bytes.iter().map(|&byte| byte as char).collect() };
    for site in extra {
        let to = second.read_u32(site).unwrap_or(0);
        let before = aligned.range(..site).next_back();
        let before = before.map(|(&there, &here)| {
            let text = first.read_u32(here).and_then(|at| andy_recomp::language::text(&first, at));
            format!(
                "{} after {there:#x} (first: {here:#x} {:?})",
                site - there,
                latin(text.unwrap_or_default())
            )
        });
        println!(
            "    {site:#010x} {:?}  {}",
            latin(andy_recomp::language::text(&second, to).unwrap_or_default()),
            before.unwrap_or_default()
        );
    }
    if let Some(path) = texts {
        // For reading: every place, what it refers to and what it would.
        // The texts are Latin-1.
        let show = |image: &Image, at: u32| -> String {
            andy_recomp::language::text(image, at)
                .unwrap_or_default()
                .iter()
                .map(|&byte| byte as char)
                .collect()
        };
        let list: Vec<_> = transplant
            .moved
            .iter()
            .map(|moved| {
                serde_json::json!({
                    "site": format!("{:#010x}", moved.site),
                    "from": format!("{:#010x}", moved.from),
                    "to": format!("{:#010x}", moved.to),
                    "first": show(&first, moved.from),
                    "second": show(&second, moved.to),
                })
            })
            .collect();
        let text = serde_json::to_string_pretty(&list).map_err(|error| error.to_string())?;
        fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

fn movie(file: &Path, yuv: Option<&Path>, pcm: Option<&Path>, limit: u64) -> Result<(), String> {
    use std::io::Write;
    let bytes = fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let streams = andy_mpeg::demux::demux(bytes).ok_or("not an MPEG-1 system stream")?;
    let first = |stream: &andy_mpeg::demux::Stream| stream.times.first().map_or(f64::NAN, |&(_, time)| time);
    println!(
        "video stream  {} bytes, {} time stamps, the first {:.3} s",
        streams.video.bytes.len(),
        streams.video.times.len(),
        first(&streams.video)
    );
    println!(
        "audio stream  {} bytes, {} time stamps, the first {:.3} s",
        streams.audio.bytes.len(),
        streams.audio.times.len(),
        first(&streams.audio)
    );
    if let Some(out) = pcm {
        let mut decoder = andy_mpeg::audio::Decoder::new(streams.audio.bytes).ok_or("no Layer II audio")?;
        println!("sound         {} Hz, {} channel(s)", decoder.rate(), decoder.channels());
        let mut writer = std::io::BufWriter::new(fs::File::create(out).map_err(|error| error.to_string())?);
        let started = std::time::Instant::now();
        let mut frames = 0u64;
        while let Some(samples) = decoder.decode() {
            for sample in samples.as_flattened() {
                writer.write_all(&sample.to_le_bytes()).map_err(|error| error.to_string())?;
            }
            frames += 1;
        }
        let (seconds, sound) = (
            started.elapsed().as_secs_f64(),
            frames as f64 * andy_mpeg::audio::FRAME as f64 / decoder.rate() as f64,
        );
        println!("decoded       {frames} frames, {sound:.2} s of sound, in {seconds:.2} s");
    }
    let Some(out) = yuv else { return Ok(()) };
    let mut decoder =
        andy_mpeg::video::Decoder::new(streams.video.bytes).map_err(|error| format!("{error:?}"))?;
    let (width, height, rate) = (decoder.width(), decoder.height(), decoder.rate());
    println!("pictures      {width} x {height}, {}/{} per second", rate.0, rate.1);
    let mut writer = std::io::BufWriter::new(fs::File::create(out).map_err(|error| error.to_string())?);
    let started = std::time::Instant::now();
    let mut count = 0;
    while count < limit {
        let Some(frame) = decoder.decode() else { break };
        for (plane, stride, (width, height)) in [
            (&frame.y, frame.stride, (width, height)),
            (&frame.cb, frame.stride / 2, (width / 2, height / 2)),
            (&frame.cr, frame.stride / 2, (width / 2, height / 2)),
        ] {
            for row in plane.chunks(stride).take(height) {
                writer.write_all(&row[..width]).map_err(|error| error.to_string())?;
            }
        }
        count += 1;
    }
    let seconds = started.elapsed().as_secs_f64();
    println!("decoded       {count} pictures in {seconds:.2} s ({:.0} per second)", count as f64 / seconds);
    Ok(())
}

fn describe(stop: &Stop) -> String {
    match stop {
        Stop::Exit(code) => format!("the program exited with code {code}"),
        Stop::Fault(fault) => format!("fault: {fault:?}"),
        Stop::Unimplemented { dll, name } => format!("not implemented yet: {dll} {name}"),
        Stop::CallbackStack { address, expected, actual } => {
            format!("callback {address:#x} returned with esp {actual:#x}, expected {expected:#x}")
        }
        Stop::Budget => "instruction limit reached".into(),
        Stop::Idle => "the limit of the machine's clock reached".into(),
        Stop::Breakpoint => "breakpoint".into(),
        Stop::Deadlock => "every thread is waiting and nothing can wake one".into(),
        Stop::BlockMismatch { address, what } => {
            format!("the compiled block at {address:#x} differs from the interpreter: {what}")
        }
        Stop::WouldBlock => "internal error: a wait escaped the scheduler".into(),
    }
}

fn info(image: &Image) {
    println!("base      {:#010x}", image.base);
    println!("entry     {:#010x}", image.entry);
    println!("linker    {}.{}", image.linker_version.0, image.linker_version.1);
    println!("timestamp {:#010x}", image.timestamp);
    println!("relocs    {}", image.relocs.len());
    println!("\nsections");
    for s in &image.sections {
        println!(
            "  {:<8} {:#010x}  virtual {:>9}  initialised {:>8}  flags {:#010x}",
            s.name,
            s.va,
            s.virtual_size,
            s.data.len(),
            s.flags
        );
    }
    println!("\nresources");
    for r in &image.resources {
        let kind = match &r.kind {
            ResourceId::Id(id) => resource_type::name(*id).to_string(),
            ResourceId::Name(name) => name.clone(),
        };
        println!("  {kind:<13} {:<16} {:>6} bytes at {:#010x}", r.name.to_string(), r.size, r.va);
    }
    println!("\nimports");
    let mut dll = "";
    for import in &image.imports {
        if import.dll != dll {
            dll = &import.dll;
            let count = image.imports.iter().filter(|i| i.dll == dll).count();
            println!("  {dll} ({count})");
        }
    }
}

fn percent(part: u32, whole: u32) -> f64 {
    if whole == 0 { 0.0 } else { 100.0 * part as f64 / whole as f64 }
}

fn report(a: &Analysis) {
    let b = &a.bytes;
    println!("\ncode section {:#010x}, {} bytes", a.code_start, a.code_len);
    for (label, count) in [
        ("instructions", b.code),
        ("jump tables", b.jump_tables),
        ("padding", b.padding),
        ("data", b.data),
        ("UNKNOWN", b.unknown),
    ] {
        println!("  {label:<13} {count:>8} bytes  {:>6.2}%", percent(count, a.code_len));
    }

    let count = |kind| a.functions.iter().filter(|f| f.kind == kind).count();
    println!("\ninstructions            {}", a.instructions);
    println!("distinct mnemonics      {}", a.mnemonics.len());
    println!("functions               {}", a.functions.len());
    println!("  called directly       {}", count(EntryKind::Called));
    println!("  address taken only    {}", count(EntryKind::AddressTaken));
    println!("  orphans               {}", count(EntryKind::Orphan));
    println!("address-taken labels    {}", a.address_taken_labels.len());
    println!("shared instructions     {}", a.shared_instructions);
    println!("direct calls            {}", a.direct_calls);
    println!("indirect calls          {}", a.indirect_calls);
    println!("jump tables             {} ({} entries)", a.jump_tables, a.jump_table_entries);
    println!("unresolved indirect jmp {}", a.unresolved_indirect_jumps.len());
    let certain = a.conflicts.iter().filter(|c| !c.speculative).count();
    println!("conflicts               {} ({certain} on certain paths)", a.conflicts.len());
    println!("unknown gaps            {}", a.unknown_gaps.len());

    let used = a.imports.iter().filter(|i| i.call_sites + i.jump_sites + i.other_refs > 0).count();
    println!("imports referenced      {used} of {}", a.imports.len());
}
