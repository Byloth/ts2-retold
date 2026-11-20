//! Command-line front end of the recompiler.

mod capture;

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
    andy boot    [EXE] [--trace] [--limit N] [--args TEXT]
                 [--stop-at ADDR] [--dump ADDR:LEN]... [--surfaces DIR]
                 [--gpu] [--frames DIR [--frame-step N]]
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
                                    game shows as a PNG file into DIR
                                    --with/--without ID switch a modification
    andy mods                       List the modifications (docs/MODIFICATIONS.md)
    andy movie FILE --yuv OUT [--pictures N]
                                    Decode a movie of the game and write its
                                    pictures as planar YUV 4:2:0, to compare
                                    with another decoder
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
    let mut gpu = false;
    let mut frames = None;
    let mut frame_step = 1u64;
    let mut yuv = None;
    let mut pictures = u64::MAX;
    let mut game_args = String::new();
    let mut limit = 2_000_000_000u64;
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
            "--frames" => frames = Some(PathBuf::from(args.next().ok_or("--frames needs a directory")?)),
            "--yuv" => yuv = Some(PathBuf::from(args.next().ok_or("--yuv needs a path")?)),
            "--pictures" => {
                pictures = args.next().and_then(|n| n.parse().ok()).ok_or("--pictures needs a number")?;
            }
            "--frame-step" => {
                frame_step = args.next().and_then(|n| n.parse().ok()).ok_or("--frame-step needs a number")?;
            }
            "--surfaces" => {
                surfaces = Some(PathBuf::from(args.next().ok_or("--surfaces needs a directory")?));
            }
            "--args" => game_args = args.next().ok_or("--args needs the command line text")?,
            "--limit" => {
                limit = args.next().and_then(|n| n.parse().ok()).ok_or("--limit needs a number")?;
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
    if command == "movie" {
        let file = positional.get(1).ok_or("movie needs a file")?;
        return movie(Path::new(file), &yuv.ok_or("movie needs --yuv OUT")?, pictures);
    }
    let exe = match positional.get(1) {
        Some(path) => PathBuf::from(path),
        None => {
            let dir = env::var_os("TS2_GAME_DIR").ok_or("no EXE given and TS2_GAME_DIR is not set")?;
            Path::new(&dir).join("toy2.exe")
        }
    };
    let bytes = fs::read(&exe).map_err(|e| format!("cannot read {}: {e}", exe.display()))?;
    let image = Image::parse(&bytes).map_err(|e| format!("{}: {e}", exe.display()))?;

    println!("file      {} ({} bytes)", exe.display(), bytes.len());
    println!("sha256    {}", builds::sha256_hex(&bytes));
    match builds::identify(&bytes) {
        Some(build) => {
            let status = if build.confirmed_retail {
                "retail"
            } else {
                "matches the installation disc's files; not yet compared with a verified disc dump"
            };
            println!("build     {} - {} ({status})", build.id, build.description);
        }
        None => println!("build     UNKNOWN - addresses and patches are not valid for this file"),
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
                game_args,
                stop_at,
                dumps,
                surfaces,
                gpu,
                frames: frames.map(|directory| (directory, frame_step)),
            },
        ),
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
    game_args: String,
    stop_at: Option<u32>,
    dumps: Vec<(u32, u32)>,
    surfaces: Option<PathBuf>,
    gpu: bool,
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
    let BootOptions { mods, game_dir, trace, limit, game_args, stop_at, dumps, surfaces, gpu, frames } =
        options;
    // The program reads the game directory and writes only to `out/run`.
    let write_dir = PathBuf::from("out/run");
    let log_path = write_dir.join("toy2.log");
    let _ = fs::remove_file(&log_path);
    let files = Files::new(game_dir, Some(write_dir));
    let mut os = Os::new(files);
    os.command_line_arguments = game_args;
    os.mods = mods;
    let mut machine = Machine::load(image, os);
    machine.breakpoint = stop_at;
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
    let stop = machine.run(limit);
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
fn movie(file: &Path, out: &Path, limit: u64) -> Result<(), String> {
    use std::io::Write;
    let bytes = fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let streams = andy_mpeg::demux::demux(&bytes).ok_or("not an MPEG-1 system stream")?;
    println!("video stream  {} bytes, {} time stamps", streams.video.bytes.len(), streams.video.times.len());
    println!("audio stream  {} bytes, {} time stamps", streams.audio.bytes.len(), streams.audio.times.len());
    let mut decoder =
        andy_mpeg::video::Decoder::new(&streams.video.bytes).map_err(|error| format!("{error:?}"))?;
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
