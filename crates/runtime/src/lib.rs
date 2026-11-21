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

pub mod audio;
pub mod files;
pub mod game;
pub mod input;
pub mod keys;
pub mod mods;
pub mod os;
pub mod threads;
pub mod typeface;
pub mod video;
pub mod win32;

use andy_cpu::interp::{self, Cpu, Fault};
use andy_cpu::memory::{Memory, Unmapped};
use andy_recomp::pe::Image;
use iced_x86::Instruction;

use crate::game::Replacement;
use crate::os::Os;
use crate::threads::{Pause, Threads};
use crate::video::{Headless, Video};
use crate::win32::Function;

/// Host functions are reached through addresses in this unmapped range: the
/// function with index `n` lives at `HOST_BASE + 4 * n`.
const HOST_BASE: u32 = 0xFFF0_0000;
/// Returning here ends a call made by the host into the program.
const CALLBACK_RETURN: u32 = 0xFFFF_FFF0;
/// Returning here ends the program.
const EXIT_ADDRESS: u32 = 0xFFFF_FFE0;
/// Returning here ends a thread other than the first.
const THREAD_EXIT: u32 = 0xFFFF_FFD0;
const STACK_TOP: u32 = 0x0020_0000;
const STACK_SIZE: u32 = 0x0010_0000;
const TEB_ADDRESS: u32 = 0x7FFD_E000;
/// Speed of the virtual clock: a machine of roughly the game's era.
pub const INSTRUCTIONS_PER_MS: u64 = 50_000;
/// How many of the most recent host calls a trace is guaranteed to keep.
const TRACE_KEPT: usize = 20_000;
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
    /// Every thread is waiting for a time later than [`Machine::clock_limit`]
    /// allows the clock to reach. The run can be continued once the limit
    /// has been moved on.
    Idle,
    /// Execution reached [`Machine::breakpoint`].
    Breakpoint,
    /// Every thread is waiting for something only another thread could do.
    Deadlock,
    /// Internal: the running thread has to wait; the host call that asked is
    /// repeated when the thread runs again. Never returned by [`Machine::run`].
    WouldBlock,
}

/// One call from the program into a host function.
#[derive(Debug, Clone)]
pub struct HostCall {
    pub name: &'static str,
    /// Index of the calling thread; 0 is the program's first thread.
    pub thread: usize,
    pub caller: u32,
    /// Nesting level: 0 for calls made by the program's main flow, more for
    /// calls made from inside a callback.
    pub depth: u32,
    pub args: Vec<u32>,
    pub result: u32,
    pub note: Option<String>,
}

/// Marks, in the table of decoded instructions, an address not decoded yet.
const NOT_DECODED: u32 = u32::MAX;
/// Entries from this value up name a replaced function instead.
const FIRST_REPLACEMENT: u32 = 0xFFFF_0000;

enum Fetched {
    Instruction(Instruction),
    Replacement(&'static Replacement),
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
    /// Registers of the thread that is running.
    pub cpu: Cpu,
    pub memory: Memory,
    pub os: Os,
    /// What draws and shows the game's surfaces. Headless until replaced.
    pub video: Box<dyn Video>,
    /// The calls to the functions being listened to
    /// ([`Machine::record_calls_to`]): the function and the first ten
    /// words of its arguments.
    pub recorded_calls: Vec<(u32, [u32; 10])>,
    /// What receives the sound. Without one nothing is mixed.
    pub audio: Option<Box<dyn audio::Audio>>,
    pub threads: Threads,
    host: Vec<HostEntry>,
    /// Addresses already given to host functions, by the function's identity.
    host_addresses: std::collections::HashMap<usize, u32>,
    /// Calls made to each host function, parallel to `host`.
    call_counts: Vec<u64>,
    /// The time spent in host functions so far, in nanoseconds, if it is
    /// being measured (`Some`): for telling the machine's own work from
    /// the work done for it.
    pub host_nanos: Option<u64>,
    /// Decoded instructions of the code section, filled on first execution.
    code_start: u32,
    decoded_index: Vec<u32>,
    decoded: Vec<Instruction>,
    /// Functions of the game replaced by the modifications that are on.
    replacements: Vec<&'static Replacement>,
    /// Value of `cpu.instructions` at which execution stops.
    deadline: u64,
    callback_depth: u32,
    /// Every host call so far, when tracing is on.
    pub trace: Option<Vec<HostCall>>,
    /// Calls dropped from the start of `trace` to bound its size.
    pub trace_dropped: usize,
    note: Option<String>,
    /// Address of the first instruction the x87 model could not run exactly.
    pub first_inexact: Option<u32>,
    /// Stop before executing the instruction at this address. A debugging
    /// aid; the address is never a host function.
    pub breakpoint: Option<u32>,
    /// How far, in milliseconds, the clock may be moved forward while every
    /// thread waits. Without a limit a waiting machine skips straight to
    /// the next thing that happens, which is what a run as fast as possible
    /// wants; a run in step with real time sets the limit to the present
    /// and is told [`Stop::Idle`] when there is nothing to do before it.
    pub clock_limit: Option<u64>,
}

impl Machine {
    /// Maps the image and prepares the first thread at the entry point.
    pub fn load(image: &Image, os: Os) -> Machine {
        let mut memory = Memory::new(Unmapped::Fault);
        for section in &image.sections {
            memory.map(section.va, section.virtual_size);
            memory.load(section.va, &section.data).expect("section was just mapped");
        }
        memory.map(STACK_TOP - STACK_SIZE, STACK_SIZE);
        memory.map(TEB_ADDRESS, 0x1000);
        // Thread environment block: exception chain, stack limits, self pointer.
        for (offset, value) in
            [(0x00, u32::MAX), (0x04, STACK_TOP), (0x08, STACK_TOP - STACK_SIZE), (0x18, TEB_ADDRESS)]
        {
            memory.write(TEB_ADDRESS + offset, 4, value).expect("thread block was just mapped");
        }

        let mut cpu = Cpu { eip: image.entry, fs_base: TEB_ADDRESS, ..Cpu::default() };
        cpu.regs[4] = STACK_TOP - 0x100;
        memory.write(cpu.regs[4], 4, EXIT_ADDRESS).expect("stack was just mapped");

        let code = image.code_section().expect("an image with an entry point has a code section");
        let mut machine = Machine {
            cpu,
            memory,
            os,
            video: Box::new(Headless::default()),
            recorded_calls: Vec::new(),
            audio: None,
            threads: Threads::new(),
            host: Vec::new(),
            host_addresses: std::collections::HashMap::new(),
            call_counts: Vec::new(),
            host_nanos: None,
            code_start: code.va,
            decoded_index: vec![NOT_DECODED; code.virtual_size as usize],
            decoded: Vec::new(),
            replacements: Vec::new(),
            deadline: u64::MAX,
            callback_depth: 0,
            trace: None,
            trace_dropped: 0,
            note: None,
            first_inexact: None,
            breakpoint: None,
            clock_limit: None,
        };
        machine.os.resources = image.resources.clone();
        for replacement in game::REPLACEMENTS {
            let offset = (replacement.address)(&machine.os.addresses).wrapping_sub(code.va) as usize;
            // What is written for the reference build's code is for it alone.
            let fits = !replacement.reference_only || machine.os.addresses == game::Addresses::REFERENCE;
            if fits
                && machine.os.mods.is_enabled(replacement.modification)
                && offset < machine.decoded_index.len()
            {
                machine.decoded_index[offset] = FIRST_REPLACEMENT + machine.replacements.len() as u32;
                machine.replacements.push(replacement);
            }
        }
        for import in &image.imports {
            if !machine.os.modules.iter().any(|module| module.eq_ignore_ascii_case(&import.dll)) {
                machine.os.modules.push(import.dll.clone());
            }
            let name = import.name.to_string();
            let stub = match win32::lookup(&import.dll, &name) {
                Some(function) => machine.host_function_address(function),
                None => machine.add_host_entry(HostEntry::Missing { dll: import.dll.clone(), name }),
            };
            machine.memory.write(import.iat_va, 4, stub).expect("import table is inside a section");
        }
        machine
    }

    fn add_host_entry(&mut self, entry: HostEntry) -> u32 {
        let stub = HOST_BASE + 4 * self.host.len() as u32;
        self.host.push(entry);
        self.call_counts.push(0);
        stub
    }

    /// Makes `function` callable by the program and returns its address:
    /// the value for an import slot, a method table or `GetProcAddress`.
    pub fn host_function_address(&mut self, function: &'static Function) -> u32 {
        let identity = function as *const Function as usize;
        if let Some(&address) = self.host_addresses.get(&identity) {
            return address;
        }
        let address = self.add_host_entry(HostEntry::Function(function));
        self.host_addresses.insert(identity, address);
        address
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

    /// What is at `address`: an instruction, or a function of the game
    /// that a modification replaces.
    fn fetch(&mut self, address: u32) -> Result<Fetched, Fault> {
        let offset = address.wrapping_sub(self.code_start) as usize;
        match self.decoded_index.get(offset).copied() {
            Some(NOT_DECODED) => {
                let instruction = interp::decode(&mut self.memory, address).ok_or(Fault::Invalid)?;
                self.decoded_index[offset] = self.decoded.len() as u32;
                self.decoded.push(instruction);
                Ok(Fetched::Instruction(instruction))
            }
            Some(index) if index < FIRST_REPLACEMENT => {
                Ok(Fetched::Instruction(self.decoded[index as usize]))
            }
            Some(index) => Ok(Fetched::Replacement(self.replacements[(index - FIRST_REPLACEMENT) as usize])),
            // Outside the code section: decode every time.
            None => interp::decode(&mut self.memory, address).map(Fetched::Instruction).ok_or(Fault::Invalid),
        }
    }

    /// From now on a call to the function at `address` is not carried
    /// out: it is written down in [`Machine::recorded_calls`] and returns
    /// at once (as a C function, leaving its arguments). For finding out
    /// what a function of the game does with another: which sprites a
    /// text function draws.
    pub fn record_calls_to(&mut self, address: u32) {
        static RECORDER: Replacement = Replacement {
            modification: "",
            address: |_| 0,
            name: "recorded call",
            callee_pops: 0,
            resume: None,
            reference_only: false,
            function: |m| {
                let (function, esp) = (m.cpu.eip, m.cpu.regs[4]);
                let mut arguments = [0; 10];
                for (number, argument) in arguments.iter_mut().enumerate() {
                    *argument = m.memory.read(esp + 4 + 4 * number as u32, 4).unwrap_or(0);
                }
                m.recorded_calls.push((function, arguments));
                Ok(())
            },
        };
        let offset = address.wrapping_sub(self.code_start) as usize;
        if offset < self.decoded_index.len() {
            self.decoded_index[offset] = FIRST_REPLACEMENT + self.replacements.len() as u32;
            self.replacements.push(&RECORDER);
        }
    }

    /// Runs the native function that stands in for a function of the game,
    /// then returns to the game's caller.
    fn run_replacement(&mut self, replacement: &Replacement) -> Result<(), Stop> {
        (replacement.function)(self)?;
        // A stretch of code inside a function: the game goes on after it.
        if let Some(resume) = replacement.resume {
            self.cpu.eip = resume;
            return Ok(());
        }
        let esp = self.cpu.regs[4];
        self.cpu.eip = self.read_stack(esp)?;
        self.cpu.regs[4] = esp + 4 + replacement.callee_pops;
        Ok(())
    }

    fn read_stack(&mut self, address: u32) -> Result<u32, Stop> {
        self.memory.read(address, 4).ok_or(Stop::Fault(Fault::Memory { address, write: false }))
    }

    fn call_host(&mut self, index: usize) -> Result<(), Stop> {
        let function = match self.host.get(index) {
            Some(HostEntry::Function(function)) => *function,
            Some(HostEntry::Missing { dll, name }) => {
                return Err(Stop::Unimplemented { dll: dll.clone(), name: name.clone() });
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

        self.os.now_ms = self.clock_ms() as u32;
        self.call_counts[index] += 1;
        // The trace entry is reserved before the call so that calls made from
        // inside a callback appear after their parent, in execution order.
        let traced = self.trace.as_mut().map(|trace| {
            if trace.len() >= 2 * TRACE_KEPT {
                // Only the most recent calls are kept.
                trace.drain(..TRACE_KEPT);
                self.trace_dropped += TRACE_KEPT;
            }
            trace.push(HostCall {
                name: function.name,
                thread: self.threads.current(),
                caller: return_address,
                depth: self.callback_depth,
                args: args.to_vec(),
                result: 0,
                note: None,
            });
            // Position in the whole sequence of calls, dropped ones included.
            self.trace_dropped + trace.len() - 1
        });
        // A note belongs to the call that wrote it: the caller's is put
        // aside while this call, possibly made from inside a callback, runs.
        let outer_note = self.note.take();
        let timed = self.host_nanos.map(|_| std::time::Instant::now());
        let outcome = (function.handler)(self, args);
        if let (Some(started), Some(nanos)) = (timed, self.host_nanos.as_mut()) {
            *nanos += started.elapsed().as_nanos() as u64;
        }
        let result = match outcome {
            Ok(result) => result,
            Err(stop) => {
                if stop == Stop::WouldBlock {
                    // The call has not happened yet: it is made again, from
                    // the start, when the thread's wait is over.
                    self.call_counts[index] -= 1;
                    if let (Some(position), Some(trace)) = (traced, self.trace.as_mut()) {
                        trace.truncate(position.saturating_sub(self.trace_dropped));
                    }
                }
                self.note = outer_note;
                return Err(stop);
            }
        };
        let dropped = self.trace_dropped;
        if let Some(call) = traced
            .and_then(|position| position.checked_sub(dropped))
            .and_then(|index| self.trace.as_mut()?.get_mut(index))
        {
            call.result = result;
            call.note = self.note.take();
        }
        self.note = outer_note;

        self.cpu.regs[0] = result;
        // stdcall: the callee removes its arguments.
        self.cpu.regs[4] = esp + 4 + 4 * count as u32;
        self.cpu.eip = return_address;
        Ok(())
    }

    /// Runs the current thread until it returns from a callback (when
    /// `in_callback`), has to let another thread run, or the machine stops.
    pub(crate) fn execute(&mut self, in_callback: bool) -> Result<Pause, Stop> {
        loop {
            let eip = self.cpu.eip;
            if eip >= HOST_BASE {
                match eip {
                    CALLBACK_RETURN if in_callback => return Ok(Pause::CallbackReturned),
                    EXIT_ADDRESS => return Err(Stop::Exit(self.cpu.regs[0])),
                    THREAD_EXIT if !in_callback => {
                        self.finish_current_thread();
                        return Ok(Pause::Yield);
                    }
                    _ => match self.call_host(((eip - HOST_BASE) / 4) as usize) {
                        Ok(()) => {}
                        Err(Stop::WouldBlock) if !in_callback => return Ok(Pause::Yield),
                        Err(stop) => return Err(stop),
                    },
                }
                continue;
            }
            if self.cpu.instructions >= self.threads.slice_end {
                // The budget ends a run only between a thread's own
                // instructions, where the run can be taken up again; inside
                // a callback it waits for the callback to return.
                if !in_callback && self.cpu.instructions >= self.deadline {
                    return Err(Stop::Budget);
                }
                // A thread inside a callback keeps running: its state cannot
                // be set aside.
                if !in_callback && self.threads.count() > 1 {
                    return Ok(Pause::Yield);
                }
                self.threads.slice_end = self.cpu.instructions + threads::TIME_SLICE;
            }
            if self.breakpoint == Some(eip) {
                return Err(Stop::Breakpoint);
            }
            let instruction = match self.fetch(eip).map_err(Stop::Fault)? {
                Fetched::Instruction(instruction) => instruction,
                Fetched::Replacement(replacement) => {
                    self.run_replacement(replacement)?;
                    continue;
                }
            };
            self.cpu.execute(&mut self.memory, &instruction).map_err(Stop::Fault)?;
            if self.cpu.fpu.inexact_model && self.first_inexact.is_none() {
                self.first_inexact = Some(eip);
            }
        }
    }

    /// Runs until the program stops or `budget` more instructions have
    /// executed. After [`Stop::Budget`] the run can be continued by calling
    /// this again.
    pub fn run(&mut self, budget: u64) -> Stop {
        self.deadline = self.cpu.instructions.saturating_add(budget);
        self.run_threads()
    }

    /// The machine's clock, in milliseconds since it started.
    pub fn clock(&self) -> u64 {
        self.clock_ms()
    }

    /// Calls a function of the program with the `stdcall` convention and
    /// returns its result. Used by host functions for window procedures and
    /// enumeration callbacks.
    pub fn call_guest(&mut self, address: u32, args: &[u32]) -> Result<u32, Stop> {
        self.call_guest_leaving(address, args, 0)
    }

    /// Calls a function of the program with the C convention, which leaves
    /// its arguments on the stack for the caller to remove.
    pub fn call_guest_c(&mut self, address: u32, args: &[u32]) -> Result<u32, Stop> {
        self.call_guest_leaving(address, args, 4 * args.len() as u32)
    }

    /// Calls a function that leaves `left` bytes of its arguments on the
    /// stack when it returns.
    fn call_guest_leaving(&mut self, address: u32, args: &[u32], left: u32) -> Result<u32, Stop> {
        let (saved_eip, base) = (self.cpu.eip, self.cpu.regs[4]);
        let mut esp = base;
        for &value in args.iter().rev().chain([&CALLBACK_RETURN]) {
            esp -= 4;
            self.memory
                .write(esp, 4, value)
                .ok_or(Stop::Fault(Fault::Memory { address: esp, write: true }))?;
        }
        self.cpu.regs[4] = esp;
        self.cpu.eip = address;
        self.callback_depth += 1;
        let outcome = self.execute(true);
        self.callback_depth -= 1;
        match outcome? {
            Pause::CallbackReturned => {}
            Pause::Yield => unreachable!("a thread inside a callback does not yield"),
        }
        if self.cpu.regs[4] != base - left {
            return Err(Stop::CallbackStack { address, expected: base - left, actual: self.cpu.regs[4] });
        }
        self.cpu.regs[4] = base;
        self.cpu.eip = saved_eip;
        Ok(self.cpu.regs[0])
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use andy_recomp::pe::{Image, SCN_MEM_EXECUTE, Section};

    use super::*;
    use crate::files::Files;

    /// A machine with an empty program, for exercising host functions.
    pub fn machine() -> Machine {
        let image = Image {
            base: 0x40_0000,
            entry: 0x40_1000,
            sections: vec![Section {
                name: ".text".into(),
                va: 0x40_1000,
                virtual_size: 0x1000,
                flags: SCN_MEM_EXECUTE,
                data: vec![0xC3],
            }],
            ..Image::default()
        };
        Machine::load(&image, Os::new(Files::new(None, None)))
    }

    impl Machine {
        /// Calls a host function by name with the given arguments.
        pub fn host(&mut self, dll: &str, name: &str, args: &[u32]) -> u32 {
            let function = win32::lookup(dll, name).expect("host function exists");
            assert_eq!(function.args as usize, args.len(), "{name} argument count");
            (function.handler)(self, args).expect("host function does not stop the machine")
        }

        /// Calls method number `slot` of a COM object the runtime created.
        pub fn method(&mut self, object: u32, slot: u32, args: &[u32]) -> u32 {
            let table = self.memory.read(object, 4).expect("an interface pointer");
            let stub = self.memory.read(table + 4 * slot, 4).expect("a method table");
            let function: &'static Function = match &self.host[((stub - HOST_BASE) / 4) as usize] {
                HostEntry::Function(function) => function,
                HostEntry::Missing { .. } => panic!("slot {slot} is not a host function"),
            };
            let all: Vec<u32> = std::iter::once(object).chain(args.iter().copied()).collect();
            assert_eq!(function.args as usize, all.len(), "{} argument count", function.name);
            (function.handler)(self, &all)
                .unwrap_or_else(|stop| panic!("{} stopped: {stop:?}", function.name))
        }

        /// Scratch memory for a test's structures.
        pub fn scratch(&mut self, size: u32) -> u32 {
            self.os.allocate(&mut self.memory, size)
        }
    }
}
