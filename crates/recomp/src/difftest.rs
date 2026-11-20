//! Differential test cases for the instruction semantics.
//!
//! Every distinct instruction encoding found in the executable is run through
//! the interpreter from several random machine states. Each case, with the
//! state the interpreter ended in, is written as one JSON line. A separate
//! tool (`tools/difftest.py`) replays the cases on an independent emulator and
//! compares. The executable provides the list of things to test, so coverage
//! is exactly "what the game contains", operand forms included.

use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::LazyLock;

use andy_cpu::fpu::{self, Fpu};
use andy_cpu::interp::{Cpu, Fault};
use andy_cpu::memory::{Memory, PAGE_SIZE, Unmapped};
use andy_cpu::sem::Flag;
use iced_x86::{Decoder, DecoderOptions, Instruction, MemorySize, Mnemonic, OpKind, Register};
use serde_json::{Value, json};

use crate::analysis::Analysis;
use crate::pe::Image;

/// Memory never touched before holds this pattern, so that both emulators can
/// materialise any page on demand and see the same bytes: page `p` is the
/// base pattern rotated by `rotation(p)`.
static PATTERN: LazyLock<[u8; PAGE_SIZE]> = LazyLock::new(|| {
    let mut state = 0x1234_5678u32;
    std::array::from_fn(|_| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 24) as u8
    })
});

fn rotation(page: u32) -> usize {
    (page.wrapping_mul(2_654_435_761) >> 20) as usize
}

fn fill(page: u32, bytes: &mut [u8; PAGE_SIZE]) {
    let rotation = rotation(page);
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = PATTERN[(i + rotation) & (PAGE_SIZE - 1)];
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u32) -> u32 {
        (((self.next() >> 32) * bound as u64) >> 32) as u32
    }

    /// Register values biased towards the edges where flag logic breaks.
    fn word(&mut self) -> u32 {
        match self.below(20) {
            0 | 1 => 0,
            2 | 3 => u32::MAX,
            4 => 0x8000_0000,
            5 => 0x7FFF_FFFF,
            6..=10 => self.below(64),
            11 => self.below(64).wrapping_neg(),
            _ => self.next() as u32,
        }
    }

    fn float(&mut self) -> f64 {
        let unit = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        match self.below(12) {
            0 => 0.0,
            1 => 1.0,
            2 | 3 => self.below(17) as f64 - 8.0,
            4 => (self.below(33) as f64 - 16.0) / 4.0,
            5 | 6 => (unit - 0.5) * 2000.0,
            7 => (unit - 0.5) * 2e6,
            8 => unit * 1e-5,
            9 => (unit - 0.5) * 8.0,
            10 => unit,
            _ => (unit - 0.5) * 1e12,
        }
    }
}

/// Control words: the runtime's default (double precision), Direct3D's
/// (single precision), truncation as set by the float-to-integer helper in
/// both precisions, and the two remaining rounding modes.
const CONTROL_WORDS: [u16; 6] = [0x027F, 0x007F, 0x0E7F, 0x0C7F, 0x067F, 0x0A7F];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fpu_json(fpu: &Fpu) -> Value {
    let regs: Vec<Value> = (0..8)
        .map(|slot| {
            if fpu.valid & (1 << slot) != 0 {
                json!(format!("{:016x}", fpu.regs[slot].to_bits()))
            } else {
                Value::Null
            }
        })
        .collect();
    json!({ "top": fpu.top, "regs": regs, "control": fpu.control, "status": fpu.status() & 0x4700 })
}

/// Converts iced's flag set to EFLAGS bit positions.
fn eflags_mask(bits: u32) -> u32 {
    use iced_x86::RflagsBits;
    [
        (RflagsBits::CF, Flag::Carry),
        (RflagsBits::PF, Flag::Parity),
        (RflagsBits::AF, Flag::Adjust),
        (RflagsBits::ZF, Flag::Zero),
        (RflagsBits::SF, Flag::Sign),
        (RflagsBits::DF, Flag::Direction),
        (RflagsBits::OF, Flag::Overflow),
    ]
    .iter()
    .filter(|(iced, _)| bits & iced != 0)
    .fold(0, |mask, (_, flag)| mask | 1 << flag.bit())
}

fn max_stack_index(i: &Instruction) -> u32 {
    (0..i.op_count())
        .filter(|&n| i.op_kind(n) == OpKind::Register)
        .map(|n| i.op_register(n))
        .filter(|r| (Register::ST0..=Register::ST7).contains(r))
        .map(|r| r as u32 - Register::ST0 as u32)
        .max()
        .unwrap_or(0)
}

fn memory_operand_address(i: &Instruction, regs: &[u32; 8]) -> Option<u32> {
    (0..i.op_count()).any(|n| i.op_kind(n) == OpKind::Memory).then(|| {
        let reg = |r: Register| {
            if r == Register::None { 0 } else { regs[r.full_register32().number()] }
        };
        reg(i.memory_base())
            .wrapping_add(reg(i.memory_index()).wrapping_mul(i.memory_index_scale()))
            .wrapping_add(i.memory_displacement32())
    })
}

#[derive(Debug, Default)]
pub struct Summary {
    pub distinct_encodings: usize,
    pub cases: usize,
    /// Mnemonics the semantics reject: number of distinct encodings and the
    /// address of the first one.
    pub unsupported: Vec<(String, (usize, u32))>,
    /// Skipped because their result is not a function of the machine state.
    pub skipped: usize,
}

/// Writes `rounds` cases per distinct instruction encoding as JSON lines.
pub fn generate(
    image: &Image,
    analysis: &Analysis,
    rounds: u32,
    out: &mut impl Write,
) -> io::Result<Summary> {
    let text = image.code_section().expect("analysis implies a code section");
    let mut seen = HashSet::new();
    let mut summary = Summary::default();
    let mut unsupported = std::collections::BTreeMap::<String, (usize, u32)>::new();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut memory = Memory::new(Unmapped::Fill(fill));

    for &va in &analysis.instruction_starts {
        let bytes = &text.data[(va - text.va) as usize..];
        let instruction = Decoder::with_ip(32, bytes, va as u64, DecoderOptions::NONE).decode();
        let code = &bytes[..instruction.len()];
        if !seen.insert(code.to_vec()) {
            continue;
        }
        summary.distinct_encodings += 1;
        if matches!(instruction.mnemonic(), Mnemonic::Cpuid | Mnemonic::Rdtsc) {
            summary.skipped += 1;
            continue;
        }

        for _ in 0..rounds {
            let mut cpu = Cpu { eip: va, ..Cpu::default() };
            for reg in &mut cpu.regs {
                *reg = rng.word();
            }
            if instruction.has_rep_prefix() || instruction.has_repne_prefix() {
                cpu.regs[1] = rng.below(20);
            }
            if matches!(instruction.mnemonic(), Mnemonic::Div | Mnemonic::Idiv) && rng.below(3) != 0 {
                // Mostly dividends that fit, so the quotient does not overflow.
                cpu.regs[2] = if instruction.mnemonic() == Mnemonic::Idiv {
                    ((cpu.regs[0] as i32) >> 31) as u32
                } else {
                    0
                };
            }
            let flag_bits = rng.next() as u32;
            for flag in Flag::ALL {
                cpu.flags[flag as usize] = flag_bits >> flag.bit() & 1;
            }
            // Deep enough for every st(i) the instruction names, with room
            // left for one push unless it names st(7).
            let needed = (max_stack_index(&instruction) + 1).max(2);
            let depth = (needed + rng.below(3)).min(7).max(needed);
            cpu.fpu.control = CONTROL_WORDS[rng.below(CONTROL_WORDS.len() as u32) as usize];
            for _ in 0..depth {
                cpu.fpu.push(rng.float());
            }
            if instruction.mnemonic() == Mnemonic::Fyl2x {
                // The oracle does not model the logarithm of zero or of a
                // negative number (it leaves the stack untouched).
                let positive = cpu.fpu.st(0).abs() + 0.125;
                cpu.fpu.set_st(0, positive);
            }
            let status_bits = rng.below(16);
            cpu.fpu.c = std::array::from_fn(|n| status_bits >> n & 1 != 0);

            memory.clear();
            memory.load(va, code).expect("fill policy maps any page");
            // Floating-point memory operands get meaningful values instead of
            // the fill pattern, which would mostly decode to huge numbers.
            let mut preset = Vec::new();
            if let Some(address) = memory_operand_address(&instruction, &cpu.regs) {
                let value = rng.float();
                let bytes: Option<Vec<u8>> = match instruction.memory_size() {
                    MemorySize::Float32 => Some((value as f32).to_le_bytes().to_vec()),
                    MemorySize::Float64 => Some(value.to_le_bytes().to_vec()),
                    MemorySize::Float80 => {
                        let (mantissa, sign_exponent) = fpu::f64_to_f80(value);
                        Some([&mantissa.to_le_bytes()[..], &sign_exponent.to_le_bytes()[..]].concat())
                    }
                    _ => None,
                };
                if let Some(bytes) = bytes {
                    memory.load(address, &bytes).expect("fill policy maps any page");
                    preset.push(json!([address, hex(&bytes)]));
                }
            }
            // Re-load the code in case the preset landed on it.
            memory.load(va, code).expect("fill policy maps any page");

            let initial = json!({
                "regs": cpu.regs, "eflags": cpu.eflags(), "fpu": fpu_json(&cpu.fpu),
            });
            memory.write_log = Some(Vec::new());
            let outcome = cpu.execute(&mut memory, &instruction);
            if outcome == Err(Fault::Unsupported) {
                let name = format!("{:?}", instruction.mnemonic()).to_lowercase();
                unsupported.entry(name).or_insert((0, va)).0 += 1;
                break;
            }
            let log = memory.write_log.take().unwrap_or_default();
            let mut writes = Vec::new();
            for (address, len) in log {
                let bytes = memory.dump(address, len as u32).expect("just written");
                writes.push(json!([address, hex(&bytes)]));
            }
            let case = json!({
                "va": va,
                "code": hex(code),
                "mnemonic": format!("{:?}", instruction.mnemonic()).to_lowercase(),
                "rep": instruction.has_rep_prefix() || instruction.has_repne_prefix(),
                "undefined": eflags_mask(instruction.rflags_undefined()),
                "preset": preset,
                "initial": initial,
                "expect": {
                    "fault": outcome.is_err(),
                    "regs": cpu.regs,
                    "eip": cpu.eip,
                    "eflags": cpu.eflags(),
                    "fpu": fpu_json(&cpu.fpu),
                    "inexact": cpu.fpu.inexact_model,
                    "writes": writes,
                },
            });
            serde_json::to_writer(&mut *out, &case)?;
            out.write_all(b"\n")?;
            summary.cases += 1;
        }
    }
    summary.unsupported = unsupported.into_iter().collect();
    Ok(summary)
}
