//! ADVAPI32: the registry.

use super::{ERROR_FILE_NOT_FOUND, functions};
use crate::os::{GAME_KEY, Os};

functions! { "ADVAPI32.dll";
    RegOpenKeyExA(5) => |m, a| {
        const HKEY_LOCAL_MACHINE: u32 = 0x8000_0002;
        let name = m.text(a[1]).to_ascii_lowercase();
        let parent = match a[0] {
            HKEY_LOCAL_MACHINE => Some("hklm".to_string()),
            handle => m.os.keys.get(&handle).cloned(),
        };
        let path = parent.map(|parent| format!("{parent}\\{name}"));
        match path {
            Some(path) if GAME_KEY.starts_with(&path) => {
                let handle = 0x8000 + m.os.keys.len() as u32 * 4;
                m.note(|| format!("{path:?}"));
                m.os.keys.insert(handle, path);
                let _ = m.memory.write(a[4], 4, handle);
                0
            }
            other => {
                m.note(|| format!("{:?} -> not found", other.unwrap_or(name)));
                ERROR_FILE_NOT_FOUND
            }
        }
    },
    RegQueryValueExA(6) => |m, a| {
        const REG_SZ: u32 = 1;
        const ERROR_MORE_DATA: u32 = 234;
        let (kind_out, data, size_io) = (a[3], a[4], a[5]);
        let name = m.text(a[1]);
        let key = m.os.keys.get(&a[0]).cloned().unwrap_or_default();
        let Some(value) = Os::registry_value(&key, &name) else {
            m.note(|| format!("{name:?} -> not found"));
            return ERROR_FILE_NOT_FOUND;
        };
        m.note(|| format!("{name:?} = {value:?}"));
        let needed = value.len() as u32 + 1;
        let capacity = if size_io != 0 { m.memory.read(size_io, 4).unwrap_or(0) } else { 0 };
        if size_io != 0 {
            let _ = m.memory.write(size_io, 4, needed);
        }
        if kind_out != 0 {
            let _ = m.memory.write(kind_out, 4, REG_SZ);
        }
        if data == 0 {
            return 0;
        }
        if capacity < needed {
            return ERROR_MORE_DATA;
        }
        m.put_string(data, needed, value.as_bytes());
        0
    },
    RegCloseKey(1) => |_, _| 0,
}
