//! The machine the game runs on.
//!
//! [`Machine`] owns the CPU state, the guest address space and the state of
//! the replacement operating system ([`os::Os`]). Everything the program
//! calls outside its own code is a *host function*: an entry in a table,
//! reached through a stub address that is never mapped. Imported functions
//! and the methods of COM objects are both host functions, implemented in
//! [`win32`] against the guest's own calling convention (arguments on the
//! guest stack). The same implementations therefore serve the interpreter
//! today and recompiled code later.
//!
//! Host functions can call back into the program ([`Machine::call_guest`]),
//! which is how window procedures and enumeration callbacks run.

pub mod files;
pub mod os;
pub mod win32;

use andy_cpu::interp::{self, Cpu, Fault};
use andy_cpu::memory::{Memory, Unmapped};
use andy_recomp::pe::Image;
use iced_x86::Instruction;

use crate::os::Os;
use crate::win32::Function;

/// Host functions are reached through addresses in this unmapped range: the
/// function with index `n` lives at `HOST_BASE + 4 * n`.
const HOST_BASE: u32 = 0xFFF0_0000;
/// Returning here ends a call made by the host into the program.
const CALLBACK_RETURN: u32 = 0xFFFF_FFF0;
/// Returning here ends the program.
const EXIT_ADDRESS: u32 = 0xFFFF_FFE0;
const STACK_TOP: u32 = 0x0020_0000;
const STACK_SIZE: u32 = 0x0010_0000;
const TEB_ADDRESS: u32 = 0x7FFD_E000;
/// Speed of the virtual clock: a machine of roughly the game's era.
const INSTRUCTIONS_PER_MS: u64 = 50_000;
/// No host function takes more arguments than this.
const MAX_ARGS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The program asked to exit.
    Exit(u32),
    Fault(Fault),
    /// A function the runtime does not implement yet.
    Unimplemented {
        dll: String,
        name: String,
    },
    /// A callback returned with the stack pointer in the wrong place: the
    /// runtime called it with the wrong number of arguments.
    CallbackStack {
        address: u32,
        expected: u32,
        actual: u32,
    },
    /// The instruction budget given to [`Machine::run`] ran out.
    Budget,
}

/// One call from the program into a host function.
#[derive(Debug, Clone)]
pub struct HostCall {
    pub name: &'static str,
    pub caller: u32,
    /// Nesting level: 0 for calls made by the program's main flow, more for
    /// calls made from inside a callback.
    pub depth: u32,
    pub args: Vec<u32>,
    pub result: u32,
    pub note: Option<String>,
}

enum HostEntry {
    Function(&'static Function),
    /// An import the runtime has no implementation for.
    Missing {
        dll: String,
        name: String,
    },
}

pub struct Machine {
    pub cpu: Cpu,
    pub memory: Memory,
    pub os: Os,
    host: Vec<HostEntry>,
    /// Calls made to each host function, parallel to `host`.
    call_counts: Vec<u64>,
    /// Decoded instructions of the code section, filled on first execution.
    code_start: u32,
    decoded_index: Vec<u32>,
    decoded: Vec<Instruction>,
    /// Value of `cpu.instructions` at which execution stops.
    deadline: u64,
    callback_depth: u32,
    /// Every host call so far, when tracing is on.
    pub trace: Option<Vec<HostCall>>,
    note: Option<String>,
    /// Address of the first instruction the x87 model could not run exactly.
    pub first_inexact: Option<u32>,
}

impl Machine {
    /// Maps the image and prepares the first thread at the entry point.
    pub fn load(image: &Image, os: Os) -> Machine {
        let mut memory = Memory::new(Unmapped::Fault);
        for section in &image.sections {
            memory.map(section.va, section.virtual_size);
            memory
                .load(section.va, &section.data)
                .expect("section was just mapped");
        }
        memory.map(STACK_TOP - STACK_SIZE, STACK_SIZE);
        memory.map(TEB_ADDRESS, 0x1000);
        // Thread environment block: exception chain, stack limits, self pointer.
        for (offset, value) in [
            (0x00, u32::MAX),
            (0x04, STACK_TOP),
            (0x08, STACK_TOP - STACK_SIZE),
            (0x18, TEB_ADDRESS),
        ] {
            memory
                .write(TEB_ADDRESS + offset, 4, value)
                .expect("thread block was just mapped");
        }

        let mut cpu = Cpu {
            eip: image.entry,
            fs_base: TEB_ADDRESS,
            ..Cpu::default()
        };
        cpu.regs[4] = STACK_TOP - 0x100;
        memory
            .write(cpu.regs[4], 4, EXIT_ADDRESS)
            .expect("stack was just mapped");

        let code = image
            .code_section()
            .expect("an image with an entry point has a code section");
        let mut machine = Machine {
            cpu,
            memory,
            os,
            host: Vec::new(),
            call_counts: Vec::new(),
            code_start: code.va,
            decoded_index: vec![u32::MAX; code.virtual_size as usize],
            decoded: Vec::new(),
            deadline: u64::MAX,
            callback_depth: 0,
            trace: None,
            note: None,
            first_inexact: None,
        };
        machine.os.resources = image.resources.clone();
        for import in &image.imports {
            let name = import.name.to_string();
            let entry = match win32::lookup(&import.dll, &name) {
                Some(function) => HostEntry::Function(function),
                None => HostEntry::Missing {
                    dll: import.dll.clone(),
                    name,
                },
            };
            let stub = machine.add_host_entry(entry);
            machine
                .memory
                .write(import.iat_va, 4, stub)
                .expect("import table is inside a section");
        }
        machine
    }

    fn add_host_entry(&mut self, entry: HostEntry) -> u32 {
        let stub = HOST_BASE + 4 * self.host.len() as u32;
        self.host.push(entry);
        self.call_counts.push(0);
        stub
    }

    /// Makes `function` callable by the program and returns its address, for
    /// use in tables the runtime builds itself (COM method tables).
    pub fn host_function_address(&mut self, function: &'static Function) -> u32 {
        self.add_host_entry(HostEntry::Function(function))
    }

    /// Host functions called so far, with their call counts.
    pub fn call_counts(&self) -> Vec<(&'static str, u64)> {
        let mut counts = std::collections::BTreeMap::<&'static str, u64>::new();
        for (entry, &count) in self.host.iter().zip(&self.call_counts) {
            if let (HostEntry::Function(function), true) = (entry, count > 0) {
                *counts.entry(function.name).or_default() += count;
            }
        }
        counts.into_iter().collect()
    }

    /// Attaches a note to the host call being traced. The closure only runs
    /// when tracing is on, so callers can format freely.
    pub fn note(&mut self, text: impl FnOnce() -> String) {
        if self.trace.is_some() {
            self.note = Some(text());
        }
    }

    fn instruction_at(&mut self, address: u32) -> Result<Instruction, Fault> {
        let offset = address.wrapping_sub(self.code_start) as usize;
        match self.decoded_index.get(offset).copied() {
            Some(u32::MAX) => {
                let instruction = interp::decode(&mut self.memory, address).ok_or(Fault::Invalid)?;
                self.decoded_index[offset] = self.decoded.len() as u32;
                self.decoded.push(instruction);
                Ok(instruction)
            }
            Some(index) => Ok(self.decoded[index as usize]),
            // Outside the code section: decode every time.
            None => interp::decode(&mut self.memory, address).ok_or(Fault::Invalid),
        }
    }

    fn read_stack(&mut self, address: u32) -> Result<u32, Stop> {
        self.memory.read(address, 4).ok_or(Stop::Fault(Fault::Memory {
            address,
            write: false,
        }))
    }

    fn call_host(&mut self, index: usize) -> Result<(), Stop> {
        let function = match self.host.get(index) {
            Some(HostEntry::Function(function)) => *function,
            Some(HostEntry::Missing { dll, name }) => {
                return Err(Stop::Unimplemented {
                    dll: dll.clone(),
                    name: name.clone(),
                });
            }
            None => return Err(Stop::Fault(Fault::Invalid)),
        };
        let esp = self.cpu.regs[4];
        let return_address = self.read_stack(esp)?;
        let count = function.args as usize;
        let mut args = [0u32; MAX_ARGS];
        for (n, arg) in args[..count].iter_mut().enumerate() {
            *arg = self.read_stack(esp + 4 + 4 * n as u32)?;
        }
        let args = &args[..count];

        self.os.now_ms = (self.cpu.instructions / INSTRUCTIONS_PER_MS) as u32;
        self.call_counts[index] += 1;
        // The trace entry is reserved before the call so that calls made from
        // inside a callback appear after their parent, in execution order.
        let traced = self.trace.as_mut().map(|trace| {
            trace.push(HostCall {
                name: function.name,
                caller: return_address,
                depth: self.callback_depth,
                args: args.to_vec(),
                result: 0,
                note: None,
            });
            trace.len() - 1
        });
        self.note = None;
        let result = (function.handler)(self, args)?;
        if let (Some(index), Some(trace)) = (traced, self.trace.as_mut()) {
            trace[index].result = result;
            trace[index].note = self.note.take();
        }

        self.cpu.regs[0] = result;
        // stdcall: the callee removes its arguments.
        self.cpu.regs[4] = esp + 4 + 4 * count as u32;
        self.cpu.eip = return_address;
        Ok(())
    }

    /// Executes until the program returns to `CALLBACK_RETURN` (when
    /// `in_callback`) or stops.
    fn execute(&mut self, in_callback: bool) -> Result<(), Stop> {
        loop {
            let eip = self.cpu.eip;
            if eip >= HOST_BASE {
                match eip {
                    CALLBACK_RETURN if in_callback => return Ok(()),
                    EXIT_ADDRESS => return Err(Stop::Exit(self.cpu.regs[0])),
                    _ => self.call_host(((eip - HOST_BASE) / 4) as usize)?,
                }
                continue;
            }
            if self.cpu.instructions >= self.deadline {
                return Err(Stop::Budget);
            }
            let instruction = self.instruction_at(eip).map_err(Stop::Fault)?;
            self.cpu
                .execute(&mut self.memory, &instruction)
                .map_err(Stop::Fault)?;
            if self.cpu.fpu.inexact_model && self.first_inexact.is_none() {
                self.first_inexact = Some(eip);
            }
        }
    }

    /// Runs until the program stops or `budget` more instructions have executed.
    pub fn run(&mut self, budget: u64) -> Stop {
        self.deadline = self.cpu.instructions.saturating_add(budget);
        match self.execute(false) {
            Err(stop) => stop,
            Ok(()) => unreachable!("only a callback returns normally"),
        }
    }

    /// Calls a function of the program with the `stdcall` convention and
    /// returns its result. Used by host functions for window procedures and
    /// enumeration callbacks.
    pub fn call_guest(&mut self, address: u32, args: &[u32]) -> Result<u32, Stop> {
        let (saved_eip, base) = (self.cpu.eip, self.cpu.regs[4]);
        let mut esp = base;
        for &value in args.iter().rev().chain([&CALLBACK_RETURN]) {
            esp -= 4;
            self.memory
                .write(esp, 4, value)
                .ok_or(Stop::Fault(Fault::Memory {
                    address: esp,
                    write: true,
                }))?;
        }
        self.cpu.regs[4] = esp;
        self.cpu.eip = address;
        self.callback_depth += 1;
        let outcome = self.execute(true);
        self.callback_depth -= 1;
        outcome?;
        if self.cpu.regs[4] != base {
            return Err(Stop::CallbackStack {
                address,
                expected: base,
                actual: self.cpu.regs[4],
            });
        }
        self.cpu.eip = saved_eip;
        Ok(self.cpu.regs[0])
    }
}
