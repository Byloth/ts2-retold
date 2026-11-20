//! Replacement for the Windows API functions the game calls.
//!
//! Functions are added as execution reaches them; an import without an
//! implementation stops the machine and names itself. Each handler receives
//! its arguments as raw 32-bit values read from the guest stack, in
//! declaration order, and returns the value for `eax`.
//!
//! The machine presents itself as Windows 98, the system the game was
//! released for (ADR 0011).

mod advapi32;
pub mod amstream;
pub mod com;
pub mod d3d;
pub mod ddraw;
pub mod dinput;
pub mod dsound;
pub mod gdi32;
pub(crate) mod kernel32;
mod ole32;
pub mod text;
pub mod user32;
pub mod winmm;

use crate::{Machine, Stop};

pub type HostResult = Result<u32, Stop>;
pub type Handler = fn(&mut Machine, &[u32]) -> HostResult;

pub struct Function {
    pub dll: &'static str,
    pub name: &'static str,
    /// Number of 32-bit arguments the function removes from the stack.
    pub args: u8,
    pub handler: Handler,
}

/// What a handler may return: a plain value, or a result when it can stop
/// the machine (by calling back into the program, or by ending it).
pub trait IntoHostResult {
    fn into_host_result(self) -> HostResult;
}

impl IntoHostResult for u32 {
    fn into_host_result(self) -> HostResult {
        Ok(self)
    }
}

impl IntoHostResult for HostResult {
    fn into_host_result(self) -> HostResult {
        self
    }
}

/// Gives a handler written as a closure its argument types.
#[inline(always)]
pub(crate) fn invoke<R: IntoHostResult>(
    handler: impl Fn(&mut Machine, &[u32]) -> R,
    machine: &mut Machine,
    args: &[u32],
) -> HostResult {
    handler(machine, args).into_host_result()
}

/// Declares the functions of one DLL as a table of `Name(argument count) => handler`.
macro_rules! functions {
    ($dll:literal; $($name:ident($args:literal) => $handler:expr,)*) => {
        pub(super) static FUNCTIONS: &[$crate::win32::Function] = &[$(
            $crate::win32::Function {
                dll: $dll,
                name: stringify!($name),
                args: $args,
                handler: {
                    fn handler(m: &mut $crate::Machine, a: &[u32]) -> $crate::win32::HostResult {
                        $crate::win32::invoke($handler, m, a)
                    }
                    handler
                },
            },
        )*];
    };
}
pub(crate) use functions;

/// Names of the functions a DLL exports only by number.
fn ordinal_name(dll: &str, name: &str) -> Option<&'static str> {
    match (dll.to_ascii_uppercase().as_str(), name) {
        ("DSOUND.DLL", "#1") => Some("DirectSoundCreate"),
        ("DSOUND.DLL", "#2") => Some("DirectSoundEnumerateA"),
        _ => None,
    }
}

pub fn lookup(dll: &str, name: &str) -> Option<&'static Function> {
    let name = ordinal_name(dll, name).unwrap_or(name);
    [
        kernel32::FUNCTIONS,
        ddraw::FUNCTIONS,
        dsound::FUNCTIONS,
        dinput::FUNCTIONS,
        advapi32::FUNCTIONS,
        user32::FUNCTIONS,
        gdi32::FUNCTIONS,
        winmm::FUNCTIONS,
        ole32::FUNCTIONS,
    ]
    .into_iter()
    .flatten()
    .find(|f| f.name == name && f.dll.eq_ignore_ascii_case(dll))
}

pub(crate) const ERROR_FILE_NOT_FOUND: u32 = 2;
pub(crate) const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;

/// Reading and writing the program's data from host functions.
impl Machine {
    /// A NUL-terminated string, for display.
    pub(crate) fn text(&mut self, address: u32) -> String {
        self.memory.c_string(address, 4096).unwrap_or_else(|| format!("<bad pointer {address:#x}>"))
    }

    /// The bytes of a NUL-terminated string, terminator excluded.
    pub(crate) fn bytes(&mut self, address: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Some(byte) = self.memory.read_u8(address.wrapping_add(bytes.len() as u32)) {
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }
        bytes
    }

    /// Reads consecutive 32-bit words; unreadable ones read as zero.
    pub(crate) fn read_words<const N: usize>(&mut self, address: u32) -> [u32; N] {
        std::array::from_fn(|n| self.memory.read(address + 4 * n as u32, 4).unwrap_or(0))
    }

    /// Writes consecutive 32-bit words; false if any of them is unmapped.
    pub(crate) fn write_words(&mut self, address: u32, words: &[u32]) -> bool {
        words
            .iter()
            .enumerate()
            .all(|(n, &word)| self.memory.write(address + 4 * n as u32, 4, word).is_some())
    }

    /// Runs `call` with a block of 32-bit words on the program's stack, the
    /// way Windows passes a structure that exists only for the duration of
    /// a callback. `call` receives the block's address.
    pub(crate) fn with_stack_words<T>(
        &mut self,
        words: &[u32],
        call: impl FnOnce(&mut Machine, u32) -> T,
    ) -> T {
        let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        self.with_stack_bytes(&bytes, call)
    }

    /// As [`Machine::with_stack_words`], for a block of bytes.
    pub(crate) fn with_stack_bytes<T>(
        &mut self,
        bytes: &[u8],
        call: impl FnOnce(&mut Machine, u32) -> T,
    ) -> T {
        let saved = self.cpu.regs[4];
        let at = (saved - bytes.len() as u32) & !3;
        self.memory.load(at, bytes).expect("the stack is mapped");
        self.cpu.regs[4] = at;
        let result = call(self, at);
        self.cpu.regs[4] = saved;
        result
    }

    /// Writes a string and its terminator, truncating to `capacity` bytes.
    /// Returns the number of characters written, terminator excluded.
    pub(crate) fn put_string(&mut self, address: u32, capacity: u32, text: &[u8]) -> u32 {
        if capacity == 0 {
            return 0;
        }
        let len = text.len().min(capacity as usize - 1);
        let ok = self.memory.load(address, &text[..len]).is_some()
            && self.memory.load(address + len as u32, &[0]).is_some();
        if ok { len as u32 } else { 0 }
    }
}
