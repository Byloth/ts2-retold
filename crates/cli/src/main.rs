//! Command-line front end of the recompiler.

mod capture;
mod frame;
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
                 [--sound FILE]
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
                                    --sound writes what the game plays as
                                    a WAVE file;
                                    --limit stops after N instructions,
                                    --seconds after N seconds of the
                                    machine's clock (which also passes
                                    while the game waits)
                                    --with/--without ID switch a modification
    andy play    [EXE] [--args TEXT] [--seconds N] [--with/--without ID]
                                    Run the game in a window, with the
                                    keyboard and the sound device;
                                    --seconds closes the window after N
                                    seconds of the machine's clock
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

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut json = None;
    let mut out = None;
    let mut rounds = 3u32;
    let mut trace = false;
    let mut mods = Modifications::default();
    let mut stop_at = None;
    let mut dumps = Vec::new();
    let mut surfaces = None;
    let mut sound = None;
    let mut gpu = false;
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

    if mods.is_enabled(andy_runtime::mods::LANGUAGE_ITALIAN) {
        image = italian(&bytes, image)?;
    }

    match command.as_str() {
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
        "boot" => boot(
            &image,
            BootOptions {
                mods,
                game_dir: exe.parent().map(Path::to_path_buf),
                trace,
                limit,
                seconds,
                game_args,
                stop_at,
                dumps,
                surfaces,
                sound,
                gpu,
                input,
                frames: frames.map(|directory| (directory, frame_step)),
            },
        ),
        "play" => {
            // The program reads the game directory and writes only to `out/run`.
            let mut files = Files::new(exe.parent().map(Path::to_path_buf), Some(PathBuf::from("out/run")));
            if let Some(dir) = italian_dir(&mods) {
                files.set_language_dir(dir);
            }
            let mut os = Os::new(files);
            os.command_line_arguments = game_args;
            os.mods = mods;
            let (machine, stop) = play::play(Machine::load(&image, os), seconds)?;
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

struct BootOptions {
    mods: Modifications,
    game_dir: Option<PathBuf>,
    trace: bool,
    limit: u64,
    /// How far the machine's clock may go.
    seconds: Option<u64>,
    game_args: String,
    stop_at: Option<u32>,
    dumps: Vec<(u32, u32)>,
    surfaces: Option<PathBuf>,
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
        mods,
        game_dir,
        trace,
        limit,
        seconds,
        game_args,
        stop_at,
        dumps,
        surfaces,
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
    if let Some(dir) = italian_dir(&mods) {
        files.set_language_dir(dir);
    }
    let mut os = Os::new(files);
    os.command_line_arguments = game_args;
    os.mods = mods;
    let mut machine = Machine::load(image, os);
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
    let started = std::time::Instant::now();
    // The clock passes with the instructions run and while every thread
    // waits: the first is bounded by the budget, the second by the limit.
    let clock_limit = seconds.map(|seconds| seconds * 1000);
    machine.clock_limit = clock_limit;
    let budget = clock_limit.map_or(limit, |clock| {
        limit.min(clock.saturating_sub(machine.clock()) * andy_runtime::INSTRUCTIONS_PER_MS)
    });
    let stop = match machine.run(budget) {
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
    let missing = machine.os.files.log.iter().filter(|(_, found)| !found).count();
    println!("files opened           {} ({missing} not found)", machine.os.files.log.len());
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
/// The Italian game's directory, if the game is to be in Italian.
fn italian_dir(mods: &Modifications) -> Option<PathBuf> {
    let dir =
        env::var_os("TS2_ITALIAN_DIR").filter(|_| mods.is_enabled(andy_runtime::mods::LANGUAGE_ITALIAN))?;
    Some(PathBuf::from(dir))
}

/// The reference image with the Italian language, taken from the user's
/// Italian disc (ADR 0016).
fn italian(reference_bytes: &[u8], reference: Image) -> Result<Image, String> {
    if builds::identify(reference_bytes).map(|build| build.id) != Some(builds::REFERENCE) {
        return Err("the Italian language can only be brought to the reference build".into());
    }
    let dir =
        env::var_os("TS2_ITALIAN_DIR").ok_or("language-italian needs TS2_ITALIAN_DIR, the Italian game")?;
    let path = Path::new(&dir).join("toy2.exe");
    let bytes = fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if builds::identify(&bytes).map(|build| build.id) != Some(builds::ITALIAN) {
        return Err(format!("{} is not the Italian build this was made for", path.display()));
    }
    let other = Image::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    let pairing = andy_recomp::pairing::pair(&reference, &other);
    let mut hybrid = andy_recomp::language::Hybrid::new(&reference, &other, &pairing);
    hybrid.texts(&[]);
    // The Italian functions, each as long as to the next function there.
    let analysis = analysis::analyze(&other).ok_or("the Italian executable has no code")?;
    let function_starts = |analysis: &analysis::Analysis| {
        let mut starts: Vec<u32> = analysis.functions.iter().map(|function| function.start).collect();
        starts.sort_unstable();
        starts.dedup();
        starts
    };
    let starts = function_starts(&analysis);
    let reference_analysis = analysis::analyze(&reference).ok_or("the executable has no code")?;
    let reference_starts = function_starts(&reference_analysis);
    hybrid.functions =
        andy_recomp::pairing::common_functions(&reference, &other, (&reference_starts, &starts))
            .into_iter()
            .map(|(reference, other, length)| (other, reference, length))
            .collect();
    hybrid.functions.sort_unstable();
    let grafts: Vec<_> = andy_recomp::language::ITALIAN_FUNCTIONS
        .iter()
        .map(|&(reference, other)| {
            let end = starts[starts.partition_point(|&start| start <= other)..].first().copied();
            let reference = (reference != 0).then_some(reference);
            end.map(|end| andy_recomp::language::Graft { reference, other: (other, end) })
                .ok_or(format!("no end found for the Italian function at {other:#x}"))
        })
        .collect::<Result<_, _>>()?;
    let counterparts = andy_recomp::language::ITALIAN_COUNTERPARTS;
    hybrid.graft(&grafts, counterparts, &analysis.instruction_starts).map_err(|problems| {
        let problems: Vec<String> = problems.iter().map(|problem| format!("{problem:x?}")).collect();
        format!("the Italian functions cannot be taken over: {}", problems.join(", "))
    })?;
    let (inferred, added) = (hybrid.inferred.clone(), hybrid.added.clone());
    let transplant = hybrid.finish();
    println!(
        "italian   {} places refer to Italian texts, {} functions are the Italian ones",
        transplant.moved.len(),
        grafts.len()
    );
    for (other, reference) in inferred {
        println!("          {other:#010x} there taken as {reference:#010x} here, by its neighbours");
    }
    for (other, now, size) in added {
        println!(
            "          {other:#010x} there, {size} bytes only the Italian build has, now at {now:#010x}"
        );
    }
    Ok(transplant.image)
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
