//! Cutting the game's code into blocks, working out which flags each must
//! leave behind, and writing them out as a Rust crate.
//!
//! A block is a run of instructions that is only ever entered at its
//! first and executed to its end: it begins where control can arrive
//! from elsewhere and ends with a transfer of control, or before the next
//! such beginning.
//!
//! **Where control can arrive** (the leaders): the targets of the code's
//! own jumps and calls; the instruction after every transfer; every
//! address of the code stored anywhere as a number (the image lists them:
//! they are relocated), which covers the tables of jumps, the pointers to
//! functions and the callbacks; and what the caller adds: the program's
//! entry and the places the runtime's own replacements start and resume
//! at.
//!
//! **Flags.** A block computes a flag only if something may read it
//! before it is set again. What may be read after a block is the union
//! of what the blocks it can go on at read first; where it goes on at an
//! address taken from a register or from the stack (a return, a jump
//! through a table, a call through a pointer), that is one of the
//! addresses control can arrive at without the code naming it: the
//! instructions after calls, the stored addresses, the caller's. An
//! address the machine goes on at with no block for it is interpreted,
//! and is taken to read every flag.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use andy_cpu::sem;
use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, OpKind};

use crate::emit::{Block, EVERY_FLAG, Emit};
use crate::pe::Image;

/// The most instructions a block is made of: a longer run is cut, so that
/// what is checked or timed block by block stays small.
const LONGEST: u32 = 128;

/// The blocks of an image, with what may be read after each.
pub struct Blocks {
    pub blocks: Vec<Block>,
    /// For each block, the flags that may be read after it.
    pub live: Vec<u8>,
    /// Leaders no block could be written for: their first instruction is
    /// one the semantics do not cover, or does not decode.
    pub unwritten: Vec<u32>,
}

fn decode(image: &Image, address: u32) -> Option<Instruction> {
    let section = image.section_at(address).filter(|section| section.is_executable())?;
    let bytes = section.data.get((address - section.va) as usize..)?;
    let bytes = &bytes[..bytes.len().min(15)];
    let instruction = Decoder::with_ip(32, bytes, address as u64, DecoderOptions::NONE).decode();
    (!instruction.is_invalid()).then_some(instruction)
}

/// The address a jump or call names, if it names one.
fn named_target(instruction: &Instruction) -> Option<u32> {
    matches!(instruction.op0_kind(), OpKind::NearBranch16 | OpKind::NearBranch32)
        .then(|| instruction.near_branch_target() as u32)
}

/// Where control can arrive from elsewhere, and of those the addresses it
/// can arrive at without the code naming them.
pub fn leaders(image: &Image, instruction_starts: &[u32], extra: &[u32]) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let starts: BTreeSet<u32> = instruction_starts.iter().copied().collect();
    let mut leaders: BTreeSet<u32> = extra.iter().copied().collect();
    let mut unnamed: BTreeSet<u32> = extra.iter().copied().collect();
    leaders.insert(image.entry);
    unnamed.insert(image.entry);
    // Every address of the code stored as a number.
    for &site in &image.relocs {
        if let Some(target) = image.read_u32(site).filter(|target| starts.contains(target)) {
            leaders.insert(target);
            unnamed.insert(target);
        }
    }
    for &start in instruction_starts {
        let Some(instruction) = decode(image, start) else { continue };
        let next = instruction.next_ip() as u32;
        match instruction.flow_control() {
            FlowControl::Next => {}
            FlowControl::Call | FlowControl::IndirectCall => {
                leaders.extend(named_target(&instruction));
                leaders.insert(next);
                // The callee returns here.
                unnamed.insert(next);
            }
            _ => {
                leaders.extend(named_target(&instruction));
                leaders.insert(next);
            }
        }
    }
    // What follows a transfer is only a leader if it is an instruction
    // (after the last return of a function comes padding, or data).
    let is_code =
        |address: &u32| starts.contains(address) || extra.contains(address) || *address == image.entry;
    leaders.retain(is_code);
    unnamed.retain(is_code);
    (leaders, unnamed)
}

/// Cuts the code into blocks and writes each down.
pub fn build(image: &Image, instruction_starts: &[u32], extra: &[u32]) -> Blocks {
    let (leaders, unnamed) = leaders(image, instruction_starts, extra);
    let starts: BTreeSet<u32> = instruction_starts.iter().copied().collect();
    let mut blocks = Vec::new();
    let mut unwritten = Vec::new();
    // A run cut for its length goes on at a block of its own.
    let mut pending: Vec<u32> = leaders.iter().rev().copied().collect();
    let mut seen: BTreeSet<u32> = leaders.clone();
    while let Some(address) = pending.pop() {
        let mut emit = Emit::new();
        let (mut count, mut at) = (0u32, address);
        // The block ends before an instruction that cannot be written: it
        // is left to the interpreter.
        while let Some(instruction) = decode(image, at).filter(|_| starts.contains(&at) || at == address) {
            let before = emit.mark();
            emit.begin(count, at);
            sem::step(&mut emit, &instruction);
            if emit.trapped().is_some() {
                emit = before;
                break;
            }
            (count, at) = (count + 1, instruction.next_ip() as u32);
            if emit.ended() || leaders.contains(&at) {
                break;
            }
            if count == LONGEST {
                // Cut for its length: it goes on at a block of its own.
                if seen.insert(at) {
                    pending.push(at);
                }
                break;
            }
        }
        if count == 0 {
            unwritten.push(address);
            continue;
        }
        let mut block = emit.finish(address, count, at);
        block.falls_to(at);
        blocks.push(block);
    }
    blocks.sort_by_key(|block| block.address);
    unwritten.sort_unstable();
    let live = live_after(&blocks, &unnamed);
    Blocks { blocks, live, unwritten }
}

/// For each block, the flags that may be read after it.
fn live_after(blocks: &[Block], unnamed: &BTreeSet<u32>) -> Vec<u8> {
    let index: BTreeMap<u32, usize> =
        blocks.iter().enumerate().map(|(index, block)| (block.address, index)).collect();
    let successors: Vec<(Vec<Option<usize>>, bool)> = blocks
        .iter()
        .map(|block| {
            let (named, anywhere) = block.successors();
            (named.iter().map(|address| index.get(address).copied()).collect(), anywhere)
        })
        .collect();
    // An address control can arrive at unnamed that has no block is
    // interpreted: every flag may be read there.
    let unnamed_blocks: Vec<usize> =
        unnamed.iter().filter_map(|address| index.get(address).copied()).collect();
    let unnamed_floor = if unnamed_blocks.len() == unnamed.len() { 0 } else { EVERY_FLAG };
    let mut live_in = vec![0u8; blocks.len()];
    let mut live_out = vec![0u8; blocks.len()];
    loop {
        let anywhere = unnamed_blocks.iter().fold(unnamed_floor, |live, &block| live | live_in[block]);
        let mut changed = false;
        for (number, block) in blocks.iter().enumerate().rev() {
            let (named, goes_anywhere) = &successors[number];
            let mut out = if *goes_anywhere { anywhere } else { 0 };
            for successor in named {
                out |= successor.map_or(EVERY_FLAG, |successor| live_in[successor]);
            }
            let into = block.reads | (out & !block.sets);
            if out != live_out[number] || into != live_in[number] {
                (live_out[number], live_in[number]) = (out, into);
                changed = true;
            }
        }
        if !changed {
            return live_out;
        }
    }
}

/// How many blocks a file of the written crate holds.
const BLOCKS_A_FILE: usize = 1500;

impl Blocks {
    /// The crate, as its files' names and texts: a manifest, a table of
    /// the blocks, and the blocks themselves in modules. `repository` is
    /// where this project's crates are (the crate depends on them by
    /// path; it is written as it is between single quotes, so that a
    /// Windows path's backslashes stay themselves), `code_sha256` says which code the blocks were written from.
    pub fn crate_files(&self, repository: &str, code_sha256: &str) -> Vec<(String, String)> {
        let mut files = Vec::new();
        files.push((
            "Cargo.toml".to_string(),
            format!(
                "# Written by `andy recompile` from the user's own copy of the game.\n\
                 # A translation of the game's code: never to be published (ADR 0001).\n\
                 [package]\nname = \"andy-game\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
                 [[bin]]\nname = \"andy-game\"\npath = \"src/main.rs\"\n\n\
                 [dependencies]\n\
                 andy = {{ path = '{repository}/crates/cli' }}\n\
                 andy-cpu = {{ path = '{repository}/crates/cpu' }}\n\
                 andy-runtime = {{ path = '{repository}/crates/runtime' }}\n\n\
                 [workspace]\n\n\
                 [profile.release]\ndebug = false\nincremental = false\nlto = \"thin\"\n\n\
                 [profile.release.package.andy-game]\nopt-level = 2\ncodegen-units = 256\n"
            ),
        ));
        let mut main = String::from(
            "//! The game's code as Rust: written by `andy recompile`.\n\
             #![allow(unused_variables, unused_parens, clippy::all)]\n\n\
             use andy_runtime::blocks::{Block, Blocks};\n\n",
        );
        let chunks: Vec<&[Block]> = self.blocks.chunks(BLOCKS_A_FILE).collect();
        for number in 0..chunks.len() {
            let _ = writeln!(main, "mod m{number:03};");
        }
        let _ = writeln!(
            main,
            "\nstatic BLOCKS: Blocks = Blocks {{\n    code_sha256: \"{code_sha256}\",\n    blocks: &["
        );
        let mut live = self.live.iter();
        for (number, chunk) in chunks.iter().enumerate() {
            let mut module = String::from(
                "use andy_cpu::fpu::{FBin, FConst, FMem, FUn};\n\
                 use andy_cpu::interp::{Cpu, Fault};\n\
                 use andy_cpu::memory::Memory;\n\
                 use andy_cpu::native;\n\
                 use andy_cpu::sem::Size;\n\n",
            );
            for block in *chunk {
                let live = *live.next().expect("a block has its flags");
                let name = format!("b{:x}", block.address);
                module.push_str(&block.rust(&name, live));
                module.push('\n');
                let _ = writeln!(
                    main,
                    "        Block {{ address: {:#x}, instructions: {}, live: {live:#x}, run: m{number:03}::{name} }},",
                    block.address, block.instructions
                );
            }
            files.push((format!("src/m{number:03}.rs"), module));
        }
        main.push_str(
            "    ],\n};\n\nfn main() -> std::process::ExitCode {\n    andy::main_with(Some(&BLOCKS))\n}\n",
        );
        files.push(("src/main.rs".to_string(), main));
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::Section;

    const CODE: u32 = 0x40_1000;

    fn image(code: Vec<u8>, relocs: Vec<u32>) -> Image {
        let section = Section {
            name: ".text".into(),
            va: CODE,
            virtual_size: code.len() as u32,
            flags: 0x2000_0020,
            data: code,
        };
        Image { base: 0x40_0000, entry: CODE, sections: vec![section], relocs, ..Image::default() }
    }

    #[test]
    fn code_is_cut_where_control_arrives_and_flags_are_kept_where_read() {
        // 0: cmp eax, 7        (3 bytes)
        // 3: jne 9             (2)
        // 5: add eax, 1        (3)
        // 8: nop               (1)
        // 9: sub ebx, 1        (3)
        // c: ret               (1)
        // d: mov eax, 0x40100c (5): the return's address stored as a number
        let code = vec![
            0x83, 0xF8, 0x07, 0x75, 0x04, 0x83, 0xC0, 0x01, 0x90, 0x83, 0xEB, 0x01, 0xC3, 0xB8, 0x0C, 0x10,
            0x40, 0x00,
        ];
        let image = image(code, vec![CODE + 0x0E]);
        let starts = [0u32, 3, 5, 8, 9, 0xC, 0xD].map(|offset| CODE + offset);
        let (leaders, unnamed) = leaders(&image, &starts, &[]);
        assert_eq!(leaders.into_iter().collect::<Vec<_>>(), [0, 5, 9, 0xC, 0xD].map(|offset| CODE + offset));
        assert_eq!(unnamed.into_iter().collect::<Vec<_>>(), [0, 0xC].map(|offset| CODE + offset));
        let built = build(&image, &starts, &[]);
        let shape: Vec<(u32, u32)> =
            built.blocks.iter().map(|block| (block.address - CODE, block.instructions)).collect();
        assert_eq!(shape, [(0, 2), (5, 2), (9, 1), (0xC, 1), (0xD, 1)]);
        assert!(built.unwritten.is_empty());
        // Nothing reads a flag after any of them: the jump reads the
        // comparison's inside its own block, and after the return control
        // arrives where no flag is read first.
        // (The last runs off the end of the code, where nothing is
        // written: every flag may be read there.)
        assert_eq!(built.live, [0, 0, 0, 0, EVERY_FLAG]);
        let files = built.crate_files("/repo", "abc");
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["Cargo.toml", "src/m000.rs", "src/main.rs"]);
        assert!(
            files[2]
                .1
                .contains("Block { address: 0x401009, instructions: 1, live: 0x0, run: m000::b401009 },")
        );
        assert!(
            files[1].1.contains("pub fn b401009(cpu: &mut Cpu, mem: &mut Memory) -> Result<(), Fault> {")
        );
    }

    #[test]
    fn a_flag_read_by_the_next_block_is_kept() {
        // 0: cmp eax, 7; 3: nop; 4: jne 4 (the jump's own address is a
        // target, so the comparison and the jump are in two blocks)
        let code = vec![0x83, 0xF8, 0x07, 0x90, 0x75, 0xFE, 0xC3];
        let image = image(code, vec![]);
        let starts = [0u32, 3, 4, 6].map(|offset| CODE + offset);
        let built = build(&image, &starts, &[]);
        let shape: Vec<(u32, u32, u8)> = built
            .blocks
            .iter()
            .zip(&built.live)
            .map(|(block, &live)| (block.address - CODE, block.instructions, live))
            .collect();
        // The zero flag (bit 3) is read after the first block and after
        // the second, which may go round.
        assert_eq!(shape, [(0, 2, 8), (4, 1, 8), (6, 1, 0)]);
        // An address the code is entered at from outside with nothing
        // written for it: every flag may be read after a return.
        let built = build(&image, &starts, &[CODE + 0x100]);
        assert_eq!(built.unwritten, [CODE + 0x100]);
        assert_eq!(built.live[2], EVERY_FLAG);
    }
}
