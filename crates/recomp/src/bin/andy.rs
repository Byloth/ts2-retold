//! Command-line front end of the recompiler.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use andy_recomp::analysis::{self, Analysis, EntryKind};
use andy_recomp::builds;
use andy_recomp::pe::Image;

const USAGE: &str = "\
andy - recompiler tools

USAGE:
    andy info    [EXE]              Print the layout of the executable
    andy analyze [EXE] [--json OUT] Discover code and functions

EXE defaults to $TS2_GAME_DIR/toy2.exe. The executable is only ever read.
Output written with --json is derived from the game: keep it out of git.";

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
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--json" => json = Some(PathBuf::from(args.next().ok_or("--json needs a path")?)),
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
            let dir =
                env::var_os("TS2_GAME_DIR").ok_or("no EXE given and TS2_GAME_DIR is not set")?;
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
            let analysis =
                analysis::analyze(&image).ok_or("no executable section at the entry point")?;
            report(&analysis);
            if let Some(path) = json {
                let text = serde_json::to_string_pretty(&analysis).map_err(|e| e.to_string())?;
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                }
                fs::write(&path, text)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                println!("\nfull analysis written to {}", path.display());
            }
        }
        other => return Err(format!("unknown command {other}\n\n{USAGE}")),
    }
    Ok(())
}

fn info(image: &Image) {
    println!("base      {:#010x}", image.base);
    println!("entry     {:#010x}", image.entry);
    println!(
        "linker    {}.{}",
        image.linker_version.0, image.linker_version.1
    );
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
    println!(
        "\ncode section {:#010x}, {} bytes",
        a.code_start, a.code_len
    );
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
    println!(
        "unresolved indirect jmp {}",
        a.unresolved_indirect_jumps.len()
    );
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
