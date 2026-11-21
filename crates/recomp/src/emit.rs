//! The backend that writes Rust: the recompiler's half of ADR 0009.
//!
//! [`andy_cpu::sem`] gives each instruction its meaning as operations on
//! a backend. The interpreter carries them out; this backend writes them
//! down, one statement of Rust for each, for a run of instructions that is
//! always executed together (a block). The Rust is compiled on the user's
//! machine and takes the interpreter's place for those instructions.
//!
//! A block is a function over the same state the interpreter has
//! (`andy_cpu::interp::Cpu` and the memory). After it has run, that state
//! is what the interpreter would have left, with one exception: a flag
//! that nothing reads before it is set again is not computed (see
//! [`crate::blocks`], which works out which flags those are).
//!
//! What is written:
//!
//! - a register that is set is stored at once, so the registers are
//!   always the machine's; its value is also remembered, and reading the
//!   register again costs nothing;
//! - a flag that is set is only remembered: it is stored when the block
//!   ends, and only if something may read it afterwards;
//! - an operation on values known when the block is written is carried
//!   out then, with the interpreter's own arithmetic;
//! - a value nothing uses is not computed. A read of memory is always
//!   made: it can fault, and a fault ends the run with the instruction's
//!   address, as in the interpreter.

use std::fmt::Write;

use andy_cpu::fpu::{FBin, FConst, FMem, FUn};
use andy_cpu::native;
use andy_cpu::sem::{Backend, Bin, Flag, Rep, Size, Trap, Un};

/// A 32-bit value: known when the block is written, or computed when it
/// runs and held in a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V {
    Known(u32),
    Temp(u32),
}

/// A floating-point value, held in a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct F(u32);

impl std::fmt::Display for V {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // With its type: a method may be called on it.
            V::Known(value) if *value < 10 => write!(f, "{value}u32"),
            V::Known(value) => write!(f, "{value:#x}u32"),
            V::Temp(number) => write!(f, "t{number}"),
        }
    }
}

impl std::fmt::Display for F {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "f{}", self.0)
    }
}

/// A variable a statement uses or sets: an integer one or a
/// floating-point one, by its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Var {
    Int(u32),
    Float(u32),
}

/// A statement of the block.
#[derive(Debug, Clone)]
enum Stmt {
    /// Sets a variable to a value that only depends on other variables:
    /// left out if nothing uses it.
    Pure { sets: Var, text: String, uses: Vec<Var> },
    /// Anything that reads or changes the machine: always kept.
    Kept { text: String, uses: Vec<Var> },
    /// A flag stored at the end of the block: kept if the flag may be
    /// read afterwards.
    Flag { flag: Flag, value: V },
    /// The body of a repeated string instruction.
    Loop { rep: Rep, body: Vec<Stmt> },
}

/// How a block ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// At the next instruction, which begins another block.
    Next(u32),
    /// At an address: a jump, a call, a return.
    Jump(V),
    /// At `taken` if the condition is not zero, else at `otherwise`.
    Branch { condition: V, taken: u32, otherwise: u32 },
}

/// A block as written down.
#[derive(Debug, Clone)]
pub struct Block {
    pub address: u32,
    /// How many instructions it is.
    pub instructions: u32,
    pub exit: Exit,
    /// The flags it reads before setting them, and those it sets, as bits
    /// by `Flag as u32`.
    pub reads: u8,
    pub sets: u8,
    statements: Vec<Stmt>,
}

const ALL_FLAGS: u8 = 0x7F;

fn bit(flag: Flag) -> u8 {
    1 << flag as u8
}

/// The backend that writes a block down.
#[derive(Clone)]
pub struct Emit {
    statements: Vec<Stmt>,
    next_int: u32,
    next_float: u32,
    /// The value each register is known to hold.
    registers: [Option<V>; 8],
    /// The value each flag is known to hold, and whether the machine's
    /// own flag is behind it (set in this block and not yet stored).
    flags: [Option<V>; 7],
    pending: [bool; 7],
    reads: u8,
    sets: u8,
    /// How deep in loops the statement being written is.
    loops: u32,
    /// The instruction being written: its number in the block and its
    /// address.
    at: (u32, u32),
    exit: Option<Exit>,
    trap: Option<Trap>,
}

impl Default for Emit {
    fn default() -> Self {
        Emit::new()
    }
}

impl Emit {
    pub fn new() -> Emit {
        Emit {
            statements: Vec::new(),
            next_int: 0,
            next_float: 0,
            registers: [None; 8],
            flags: [None; 7],
            pending: [false; 7],
            reads: 0,
            sets: 0,
            loops: 0,
            at: (0, 0),
            exit: None,
            trap: None,
        }
    }

    /// Begins the instruction at `address`, the `number`-th of the block.
    pub fn begin(&mut self, number: u32, address: u32) {
        self.at = (number, address);
    }

    /// Whether the instruction just written transfers control (and so
    /// ends the block), and whether it is one the semantics do not cover.
    pub fn ended(&self) -> bool {
        self.exit.is_some()
    }

    pub fn trapped(&self) -> Option<Trap> {
        self.trap
    }

    /// Everything written so far, to return to if the next instruction
    /// turns out not to be writable.
    pub fn mark(&self) -> Emit {
        self.clone()
    }

    /// The block, of `instructions` instructions from `address`; `next`
    /// is where it goes on if its last instruction does not say.
    pub fn finish(self, address: u32, instructions: u32, next: u32) -> Block {
        let mut statements = self.statements;
        for flag in Flag::ALL {
            if let (true, Some(value)) = (self.pending[flag as usize], self.flags[flag as usize]) {
                statements.push(Stmt::Flag { flag, value });
            }
        }
        Block {
            address,
            instructions,
            exit: self.exit.unwrap_or(Exit::Next(next)),
            reads: self.reads,
            sets: self.sets,
            statements,
        }
    }

    fn uses(values: &[V]) -> Vec<Var> {
        values
            .iter()
            .filter_map(|value| if let V::Temp(n) = value { Some(Var::Int(*n)) } else { None })
            .collect()
    }

    fn pure(&mut self, text: String, uses: &[V]) -> V {
        let temp = self.next_int;
        self.next_int += 1;
        self.statements.push(Stmt::Pure { sets: Var::Int(temp), text, uses: Self::uses(uses) });
        V::Temp(temp)
    }

    /// A statement that is kept and sets an integer variable; `text` is
    /// what stands after `let tN = `.
    fn kept_value(&mut self, text: String, uses: &[V]) -> V {
        let temp = self.next_int;
        self.next_int += 1;
        self.statements.push(Stmt::Kept { text: format!("let t{temp} = {text};"), uses: Self::uses(uses) });
        V::Temp(temp)
    }

    fn kept(&mut self, text: String, uses: Vec<Var>) {
        self.statements.push(Stmt::Kept { text, uses });
    }

    /// A kept statement that sets a floating-point variable.
    fn kept_float(&mut self, text: String, uses: Vec<Var>) -> F {
        let temp = self.next_float;
        self.next_float += 1;
        self.statements.push(Stmt::Kept { text: format!("let f{temp} = {text};"), uses });
        F(temp)
    }

    /// What a faulting instruction answers: the fault, with the
    /// instruction's place.
    fn fault(&self, fault: &str) -> String {
        format!("return native::fault(cpu, {}, {:#x}, Fault::{fault})", self.at.0, self.at.1)
    }

    /// Stores every flag the machine's own is behind.
    fn store_flags(&mut self) {
        for flag in Flag::ALL {
            let index = flag as usize;
            if let (true, Some(value)) = (self.pending[index], self.flags[index]) {
                self.kept(format!("cpu.flags[{index}] = {value};"), Self::uses(&[value]));
                self.pending[index] = false;
            }
        }
    }
}

fn size_name(size: Size) -> &'static str {
    match size {
        Size::B1 => "Size::B1",
        Size::B2 => "Size::B2",
        Size::B4 => "Size::B4",
    }
}

impl Backend for Emit {
    type V = V;
    type F = F;

    fn imm(&mut self, value: u32) -> V {
        V::Known(value)
    }

    fn reg(&mut self, number: usize) -> V {
        if let Some(value) = self.registers[number] {
            return value;
        }
        let value = self.kept_value(format!("cpu.regs[{number}]"), &[]);
        self.registers[number] = Some(value);
        value
    }

    fn set_reg(&mut self, number: usize, value: V) {
        self.kept(format!("cpu.regs[{number}] = {value};"), Self::uses(&[value]));
        self.registers[number] = Some(value);
    }

    fn flag(&mut self, flag: Flag) -> V {
        let index = flag as usize;
        if let Some(value) = self.flags[index] {
            return value;
        }
        if self.sets & bit(flag) == 0 {
            self.reads |= bit(flag);
        }
        let value = self.kept_value(format!("cpu.flags[{index}]"), &[]);
        self.flags[index] = Some(value);
        value
    }

    fn set_flag(&mut self, flag: Flag, value: V) {
        let index = flag as usize;
        self.flags[index] = Some(value);
        if self.loops == 0 {
            self.pending[index] = true;
            self.sets |= bit(flag);
        } else {
            // In a loop the flag is stored at once: the next round and
            // the loop's own test read the machine's.
            self.kept(format!("cpu.flags[{index}] = {value};"), Self::uses(&[value]));
        }
    }

    fn fs_base(&mut self) -> V {
        self.kept_value("cpu.fs_base".into(), &[])
    }

    fn load(&mut self, address: V, size: Size) -> V {
        let fault = self.fault(&format!("Memory {{ address: {address}, write: false }}"));
        let text = format!("match mem.read({address}, {}) {{ Some(v) => v, None => {fault} }}", size.bytes());
        self.kept_value(text, &[address])
    }

    fn store(&mut self, address: V, size: Size, value: V) {
        let fault = self.fault(&format!("Memory {{ address: {address}, write: true }}"));
        let text = format!("if mem.write({address}, {}, {value}).is_none() {{ {fault}; }}", size.bytes());
        self.kept(text, Self::uses(&[address, value]));
    }

    fn bin(&mut self, op: Bin, a: V, b: V) -> V {
        if let (V::Known(a), V::Known(b)) = (a, b) {
            return V::Known(native::bin(op, a, b));
        }
        // What leaves a value as it is.
        match (op, a, b) {
            (Bin::Add | Bin::Or | Bin::Xor, value, V::Known(0))
            | (Bin::Add | Bin::Or | Bin::Xor, V::Known(0), value) => {
                return value;
            }
            (Bin::Sub | Bin::Shl | Bin::Shr | Bin::Sar, value, V::Known(0)) => return value,
            (Bin::And, value, V::Known(u32::MAX)) | (Bin::And, V::Known(u32::MAX), value) => return value,
            (Bin::And | Bin::Mul, _, V::Known(0)) | (Bin::And | Bin::Mul, V::Known(0), _) => {
                return V::Known(0);
            }
            _ => {}
        }
        let text = match op {
            Bin::Add => format!("{a}.wrapping_add({b})"),
            Bin::Sub => format!("{a}.wrapping_sub({b})"),
            Bin::And => format!("{a} & {b}"),
            Bin::Or => format!("{a} | {b}"),
            Bin::Xor => format!("{a} ^ {b}"),
            Bin::Shl => format!("{a}.wrapping_shl({b})"),
            Bin::Shr => format!("{a}.wrapping_shr({b})"),
            Bin::Sar => format!("({a} as i32).wrapping_shr({b}) as u32"),
            Bin::Mul => format!("{a}.wrapping_mul({b})"),
            Bin::MulHighU => format!("(({a} as u64 * {b} as u64) >> 32) as u32"),
            Bin::MulHighS => format!("(({a} as i32 as i64 * {b} as i32 as i64) >> 32) as u32"),
            Bin::Eq => format!("({a} == {b}) as u32"),
            Bin::Ne => format!("({a} != {b}) as u32"),
            Bin::LtU => format!("({a} < {b}) as u32"),
        };
        self.pure(text, &[a, b])
    }

    fn un(&mut self, op: Un, a: V) -> V {
        if let V::Known(a) = a {
            return V::Known(native::un(op, a));
        }
        let text = match op {
            Un::Not => format!("!{a}"),
            Un::Neg => format!("{a}.wrapping_neg()"),
            Un::SignExtend8 => format!("{a} as i8 as u32"),
            Un::SignExtend16 => format!("{a} as i16 as u32"),
            Un::ParityEven => format!("(({a} as u8).count_ones() + 1) & 1"),
        };
        self.pure(text, &[a])
    }

    fn select(&mut self, condition: V, then: V, otherwise: V) -> V {
        match condition {
            V::Known(0) => otherwise,
            V::Known(_) => then,
            _ if then == otherwise => then,
            _ => self.pure(
                format!("if {condition} != 0 {{ {then} }} else {{ {otherwise} }}"),
                &[condition, then, otherwise],
            ),
        }
    }

    fn divide(&mut self, signed: bool, size: Size, high: V, low: V, divisor: V) -> (V, V) {
        let (quotient, remainder) = (self.next_int, self.next_int + 1);
        self.next_int += 2;
        let fault = self.fault("Divide");
        let text = format!(
            "let (t{quotient}, t{remainder}) = match native::divide({signed}, {}, {high}, {low}, {divisor}) \
             {{ Some(v) => v, None => {fault} }};",
            size_name(size)
        );
        self.kept(text, Self::uses(&[high, low, divisor]));
        (V::Temp(quotient), V::Temp(remainder))
    }

    fn repeat(&mut self, rep: Rep, mut body: impl FnMut(&mut Self)) {
        // The loop reads the machine's own flags and registers, round
        // after round: they are brought up to date before it, and
        // nothing known before it is known inside or after.
        self.store_flags();
        let outer = std::mem::take(&mut self.statements);
        (self.registers, self.flags) = ([None; 8], [None; 7]);
        self.loops += 1;
        body(self);
        self.loops -= 1;
        let body = std::mem::replace(&mut self.statements, outer);
        self.statements.push(Stmt::Loop { rep, body });
        (self.registers, self.flags) = ([None; 8], [None; 7]);
    }

    fn cpuid(&mut self) {
        self.kept("native::cpuid(cpu);".into(), Vec::new());
        self.registers = [None; 8];
    }

    fn rdtsc(&mut self) {
        // The instructions of this block before this one have run but are
        // not counted yet.
        self.kept(format!("native::rdtsc(cpu, {});", self.at.0), Vec::new());
        self.registers = [None; 8];
    }

    fn jump(&mut self, target: V) {
        self.exit = Some(Exit::Jump(target));
    }

    fn jump_if(&mut self, condition: V, target: u32) {
        // Where it goes on otherwise is filled in when the block is
        // finished: the builder says where the next instruction is.
        self.exit = Some(Exit::Branch { condition, taken: target, otherwise: 0 });
    }

    fn call(&mut self, target: V, return_address: u32) {
        let esp = self.reg(4);
        let esp = self.bin(Bin::Sub, esp, V::Known(4));
        self.store(esp, Size::B4, V::Known(return_address));
        self.set_reg(4, esp);
        self.exit = Some(Exit::Jump(target));
    }

    fn ret(&mut self, extra: u32) {
        let esp = self.reg(4);
        let target = self.load(esp, Size::B4);
        let esp = self.bin(Bin::Add, esp, V::Known(4u32.wrapping_add(extra)));
        self.set_reg(4, esp);
        self.exit = Some(Exit::Jump(target));
    }

    fn trap(&mut self, trap: Trap) {
        self.trap = Some(trap);
    }

    fn st(&mut self, i: u32) -> F {
        self.kept_float(format!("cpu.fpu.st({i})"), Vec::new())
    }

    fn set_st(&mut self, i: u32, value: F) {
        self.kept(format!("cpu.fpu.set_st({i}, {value});"), vec![Var::Float(value.0)]);
    }

    fn fpush(&mut self, value: F) {
        self.kept(format!("cpu.fpu.push({value});"), vec![Var::Float(value.0)]);
    }

    fn fpop(&mut self) {
        self.kept("cpu.fpu.pop();".into(), Vec::new());
    }

    fn fconst(&mut self, constant: FConst) -> F {
        self.kept_float(format!("FConst::{constant:?}.value()"), Vec::new())
    }

    fn fload(&mut self, address: V, format: FMem) -> F {
        let fault = self.fault(&format!("Memory {{ address: {address}, write: false }}"));
        let text = format!(
            "match native::fload(mem, {address}, FMem::{format:?}) {{ Some(v) => v, None => {fault} }}"
        );
        self.kept_float(text, Self::uses(&[address]))
    }

    fn fstore(&mut self, address: V, format: FMem, value: F) {
        let fault = self.fault(&format!("Memory {{ address: {address}, write: true }}"));
        let text = format!(
            "if native::fstore(cpu, mem, {address}, FMem::{format:?}, {value}).is_none() {{ {fault}; }}"
        );
        let mut uses = Self::uses(&[address]);
        uses.push(Var::Float(value.0));
        self.kept(text, uses);
    }

    fn fbin(&mut self, op: FBin, a: F, b: F) -> F {
        self.kept_float(
            format!("cpu.fpu.binary(FBin::{op:?}, {a}, {b})"),
            vec![Var::Float(a.0), Var::Float(b.0)],
        )
    }

    fn fun(&mut self, op: FUn, a: F) -> F {
        self.kept_float(format!("cpu.fpu.unary(FUn::{op:?}, {a})"), vec![Var::Float(a.0)])
    }

    fn fcompare(&mut self, a: F, b: F) {
        self.kept(format!("cpu.fpu.compare({a}, {b});"), vec![Var::Float(a.0), Var::Float(b.0)]);
    }

    fn fexamine(&mut self) {
        self.kept("cpu.fpu.examine();".into(), Vec::new());
    }

    fn fremainder(&mut self, nearest: bool) {
        self.kept(format!("cpu.fpu.remainder({nearest});"), Vec::new());
    }

    fn fstatus(&mut self) -> V {
        self.kept_value("cpu.fpu.status() as u32".into(), &[])
    }

    fn fcontrol(&mut self) -> V {
        self.kept_value("cpu.fpu.control as u32".into(), &[])
    }

    fn set_fcontrol(&mut self, value: V) {
        self.kept(format!("cpu.fpu.control = {value} as u16;"), Self::uses(&[value]));
    }

    fn finit(&mut self) {
        self.kept("cpu.fpu.init();".into(), Vec::new());
    }
}

impl Block {
    /// Where the block can go on, as far as its own text says: the
    /// addresses, and whether it can go on anywhere else (a return, a jump
    /// or call through a value).
    pub fn successors(&self) -> (Vec<u32>, bool) {
        match self.exit {
            Exit::Next(next) => (vec![next], false),
            Exit::Jump(V::Known(target)) => (vec![target], false),
            Exit::Jump(V::Temp(_)) => (Vec::new(), true),
            Exit::Branch { taken, otherwise, .. } => (vec![taken, otherwise], false),
        }
    }

    /// Fills in where a conditional jump goes on when it is not taken.
    pub fn falls_to(&mut self, next: u32) {
        if let Exit::Branch { otherwise, .. } = &mut self.exit {
            *otherwise = next;
        }
    }

    /// The block as a Rust function named `name`. `live` are the flags
    /// that may be read after it: the others are not stored, and what
    /// only they needed is not computed.
    pub fn rust(&self, name: &str, live: u8) -> String {
        // What the block's end uses.
        let mut used: std::collections::HashSet<Var> = std::collections::HashSet::new();
        let exit_value = match self.exit {
            Exit::Jump(value) => Some(value),
            Exit::Branch { condition, .. } => Some(condition),
            Exit::Next(_) => None,
        };
        if let Some(V::Temp(temp)) = exit_value {
            used.insert(Var::Int(temp));
        }
        // Backwards: a statement is written if it is kept or if what it
        // sets is used by one that is written.
        fn uses_of(statements: &[Stmt], used: &mut std::collections::HashSet<Var>) {
            for statement in statements {
                match statement {
                    Stmt::Pure { uses, .. } | Stmt::Kept { uses, .. } => used.extend(uses.iter().copied()),
                    Stmt::Flag { value: V::Temp(temp), .. } => {
                        used.insert(Var::Int(*temp));
                    }
                    Stmt::Flag { .. } => {}
                    Stmt::Loop { body, .. } => uses_of(body, used),
                }
            }
        }
        let mut written = vec![false; self.statements.len()];
        for (index, statement) in self.statements.iter().enumerate().rev() {
            written[index] = match statement {
                Stmt::Pure { sets, uses, .. } => {
                    let wanted = used.contains(sets);
                    if wanted {
                        used.extend(uses.iter().copied());
                    }
                    wanted
                }
                Stmt::Kept { uses, .. } => {
                    used.extend(uses.iter().copied());
                    true
                }
                Stmt::Flag { flag, value } => {
                    let wanted = live & bit(*flag) != 0;
                    if let (true, V::Temp(temp)) = (wanted, value) {
                        used.insert(Var::Int(*temp));
                    }
                    wanted
                }
                Stmt::Loop { body, .. } => {
                    uses_of(body, &mut used);
                    true
                }
            };
        }
        fn write(out: &mut String, statement: &Stmt, indent: &str) {
            match statement {
                Stmt::Pure { sets: Var::Int(temp), text, .. } => {
                    let _ = writeln!(out, "{indent}let t{temp} = {text};");
                }
                Stmt::Pure { sets: Var::Float(temp), text, .. } => {
                    let _ = writeln!(out, "{indent}let f{temp} = {text};");
                }
                Stmt::Kept { text, .. } => {
                    let _ = writeln!(out, "{indent}{text}");
                }
                Stmt::Flag { flag, value } => {
                    let _ = writeln!(out, "{indent}cpu.flags[{}] = {value};", *flag as usize);
                }
                Stmt::Loop { rep, body } => {
                    // As the interpreter repeats: while the count is not
                    // zero, the body, one off the count, and for the two
                    // conditional kinds a look at the zero flag.
                    let _ = writeln!(out, "{indent}while cpu.regs[1] != 0 {{");
                    let inner = format!("{indent}    ");
                    for statement in body {
                        write(out, statement, &inner);
                    }
                    let _ = writeln!(out, "{inner}cpu.regs[1] -= 1;");
                    let zero = Flag::Zero as usize;
                    match rep {
                        Rep::Count => {}
                        Rep::WhileEqual => {
                            let _ = writeln!(out, "{inner}if cpu.flags[{zero}] == 0 {{ break; }}");
                        }
                        Rep::WhileNotEqual => {
                            let _ = writeln!(out, "{inner}if cpu.flags[{zero}] != 0 {{ break; }}");
                        }
                    }
                    let _ = writeln!(out, "{indent}}}");
                }
            }
        }
        let mut out = String::new();
        let _ = writeln!(out, "pub fn {name}(cpu: &mut Cpu, mem: &mut Memory) -> Result<(), Fault> {{");
        for (statement, _) in self.statements.iter().zip(&written).filter(|(_, written)| **written) {
            write(&mut out, statement, "    ");
        }
        let next = match self.exit {
            Exit::Next(next) => format!("{next:#x}"),
            Exit::Jump(target) => target.to_string(),
            Exit::Branch { condition, taken, otherwise } => {
                format!("if {condition} != 0 {{ {taken:#x} }} else {{ {otherwise:#x} }}")
            }
        };
        let _ = writeln!(out, "    cpu.eip = {next};");
        let _ = writeln!(out, "    cpu.instructions += {};", self.instructions);
        let _ = writeln!(out, "    Ok(())\n}}");
        out
    }
}

/// The flags, as bits by `Flag as u32`: all of them.
pub const EVERY_FLAG: u8 = ALL_FLAGS;

#[cfg(test)]
mod tests {
    use super::*;
    use andy_cpu::sem;
    use iced_x86::{Decoder, DecoderOptions};

    /// Writes down the instructions in `code`, placed at 0x1000, as one
    /// block.
    fn block(code: &[u8]) -> Block {
        let mut decoder = Decoder::with_ip(32, code, 0x1000, DecoderOptions::NONE);
        let mut emit = Emit::new();
        let (mut count, mut next) = (0, 0x1000);
        while decoder.can_decode() && !emit.ended() {
            let instruction = decoder.decode();
            emit.begin(count, instruction.ip() as u32);
            sem::step(&mut emit, &instruction);
            assert_eq!(emit.trapped(), None);
            (count, next) = (count + 1, instruction.next_ip() as u32);
        }
        let mut block = emit.finish(0x1000, count, next);
        block.falls_to(next);
        block
    }

    #[test]
    fn a_register_set_is_stored_and_remembered() {
        // mov eax, 5; add eax, ebx; mov ecx, eax
        let block = block(&[0xB8, 5, 0, 0, 0, 0x01, 0xD8, 0x89, 0xC1]);
        let text = block.rust("b", 0);
        assert_eq!(
            text,
            "pub fn b(cpu: &mut Cpu, mem: &mut Memory) -> Result<(), Fault> {\n    \
             cpu.regs[0] = 5u32;\n    \
             let t0 = cpu.regs[3];\n    \
             let t1 = 5u32.wrapping_add(t0);\n    \
             cpu.regs[0] = t1;\n    \
             cpu.regs[1] = t1;\n    \
             cpu.eip = 0x1009;\n    \
             cpu.instructions += 3;\n    \
             Ok(())\n}\n"
        );
        assert_eq!((block.instructions, block.exit), (3, Exit::Next(0x1009)));
        // The add sets six flags and reads none.
        assert_eq!((block.reads, block.sets), (0, EVERY_FLAG & !bit(Flag::Direction)));
        // With the zero flag wanted afterwards, it is computed and stored.
        let text = block.rust("b", bit(Flag::Zero));
        assert!(text.contains("cpu.flags[3] = ") && !text.contains("cpu.flags[0] = "), "{text}");
    }

    #[test]
    fn a_conditional_jump_reads_the_flags_set_before_it_in_the_block() {
        // cmp eax, 7; jne +2
        let block = block(&[0x83, 0xF8, 0x07, 0x75, 0x02]);
        assert_eq!(block.reads, 0);
        let Exit::Branch { taken, otherwise, .. } = block.exit else { panic!("{:?}", block.exit) };
        assert_eq!((taken, otherwise), (0x1007, 0x1005));
        assert_eq!(block.successors(), (vec![0x1007, 0x1005], false));
        let text = block.rust("b", 0);
        // The comparison is made, and no flag is stored.
        assert!(text.contains("cpu.eip = if t") && !text.contains("cpu.flags"), "{text}");
        // jne alone reads the zero flag as the machine has it.
        let alone = block_of(&[0x75, 0x02]);
        assert_eq!(alone.reads, bit(Flag::Zero));
        assert!(alone.rust("b", 0).contains("let t0 = cpu.flags[3];"));
    }

    fn block_of(code: &[u8]) -> Block {
        block(code)
    }

    #[test]
    fn memory_is_read_and_written_with_the_fault_of_its_instruction() {
        // mov eax, [ebx+8]; ret
        let block = block(&[0x8B, 0x43, 0x08, 0xC3]);
        let text = block.rust("b", 0);
        assert!(
            text.contains(
                "let t2 = match mem.read(t1, 4) { Some(v) => v, None => return native::fault(cpu, 0, 0x1000, \
                 Fault::Memory { address: t1, write: false }) };"
            ),
            "{text}"
        );
        // The return reads its address from the stack, as the second
        // instruction, and goes there.
        assert!(text.contains("native::fault(cpu, 1, 0x1003, Fault::Memory"), "{text}");
        assert_eq!(block.successors(), (Vec::new(), true));
        assert!(text.contains("cpu.instructions += 2;"));
    }

    #[test]
    fn a_repeated_instruction_is_a_loop_over_the_machines_own_state() {
        // cld; rep movsd; mov eax, ecx
        let block = block(&[0xFC, 0xF3, 0xA5, 0x89, 0xC8]);
        let text = block.rust("b", 0);
        // The direction flag set before the loop is stored for it.
        let stored = text.find("cpu.flags[5] = 0u32;").expect(&text);
        let looped = text.find("while cpu.regs[1] != 0 {").expect(&text);
        assert!(stored < looped, "{text}");
        assert!(text.contains("cpu.regs[1] -= 1;"));
        // After the loop the count is read anew.
        let after = &text[text.rfind("    }\n").unwrap()..];
        assert!(after.contains("= cpu.regs[1];"), "{text}");
        assert_eq!(block.instructions, 3);
    }
}
