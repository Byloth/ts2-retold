//! The machine the game runs on.
//!
//! [`Machine`] owns the CPU state, the guest address space and the state of
//! the replacement operating system. Imported functions are implemented in
//! [`win32`] against the guest's own calling convention (arguments on the
//! guest stack), so the same implementations serve the interpreter today and
//! recompiled code later.

pub mod files;
pub mod win32;

use std::collections::BTreeMap;

use andy_cpu::interp::{self, Cpu, Fault};
use andy_cpu::memory::{Memory, Unmapped};
use andy_recomp::pe::Image;
use iced_x86::Instruction;

/// Imports resolve to addresses in this unmapped range; reaching one means
/// "call the host function with this index".
const IMPORT_BASE: u32 = 0xFFF0_0000;
/// Returning here ends the program.
const EXIT_ADDRESS: u32 = 0xFFEF_FFF0;
const STACK_TOP: u32 = 0x0020_0000;
const STACK_SIZE: u32 = 0x0010_0000;
const TEB_ADDRESS: u32 = 0x7FFD_E000;
/// Speed of the virtual clock: a machine of roughly the game's era.
const INSTRUCTIONS_PER_MS: u64 = 50_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The program asked to exit.
    Exit(u32),
    Fault(Fault),
    /// An imported function that the runtime does not implement yet.
    UnimplementedImport {
        dll: String,
        name: String,
    },
    /// The instruction budget given to [`Machine::run`] ran out.
    Budget,
}

/// One call from the game into the operating system.
#[derive(Debug, Clone)]
pub struct ImportCall {
    pub name: &'static str,
    pub caller: u32,
    pub args: Vec<u32>,
    pub result: u32,
    pub detail: Option<String>,
}

struct ImportSlot {
    dll: String,
    name: String,
    function: Option<&'static win32::Function>,
}

pub struct Machine {
    pub cpu: Cpu,
    pub memory: Memory,
    pub os: win32::Os,
    imports: Vec<ImportSlot>,
    /// Decoded instructions of the code section, filled on first execution.
    code_start: u32,
    decoded_index: Vec<u32>,
    decoded: Vec<Instruction>,
    /// Every import call so far, when tracing is on.
    pub trace: Option<Vec<ImportCall>>,
    pub import_counts: BTreeMap<&'static str, u64>,
    /// Address of the first instruction the x87 model could not run exactly.
    pub first_inexact: Option<u32>,
}

impl Machine {
    /// Maps the image and prepares the first thread at the entry point.
    pub fn load(image: &Image, os: win32::Os) -> Machine {
        let mut memory = Memory::new(Unmapped::Fault);
        for section in &image.sections {
            memory.map(section.va, section.virtual_size);
            memory
                .load(section.va, &section.data)
                .expect("section was just mapped");
        }
        let mut imports = Vec::with_capacity(image.imports.len());
        for (index, import) in image.imports.iter().enumerate() {
            let name = import.name.to_string();
            let stub = IMPORT_BASE + index as u32 * 4;
            memory
                .write(import.iat_va, 4, stub)
                .expect("import table is inside a section");
            imports.push(ImportSlot {
                function: win32::lookup(&import.dll, &name),
                dll: import.dll.clone(),
                name,
            });
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
        Machine {
            cpu,
            memory,
            os,
            imports,
            code_start: code.va,
            decoded_index: vec![u32::MAX; code.virtual_size as usize],
            decoded: Vec::new(),
            trace: None,
            import_counts: BTreeMap::new(),
            first_inexact: None,
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

    fn call_import(&mut self, index: usize) -> Result<(), Stop> {
        let slot = &self.imports[index];
        let Some(function) = slot.function else {
            return Err(Stop::UnimplementedImport {
                dll: slot.dll.clone(),
                name: slot.name.clone(),
            });
        };
        let esp = self.cpu.regs[4];
        let read = |memory: &mut Memory, at: u32| {
            memory.read(at, 4).ok_or(Stop::Fault(Fault::Memory {
                address: at,
                write: false,
            }))
        };
        let return_address = read(&mut self.memory, esp)?;
        let mut args = Vec::with_capacity(function.args as usize);
        for n in 0..function.args as u32 {
            args.push(read(&mut self.memory, esp + 4 + 4 * n)?);
        }
        self.os.now_ms = (self.cpu.instructions / INSTRUCTIONS_PER_MS) as u32;
        let mut call = win32::Call {
            memory: &mut self.memory,
            os: &mut self.os,
            args: &args,
            detail: None,
        };
        let result = (function.handler)(&mut call);
        let detail = call.detail.take();
        if let Some(code) = self.os.exit_code {
            return Err(Stop::Exit(code));
        }
        *self.import_counts.entry(function.name).or_default() += 1;
        if let Some(trace) = &mut self.trace {
            trace.push(ImportCall {
                name: function.name,
                caller: return_address,
                args,
                result,
                detail,
            });
        }
        self.cpu.regs[0] = result;
        // stdcall: the callee removes its arguments.
        self.cpu.regs[4] = esp + 4 + 4 * function.args as u32;
        self.cpu.eip = return_address;
        Ok(())
    }

    /// Runs until the program stops or `budget` instructions have executed.
    pub fn run(&mut self, budget: u64) -> Stop {
        let end = self.cpu.instructions + budget;
        while self.cpu.instructions < end {
            let eip = self.cpu.eip;
            if eip >= IMPORT_BASE {
                if let Err(stop) = self.call_import(((eip - IMPORT_BASE) / 4) as usize) {
                    return stop;
                }
                continue;
            }
            if eip == EXIT_ADDRESS {
                return Stop::Exit(self.cpu.regs[0]);
            }
            let instruction = match self.instruction_at(eip) {
                Ok(instruction) => instruction,
                Err(fault) => return Stop::Fault(fault),
            };
            if let Err(fault) = self.cpu.execute(&mut self.memory, &instruction) {
                return Stop::Fault(fault);
            }
            if self.cpu.fpu.inexact_model && self.first_inexact.is_none() {
                self.first_inexact = Some(eip);
            }
        }
        Stop::Budget
    }
}
