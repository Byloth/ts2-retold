//! KERNEL32: process, memory, files, strings and locale.

use super::{ERROR_CALL_NOT_IMPLEMENTED, ERROR_FILE_NOT_FOUND, HostResult, functions};
use crate::files::Disposition;
use crate::os::{HEAP_HEADER, Os};
use crate::{Machine, Stop};

/// `GetVersion` on Windows 98 SE: version 4.10, high bit set for the 9x family.
const WINDOWS_VERSION: u32 = 0xC000_0A04;
pub(crate) const IMAGE_BASE: u32 = 0x0040_0000;
const MEM_COMMIT: u32 = 0x1000;
/// The executable as the program sees it, inside `files::INSTALL_DIR`.
const MODULE_PATH: &str = "C:\\TOY2\\TOY2.EXE";

functions! { "KERNEL32.dll";
    GetVersion(0) => |_, _| WINDOWS_VERSION,
    GetCommandLineA(0) => |m, _| {
        if m.os.command_line == 0 {
            m.os.command_line = m.os.string(&mut m.memory, MODULE_PATH.as_bytes());
        }
        m.os.command_line
    },
    GetStartupInfoA(1) => |m, a| {
        // STARTUPINFOA is 68 bytes; only its size field is non-zero.
        let at = a[0];
        let _ = m.memory.load(at, &[0; 68]);
        let _ = m.memory.write(at, 4, 68);
        0
    },
    GetModuleHandleA(1) => |m, a| {
        if a[0] == 0 {
            IMAGE_BASE
        } else {
            let name = m.text(a[0]);
            m.note(|| format!("{name:?} -> not loaded"));
            0
        }
    },
    GetModuleFileNameA(3) => |m, a| m.put_string(a[1], a[2], MODULE_PATH.as_bytes()),
    GetEnvironmentStrings(0) => environment,
    // Windows 98 has no Unicode environment; the runtime falls back to ANSI.
    GetEnvironmentStringsW(0) => |_, _| 0,
    FreeEnvironmentStringsA(1) => |_, _| 1,
    FreeEnvironmentStringsW(1) => |_, _| 1,
    // A windowed program starts without standard handles.
    GetStdHandle(1) => |_, _| 0,
    GetFileType(1) => |m, a| m.os.files.get(a[0]).is_some() as u32, // FILE_TYPE_DISK
    CreateFileA(7) => |m, a| {
        const GENERIC_WRITE: u32 = 0x4000_0000;
        let name = m.text(a[0]);
        let disposition = match a[4] {
            1 | 2 | 5 => Disposition::CreateAlways,
            4 => Disposition::OpenAlways,
            _ => Disposition::OpenExisting,
        };
        let handle = m.os.files.open(&name, a[1] & GENERIC_WRITE != 0, disposition);
        m.note(|| format!("{name:?}{}", if handle.is_some() { "" } else { " -> not found" }));
        handle.unwrap_or_else(|| {
            m.os.last_error = ERROR_FILE_NOT_FOUND;
            u32::MAX
        })
    },
    ReadFile(5) => |m, a| {
        let (handle, buffer, len, count_out) = (a[0], a[1], a[2], a[3]);
        let Some(bytes) = m.os.files.read(handle, len) else { return 0 };
        let count = bytes.len() as u32;
        if m.memory.load(buffer, bytes).is_none() {
            return 0;
        }
        if count_out != 0 {
            let _ = m.memory.write(count_out, 4, count);
        }
        1
    },
    WriteFile(5) => |m, a| {
        let (handle, buffer, len, count_out) = (a[0], a[1], a[2], a[3]);
        let Some(bytes) = m.memory.dump(buffer, len) else { return 0 };
        if m.os.files.write(handle, &bytes).is_none() {
            return 0;
        }
        if count_out != 0 {
            let _ = m.memory.write(count_out, 4, len);
        }
        1
    },
    SetFilePointer(4) => |m, a| {
        let (handle, distance, method) = (a[0], a[1] as i32 as i64, a[3]);
        let Some(file) = m.os.files.get(handle) else { return u32::MAX };
        let origin = match method {
            0 => 0,
            1 => file.position as i64,
            _ => file.data.len() as i64,
        };
        let Ok(position) = u32::try_from(origin + distance) else { return u32::MAX };
        file.position = position as u64;
        position
    },
    FlushFileBuffers(1) => |_, _| 1,
    CloseHandle(1) => |m, a| {
        m.os.files.close(a[0]);
        1
    },
    SetHandleCount(1) => |_, a| a[0],
    GetACP(0) => |_, _| 1252,
    GetOEMCP(0) => |_, _| 437,
    GetCPInfo(2) => |m, a| {
        // CPINFO: MaxCharSize, DefaultChar[2], LeadByte[12]. Single-byte code page.
        let at = a[1];
        let ok = m.memory.load(at, &[0; 18]).is_some()
            && m.memory.write(at, 4, 1).is_some()
            && m.memory.write(at + 4, 1, b'?' as u32).is_some();
        ok as u32
    },
    GetLastError(0) => |m, _| m.os.last_error,
    GetCurrentProcess(0) => |_, _| u32::MAX,
    ExitProcess(1) => |_, a| -> HostResult { Err(Stop::Exit(a[0])) },
    TerminateProcess(2) => |_, a| -> HostResult { Err(Stop::Exit(a[1])) },
    // One thread so far: locks are free.
    InitializeCriticalSection(1) => |_, _| 0,
    EnterCriticalSection(1) => |_, _| 0,
    LeaveCriticalSection(1) => |_, _| 0,
    IsBadWritePtr(2) => |m, a| {
        let last = a[0].wrapping_add(a[1].saturating_sub(1));
        !(m.memory.is_mapped(a[0]) && m.memory.is_mapped(last)) as u32
    },
    OutputDebugStringA(1) => |m, a| {
        let text = m.text(a[0]);
        m.os.debug_output.push(text.clone());
        m.note(|| format!("{:?}", text.trim_end()));
        0
    },

    HeapCreate(3) => |_, _| Os::heap_handle(),
    HeapDestroy(1) => |_, _| 1,
    HeapAlloc(3) => |m, a| m.os.allocate(&mut m.memory, a[2]),
    HeapFree(3) => |_, _| 1,
    HeapSize(3) => |m, a| m.os.block_size(&mut m.memory, a[2]).unwrap_or(u32::MAX),
    HeapReAlloc(4) => |m, a| {
        let (block, size) = (a[2], a[3]);
        let Some(old) = m.os.block_size(&mut m.memory, block) else { return 0 };
        if size <= old {
            let _ = m.memory.write(block - HEAP_HEADER, 4, size);
            return block;
        }
        let new = m.os.allocate(&mut m.memory, size);
        if new != 0 {
            let bytes = m.memory.dump(block, old).expect("heap blocks are mapped");
            m.memory.load(new, &bytes).expect("block was just mapped");
        }
        new
    },
    VirtualAlloc(4) => |m, a| {
        let (address, size, kind) = (a[0], a[1], a[2]);
        let size = size.saturating_add(0xFFF) & !0xFFF;
        if address != 0 {
            // Committing inside a region reserved earlier.
            let start = address & !0xFFF;
            if kind & MEM_COMMIT != 0 {
                m.memory.map(start, size + (address & 0xFFF));
            }
            return start;
        }
        m.os.reserve(&mut m.memory, size)
    },
    VirtualFree(3) => |_, _| 1,

    // Windows 98 implements the ANSI variants only; the C runtime probes
    // the Unicode ones first and falls back.
    GetStringTypeW(4) => not_implemented,
    LCMapStringW(6) => not_implemented,
    GetStringTypeA(5) => |m, a| {
        let (source, count, out) = (a[2], a[3], a[4]);
        let bytes = if count == u32::MAX { m.bytes(source) } else { m.memory.dump(source, count).unwrap_or_default() };
        for (n, &byte) in bytes.iter().enumerate() {
            if m.memory.write(out + 2 * n as u32, 2, character_type(byte) as u32).is_none() {
                return 0;
            }
        }
        1
    },
    LCMapStringA(6) => |m, a| {
        const LCMAP_LOWERCASE: u32 = 0x100;
        const LCMAP_UPPERCASE: u32 = 0x200;
        let (flags, source, count, out, capacity) = (a[1], a[2], a[3], a[4], a[5]);
        let mut bytes = if count == u32::MAX {
            let mut bytes = m.bytes(source);
            bytes.push(0);
            bytes
        } else {
            m.memory.dump(source, count).unwrap_or_default()
        };
        if flags & LCMAP_LOWERCASE != 0 {
            bytes.make_ascii_lowercase();
        } else if flags & LCMAP_UPPERCASE != 0 {
            bytes.make_ascii_uppercase();
        }
        if capacity == 0 {
            return bytes.len() as u32;
        }
        if bytes.len() as u32 > capacity || m.memory.load(out, &bytes).is_none() {
            return 0;
        }
        bytes.len() as u32
    },
    MultiByteToWideChar(6) => |m, a| {
        let (source, count, out, capacity) = (a[2], a[3], a[4], a[5]);
        let bytes = if count == u32::MAX {
            let mut bytes = m.bytes(source);
            bytes.push(0);
            bytes
        } else {
            m.memory.dump(source, count).unwrap_or_default()
        };
        if capacity == 0 {
            return bytes.len() as u32;
        }
        if bytes.len() as u32 > capacity {
            return 0;
        }
        // Code page 1252 is treated as Latin-1 here.
        for (n, &byte) in bytes.iter().enumerate() {
            if m.memory.write(out + 2 * n as u32, 2, byte as u32).is_none() {
                return 0;
            }
        }
        bytes.len() as u32
    },
    GetProcAddress(2) => |m, a| {
        let name = if a[1] > 0xFFFF { m.text(a[1]) } else { format!("#{}", a[1]) };
        m.note(|| format!("{name:?} -> not found"));
        0
    },
    LoadLibraryA(1) => |m, a| {
        let name = m.text(a[0]);
        m.note(|| format!("{name:?} -> not found"));
        0
    },
    UnhandledExceptionFilter(1) => |_, _| 0,
    Sleep(1) => |_, _| 0,
    MulDiv(3) => |_, a| {
        let (a, b, d) = (a[0] as i32 as i64, a[1] as i32 as i64, a[2] as i32 as i64);
        if d == 0 {
            return u32::MAX;
        }
        // Rounded to nearest, halves away from zero.
        let product = a * b;
        let half = d.abs() / 2;
        let rounded = if (product < 0) != (d < 0) { (product - half * d.signum()) / d } else { (product + half * d.signum()) / d };
        i32::try_from(rounded).map_or(u32::MAX, |r| r as u32)
    },
    lstrcatA(2) => |m, a| {
        let existing = m.bytes(a[0]).len() as u32;
        let bytes = m.bytes(a[1]);
        m.put_string(a[0] + existing, u32::MAX, &bytes);
        a[0]
    },
    lstrcmpA(2) => |m, a| compare(&m.bytes(a[0]), &m.bytes(a[1])),
    lstrcmpiA(2) => |m, a| {
        compare(&m.bytes(a[0]).to_ascii_lowercase(), &m.bytes(a[1]).to_ascii_lowercase())
    },
    lstrlenA(1) => |m, a| m.text(a[0]).len() as u32,
    lstrcpyA(2) => |m, a| {
        let bytes = m.bytes(a[1]);
        m.put_string(a[0], u32::MAX, &bytes);
        a[0]
    },
}

fn environment(m: &mut Machine, _: &[u32]) -> u32 {
    if m.os.environment == 0 {
        // An empty block: just the terminating empty string.
        m.os.environment = m.os.string(&mut m.memory, &[0]);
    }
    m.os.environment
}

fn not_implemented(m: &mut Machine, _: &[u32]) -> u32 {
    m.os.last_error = ERROR_CALL_NOT_IMPLEMENTED;
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
