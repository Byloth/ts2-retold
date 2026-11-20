//! Replacement for the Windows API functions the game imports.
//!
//! Functions are added as execution reaches them; an import without an entry
//! here stops the machine with [`crate::Stop::UnimplementedImport`]. Each
//! handler receives its arguments as raw 32-bit values read from the guest
//! stack, in declaration order, and returns the value for `eax`.
//!
//! The machine presents itself as Windows 98, the system the game was
//! released for.

use std::collections::HashMap;

use andy_cpu::memory::Memory;

use crate::files::{Disposition, Files, INSTALL_DIR};

/// `GetVersion` on Windows 98 SE: version 4.10, high bit set for the 9x family.
const WINDOWS_VERSION: u32 = 0xC000_0A04;
pub const IMAGE_BASE: u32 = 0x0040_0000;
const HEAP_START: u32 = 0x1000_0000;
const HEAP_LIMIT: u32 = 0x4000_0000;
const VIRTUAL_START: u32 = 0x4000_0000;
const VIRTUAL_LIMIT: u32 = 0x7000_0000;
const MEM_COMMIT: u32 = 0x1000;
/// Size of the bookkeeping word in front of every heap block.
const HEADER: u32 = 8;
/// The executable as the program sees it, inside `files::INSTALL_DIR`.
const MODULE_PATH: &str = "C:\\TOY2\\TOY2.EXE";

/// State of the replacement operating system.
#[derive(Debug, Default)]
pub struct Os {
    heap_next: u32,
    virtual_next: u32,
    /// Guest addresses of strings handed out once and kept alive.
    command_line: u32,
    environment: u32,
    pub last_error: u32,
    pub exit_code: Option<u32>,
    /// Text the program sent to the debugger.
    pub debug_output: Vec<String>,
    pub files: Files,
    /// Milliseconds since start, set by the machine before each call. The
    /// clock is a function of the instructions executed, so runs repeat.
    pub now_ms: u32,
    /// Open registry keys: handle to lower-case path.
    keys: HashMap<u32, String>,
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

    /// The registry as the installer leaves it: one key with the install
    /// directory and the directory of the disc (here the same place).
    fn registry_value(key: &str, value: &str) -> Option<String> {
        match (key, value.to_ascii_lowercase().as_str()) {
            (GAME_KEY, "path" | "cdpath") => Some(format!("{INSTALL_DIR}\\")),
            _ => None,
        }
    }

    /// Allocates `size` bytes of zeroed guest memory that are never reused.
    fn allocate(&mut self, memory: &mut Memory, size: u32) -> u32 {
        let total = size.saturating_add(HEADER).saturating_add(15) & !15;
        if total < size || self.heap_next.saturating_add(total) > HEAP_LIMIT {
            return 0;
        }
        let block = self.heap_next + HEADER;
        memory.map(self.heap_next, total);
        memory
            .write(self.heap_next, 4, size)
            .expect("block was just mapped");
        self.heap_next += total;
        block
    }

    fn block_size(&self, memory: &mut Memory, block: u32) -> Option<u32> {
        (HEAP_START + HEADER..self.heap_next)
            .contains(&block)
            .then(|| memory.read(block - HEADER, 4))?
    }

    fn string(&mut self, memory: &mut Memory, text: &[u8]) -> u32 {
        let at = self.allocate(memory, text.len() as u32 + 1);
        memory.load(at, text).expect("block was just mapped");
        at
    }
}

/// One call into the operating system.
pub struct Call<'a> {
    pub memory: &'a mut Memory,
    pub os: &'a mut Os,
    pub args: &'a [u32],
    /// Human-readable note for the trace (decoded strings and the like).
    pub detail: Option<String>,
}

impl Call<'_> {
    fn text(&mut self, address: u32) -> String {
        self.memory
            .c_string(address, 4096)
            .unwrap_or_else(|| format!("<bad pointer {address:#x}>"))
    }

    /// The bytes of a NUL-terminated string, terminator excluded.
    fn bytes(&mut self, address: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Some(byte) = self.memory.read_u8(address.wrapping_add(bytes.len() as u32)) {
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }
        bytes
    }

    fn note(&mut self, detail: impl Into<String>) {
        self.detail = Some(detail.into());
    }

    /// Writes a string and its terminator, truncating to `capacity` bytes.
    /// Returns the number of characters written, terminator excluded.
    fn put_string(&mut self, address: u32, capacity: u32, text: &[u8]) -> u32 {
        if capacity == 0 {
            return 0;
        }
        let len = text.len().min(capacity as usize - 1);
        let ok = self.memory.load(address, &text[..len]).is_some()
            && self.memory.load(address + len as u32, &[0]).is_some();
        if ok { len as u32 } else { 0 }
    }
}

pub struct Function {
    pub dll: &'static str,
    pub name: &'static str,
    /// Number of 32-bit arguments the function removes from the stack.
    pub args: u8,
    pub handler: fn(&mut Call) -> u32,
}

pub fn lookup(dll: &str, name: &str) -> Option<&'static Function> {
    FUNCTIONS
        .iter()
        .find(|f| f.name == name && f.dll.eq_ignore_ascii_case(dll))
}

macro_rules! functions {
    ($($dll:literal { $($name:ident($args:literal) => $handler:expr,)* })*) => {
        static FUNCTIONS: &[Function] = &[
            $($(Function { dll: $dll, name: stringify!($name), args: $args, handler: $handler },)*)*
        ];
    };
}

functions! {
    "KERNEL32.dll" {
        GetVersion(0) => |_| WINDOWS_VERSION,
        GetCommandLineA(0) => |c| {
            if c.os.command_line == 0 {
                c.os.command_line = c.os.string(c.memory, MODULE_PATH.as_bytes());
            }
            c.os.command_line
        },
        GetStartupInfoA(1) => |c| {
            // STARTUPINFOA is 68 bytes; only its size field is non-zero.
            let at = c.args[0];
            let _ = c.memory.load(at, &[0; 68]);
            let _ = c.memory.write(at, 4, 68);
            0
        },
        GetModuleHandleA(1) => |c| {
            if c.args[0] == 0 {
                IMAGE_BASE
            } else {
                let name = c.text(c.args[0]);
                c.note(format!("{name:?} -> not loaded"));
                0
            }
        },
        GetModuleFileNameA(3) => |c| c.put_string(c.args[1], c.args[2], MODULE_PATH.as_bytes()),
        GetEnvironmentStrings(0) => environment,
        // Windows 98 has no Unicode environment; the runtime falls back to ANSI.
        GetEnvironmentStringsW(0) => |_| 0,
        FreeEnvironmentStringsA(1) => |_| 1,
        FreeEnvironmentStringsW(1) => |_| 1,
        // A windowed program starts without standard handles.
        GetStdHandle(1) => |_| 0,
        GetFileType(1) => |c| c.os.files.get(c.args[0]).is_some() as u32, // FILE_TYPE_DISK
        CreateFileA(7) => |c| {
            const GENERIC_WRITE: u32 = 0x4000_0000;
            let name = c.text(c.args[0]);
            let disposition = match c.args[4] {
                1 | 2 | 5 => Disposition::CreateAlways,
                4 => Disposition::OpenAlways,
                _ => Disposition::OpenExisting,
            };
            let handle = c.os.files.open(&name, c.args[1] & GENERIC_WRITE != 0, disposition);
            c.note(format!("{name:?}{}", if handle.is_some() { "" } else { " -> not found" }));
            handle.unwrap_or_else(|| {
                c.os.last_error = ERROR_FILE_NOT_FOUND;
                u32::MAX
            })
        },
        ReadFile(5) => |c| {
            let (handle, buffer, len, count_out) = (c.args[0], c.args[1], c.args[2], c.args[3]);
            let Some(bytes) = c.os.files.read(handle, len) else { return 0 };
            let count = bytes.len() as u32;
            if c.memory.load(buffer, bytes).is_none() {
                return 0;
            }
            if count_out != 0 {
                let _ = c.memory.write(count_out, 4, count);
            }
            1
        },
        WriteFile(5) => |c| {
            let (handle, buffer, len, count_out) = (c.args[0], c.args[1], c.args[2], c.args[3]);
            let Some(bytes) = c.memory.dump(buffer, len) else { return 0 };
            if c.os.files.write(handle, &bytes).is_none() {
                return 0;
            }
            if count_out != 0 {
                let _ = c.memory.write(count_out, 4, len);
            }
            1
        },
        SetFilePointer(4) => |c| {
            let (handle, distance, method) = (c.args[0], c.args[1] as i32 as i64, c.args[3]);
            let Some(file) = c.os.files.get(handle) else { return u32::MAX };
            let origin = match method {
                0 => 0,
                1 => file.position as i64,
                _ => file.data.len() as i64,
            };
            let Ok(position) = u32::try_from(origin + distance) else { return u32::MAX };
            file.position = position as u64;
            position
        },
        FlushFileBuffers(1) => |_| 1,
        CloseHandle(1) => |c| {
            c.os.files.close(c.args[0]);
            1
        },
        SetHandleCount(1) => |c| c.args[0],
        GetACP(0) => |_| 1252,
        GetOEMCP(0) => |_| 437,
        GetCPInfo(2) => |c| {
            // CPINFO: MaxCharSize, DefaultChar[2], LeadByte[12]. Single-byte code page.
            let at = c.args[1];
            let ok = c.memory.load(at, &[0; 18]).is_some()
                && c.memory.write(at, 4, 1).is_some()
                && c.memory.write(at + 4, 1, b'?' as u32).is_some();
            ok as u32
        },
        GetLastError(0) => |c| c.os.last_error,
        GetCurrentProcess(0) => |_| u32::MAX,
        ExitProcess(1) => |c| {
            c.os.exit_code = Some(c.args[0]);
            0
        },
        TerminateProcess(2) => |c| {
            c.os.exit_code = Some(c.args[1]);
            1
        },
        // One thread so far: locks are free.
        InitializeCriticalSection(1) => |_| 0,
        EnterCriticalSection(1) => |_| 0,
        LeaveCriticalSection(1) => |_| 0,
        IsBadWritePtr(2) => |c| {
            let last = c.args[0].wrapping_add(c.args[1].saturating_sub(1));
            !(c.memory.is_mapped(c.args[0]) && c.memory.is_mapped(last)) as u32
        },
        OutputDebugStringA(1) => |c| {
            let text = c.text(c.args[0]);
            c.os.debug_output.push(text.clone());
            c.note(format!("{:?}", text.trim_end()));
            0
        },

        HeapCreate(3) => |_| HEAP_START,
        HeapDestroy(1) => |_| 1,
        HeapAlloc(3) => |c| c.os.allocate(c.memory, c.args[2]),
        HeapFree(3) => |_| 1,
        HeapSize(3) => |c| c.os.block_size(c.memory, c.args[2]).unwrap_or(u32::MAX),
        HeapReAlloc(4) => |c| {
            let (block, size) = (c.args[2], c.args[3]);
            let Some(old) = c.os.block_size(c.memory, block) else { return 0 };
            if size <= old {
                let _ = c.memory.write(block - HEADER, 4, size);
                return block;
            }
            let new = c.os.allocate(c.memory, size);
            if new != 0 {
                let bytes = c.memory.dump(block, old).expect("heap blocks are mapped");
                c.memory.load(new, &bytes).expect("block was just mapped");
            }
            new
        },
        VirtualAlloc(4) => |c| {
            let (address, size, kind) = (c.args[0], c.args[1], c.args[2]);
            let size = size.saturating_add(0xFFF) & !0xFFF;
            if address != 0 {
                // Committing inside a region reserved earlier.
                let start = address & !0xFFF;
                if kind & MEM_COMMIT != 0 {
                    c.memory.map(start, size + (address & 0xFFF));
                }
                return start;
            }
            // Regions are aligned to 64 KiB, as on Windows.
            let start = c.os.virtual_next;
            let Some(next) = start.checked_add(size.saturating_add(0xFFFF) & !0xFFFF) else { return 0 };
            if next > VIRTUAL_LIMIT {
                return 0;
            }
            c.os.virtual_next = next;
            // Reserved-only pages are mapped too: there is no cost and no
            // program relies on touching them to fault.
            c.memory.map(start, size);
            start
        },
        VirtualFree(3) => |_| 1,

        // Windows 98 implements the ANSI variants only; the C runtime probes
        // the Unicode ones first and falls back.
        GetStringTypeW(4) => not_implemented,
        LCMapStringW(6) => not_implemented,
        GetStringTypeA(5) => |c| {
            let (source, count, out) = (c.args[2], c.args[3], c.args[4]);
            let bytes = if count == u32::MAX { c.bytes(source) } else { c.memory.dump(source, count).unwrap_or_default() };
            for (n, &byte) in bytes.iter().enumerate() {
                if c.memory.write(out + 2 * n as u32, 2, character_type(byte) as u32).is_none() {
                    return 0;
                }
            }
            1
        },
        LCMapStringA(6) => |c| {
            const LCMAP_LOWERCASE: u32 = 0x100;
            const LCMAP_UPPERCASE: u32 = 0x200;
            let (flags, source, count, out, capacity) = (c.args[1], c.args[2], c.args[3], c.args[4], c.args[5]);
            let mut bytes = if count == u32::MAX {
                let mut bytes = c.bytes(source);
                bytes.push(0);
                bytes
            } else {
                c.memory.dump(source, count).unwrap_or_default()
            };
            if flags & LCMAP_LOWERCASE != 0 {
                bytes.make_ascii_lowercase();
            } else if flags & LCMAP_UPPERCASE != 0 {
                bytes.make_ascii_uppercase();
            }
            if capacity == 0 {
                return bytes.len() as u32;
            }
            if bytes.len() as u32 > capacity || c.memory.load(out, &bytes).is_none() {
                return 0;
            }
            bytes.len() as u32
        },
        MultiByteToWideChar(6) => |c| {
            let (source, count, out, capacity) = (c.args[2], c.args[3], c.args[4], c.args[5]);
            let bytes = if count == u32::MAX {
                let mut bytes = c.bytes(source);
                bytes.push(0);
                bytes
            } else {
                c.memory.dump(source, count).unwrap_or_default()
            };
            if capacity == 0 {
                return bytes.len() as u32;
            }
            if bytes.len() as u32 > capacity {
                return 0;
            }
            // Code page 1252 is treated as Latin-1 here.
            for (n, &byte) in bytes.iter().enumerate() {
                if c.memory.write(out + 2 * n as u32, 2, byte as u32).is_none() {
                    return 0;
                }
            }
            bytes.len() as u32
        },
        GetProcAddress(2) => |c| {
            let name = if c.args[1] > 0xFFFF { c.text(c.args[1]) } else { format!("#{}", c.args[1]) };
            c.note(format!("{name:?} -> not found"));
            0
        },
        LoadLibraryA(1) => |c| {
            let name = c.text(c.args[0]);
            c.note(format!("{name:?} -> not found"));
            0
        },
        UnhandledExceptionFilter(1) => |_| 0,
        Sleep(1) => |_| 0,
        MulDiv(3) => |c| {
            let (a, b, d) = (c.args[0] as i32 as i64, c.args[1] as i32 as i64, c.args[2] as i32 as i64);
            if d == 0 {
                return u32::MAX;
            }
            // Rounded to nearest, halves away from zero.
            let product = a * b;
            let half = d.abs() / 2;
            let rounded = if (product < 0) != (d < 0) { (product - half * d.signum()) / d } else { (product + half * d.signum()) / d };
            i32::try_from(rounded).map_or(u32::MAX, |r| r as u32)
        },
        lstrcatA(2) => |c| {
            let existing = c.bytes(c.args[0]).len() as u32;
            let bytes = c.bytes(c.args[1]);
            c.put_string(c.args[0] + existing, u32::MAX, &bytes);
            c.args[0]
        },
        lstrcmpA(2) => |c| compare(&c.bytes(c.args[0]), &c.bytes(c.args[1])),
        lstrcmpiA(2) => |c| {
            compare(&c.bytes(c.args[0]).to_ascii_lowercase(), &c.bytes(c.args[1]).to_ascii_lowercase())
        },
        lstrlenA(1) => |c| c.text(c.args[0]).len() as u32,
        lstrcpyA(2) => |c| {
            let bytes = c.bytes(c.args[1]);
            c.put_string(c.args[0], u32::MAX, &bytes);
            c.args[0]
        },
    }
    "WINMM.dll" {
        timeGetTime(0) => |c| c.os.now_ms,
    }
    "ADVAPI32.dll" {
        RegOpenKeyExA(5) => |c| {
            const HKEY_LOCAL_MACHINE: u32 = 0x8000_0002;
            let name = c.text(c.args[1]).to_ascii_lowercase();
            let parent = match c.args[0] {
                HKEY_LOCAL_MACHINE => Some("hklm".to_string()),
                handle => c.os.keys.get(&handle).cloned(),
            };
            let path = parent.map(|parent| format!("{parent}\\{name}"));
            match path {
                Some(path) if GAME_KEY.starts_with(&path) => {
                    let handle = 0x8000 + c.os.keys.len() as u32 * 4;
                    c.note(format!("{path:?}"));
                    c.os.keys.insert(handle, path);
                    let _ = c.memory.write(c.args[4], 4, handle);
                    0
                }
                other => {
                    c.note(format!("{:?} -> not found", other.unwrap_or(name)));
                    ERROR_FILE_NOT_FOUND
                }
            }
        },
        RegQueryValueExA(6) => |c| {
            const REG_SZ: u32 = 1;
            const ERROR_MORE_DATA: u32 = 234;
            let (kind_out, data, size_io) = (c.args[3], c.args[4], c.args[5]);
            let name = c.text(c.args[1]);
            let key = c.os.keys.get(&c.args[0]).cloned().unwrap_or_default();
            let Some(value) = Os::registry_value(&key, &name) else {
                c.note(format!("{name:?} -> not found"));
                return ERROR_FILE_NOT_FOUND;
            };
            c.note(format!("{name:?} = {value:?}"));
            let needed = value.len() as u32 + 1;
            let capacity = if size_io != 0 { c.memory.read(size_io, 4).unwrap_or(0) } else { 0 };
            if size_io != 0 {
                let _ = c.memory.write(size_io, 4, needed);
            }
            if kind_out != 0 {
                let _ = c.memory.write(kind_out, 4, REG_SZ);
            }
            if data == 0 {
                return 0;
            }
            if capacity < needed {
                return ERROR_MORE_DATA;
            }
            c.put_string(data, needed, value.as_bytes());
            0
        },
        RegCloseKey(1) => |_| 0,
    }
    "USER32.dll" {
        MessageBoxA(4) => |c| {
            let (text, caption) = (c.text(c.args[1]), c.text(c.args[2]));
            c.note(format!("[{caption}] {text}"));
            1 // IDOK
        },
    }
}

const ERROR_FILE_NOT_FOUND: u32 = 2;
/// Registry key written by the game's installer.
const GAME_KEY: &str = "hklm\\software\\travellerstalestoystory2";
const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;

fn not_implemented(c: &mut Call) -> u32 {
    c.os.last_error = ERROR_CALL_NOT_IMPLEMENTED;
    0
}

fn compare(a: &[u8], b: &[u8]) -> u32 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => u32::MAX,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `CT_CTYPE1` classification of a byte, ASCII only.
fn character_type(byte: u8) -> u16 {
    const UPPER: u16 = 0x001;
    const LOWER: u16 = 0x002;
    const DIGIT: u16 = 0x004;
    const SPACE: u16 = 0x008;
    const PUNCT: u16 = 0x010;
    const CNTRL: u16 = 0x020;
    const BLANK: u16 = 0x040;
    const XDIGIT: u16 = 0x080;
    const ALPHA: u16 = 0x100;
    let mut kind = 0;
    if byte.is_ascii_uppercase() {
        kind |= UPPER | ALPHA;
    }
    if byte.is_ascii_lowercase() {
        kind |= LOWER | ALPHA;
    }
    if byte.is_ascii_digit() {
        kind |= DIGIT;
    }
    if byte.is_ascii_hexdigit() {
        kind |= XDIGIT;
    }
    if matches!(byte, b' ' | 0x09..=0x0D) {
        kind |= SPACE;
    }
    if byte.is_ascii_punctuation() {
        kind |= PUNCT;
    }
    if byte.is_ascii_control() {
        kind |= CNTRL;
    }
    if matches!(byte, b' ' | b'\t') {
        kind |= BLANK;
    }
    kind
}

fn environment(c: &mut Call) -> u32 {
    if c.os.environment == 0 {
        // An empty block: just the terminating empty string.
        c.os.environment = c.os.string(c.memory, &[0]);
    }
    c.os.environment
}
