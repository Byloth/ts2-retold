//! State of the replacement operating system.

use std::collections::HashMap;

use andy_cpu::memory::Memory;

use andy_recomp::pe::Resource;

use crate::files::{Files, INSTALL_DIR};
use crate::win32::com::Com;
use crate::win32::user32::User;
use crate::win32::{ddraw, dsound, winmm};

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
    /// The executable's resources (their data is in guest memory).
    pub resources: Vec<Resource>,
    pub user: User,
    pub(crate) com: Com,
    pub(crate) ddraw: ddraw::State,
    pub(crate) dsound: dsound::State,
    pub(crate) mmio: winmm::State,
    pub(crate) movable_blocks: Vec<crate::win32::kernel32::MovableBlock>,
    /// Open registry keys: handle to lower-case path.
    pub(crate) keys: HashMap<u32, String>,
}

impl Os {
    pub fn new(files: Files) -> Os {
        Os {
            heap_next: HEAP_START,
            virtual_next: VIRTUAL_START,
            files,
            ..Os::default()
        }
    }

    /// The handle of the process heap.
    pub(crate) fn heap_handle() -> u32 {
        HEAP_START
    }

    /// The registry as the installer leaves it: one key with the directory
    /// of the game's data and the directory of the disc. The game looks for
    /// `sfx\`, `cd.txt` and its levels under the first and for `validate.tta`
    /// under the second; in the copies in circulation both are the `data`
    /// directory next to the executable, which holds what was on the disc.
    pub(crate) fn registry_value(key: &str, value: &str) -> Option<String> {
        match (key, value.to_ascii_lowercase().as_str()) {
            (GAME_KEY, "path" | "cdpath") => Some(format!("{INSTALL_DIR}\\data\\")),
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
        memory
            .write(self.heap_next, 4, size)
            .expect("block was just mapped");
        self.heap_next += total;
        block
    }

    pub(crate) fn block_size(&self, memory: &mut Memory, block: u32) -> Option<u32> {
        (HEAP_START + HEAP_HEADER..self.heap_next)
            .contains(&block)
            .then(|| memory.read(block - HEAP_HEADER, 4))?
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
