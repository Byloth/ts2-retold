//! The game's code as compiled Rust, in place of the interpreter.
//!
//! `andy recompile` writes a crate with one function for each block of
//! the game's code (`andy_recomp::blocks`). A machine that is given the
//! blocks runs one whenever control is at a block's first instruction,
//! and interprets everything else: an instruction the blocks do not
//! cover, and the last instructions before the running thread's time is
//! up, so that threads change at the same instruction as they do in the
//! interpreter. Whatever is run either way, the machine's state after it
//! is the same, but for flags nothing reads before they are set again.
//!
//! A machine can also check the blocks as it goes: each block is run,
//! taken back, and run again by the interpreter, and the two outcomes
//! are compared.

use std::collections::BTreeMap;

use andy_cpu::interp::{Cpu, Fault};
use andy_cpu::memory::Memory;
use andy_cpu::sem::Flag;

use crate::{Fetched, Machine, Stop};

/// A block as a function: it runs the block's instructions on the
/// machine's state, leaves `eip` at the next instruction and counts the
/// instructions run. On a fault `eip` is at the faulting instruction.
pub type BlockFn = fn(&mut Cpu, &mut Memory) -> Result<(), Fault>;

pub struct Block {
    pub address: u32,
    /// How many instructions it is.
    pub instructions: u32,
    /// The flags that may be read after it, as bits by `Flag as u32`:
    /// the others it does not compute.
    pub live: u8,
    pub run: BlockFn,
}

/// The blocks of one image.
pub struct Blocks {
    /// The SHA-256 of the code the blocks were written from: the code
    /// section of the image as it was loaded, with the changes the
    /// modifications chosen make to it.
    pub code_sha256: &'static str,
    /// By address.
    pub blocks: &'static [Block],
}

impl Machine {
    /// Gives the machine the blocks of the image it has loaded. Whoever
    /// calls this has checked that they were written from this code.
    pub fn use_blocks(&mut self, blocks: &'static Blocks) {
        self.block_index = vec![0; self.decoded_index.len()];
        for (number, block) in blocks.blocks.iter().enumerate() {
            let offset = block.address.wrapping_sub(self.code_start) as usize;
            // Where a modification replaces the game's code the
            // replacement runs, not the block.
            let replaced = |index: &u32| *index >= crate::FIRST_REPLACEMENT && *index != crate::NOT_DECODED;
            if self.decoded_index.get(offset).is_some_and(|index| !replaced(index)) {
                self.block_index[offset] = number as u32 + 1;
            }
        }
        self.blocks = Some(blocks);
    }

    /// The block that begins at `eip`, if the machine has one.
    #[inline]
    pub(crate) fn block_at(&self, eip: u32) -> Option<&'static Block> {
        let offset = eip.wrapping_sub(self.code_start) as usize;
        match self.block_index.get(offset) {
            Some(&number) if number != 0 => Some(&self.blocks?.blocks[number as usize - 1]),
            _ => None,
        }
    }

    /// Runs a block and checks it against the interpreter: the block is
    /// run and taken back, the interpreter runs the same instructions,
    /// and the registers, the x87 unit, the flags that may be read
    /// afterwards and every byte written must be the same. The machine
    /// goes on from what the interpreter left.
    pub(crate) fn run_block_checked(&mut self, block: &'static Block) -> Result<(), Stop> {
        let before = self.cpu.clone();
        let outer_log = self.memory.undo_log.replace(Vec::new());
        let compiled = (block.run)(&mut self.cpu, &mut self.memory);
        let log = self.memory.undo_log.replace(Vec::new()).unwrap_or_default();
        let compiled_cpu = std::mem::replace(&mut self.cpu, before);
        let written = self.written(&log);
        self.memory.undo_log = None;
        self.memory.undo(&log);
        self.memory.undo_log = Some(Vec::new());
        let mut interpreted = Ok(());
        for _ in 0..block.instructions {
            let step = match self.fetch(self.cpu.eip) {
                Ok(Fetched::Instruction(instruction)) => self.cpu.execute(&mut self.memory, &instruction),
                Ok(Fetched::Replacement(_)) => {
                    return Err(self.mismatch(block, "a replacement stands inside it".into()));
                }
                Err(fault) => Err(fault),
            };
            if step.is_err() {
                interpreted = step;
                break;
            }
        }
        let log = std::mem::replace(&mut self.memory.undo_log, outer_log).unwrap_or_default();
        let rewritten = self.written(&log);
        let (ours, theirs) = (&compiled_cpu, &self.cpu);
        let mut differences = Vec::new();
        if compiled != interpreted {
            differences.push(format!("it ended {compiled:?}, the interpreter {interpreted:?}"));
        }
        if (ours.eip, ours.instructions) != (theirs.eip, theirs.instructions) {
            differences.push(format!(
                "it left eip {:#x} and {} instructions counted, the interpreter {:#x} and {}",
                ours.eip, ours.instructions, theirs.eip, theirs.instructions
            ));
        }
        // After a fault the instruction's effects are partial on both
        // sides, and need not be the same parts.
        if interpreted.is_ok() {
            for (number, (ours, theirs)) in ours.regs.iter().zip(&theirs.regs).enumerate() {
                if ours != theirs {
                    differences
                        .push(format!("register {number} is {ours:#x}, the interpreter's {theirs:#x}"));
                }
            }
            for flag in Flag::ALL.into_iter().filter(|&flag| block.live & (1 << flag as u8) != 0) {
                let (ours, theirs) = (ours.flags[flag as usize], theirs.flags[flag as usize]);
                if ours != theirs {
                    differences.push(format!("the {flag:?} flag is {ours}, the interpreter's {theirs}"));
                }
            }
            // Bit for bit: a number that is not one differs from itself.
            let bits = |fpu: &andy_cpu::fpu::Fpu| {
                (fpu.regs.map(f64::to_bits), fpu.top, fpu.valid, fpu.control, fpu.c, fpu.inexact_model)
            };
            if bits(&ours.fpu) != bits(&theirs.fpu) {
                differences
                    .push(format!("the x87 unit is {:?}, the interpreter's {:?}", ours.fpu, theirs.fpu));
            }
            if written != rewritten {
                let ours: Vec<String> =
                    written.iter().map(|(at, byte)| format!("{at:#x}={byte:#04x}")).collect();
                let theirs: Vec<String> =
                    rewritten.iter().map(|(at, byte)| format!("{at:#x}={byte:#04x}")).collect();
                differences.push(format!(
                    "it wrote {}, the interpreter {}",
                    ours.join(" "),
                    theirs.join(" ")
                ));
            }
        }
        if !differences.is_empty() {
            return Err(self.mismatch(block, differences.join("; ")));
        }
        interpreted.map_err(Stop::Fault)
    }

    /// The bytes an undo log's writes left, by address.
    fn written(&mut self, log: &[(u32, u8)]) -> BTreeMap<u32, u8> {
        log.iter().filter_map(|&(address, _)| Some((address, self.memory.read_u8(address)?))).collect()
    }

    fn mismatch(&self, block: &Block, what: String) -> Stop {
        Stop::BlockMismatch { address: block.address, what }
    }
}
