//! Command-line front end of the recompiler.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use andy_recomp::analysis::{self, Analysis, EntryKind};
use andy_recomp::pe::Image;
use andy_recomp::{builds, difftest};
use andy_runtime::files::Files;
use andy_runtime::win32::Os;
use andy_runtime::{Machine, Stop};

const USAGE: &str = "\
andy - recompiler tools

USAGE:
    andy info    [EXE]              Print the layout of the executable
    andy analyze [EXE] [--json OUT] Discover code and functions
    andy boot    [EXE] [--trace] [--limit N]
                                    Run the game in the interpreter until it
                                    stops, and report how far it got
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
            "--limit" => {
                limit = args
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or("--limit needs a number")?;
            }
            "--rounds" => {
                rounds = args
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or("--rounds needs a number")?;
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
                "provisional, not yet compared with a disc"
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
        "boot" => boot(&image, exe.parent().map(Path::to_path_buf), trace, limit),
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

fn boot(image: &Image, game_dir: Option<PathBuf>, trace: bool, limit: u64) {
    // The program reads the game directory and writes only to `out/run`.
    let files = Files::new(game_dir, Some(PathBuf::from("out/run")));
    let mut machine = Machine::load(image, Os::new(files));
    machine.trace = Some(Vec::new());
    let started = std::time::Instant::now();
    let stop = machine.run(limit);
    let elapsed = started.elapsed().as_secs_f64();

    let calls = machine.trace.take().unwrap_or_default();
    if trace {
        println!();
        for call in &calls {
            let args: Vec<String> = call.args.iter().map(|a| format!("{a:#x}")).collect();
            let detail = call
                .detail
                .as_deref()
                .map(|d| format!("   ; {d}"))
                .unwrap_or_default();
            println!(
                "  {:#010x}  {}({}) = {:#x}{detail}",
                call.caller,
                call.name,
                args.join(", "),
                call.result
            );
        }
    }
    println!("\ninstructions executed  {}", machine.cpu.instructions);
    println!(
        "speed                  {:.1} million/s",
        machine.cpu.instructions as f64 / elapsed / 1e6
    );
    println!(
        "system calls           {} ({} distinct)",
        calls.len(),
        machine.import_counts.len()
    );
    let mut counts: Vec<_> = machine.import_counts.iter().collect();
    counts.sort_by(|a, b| b.1.cmp(a.1));
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
    let missing: Vec<&str> = machine
        .os
        .files
        .log
        .iter()
        .filter(|(_, found)| !found)
        .map(|(name, _)| name.as_str())
        .collect();
    println!(
        "files opened           {} ({} not found)",
        machine.os.files.log.len(),
        missing.len()
    );
    println!("\nstopped at eip {:#010x}: {}", machine.cpu.eip, describe(&stop));
    if let Stop::Fault(_) = stop {
        let r = &machine.cpu.regs;
        println!(
            "  eax {:08x} ecx {:08x} edx {:08x} ebx {:08x} esp {:08x} ebp {:08x} esi {:08x} edi {:08x}",
            r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]
        );
    }
}

fn describe(stop: &Stop) -> String {
    match stop {
        Stop::Exit(code) => format!("the program exited with code {code}"),
        Stop::Fault(fault) => format!("fault: {fault:?}"),
        Stop::UnimplementedImport { dll, name } => format!("import not implemented yet: {dll} {name}"),
        Stop::Budget => "instruction limit reached".into(),
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
    if whole == 0 {
        0.0
    } else {
        100.0 * part as f64 / whole as f64
    }
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
        println!(
            "  {label:<13} {count:>8} bytes  {:>6.2}%",
            percent(count, a.code_len)
        );
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
    println!(
        "jump tables             {} ({} entries)",
        a.jump_tables, a.jump_table_entries
    );
    println!("unresolved indirect jmp {}", a.unresolved_indirect_jumps.len());
    let certain = a.conflicts.iter().filter(|c| !c.speculative).count();
    println!(
        "conflicts               {} ({certain} on certain paths)",
        a.conflicts.len()
    );
    println!("unknown gaps            {}", a.unknown_gaps.len());

    let used = a
        .imports
        .iter()
        .filter(|i| i.call_sites + i.jump_sites + i.other_refs > 0)
        .count();
    println!("imports referenced      {used} of {}", a.imports.len());
}
