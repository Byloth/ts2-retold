//! The interpreter: the backend that computes values immediately.
//!
//! It exists to bring the game up before the recompiler does, and to be the
//! reference that generated code is tested against.

use iced_x86::{Decoder, DecoderOptions, Instruction};

use crate::fpu::{FBin, FConst, FMem, FUn, Fpu};
use crate::memory::Memory;
use crate::native;
use crate::sem::{self, Backend, Bin, Flag, Rep, Size, Trap, Un};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Access to unmapped memory.
    Memory {
        address: u32,
        write: bool,
    },
    /// Division by zero or quotient overflow.
    Divide,
    Breakpoint,
    /// The instruction at `eip` is outside the supported subset.
    Unsupported,
    /// The bytes at `eip` are not a valid instruction.
    Invalid,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cpu {
    /// eax ecx edx ebx esp ebp esi edi
    pub regs: [u32; 8],
    pub eip: u32,
    /// One 0/1 value per [`Flag`], indexed by `Flag as usize`.
    pub flags: [u32; 7],
    pub fs_base: u32,
    pub fpu: Fpu,
    /// Instructions executed; also the time-stamp counter.
    pub instructions: u64,
}

impl Cpu {
    pub fn eflags(&self) -> u32 {
        Flag::ALL.iter().fold(2, |value, &flag| value | self.flags[flag as usize] << flag.bit())
    }

    pub fn set_eflags(&mut self, value: u32) {
        for flag in Flag::ALL {
            self.flags[flag as usize] = value >> flag.bit() & 1;
        }
    }

    /// Executes one already decoded instruction located at `self.eip`.
    ///
    /// On a fault `eip` still points at the instruction; registers and memory
    /// may hold part of its effect.
    pub fn execute(&mut self, memory: &mut Memory, instruction: &Instruction) -> Result<(), Fault> {
        let mut run = Run { next: instruction.next_ip() as u32, cpu: self, memory, fault: None };
        sem::step(&mut run, instruction);
        let (next, fault) = (run.next, run.fault);
        match fault {
            Some(fault) => Err(fault),
            None => {
                self.eip = next;
                self.instructions += 1;
                Ok(())
            }
        }
    }

    /// Decodes and executes the instruction at `self.eip`.
    pub fn step(&mut self, memory: &mut Memory) -> Result<(), Fault> {
        let instruction = decode(memory, self.eip).ok_or(Fault::Invalid)?;
        self.execute(memory, &instruction)
    }
}

/// Decodes the instruction at `address` straight from guest memory.
pub fn decode(memory: &mut Memory, address: u32) -> Option<Instruction> {
    let mut bytes = [0u8; 15];
    let mut len = 0;
    for (i, byte) in bytes.iter_mut().enumerate() {
        match memory.read_u8(address.wrapping_add(i as u32)) {
            Some(value) => *byte = value,
            None => break,
        }
        len = i + 1;
    }
    let instruction = Decoder::with_ip(32, &bytes[..len], address as u64, DecoderOptions::NONE).decode();
    (!instruction.is_invalid()).then_some(instruction)
}

struct Run<'a> {
    cpu: &'a mut Cpu,
    memory: &'a mut Memory,
    next: u32,
    fault: Option<Fault>,
}

impl Run<'_> {
    fn fail(&mut self, fault: Fault) {
        self.fault.get_or_insert(fault);
    }

    fn read(&mut self, address: u32, len: u32) -> u32 {
        match self.memory.read(address, len) {
            Some(value) => value,
            None => {
                self.fail(Fault::Memory { address, write: false });
                0
            }
        }
    }

    fn write(&mut self, address: u32, len: u32, value: u32) {
        // After a fault nothing more is written: the instruction is abandoned.
        if self.fault.is_none() && self.memory.write(address, len, value).is_none() {
            self.fail(Fault::Memory { address, write: true });
        }
    }
}

impl Backend for Run<'_> {
    type V = u32;
    type F = f64;

    #[inline]
    fn imm(&mut self, value: u32) -> u32 {
        value
    }

    #[inline]
    fn reg(&mut self, number: usize) -> u32 {
        self.cpu.regs[number]
    }

    #[inline]
    fn set_reg(&mut self, number: usize, value: u32) {
        self.cpu.regs[number] = value;
    }

    #[inline]
    fn flag(&mut self, flag: Flag) -> u32 {
        self.cpu.flags[flag as usize]
    }

    #[inline]
    fn set_flag(&mut self, flag: Flag, value: u32) {
        self.cpu.flags[flag as usize] = value;
    }

    fn fs_base(&mut self) -> u32 {
        self.cpu.fs_base
    }

    #[inline]
    fn load(&mut self, address: u32, size: Size) -> u32 {
        self.read(address, size.bytes())
    }

    #[inline]
    fn store(&mut self, address: u32, size: Size, value: u32) {
        self.write(address, size.bytes(), value);
    }

    #[inline]
    fn bin(&mut self, op: Bin, a: u32, b: u32) -> u32 {
        native::bin(op, a, b)
    }

    #[inline]
    fn un(&mut self, op: Un, a: u32) -> u32 {
        native::un(op, a)
    }

    #[inline]
    fn select(&mut self, condition: u32, then: u32, otherwise: u32) -> u32 {
        if condition != 0 { then } else { otherwise }
    }

    fn divide(&mut self, signed: bool, size: Size, high: u32, low: u32, divisor: u32) -> (u32, u32) {
        native::divide(signed, size, high, low, divisor).unwrap_or_else(|| {
            self.fail(Fault::Divide);
            (0, 0)
        })
    }

    fn repeat(&mut self, rep: Rep, mut body: impl FnMut(&mut Self)) {
        const ECX: usize = 1;
        while self.cpu.regs[ECX] != 0 && self.fault.is_none() {
            body(self);
            if self.fault.is_some() {
                break;
            }
            self.cpu.regs[ECX] -= 1;
            let zero = self.cpu.flags[Flag::Zero as usize] != 0;
            match rep {
                Rep::Count => {}
                Rep::WhileEqual if !zero => break,
                Rep::WhileNotEqual if zero => break,
                _ => {}
            }
        }
    }

    fn cpuid(&mut self) {
        native::cpuid(self.cpu);
    }

    fn rdtsc(&mut self) {
        // Deterministic: derived from the number of instructions executed.
        native::rdtsc(self.cpu, 0);
    }

    fn jump(&mut self, target: u32) {
        self.next = target;
    }

    fn jump_if(&mut self, condition: u32, target: u32) {
        if condition != 0 {
            self.next = target;
        }
    }

    fn call(&mut self, target: u32, return_address: u32) {
        let esp = self.cpu.regs[4].wrapping_sub(4);
        self.write(esp, 4, return_address);
        if self.fault.is_none() {
            self.cpu.regs[4] = esp;
            self.next = target;
        }
    }

    fn ret(&mut self, extra: u32) {
        let esp = self.cpu.regs[4];
        self.next = self.read(esp, 4);
        if self.fault.is_none() {
            self.cpu.regs[4] = esp.wrapping_add(4).wrapping_add(extra);
        }
    }

    fn trap(&mut self, trap: Trap) {
        self.fail(match trap {
            Trap::Breakpoint => Fault::Breakpoint,
            Trap::Unsupported => Fault::Unsupported,
        });
    }

    fn st(&mut self, i: u32) -> f64 {
        self.cpu.fpu.st(i)
    }

    fn set_st(&mut self, i: u32, value: f64) {
        self.cpu.fpu.set_st(i, value);
    }

    fn fpush(&mut self, value: f64) {
        self.cpu.fpu.push(value);
    }

    fn fpop(&mut self) {
        self.cpu.fpu.pop();
    }

    fn fconst(&mut self, constant: FConst) -> f64 {
        constant.value()
    }

    fn fload(&mut self, address: u32, format: FMem) -> f64 {
        // What cannot be read is the first byte of the number or one after
        // it; the fault names where the number begins.
        native::fload(self.memory, address, format).unwrap_or_else(|| {
            self.fail(Fault::Memory { address, write: false });
            0.0
        })
    }

    fn fstore(&mut self, address: u32, format: FMem, value: f64) {
        // After a fault nothing more is written: the instruction is abandoned.
        if self.fault.is_none() && native::fstore(self.cpu, self.memory, address, format, value).is_none() {
            self.fail(Fault::Memory { address, write: true });
        }
    }

    fn fbin(&mut self, op: FBin, a: f64, b: f64) -> f64 {
        self.cpu.fpu.binary(op, a, b)
    }

    fn fun(&mut self, op: FUn, a: f64) -> f64 {
        self.cpu.fpu.unary(op, a)
    }

    fn fcompare(&mut self, a: f64, b: f64) {
        self.cpu.fpu.compare(a, b);
    }

    fn fexamine(&mut self) {
        self.cpu.fpu.examine();
    }

    fn fremainder(&mut self, nearest: bool) {
        self.cpu.fpu.remainder(nearest);
    }

    fn fstatus(&mut self) -> u32 {
        self.cpu.fpu.status() as u32
    }

    fn fcontrol(&mut self) -> u32 {
        self.cpu.fpu.control as u32
    }

    fn set_fcontrol(&mut self, value: u32) {
        self.cpu.fpu.control = value as u16;
    }

    fn finit(&mut self) {
        self.cpu.fpu.init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Unmapped;

    const CODE: u32 = 0x1000;
    const STACK: u32 = 0x8000;

    fn run(code: &[u8], setup: impl FnOnce(&mut Cpu)) -> (Cpu, Memory) {
        let mut memory = Memory::new(Unmapped::Fault);
        memory.map(CODE, 0x1000);
        memory.map(STACK - 0x1000, 0x2000);
        memory.load(CODE, code).unwrap();
        let mut cpu = Cpu { eip: CODE, ..Cpu::default() };
        cpu.regs[4] = STACK;
        setup(&mut cpu);
        let end = CODE + code.len() as u32;
        while cpu.eip != end {
            cpu.step(&mut memory).unwrap();
        }
        (cpu, memory)
    }

    #[test]
    fn add_sets_carry_overflow_and_partial_registers() {
        // add al, 0x7F ; mov ah, al
        let (cpu, _) = run(&[0x04, 0x7F, 0x88, 0xC4], |cpu| cpu.regs[0] = 0xAABB_CC01);
        assert_eq!(cpu.regs[0], 0xAABB_8080);
        assert_eq!(cpu.eflags() & 0x8D5, 0x890); // OF, SF, AF; no CF, ZF, PF
    }

    #[test]
    fn rep_movsd_and_call_ret() {
        // call +1 ; (skipped) int3 ; rep movsd ; ret-less end
        #[rustfmt::skip]
        let code = [
            0xE8, 0x01, 0x00, 0x00, 0x00, // call 0x1006
            0xCC,
            0xF3, 0xA5,                   // rep movsd
        ];
        let (cpu, mut memory) = run(&code, |cpu| {
            cpu.regs[1] = 2;
            cpu.regs[6] = CODE;
            cpu.regs[7] = STACK;
        });
        assert_eq!(cpu.regs[4], STACK - 4);
        assert_eq!(memory.read(STACK - 4, 4), Some(CODE + 5));
        assert_eq!(memory.read(STACK, 4), Some(0x0000_01E8));
        assert_eq!((cpu.regs[1], cpu.regs[6], cpu.regs[7]), (0, CODE + 8, STACK + 8));
    }

    #[test]
    fn signed_division_and_its_faults() {
        // cdq ; idiv ecx
        let (cpu, _) = run(&[0x99, 0xF7, 0xF9], |cpu| {
            cpu.regs[0] = (-7i32) as u32;
            cpu.regs[1] = 2;
        });
        assert_eq!((cpu.regs[0] as i32, cpu.regs[2] as i32), (-3, -1));

        let mut memory = Memory::new(Unmapped::Fault);
        memory.map(CODE, 0x1000);
        memory.load(CODE, &[0xF7, 0xF9]).unwrap();
        let mut cpu = Cpu { eip: CODE, ..Cpu::default() };
        assert_eq!(cpu.step(&mut memory), Err(Fault::Divide));
        cpu.regs[0] = 0x8000_0000;
        cpu.regs[2] = 0xFFFF_FFFF;
        cpu.regs[1] = 0xFFFF_FFFF;
        assert_eq!(cpu.step(&mut memory), Err(Fault::Divide));
        assert_eq!(cpu.eip, CODE);
    }

    #[test]
    fn float_to_integer_the_way_the_runtime_does_it() {
        // fld dword [0x8000] ; fmul st0, st0 ; fistp dword [0x8004]
        #[rustfmt::skip]
        let code = [
            0xD9, 0x05, 0x00, 0x80, 0x00, 0x00,
            0xD8, 0xC8,
            0xDB, 0x1D, 0x04, 0x80, 0x00, 0x00,
        ];
        let mut memory = Memory::new(Unmapped::Fault);
        memory.map(CODE, 0x1000);
        memory.map(STACK, 0x1000);
        memory.load(CODE, &code).unwrap();
        memory.write(STACK, 4, 2.5f32.to_bits()).unwrap();
        let mut cpu = Cpu { eip: CODE, ..Cpu::default() };
        cpu.fpu.control = 0x0F7F; // truncate, as `_ftol` sets it
        for _ in 0..3 {
            cpu.step(&mut memory).unwrap();
        }
        assert_eq!(memory.read(STACK + 4, 4), Some(6));
        assert_eq!(cpu.fpu.depth(), 0);
    }

    #[test]
    fn unmapped_access_reports_the_address() {
        let mut memory = Memory::new(Unmapped::Fault);
        memory.map(CODE, 0x1000);
        memory.load(CODE, &[0xA1, 0x00, 0x00, 0x50, 0x00]).unwrap(); // mov eax, [0x500000]
        let mut cpu = Cpu { eip: CODE, ..Cpu::default() };
        assert_eq!(cpu.step(&mut memory), Err(Fault::Memory { address: 0x50_0000, write: false }));
    }
}
