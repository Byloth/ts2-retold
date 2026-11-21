//! State of the replacement operating system.

use std::collections::HashMap;

use andy_cpu::memory::Memory;

use andy_recomp::pe::Resource;

use crate::files::{Files, INSTALL_DIR};
use crate::mods::Modifications;
use crate::win32::com::Com;
use crate::win32::gdi32::Gdi;
use crate::win32::user32::User;
use crate::win32::{amstream, ddraw, dinput, dsound, winmm};

const HEAP_START: u32 = 0x1000_0000;
const HEAP_LIMIT: u32 = 0x4000_0000;
const VIRTUAL_START: u32 = 0x4000_0000;
const VIRTUAL_LIMIT: u32 = 0x7000_0000;
/// Size of the bookkeeping word in front of every heap block.
pub(crate) const HEAP_HEADER: u32 = 8;
/// Registry key written by the game's installer.
pub(crate) const GAME_KEY: &str = "hklm\\software\\travellerstalestoystory2";

#[derive(Debug, Default)]
pub struct Os {
    heap_next: u32,
    virtual_next: u32,
    /// Texts the runtime hands to the game again and again, by where
    /// each was put ([`Os::kept_string`]).
    kept_strings: std::collections::HashMap<Vec<u8>, u32>,
    /// Guest addresses of strings handed out once and kept alive.
    pub(crate) command_line: u32,
    /// What follows the program name on its command line.
    pub command_line_arguments: String,
    pub(crate) environment: u32,
    pub last_error: u32,
    /// Text the program sent to the debugger.
    pub debug_output: Vec<String>,
    pub files: Files,
    /// Milliseconds since start, set by the machine before each host call.
    /// The clock is a function of the instructions executed, so runs repeat.
    pub now_ms: u32,
    /// Names of the DLLs the executable imports from, which are therefore
    /// loaded. A module's handle encodes its position in this list.
    pub modules: Vec<String>,
    /// The executable's resources (their data is in guest memory).
    pub resources: Vec<Resource>,
    pub user: User,
    pub gdi: Gdi,
    /// The modifications switched on for this run.
    pub mods: Modifications,
    /// Where the things the runtime knows are in this build of the game.
    pub addresses: crate::game::Addresses,
    pub(crate) com: Com,
    pub ddraw: ddraw::State,
    pub(crate) dsound: dsound::State,
    pub(crate) dinput: dinput::State,
    pub(crate) amstream: amstream::State,
    /// What the keyboard and mouse are doing; written by the embedder.
    pub input: dinput::Input,
    /// A recording of input being played back, if any.
    pub recording: Option<crate::input::Recording>,
    /// A recording of input being made, if any.
    pub recorder: Option<crate::input::Recorder>,
    /// How many times the program has read the keyboard: the clock of
    /// recorded input.
    pub keyboard_reads: u64,
    pub(crate) mmio: winmm::State,
    pub(crate) movable_blocks: Vec<crate::win32::kernel32::MovableBlock>,
    /// Open registry keys: handle to lower-case path.
    pub(crate) keys: HashMap<u32, String>,
}

impl Os {
    pub fn new(files: Files) -> Os {
        Os { heap_next: HEAP_START, virtual_next: VIRTUAL_START, files, ..Os::default() }
    }

    /// The handle of the process heap.
    pub(crate) fn heap_handle() -> u32 {
        HEAP_START
    }

    /// The registry as the installer leaves it, confirmed on an installed
    /// copy: `path` is the `data` directory (levels, `sfx\`, `cd.txt`) and
    /// `cdpath` the directory above it (`validate.tta`).
    pub(crate) fn registry_value(key: &str, value: &str) -> Option<String> {
        match (key, value.to_ascii_lowercase().as_str()) {
            (GAME_KEY, "path") => Some(format!("{INSTALL_DIR}\\data\\")),
            (GAME_KEY, "cdpath") => Some(format!("{INSTALL_DIR}\\")),
            _ => None,
        }
    }

    /// Allocates `size` bytes of zeroed guest memory that are never reused.
    pub fn allocate(&mut self, memory: &mut Memory, size: u32) -> u32 {
        let total = size.saturating_add(HEAP_HEADER).saturating_add(15) & !15;
        if total < size || self.heap_next.saturating_add(total) > HEAP_LIMIT {
            return 0;
        }
        let block = self.heap_next + HEAP_HEADER;
        memory.map(self.heap_next, total);
        memory.write(self.heap_next, 4, size).expect("block was just mapped");
        self.heap_next += total;
        block
    }

    pub(crate) fn block_size(&self, memory: &mut Memory, block: u32) -> Option<u32> {
        (HEAP_START + HEAP_HEADER..self.heap_next)
            .contains(&block)
            .then(|| memory.read(block - HEAP_HEADER, 4))?
    }

    /// A NUL-terminated copy of `text` in guest memory that is made once
    /// and kept: for texts the runtime hands to the game again and again.
    pub fn kept_string(&mut self, memory: &mut Memory, text: &[u8]) -> u32 {
        if let Some(&at) = self.kept_strings.get(text) {
            return at;
        }
        let at = self.string(memory, text);
        self.kept_strings.insert(text.to_vec(), at);
        at
    }

    /// Stores a NUL-terminated copy of `text` in guest memory.
    pub fn string(&mut self, memory: &mut Memory, text: &[u8]) -> u32 {
        let at = self.allocate(memory, text.len() as u32 + 1);
        memory.load(at, text).expect("block was just mapped");
        at
    }

    /// Reserves `size` bytes of address space, aligned to 64 KiB as on
    /// Windows, and maps them.
    pub(crate) fn reserve(&mut self, memory: &mut Memory, size: u32) -> u32 {
        let start = self.virtual_next;
        let Some(next) = start.checked_add(size.saturating_add(0xFFFF) & !0xFFFF) else {
            return 0;
        };
        if next > VIRTUAL_LIMIT {
            return 0;
        }
        self.virtual_next = next;
        // Reserved-only pages are mapped too: it costs nothing here and no
        // program relies on touching them to fault.
        memory.map(start, size);
        start
    }
}
