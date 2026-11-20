//! The meaning of each x86 instruction, expressed over an abstract [`Backend`].
//!
//! Everything here is straight-line data flow: values are produced by backend
//! operations and never inspected, so the same code can either compute them
//! (the interpreter) or record them (the recompiler). Data-dependent choices go
//! through [`Backend::select`], repetition through [`Backend::repeat`], and
//! control transfers through the `jump`/`call`/`ret` primitives.
//!
//! Flags are computed explicitly, one flag at a time, from operands and
//! result. That is deliberately naive: a recording backend sees each flag as
//! an independent pure expression and can drop the ones nobody reads.
//!
//! Scope: the 130 mnemonics found in the game executable (see
//! `docs/CODE-DISCOVERY.md`). Anything else reaches [`Backend::trap`].

use iced_x86::{ConditionCode, Instruction, MemorySize, Mnemonic, OpKind, Register};

use crate::fpu::{FBin, FConst, FMem, FUn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    B1 = 1,
    B2 = 2,
    B4 = 4,
}

impl Size {
    pub fn bytes(self) -> u32 {
        self as u32
    }

    pub fn bits(self) -> u32 {
        self as u32 * 8
    }

    pub fn mask(self) -> u32 {
        u32::MAX >> (32 - self.bits())
    }

    fn from_bytes(bytes: usize) -> Option<Size> {
        match bytes {
            1 => Some(Size::B1),
            2 => Some(Size::B2),
            4 => Some(Size::B4),
            _ => None,
        }
    }
}

/// Status and control flags, each held as a separate 0/1 value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Carry,
    Parity,
    Adjust,
    Zero,
    Sign,
    Direction,
    Overflow,
}

impl Flag {
    pub const ALL: [Flag; 7] =
        [Flag::Carry, Flag::Parity, Flag::Adjust, Flag::Zero, Flag::Sign, Flag::Direction, Flag::Overflow];

    /// Bit position in EFLAGS.
    pub fn bit(self) -> u32 {
        match self {
            Flag::Carry => 0,
            Flag::Parity => 2,
            Flag::Adjust => 4,
            Flag::Zero => 6,
            Flag::Sign => 7,
            Flag::Direction => 10,
            Flag::Overflow => 11,
        }
    }
}

/// Binary operations on 32-bit values. Shift and rotate counts are taken
/// modulo 32; comparisons yield 0 or 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bin {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Sar,
    Mul,
    /// High half of the unsigned 64-bit product.
    MulHighU,
    /// High half of the signed 64-bit product.
    MulHighS,
    Eq,
    Ne,
    LtU,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Un {
    Not,
    Neg,
    SignExtend8,
    SignExtend16,
    /// 1 if the low byte has an even number of set bits.
    ParityEven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rep {
    /// Repeat while `ecx != 0` (`rep`).
    Count,
    /// Repeat while `ecx != 0` and the zero flag is set (`repe`).
    WhileEqual,
    /// Repeat while `ecx != 0` and the zero flag is clear (`repne`).
    WhileNotEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    /// `int3`
    Breakpoint,
    /// An instruction or operand form outside the supported subset.
    Unsupported,
}

pub trait Backend: Sized {
    /// A 32-bit integer value.
    type V: Copy;
    /// A floating-point value.
    type F: Copy;

    fn imm(&mut self, value: u32) -> Self::V;
    /// General-purpose register by number: eax ecx edx ebx esp ebp esi edi.
    fn reg(&mut self, number: usize) -> Self::V;
    fn set_reg(&mut self, number: usize, value: Self::V);
    fn flag(&mut self, flag: Flag) -> Self::V;
    fn set_flag(&mut self, flag: Flag, value: Self::V);
    fn fs_base(&mut self) -> Self::V;
    /// Zero-extending load.
    fn load(&mut self, address: Self::V, size: Size) -> Self::V;
    /// Truncating store.
    fn store(&mut self, address: Self::V, size: Size, value: Self::V);
    fn bin(&mut self, op: Bin, a: Self::V, b: Self::V) -> Self::V;
    fn un(&mut self, op: Un, a: Self::V) -> Self::V;
    /// `if condition != 0 { then } else { otherwise }`
    fn select(&mut self, condition: Self::V, then: Self::V, otherwise: Self::V) -> Self::V;
    /// Divides `high:low` by `divisor`, all `size` wide, giving
    /// `(quotient, remainder)`. Faults on a zero divisor or an overflowing
    /// quotient, as the hardware does.
    fn divide(
        &mut self,
        signed: bool,
        size: Size,
        high: Self::V,
        low: Self::V,
        divisor: Self::V,
    ) -> (Self::V, Self::V);
    /// Runs `body` once per iteration of a `rep`-prefixed string instruction,
    /// decrementing `ecx` after each.
    fn repeat(&mut self, rep: Rep, body: impl FnMut(&mut Self));
    fn cpuid(&mut self);
    fn rdtsc(&mut self);

    fn jump(&mut self, target: Self::V);
    fn jump_if(&mut self, condition: Self::V, target: u32);
    fn call(&mut self, target: Self::V, return_address: u32);
    /// Pops the return address, then `extra` more bytes.
    fn ret(&mut self, extra: u32);
    fn trap(&mut self, trap: Trap);

    fn st(&mut self, i: u32) -> Self::F;
    fn set_st(&mut self, i: u32, value: Self::F);
    fn fpush(&mut self, value: Self::F);
    fn fpop(&mut self);
    fn fconst(&mut self, constant: FConst) -> Self::F;
    fn fload(&mut self, address: Self::V, format: FMem) -> Self::F;
    fn fstore(&mut self, address: Self::V, format: FMem, value: Self::F);
    fn fbin(&mut self, op: FBin, a: Self::F, b: Self::F) -> Self::F;
    fn fun(&mut self, op: FUn, a: Self::F) -> Self::F;
    fn fcompare(&mut self, a: Self::F, b: Self::F);
    fn fexamine(&mut self);
    fn fremainder(&mut self, nearest: bool);
    fn fstatus(&mut self) -> Self::V;
    fn fcontrol(&mut self) -> Self::V;
    fn set_fcontrol(&mut self, value: Self::V);
    fn finit(&mut self);
}

const ESP: usize = 4;
const EBP: usize = 5;
const ESI: usize = 6;
const EDI: usize = 7;

// ---------------------------------------------------------------------------
// Small helpers

fn add<B: Backend>(b: &mut B, x: B::V, y: B::V) -> B::V {
    b.bin(Bin::Add, x, y)
}

fn sub<B: Backend>(b: &mut B, x: B::V, y: B::V) -> B::V {
    b.bin(Bin::Sub, x, y)
}

fn and<B: Backend>(b: &mut B, x: B::V, y: B::V) -> B::V {
    b.bin(Bin::And, x, y)
}

fn or<B: Backend>(b: &mut B, x: B::V, y: B::V) -> B::V {
    b.bin(Bin::Or, x, y)
}

fn xor<B: Backend>(b: &mut B, x: B::V, y: B::V) -> B::V {
    b.bin(Bin::Xor, x, y)
}

fn add_imm<B: Backend>(b: &mut B, x: B::V, k: u32) -> B::V {
    let k = b.imm(k);
    b.bin(Bin::Add, x, k)
}

fn and_imm<B: Backend>(b: &mut B, x: B::V, k: u32) -> B::V {
    let k = b.imm(k);
    b.bin(Bin::And, x, k)
}

fn shl_imm<B: Backend>(b: &mut B, x: B::V, k: u32) -> B::V {
    let k = b.imm(k);
    b.bin(Bin::Shl, x, k)
}

fn shr_imm<B: Backend>(b: &mut B, x: B::V, k: u32) -> B::V {
    let k = b.imm(k);
    b.bin(Bin::Shr, x, k)
}

/// Bit `n` of `x`, as 0 or 1.
fn bit<B: Backend>(b: &mut B, x: B::V, n: u32) -> B::V {
    let shifted = shr_imm(b, x, n);
    and_imm(b, shifted, 1)
}

fn not1<B: Backend>(b: &mut B, x: B::V) -> B::V {
    let one = b.imm(1);
    b.bin(Bin::Xor, x, one)
}

fn is_zero<B: Backend>(b: &mut B, x: B::V) -> B::V {
    let zero = b.imm(0);
    b.bin(Bin::Eq, x, zero)
}

fn truncate<B: Backend>(b: &mut B, x: B::V, size: Size) -> B::V {
    if size == Size::B4 { x } else { and_imm(b, x, size.mask()) }
}

fn sign_extend<B: Backend>(b: &mut B, x: B::V, size: Size) -> B::V {
    match size {
        Size::B1 => b.un(Un::SignExtend8, x),
        Size::B2 => b.un(Un::SignExtend16, x),
        Size::B4 => x,
    }
}

// ---------------------------------------------------------------------------
// Operands

fn gpr(register: Register) -> Option<(usize, Size, bool)> {
    let size = Size::from_bytes(register.size())?;
    if !(register.is_gpr8() || register.is_gpr16() || register.is_gpr32()) {
        return None;
    }
    let high = matches!(register, Register::AH | Register::CH | Register::DH | Register::BH);
    Some((register.full_register32().number(), size, high))
}

fn read_register<B: Backend>(b: &mut B, register: Register) -> B::V {
    let Some((number, size, high)) = gpr(register) else {
        b.trap(Trap::Unsupported);
        return b.imm(0);
    };
    let full = b.reg(number);
    match (size, high) {
        (Size::B4, _) => full,
        (Size::B2, _) => and_imm(b, full, 0xFFFF),
        (Size::B1, false) => and_imm(b, full, 0xFF),
        (Size::B1, true) => {
            let shifted = shr_imm(b, full, 8);
            and_imm(b, shifted, 0xFF)
        }
    }
}

/// Writes a register, preserving the bits of the full register that the
/// narrower name does not cover. `value` must already be truncated.
fn write_register<B: Backend>(b: &mut B, register: Register, value: B::V) {
    let Some((number, size, high)) = gpr(register) else {
        b.trap(Trap::Unsupported);
        return;
    };
    let merged = match (size, high) {
        (Size::B4, _) => value,
        (Size::B2, _) => merge(b, number, 0xFFFF_0000, value),
        (Size::B1, false) => merge(b, number, 0xFFFF_FF00, value),
        (Size::B1, true) => {
            let shifted = shl_imm(b, value, 8);
            merge(b, number, 0xFFFF_00FF, shifted)
        }
    };
    b.set_reg(number, merged);
}

fn merge<B: Backend>(b: &mut B, number: usize, keep: u32, value: B::V) -> B::V {
    let old = b.reg(number);
    let kept = and_imm(b, old, keep);
    or(b, kept, value)
}

/// Effective address of the instruction's memory operand.
fn address<B: Backend>(b: &mut B, i: &Instruction, with_segment: bool) -> B::V {
    let mut sum: Option<B::V> = None;
    if i.memory_base() != Register::None {
        sum = Some(read_register(b, i.memory_base()));
    }
    if i.memory_index() != Register::None {
        let index = read_register(b, i.memory_index());
        let scaled = match i.memory_index_scale() {
            1 => index,
            scale => shl_imm(b, index, scale.trailing_zeros()),
        };
        sum = Some(match sum {
            Some(base) => add(b, base, scaled),
            None => scaled,
        });
    }
    let displacement = i.memory_displacement32();
    let mut result = match sum {
        Some(partial) if displacement == 0 => partial,
        Some(partial) => add_imm(b, partial, displacement),
        None => b.imm(displacement),
    };
    if with_segment && i.memory_segment() == Register::FS {
        let base = b.fs_base();
        result = add(b, result, base);
    }
    result
}

fn operand_size<B: Backend>(b: &mut B, i: &Instruction, n: u32) -> Size {
    let size = match i.op_kind(n) {
        OpKind::Register => gpr(i.op_register(n)).map(|(_, size, _)| size),
        OpKind::Memory => Size::from_bytes(i.memory_size().size()),
        OpKind::Immediate8 => Some(Size::B1),
        OpKind::Immediate16 | OpKind::Immediate8to16 => Some(Size::B2),
        OpKind::Immediate32 | OpKind::Immediate8to32 => Some(Size::B4),
        _ => None,
    };
    size.unwrap_or_else(|| {
        b.trap(Trap::Unsupported);
        Size::B4
    })
}

fn read<B: Backend>(b: &mut B, i: &Instruction, n: u32) -> B::V {
    match i.op_kind(n) {
        OpKind::Register => read_register(b, i.op_register(n)),
        OpKind::Memory => {
            let size = operand_size(b, i, n);
            let at = address(b, i, true);
            b.load(at, size)
        }
        OpKind::Immediate8
        | OpKind::Immediate16
        | OpKind::Immediate32
        | OpKind::Immediate8to16
        | OpKind::Immediate8to32 => {
            let size = operand_size(b, i, n);
            b.imm(i.immediate(n) as u32 & size.mask())
        }
        _ => {
            b.trap(Trap::Unsupported);
            b.imm(0)
        }
    }
}

fn write<B: Backend>(b: &mut B, i: &Instruction, n: u32, value: B::V) {
    match i.op_kind(n) {
        OpKind::Register => write_register(b, i.op_register(n), value),
        OpKind::Memory => {
            let size = operand_size(b, i, n);
            let at = address(b, i, true);
            b.store(at, size, value);
        }
        _ => b.trap(Trap::Unsupported),
    }
}

fn push_sized<B: Backend>(b: &mut B, value: B::V, size: Size) {
    let esp = b.reg(ESP);
    let esp = add_imm(b, esp, size.bytes().wrapping_neg());
    b.store(esp, size, value);
    b.set_reg(ESP, esp);
}

fn pop_sized<B: Backend>(b: &mut B, size: Size) -> B::V {
    let esp = b.reg(ESP);
    let value = b.load(esp, size);
    let esp = add_imm(b, esp, size.bytes());
    b.set_reg(ESP, esp);
    value
}

fn push<B: Backend>(b: &mut B, value: B::V) {
    push_sized(b, value, Size::B4);
}

fn pop<B: Backend>(b: &mut B) -> B::V {
    pop_sized(b, Size::B4)
}

// ---------------------------------------------------------------------------
// Flags

fn set_result_flags<B: Backend>(b: &mut B, result: B::V, size: Size) {
    let zero = is_zero(b, result);
    b.set_flag(Flag::Zero, zero);
    let sign = bit(b, result, size.bits() - 1);
    b.set_flag(Flag::Sign, sign);
    let parity = b.un(Un::ParityEven, result);
    b.set_flag(Flag::Parity, parity);
}

/// Flags of `result = x + y (+ carry in)`. The carry-out formula holds for
/// any carry-in, so `add` and `adc` share it.
fn set_add_flags<B: Backend>(b: &mut B, x: B::V, y: B::V, result: B::V, size: Size, carry: bool) {
    let msb = size.bits() - 1;
    let x_and_y = and(b, x, y);
    let x_or_y = or(b, x, y);
    let not_result = b.un(Un::Not, result);
    let propagated = and(b, x_or_y, not_result);
    let carries = or(b, x_and_y, propagated);
    if carry {
        let carry_out = bit(b, carries, msb);
        b.set_flag(Flag::Carry, carry_out);
    }
    let x_changed = xor(b, x, result);
    let y_changed = xor(b, y, result);
    let both = and(b, x_changed, y_changed);
    let overflow = bit(b, both, msb);
    b.set_flag(Flag::Overflow, overflow);
    set_adjust_flag(b, x, y, result);
    set_result_flags(b, result, size);
}

/// Flags of `result = x - y (- borrow in)`.
fn set_sub_flags<B: Backend>(b: &mut B, x: B::V, y: B::V, result: B::V, size: Size, carry: bool) {
    let msb = size.bits() - 1;
    let not_x = b.un(Un::Not, x);
    let generate = and(b, not_x, y);
    let either = or(b, not_x, y);
    let propagated = and(b, either, result);
    let borrows = or(b, generate, propagated);
    if carry {
        let borrow_out = bit(b, borrows, msb);
        b.set_flag(Flag::Carry, borrow_out);
    }
    let operands_differ = xor(b, x, y);
    let x_changed = xor(b, x, result);
    let both = and(b, operands_differ, x_changed);
    let overflow = bit(b, both, msb);
    b.set_flag(Flag::Overflow, overflow);
    set_adjust_flag(b, x, y, result);
    set_result_flags(b, result, size);
}

fn set_adjust_flag<B: Backend>(b: &mut B, x: B::V, y: B::V, result: B::V) {
    let mixed = xor(b, x, y);
    let mixed = xor(b, mixed, result);
    let adjust = bit(b, mixed, 4);
    b.set_flag(Flag::Adjust, adjust);
}

fn set_logic_flags<B: Backend>(b: &mut B, result: B::V, size: Size) {
    let zero = b.imm(0);
    b.set_flag(Flag::Carry, zero);
    b.set_flag(Flag::Overflow, zero);
    b.set_flag(Flag::Adjust, zero);
    set_result_flags(b, result, size);
}

fn condition<B: Backend>(b: &mut B, code: ConditionCode) -> B::V {
    let less = |b: &mut B| {
        let sign = b.flag(Flag::Sign);
        let overflow = b.flag(Flag::Overflow);
        xor(b, sign, overflow)
    };
    let (value, negate) = match code {
        ConditionCode::o | ConditionCode::no => (b.flag(Flag::Overflow), code == ConditionCode::no),
        ConditionCode::b | ConditionCode::ae => (b.flag(Flag::Carry), code == ConditionCode::ae),
        ConditionCode::e | ConditionCode::ne => (b.flag(Flag::Zero), code == ConditionCode::ne),
        ConditionCode::be | ConditionCode::a => {
            let carry = b.flag(Flag::Carry);
            let zero = b.flag(Flag::Zero);
            (or(b, carry, zero), code == ConditionCode::a)
        }
        ConditionCode::s | ConditionCode::ns => (b.flag(Flag::Sign), code == ConditionCode::ns),
        ConditionCode::p | ConditionCode::np => (b.flag(Flag::Parity), code == ConditionCode::np),
        ConditionCode::l | ConditionCode::ge => (less(b), code == ConditionCode::ge),
        ConditionCode::le | ConditionCode::g => {
            let less = less(b);
            let zero = b.flag(Flag::Zero);
            (or(b, less, zero), code == ConditionCode::g)
        }
        ConditionCode::None => {
            b.trap(Trap::Unsupported);
            (b.imm(0), false)
        }
    };
    if negate { not1(b, value) } else { value }
}

/// Keeps the old value of each listed flag when `unchanged` is 1. Shifts by a
/// count of zero leave every flag alone.
fn keep_flags_if<B: Backend>(b: &mut B, unchanged: B::V, flags: &[Flag], old: &[B::V]) {
    for (&flag, &old) in flags.iter().zip(old) {
        let new = b.flag(flag);
        let value = b.select(unchanged, old, new);
        b.set_flag(flag, value);
    }
}

const ARITHMETIC_FLAGS: [Flag; 6] =
    [Flag::Carry, Flag::Parity, Flag::Adjust, Flag::Zero, Flag::Sign, Flag::Overflow];

// ---------------------------------------------------------------------------
// Integer instructions

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arith {
    Add,
    Adc,
    Sub,
    Sbb,
    Cmp,
}

fn arithmetic<B: Backend>(b: &mut B, i: &Instruction, kind: Arith) {
    let size = operand_size(b, i, 0);
    let x = read(b, i, 0);
    let y = read(b, i, 1);
    let result = match kind {
        Arith::Add => add(b, x, y),
        Arith::Sub | Arith::Cmp => sub(b, x, y),
        Arith::Adc => {
            let carry = b.flag(Flag::Carry);
            let sum = add(b, x, y);
            add(b, sum, carry)
        }
        Arith::Sbb => {
            let carry = b.flag(Flag::Carry);
            let difference = sub(b, x, y);
            sub(b, difference, carry)
        }
    };
    let result = truncate(b, result, size);
    match kind {
        Arith::Add | Arith::Adc => set_add_flags(b, x, y, result, size, true),
        _ => set_sub_flags(b, x, y, result, size, true),
    }
    if kind != Arith::Cmp {
        write(b, i, 0, result);
    }
}

fn logic<B: Backend>(b: &mut B, i: &Instruction, op: Bin, store: bool) {
    let size = operand_size(b, i, 0);
    let x = read(b, i, 0);
    let y = read(b, i, 1);
    let result = b.bin(op, x, y);
    set_logic_flags(b, result, size);
    if store {
        write(b, i, 0, result);
    }
}

fn increment<B: Backend>(b: &mut B, i: &Instruction, decrement: bool) {
    let size = operand_size(b, i, 0);
    let x = read(b, i, 0);
    let one = b.imm(1);
    let result = if decrement { sub(b, x, one) } else { add(b, x, one) };
    let result = truncate(b, result, size);
    if decrement {
        set_sub_flags(b, x, one, result, size, false);
    } else {
        set_add_flags(b, x, one, result, size, false);
    }
    write(b, i, 0, result);
}

fn negate<B: Backend>(b: &mut B, i: &Instruction) {
    let size = operand_size(b, i, 0);
    let x = read(b, i, 0);
    let zero = b.imm(0);
    let result = sub(b, zero, x);
    let result = truncate(b, result, size);
    set_sub_flags(b, zero, x, result, size, true);
    write(b, i, 0, result);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shift {
    Left,
    LogicalRight,
    ArithmeticRight,
}

fn shift<B: Backend>(b: &mut B, i: &Instruction, kind: Shift) {
    let size = operand_size(b, i, 0);
    let bits = size.bits();
    let x = read(b, i, 0);
    let raw_count = read(b, i, 1);
    let count = and_imm(b, raw_count, 31);
    let unchanged = is_zero(b, count);
    let old: Vec<B::V> = ARITHMETIC_FLAGS.iter().map(|&flag| b.flag(flag)).collect();

    let count_minus_one = add_imm(b, count, u32::MAX);
    let (result, carry, overflow) = match kind {
        Shift::Left => {
            let shifted = b.bin(Bin::Shl, x, count);
            let result = truncate(b, shifted, size);
            // The last bit shifted out is bit (bits - count); none if count > bits.
            let bits_value = b.imm(bits);
            let position = sub(b, bits_value, count);
            let moved = b.bin(Bin::Shr, x, position);
            let last_out = and_imm(b, moved, 1);
            let beyond = b.bin(Bin::LtU, bits_value, count);
            let zero = b.imm(0);
            let carry = b.select(beyond, zero, last_out);
            let msb = bit(b, result, bits - 1);
            let overflow = xor(b, msb, carry);
            (result, carry, overflow)
        }
        Shift::LogicalRight => {
            let result = b.bin(Bin::Shr, x, count);
            let moved = b.bin(Bin::Shr, x, count_minus_one);
            let carry = and_imm(b, moved, 1);
            let overflow = bit(b, x, bits - 1);
            (result, carry, overflow)
        }
        Shift::ArithmeticRight => {
            let wide = sign_extend(b, x, size);
            let shifted = b.bin(Bin::Sar, wide, count);
            let result = truncate(b, shifted, size);
            let moved = b.bin(Bin::Sar, wide, count_minus_one);
            let carry = and_imm(b, moved, 1);
            (result, carry, b.imm(0))
        }
    };
    b.set_flag(Flag::Carry, carry);
    b.set_flag(Flag::Overflow, overflow);
    let zero = b.imm(0);
    b.set_flag(Flag::Adjust, zero);
    set_result_flags(b, result, size);
    keep_flags_if(b, unchanged, &ARITHMETIC_FLAGS, &old);
    write(b, i, 0, result);
}

/// `rol`: only carry and overflow change.
fn rotate_left<B: Backend>(b: &mut B, i: &Instruction) {
    let size = operand_size(b, i, 0);
    let bits = size.bits();
    let x = read(b, i, 0);
    let raw_count = read(b, i, 1);
    let masked = and_imm(b, raw_count, 31);
    let unchanged = is_zero(b, masked);
    let old = [b.flag(Flag::Carry), b.flag(Flag::Overflow)];
    // bits is a power of two, so the reduced count is a mask away.
    let count = and_imm(b, masked, bits - 1);
    let left = b.bin(Bin::Shl, x, count);
    let bits_value = b.imm(bits);
    let back = sub(b, bits_value, count);
    let right = b.bin(Bin::Shr, x, back);
    let no_rotation = is_zero(b, count);
    let zero = b.imm(0);
    let right = b.select(no_rotation, zero, right);
    let combined = or(b, left, right);
    let result = truncate(b, combined, size);
    let carry = and_imm(b, result, 1);
    b.set_flag(Flag::Carry, carry);
    let msb = bit(b, result, bits - 1);
    let overflow = xor(b, msb, carry);
    b.set_flag(Flag::Overflow, overflow);
    keep_flags_if(b, unchanged, &[Flag::Carry, Flag::Overflow], &old);
    write(b, i, 0, result);
}

/// `rcr x, 1`, the only form the program uses (64-bit shifts in the runtime).
fn rotate_carry_right_once<B: Backend>(b: &mut B, i: &Instruction) {
    if i.op_kind(1) != OpKind::Immediate8 || i.immediate(1) != 1 {
        b.trap(Trap::Unsupported);
        return;
    }
    let size = operand_size(b, i, 0);
    let bits = size.bits();
    let x = read(b, i, 0);
    let carry_in = b.flag(Flag::Carry);
    let shifted = shr_imm(b, x, 1);
    let top = shl_imm(b, carry_in, bits - 1);
    let result = or(b, shifted, top);
    let carry_out = and_imm(b, x, 1);
    b.set_flag(Flag::Carry, carry_out);
    let msb = bit(b, result, bits - 1);
    let next = bit(b, result, bits - 2);
    let overflow = xor(b, msb, next);
    b.set_flag(Flag::Overflow, overflow);
    write(b, i, 0, result);
}

/// `shld r/m32, r32, count`
fn shift_left_double<B: Backend>(b: &mut B, i: &Instruction) {
    let size = operand_size(b, i, 0);
    if size != Size::B4 {
        b.trap(Trap::Unsupported);
        return;
    }
    let x = read(b, i, 0);
    let fill = read(b, i, 1);
    let raw_count = read(b, i, 2);
    let count = and_imm(b, raw_count, 31);
    let unchanged = is_zero(b, count);
    let old: Vec<B::V> = ARITHMETIC_FLAGS.iter().map(|&flag| b.flag(flag)).collect();
    let high = b.bin(Bin::Shl, x, count);
    let thirty_two = b.imm(32);
    let back = sub(b, thirty_two, count);
    let low = b.bin(Bin::Shr, fill, back);
    let shifted = or(b, high, low);
    let result = b.select(unchanged, x, shifted);
    let moved = b.bin(Bin::Shr, x, back);
    let carry = and_imm(b, moved, 1);
    b.set_flag(Flag::Carry, carry);
    let changed = xor(b, x, result);
    let overflow = bit(b, changed, 31);
    b.set_flag(Flag::Overflow, overflow);
    let zero = b.imm(0);
    b.set_flag(Flag::Adjust, zero);
    set_result_flags(b, result, size);
    keep_flags_if(b, unchanged, &ARITHMETIC_FLAGS, &old);
    write(b, i, 0, result);
}

/// One-operand `mul`/`imul`: `edx:eax = eax * operand` (and narrower forms).
fn multiply_wide<B: Backend>(b: &mut B, i: &Instruction, signed: bool) {
    let size = operand_size(b, i, 0);
    let y = read(b, i, 0);
    let overflow = match size {
        Size::B4 => {
            let x = b.reg(0);
            let low = b.bin(Bin::Mul, x, y);
            let high = b.bin(if signed { Bin::MulHighS } else { Bin::MulHighU }, x, y);
            b.set_reg(0, low);
            b.set_reg(2, high);
            let expected = if signed {
                let thirty_one = b.imm(31);
                b.bin(Bin::Sar, low, thirty_one)
            } else {
                b.imm(0)
            };
            b.bin(Bin::Ne, high, expected)
        }
        Size::B2 | Size::B1 => {
            let accumulator = if size == Size::B2 { Register::AX } else { Register::AL };
            let x = read_register(b, accumulator);
            let (x, y) = if signed { (sign_extend(b, x, size), sign_extend(b, y, size)) } else { (x, y) };
            let product = b.bin(Bin::Mul, x, y);
            let low = truncate(b, product, size);
            if size == Size::B2 {
                write_register(b, Register::AX, low);
                let high = shr_imm(b, product, 16);
                let high = and_imm(b, high, 0xFFFF);
                write_register(b, Register::DX, high);
            } else {
                let ax = and_imm(b, product, 0xFFFF);
                write_register(b, Register::AX, ax);
            }
            let expected = if signed { sign_extend(b, low, size) } else { low };
            b.bin(Bin::Ne, product, expected)
        }
    };
    b.set_flag(Flag::Carry, overflow);
    b.set_flag(Flag::Overflow, overflow);
}

/// Two- and three-operand `imul`: truncated signed product.
fn multiply_truncated<B: Backend>(b: &mut B, i: &Instruction) {
    let size = operand_size(b, i, 0);
    let (x, y) =
        if i.op_count() == 3 { (read(b, i, 1), read(b, i, 2)) } else { (read(b, i, 0), read(b, i, 1)) };
    let (result, overflow) = if size == Size::B4 {
        let low = b.bin(Bin::Mul, x, y);
        let high = b.bin(Bin::MulHighS, x, y);
        let thirty_one = b.imm(31);
        let expected = b.bin(Bin::Sar, low, thirty_one);
        (low, b.bin(Bin::Ne, high, expected))
    } else {
        let x = sign_extend(b, x, size);
        let y = sign_extend(b, y, size);
        let product = b.bin(Bin::Mul, x, y);
        let low = truncate(b, product, size);
        let expected = sign_extend(b, low, size);
        (low, b.bin(Bin::Ne, product, expected))
    };
    b.set_flag(Flag::Carry, overflow);
    b.set_flag(Flag::Overflow, overflow);
    write(b, i, 0, result);
}

fn divide<B: Backend>(b: &mut B, i: &Instruction, signed: bool) {
    let size = operand_size(b, i, 0);
    let divisor = read(b, i, 0);
    match size {
        Size::B4 => {
            let (high, low) = (b.reg(2), b.reg(0));
            let (quotient, remainder) = b.divide(signed, size, high, low, divisor);
            b.set_reg(0, quotient);
            b.set_reg(2, remainder);
        }
        Size::B2 => {
            let (high, low) = (read_register(b, Register::DX), read_register(b, Register::AX));
            let (quotient, remainder) = b.divide(signed, size, high, low, divisor);
            write_register(b, Register::AX, quotient);
            write_register(b, Register::DX, remainder);
        }
        Size::B1 => {
            let (high, low) = (read_register(b, Register::AH), read_register(b, Register::AL));
            let (quotient, remainder) = b.divide(signed, size, high, low, divisor);
            write_register(b, Register::AL, quotient);
            write_register(b, Register::AH, remainder);
        }
    }
}

fn eflags<B: Backend>(b: &mut B) -> B::V {
    let mut value = b.imm(2); // bit 1 always reads as set
    for flag in Flag::ALL {
        let bit_value = b.flag(flag);
        let placed = shl_imm(b, bit_value, flag.bit());
        value = or(b, value, placed);
    }
    value
}

fn set_eflags<B: Backend>(b: &mut B, value: B::V, flags: &[Flag]) {
    for &flag in flags {
        let bit_value = bit(b, value, flag.bit());
        b.set_flag(flag, bit_value);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StringOp {
    Move,
    Store,
    Scan,
    Compare,
}

fn string_step<B: Backend>(b: &mut B, op: StringOp, size: Size) {
    let direction = b.flag(Flag::Direction);
    let forward = b.imm(size.bytes());
    let backward = b.imm(size.bytes().wrapping_neg());
    let delta = b.select(direction, backward, forward);
    let advance = |b: &mut B, register: usize| {
        let pointer = b.reg(register);
        let next = add(b, pointer, delta);
        b.set_reg(register, next);
    };
    let accumulator = |b: &mut B| {
        let eax = b.reg(0);
        truncate(b, eax, size)
    };
    match op {
        StringOp::Move => {
            let source = b.reg(ESI);
            let value = b.load(source, size);
            let destination = b.reg(EDI);
            b.store(destination, size, value);
            advance(b, ESI);
            advance(b, EDI);
        }
        StringOp::Store => {
            let value = accumulator(b);
            let destination = b.reg(EDI);
            b.store(destination, size, value);
            advance(b, EDI);
        }
        StringOp::Scan => {
            let x = accumulator(b);
            let destination = b.reg(EDI);
            let y = b.load(destination, size);
            let difference = sub(b, x, y);
            let result = truncate(b, difference, size);
            set_sub_flags(b, x, y, result, size, true);
            advance(b, EDI);
        }
        StringOp::Compare => {
            let source = b.reg(ESI);
            let x = b.load(source, size);
            let destination = b.reg(EDI);
            let y = b.load(destination, size);
            let difference = sub(b, x, y);
            let result = truncate(b, difference, size);
            set_sub_flags(b, x, y, result, size, true);
            advance(b, ESI);
            advance(b, EDI);
        }
    }
}

fn string<B: Backend>(b: &mut B, i: &Instruction, op: StringOp, size: Size) {
    let compares = matches!(op, StringOp::Scan | StringOp::Compare);
    let rep = if i.has_repne_prefix() {
        Some(if compares { Rep::WhileNotEqual } else { Rep::Count })
    } else if i.has_rep_prefix() {
        Some(if compares { Rep::WhileEqual } else { Rep::Count })
    } else {
        None
    };
    match rep {
        Some(rep) => b.repeat(rep, |b| string_step(b, op, size)),
        None => string_step(b, op, size),
    }
}

// ---------------------------------------------------------------------------
// x87 instructions

fn float_format(i: &Instruction) -> Option<FMem> {
    match i.memory_size() {
        MemorySize::Float32 => Some(FMem::F32),
        MemorySize::Float64 => Some(FMem::F64),
        MemorySize::Float80 => Some(FMem::F80),
        MemorySize::Int16 => Some(FMem::I16),
        MemorySize::Int32 => Some(FMem::I32),
        MemorySize::Int64 => Some(FMem::I64),
        _ => None,
    }
}

fn load_float<B: Backend>(b: &mut B, i: &Instruction) -> B::F {
    let at = address(b, i, true);
    match float_format(i) {
        Some(format) => b.fload(at, format),
        None => {
            b.trap(Trap::Unsupported);
            b.fconst(FConst::Zero)
        }
    }
}

fn stack_index(register: Register) -> u32 {
    (register as u32).wrapping_sub(Register::ST0 as u32) & 7
}

/// `fadd`-family instructions: `destination = destination op source`, or with
/// the operands swapped for the reversed forms, optionally popping.
fn float_arithmetic<B: Backend>(b: &mut B, i: &Instruction, op: FBin, reversed: bool, pop: bool) {
    let (destination, x, y) = if i.op_kind(0) == OpKind::Memory {
        let source = load_float(b, i);
        (0, b.st(0), source)
    } else {
        let destination = stack_index(i.op_register(0));
        let source = stack_index(i.op_register(1));
        (destination, b.st(destination), b.st(source))
    };
    let result = if reversed { b.fbin(op, y, x) } else { b.fbin(op, x, y) };
    b.set_st(destination, result);
    if pop {
        b.fpop();
    }
}

fn float_compare<B: Backend>(b: &mut B, i: &Instruction, pops: u32) {
    let x = b.st(0);
    let y = if i.op_count() == 0 {
        b.st(1)
    } else if i.op_kind(0) == OpKind::Memory {
        load_float(b, i)
    } else {
        // One- and two-operand register forms both name the source last.
        let source = i.op_register(i.op_count() - 1);
        b.st(stack_index(source))
    };
    b.fcompare(x, y);
    for _ in 0..pops {
        b.fpop();
    }
}

fn float_store<B: Backend>(b: &mut B, i: &Instruction, pop: bool) {
    let value = b.st(0);
    if i.op_kind(0) == OpKind::Memory {
        let at = address(b, i, true);
        match float_format(i) {
            Some(format) => b.fstore(at, format, value),
            None => b.trap(Trap::Unsupported),
        }
    } else {
        b.set_st(stack_index(i.op_register(0)), value);
    }
    if pop {
        b.fpop();
    }
}

fn float_unary<B: Backend>(b: &mut B, op: FUn) {
    let x = b.st(0);
    let result = b.fun(op, x);
    b.set_st(0, result);
}

/// Returns false if `i` is not an x87 instruction handled here.
fn float<B: Backend>(b: &mut B, i: &Instruction) -> bool {
    use Mnemonic::*;
    match i.mnemonic() {
        Fadd | Fiadd => float_arithmetic(b, i, FBin::Add, false, false),
        Faddp => float_arithmetic(b, i, FBin::Add, false, true),
        Fsub | Fisub => float_arithmetic(b, i, FBin::Sub, false, false),
        Fsubp => float_arithmetic(b, i, FBin::Sub, false, true),
        Fsubr | Fisubr => float_arithmetic(b, i, FBin::Sub, true, false),
        Fsubrp => float_arithmetic(b, i, FBin::Sub, true, true),
        Fmul | Fimul => float_arithmetic(b, i, FBin::Mul, false, false),
        Fmulp => float_arithmetic(b, i, FBin::Mul, false, true),
        Fdiv | Fidiv => float_arithmetic(b, i, FBin::Div, false, false),
        Fdivp => float_arithmetic(b, i, FBin::Div, false, true),
        Fdivr | Fidivr => float_arithmetic(b, i, FBin::Div, true, false),
        Fdivrp => float_arithmetic(b, i, FBin::Div, true, true),
        Fld | Fild => {
            let value = if i.op_kind(0) == OpKind::Memory {
                load_float(b, i)
            } else {
                b.st(stack_index(i.op_register(0)))
            };
            b.fpush(value);
        }
        Fst => float_store(b, i, false),
        Fstp | Fistp => float_store(b, i, true),
        Fist => float_store(b, i, false),
        Fxch => {
            let other = stack_index(i.op_register(i.op_count().max(1) - 1));
            let (x, y) = (b.st(0), b.st(other));
            b.set_st(0, y);
            b.set_st(other, x);
        }
        Fcom => float_compare(b, i, 0),
        Fcomp => float_compare(b, i, 1),
        Fcompp => float_compare(b, i, 2),
        Ftst => {
            let x = b.st(0);
            let zero = b.fconst(FConst::Zero);
            b.fcompare(x, zero);
        }
        Fxam => b.fexamine(),
        Fld1 => constant(b, FConst::One),
        Fldz => constant(b, FConst::Zero),
        Fldpi => constant(b, FConst::Pi),
        Fldl2e => constant(b, FConst::Log2E),
        Fldlg2 => constant(b, FConst::Log10Of2),
        Fldln2 => constant(b, FConst::LnOf2),
        Fabs => float_unary(b, FUn::Abs),
        Fchs => float_unary(b, FUn::Neg),
        Fsqrt => float_unary(b, FUn::Sqrt),
        Fsin => float_unary(b, FUn::Sin),
        Fcos => float_unary(b, FUn::Cos),
        Frndint => float_unary(b, FUn::Round),
        F2xm1 => float_unary(b, FUn::Exp2M1),
        Fptan => {
            float_unary(b, FUn::Tan);
            constant(b, FConst::One);
        }
        Fpatan | Fyl2x => {
            let (x, y) = (b.st(1), b.st(0));
            let op = if i.mnemonic() == Fpatan { FBin::Atan2 } else { FBin::Yl2x };
            let result = b.fbin(op, x, y);
            b.fpop();
            b.set_st(0, result);
        }
        Fscale => {
            let (x, y) = (b.st(0), b.st(1));
            let result = b.fbin(FBin::Scale, x, y);
            b.set_st(0, result);
        }
        Fprem => b.fremainder(false),
        Fprem1 => b.fremainder(true),
        Fnstsw => {
            let status = b.fstatus();
            write(b, i, 0, status);
        }
        Fnstcw => {
            let control = b.fcontrol();
            write(b, i, 0, control);
        }
        Fldcw => {
            let control = read(b, i, 0);
            b.set_fcontrol(control);
        }
        Fninit => b.finit(),
        // No exception is ever left pending in this model.
        Fnclex | Wait => {}
        // State save/restore: only in runtime error paths. Not modelled yet.
        Fnsave | Frstor | Fnstenv | Fldenv => b.trap(Trap::Unsupported),
        _ => return false,
    }
    true
}

fn constant<B: Backend>(b: &mut B, constant: FConst) {
    let value = b.fconst(constant);
    b.fpush(value);
}

// ---------------------------------------------------------------------------
// Entry point

/// Applies the effect of one instruction to the backend.
///
/// If no control-transfer primitive is invoked, execution continues at
/// `instruction.next_ip()`.
pub fn step<B: Backend>(b: &mut B, i: &Instruction) {
    use Mnemonic::*;
    let mnemonic = i.mnemonic();
    match mnemonic {
        Mov => {
            let value = read(b, i, 1);
            write(b, i, 0, value);
        }
        Movzx => {
            let value = read(b, i, 1);
            write(b, i, 0, value);
        }
        Movsx => {
            let source_size = operand_size(b, i, 1);
            let size = operand_size(b, i, 0);
            let value = read(b, i, 1);
            let extended = sign_extend(b, value, source_size);
            let result = truncate(b, extended, size);
            write(b, i, 0, result);
        }
        Lea => {
            let at = address(b, i, false);
            let size = operand_size(b, i, 0);
            let result = truncate(b, at, size);
            write(b, i, 0, result);
        }
        Xchg => {
            let (x, y) = (read(b, i, 0), read(b, i, 1));
            write(b, i, 0, y);
            write(b, i, 1, x);
        }
        Push => {
            // A 16-bit operand moves the stack pointer by 2.
            let size = operand_size(b, i, 0);
            let value = read(b, i, 0);
            if size == Size::B1 { b.trap(Trap::Unsupported) } else { push_sized(b, value, size) }
        }
        Pop => {
            let size = operand_size(b, i, 0);
            if size == Size::B1 {
                b.trap(Trap::Unsupported);
            } else {
                // The stack pointer is already incremented when the
                // destination address is computed.
                let value = pop_sized(b, size);
                write(b, i, 0, value);
            }
        }
        Pushfd => {
            let value = eflags(b);
            push(b, value);
        }
        Popfd => {
            let value = pop(b);
            set_eflags(b, value, &Flag::ALL);
        }
        Sahf => {
            let ah = read_register(b, Register::AH);
            set_eflags(b, ah, &[Flag::Carry, Flag::Parity, Flag::Adjust, Flag::Zero, Flag::Sign]);
        }
        Leave => {
            let frame = b.reg(EBP);
            b.set_reg(ESP, frame);
            let saved = pop(b);
            b.set_reg(EBP, saved);
        }

        Add => arithmetic(b, i, Arith::Add),
        Adc => arithmetic(b, i, Arith::Adc),
        Sub => arithmetic(b, i, Arith::Sub),
        Sbb => arithmetic(b, i, Arith::Sbb),
        Cmp => arithmetic(b, i, Arith::Cmp),
        And => logic(b, i, Bin::And, true),
        Or => logic(b, i, Bin::Or, true),
        Xor => logic(b, i, Bin::Xor, true),
        Test => logic(b, i, Bin::And, false),
        Inc => increment(b, i, false),
        Dec => increment(b, i, true),
        Neg => negate(b, i),
        Not => {
            let size = operand_size(b, i, 0);
            let x = read(b, i, 0);
            let inverted = b.un(Un::Not, x);
            let result = truncate(b, inverted, size);
            write(b, i, 0, result);
        }
        Shl | Sal => shift(b, i, Shift::Left),
        Shr => shift(b, i, Shift::LogicalRight),
        Sar => shift(b, i, Shift::ArithmeticRight),
        Rol => rotate_left(b, i),
        Rcr => rotate_carry_right_once(b, i),
        Shld => shift_left_double(b, i),
        Mul => multiply_wide(b, i, false),
        Imul if i.op_count() == 1 => multiply_wide(b, i, true),
        Imul => multiply_truncated(b, i),
        Div => divide(b, i, false),
        Idiv => divide(b, i, true),
        Cdq => {
            let eax = b.reg(0);
            let thirty_one = b.imm(31);
            let sign = b.bin(Bin::Sar, eax, thirty_one);
            b.set_reg(2, sign);
        }
        Cwde => {
            let eax = b.reg(0);
            let extended = b.un(Un::SignExtend16, eax);
            b.set_reg(0, extended);
        }
        Xlatb => {
            let al = read_register(b, Register::AL);
            let table = b.reg(3);
            let at = add(b, table, al);
            let value = b.load(at, Size::B1);
            write_register(b, Register::AL, value);
        }

        Seto | Setno | Setb | Setae | Sete | Setne | Setbe | Seta | Sets | Setns | Setp | Setnp | Setl
        | Setge | Setle | Setg => {
            let value = condition(b, i.condition_code());
            write(b, i, 0, value);
        }
        Jo | Jno | Jb | Jae | Je | Jne | Jbe | Ja | Js | Jns | Jp | Jnp | Jl | Jge | Jle | Jg => {
            let taken = condition(b, i.condition_code());
            b.jump_if(taken, i.near_branch_target() as u32);
        }
        Jmp => {
            let target = branch_target(b, i);
            b.jump(target);
        }
        Call => {
            let target = branch_target(b, i);
            b.call(target, i.next_ip() as u32);
        }
        Ret => {
            let extra = if i.op_count() == 1 { i.immediate(0) as u32 } else { 0 };
            b.ret(extra);
        }

        Movsb => string(b, i, StringOp::Move, Size::B1),
        Movsw => string(b, i, StringOp::Move, Size::B2),
        Movsd => string(b, i, StringOp::Move, Size::B4),
        Stosb => string(b, i, StringOp::Store, Size::B1),
        Stosw => string(b, i, StringOp::Store, Size::B2),
        Stosd => string(b, i, StringOp::Store, Size::B4),
        Scasb => string(b, i, StringOp::Scan, Size::B1),
        Cmpsb => string(b, i, StringOp::Compare, Size::B1),
        Cmpsd => string(b, i, StringOp::Compare, Size::B4),

        Cld | Std => {
            let value = b.imm((mnemonic == Std) as u32);
            b.set_flag(Flag::Direction, value);
        }
        Nop => {}
        Int3 => b.trap(Trap::Breakpoint),
        Cpuid => b.cpuid(),
        Rdtsc => b.rdtsc(),
        _ => {
            if !float(b, i) {
                b.trap(Trap::Unsupported);
            }
        }
    }
}

fn branch_target<B: Backend>(b: &mut B, i: &Instruction) -> B::V {
    match i.op_kind(0) {
        OpKind::NearBranch32 => b.imm(i.near_branch_target() as u32),
        OpKind::Register | OpKind::Memory if operand_size(b, i, 0) == Size::B4 => read(b, i, 0),
        _ => {
            b.trap(Trap::Unsupported);
            b.imm(0)
        }
    }
}
