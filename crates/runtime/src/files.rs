//! The game's view of the disk.
//!
//! The program believes it is installed in `C:\TOY2`. Reads are served from
//! the user's own game directory, which is never written to; anything the
//! program creates (settings, saves, logs) goes to a separate directory and
//! shadows the original on later reads. Names are matched without regard to
//! case, as on Windows.
//!
//! Files are held in memory while open. The storage behind this type is the
//! only part that touches the host file system, which keeps it replaceable in
//! a browser.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Directory the program believes it runs from, without a trailing separator.
pub const INSTALL_DIR: &str = "C:\\TOY2";

#[derive(Debug)]
pub struct OpenFile {
    /// Path relative to the install directory, normalised.
    pub name: String,
    pub data: Vec<u8>,
    pub position: u64,
    pub writable: bool,
    dirty: bool,
}

#[derive(Debug, Default)]
pub struct Files {
    game_dir: Option<PathBuf>,
    write_dir: Option<PathBuf>,
    open: HashMap<u32, OpenFile>,
    next_handle: u32,
    /// Every name the program tried to open, with the outcome.
    pub log: Vec<(String, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Fail if the file does not exist.
    OpenExisting,
    /// Create the file if needed, keeping its contents.
    OpenAlways,
    /// Start from an empty file, creating it if needed.
    CreateAlways,
}

impl Files {
    pub fn new(game_dir: Option<PathBuf>, write_dir: Option<PathBuf>) -> Files {
        Files {
            game_dir,
            write_dir,
            next_handle: 0x100,
            ..Files::default()
        }
    }

    /// Turns a path as the program wrote it into components relative to the
    /// install directory. `None` if it points outside.
    fn components(guest_path: &str) -> Option<Vec<String>> {
        let unified = guest_path.replace('/', "\\");
        let upper = unified.to_ascii_uppercase();
        let relative = if let Some(rest) = upper.strip_prefix(INSTALL_DIR) {
            &unified[unified.len() - rest.len()..]
        } else if upper.len() >= 2 && upper.as_bytes()[1] == b':' || upper.starts_with("\\\\") {
            return None; // another drive, or a device path
        } else {
            &unified[..]
        };
        let mut parts = Vec::new();
        for part in relative.split('\\') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                name => parts.push(name.to_string()),
            }
        }
        (!parts.is_empty()).then_some(parts)
    }

    /// Finds `parts` under `root`, ignoring case.
    fn resolve(root: &Path, parts: &[String]) -> Option<PathBuf> {
        let mut path = root.to_path_buf();
        for part in parts {
            let exact = path.join(part);
            path = if exact.exists() {
                exact
            } else {
                fs::read_dir(&path)
                    .ok()?
                    .filter_map(Result::ok)
                    .find(|entry| entry.file_name().to_string_lossy().eq_ignore_ascii_case(part))?
                    .path()
            };
        }
        Some(path)
    }

    fn read_existing(&self, parts: &[String]) -> Option<Vec<u8>> {
        [&self.write_dir, &self.game_dir]
            .into_iter()
            .flatten()
            .filter_map(|root| Self::resolve(root, parts))
            .find(|path| path.is_file())
            .and_then(|path| fs::read(path).ok())
    }

    pub fn open(&mut self, guest_path: &str, writable: bool, disposition: Disposition) -> Option<u32> {
        let file = Self::components(guest_path).and_then(|parts| {
            let existing = match disposition {
                Disposition::CreateAlways => None,
                _ => self.read_existing(&parts),
            };
            let data = match (existing, disposition) {
                (Some(data), _) => data,
                (None, Disposition::OpenExisting) => return None,
                (None, _) => Vec::new(),
            };
            let created = disposition == Disposition::CreateAlways;
            Some(OpenFile {
                name: parts.join("/"),
                data,
                position: 0,
                writable,
                dirty: created,
            })
        });
        self.log.push((guest_path.to_string(), file.is_some()));
        let file = file?;
        let handle = self.next_handle;
        self.next_handle += 4;
        self.open.insert(handle, file);
        Some(handle)
    }

    pub fn get(&mut self, handle: u32) -> Option<&mut OpenFile> {
        self.open.get_mut(&handle)
    }

    pub fn read(&mut self, handle: u32, len: u32) -> Option<&[u8]> {
        let file = self.open.get_mut(&handle)?;
        let start = (file.position as usize).min(file.data.len());
        let end = start.saturating_add(len as usize).min(file.data.len());
        file.position = end as u64;
        Some(&file.data[start..end])
    }

    pub fn write(&mut self, handle: u32, bytes: &[u8]) -> Option<()> {
        let file = self.open.get_mut(&handle).filter(|file| file.writable)?;
        let start = file.position as usize;
        let end = start.checked_add(bytes.len())?;
        if file.data.len() < end {
            file.data.resize(end, 0);
        }
        file.data[start..end].copy_from_slice(bytes);
        file.position = end as u64;
        file.dirty = true;
        Some(())
    }

    /// Closes the file, saving it to the write directory if it was modified.
    pub fn close(&mut self, handle: u32) -> bool {
        let Some(file) = self.open.remove(&handle) else {
            return false;
        };
        if file.dirty
            && let Some(root) = &self.write_dir
        {
            let path = root.join(file.name.to_ascii_lowercase());
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, &file.data);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_confined_to_the_install_directory() {
        let parts = |path| Files::components(path).map(|p| p.join("/"));
        assert_eq!(
            parts("C:\\TOY2\\data\\LEVEL01\\level.ngn").as_deref(),
            Some("data/LEVEL01/level.ngn")
        );
        assert_eq!(parts("c:\\toy2\\toy2.cfg").as_deref(), Some("toy2.cfg"));
        assert_eq!(parts("toy2.err").as_deref(), Some("toy2.err"));
        assert_eq!(
            parts(".\\data\\..\\validate.tta").as_deref(),
            Some("validate.tta")
        );
        assert_eq!(parts("..\\secret"), None);
        assert_eq!(parts("D:\\validate.tta"), None);
        assert_eq!(parts("\\\\.\\vwin32"), None);
        assert_eq!(parts("C:\\TOY2"), None);
    }

    #[test]
    fn reads_ignore_case_and_writes_never_touch_the_game_directory() {
        let root = std::env::temp_dir().join(format!("andy-files-test-{}", std::process::id()));
        let (game, write) = (root.join("game"), root.join("write"));
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::write(game.join("Data").join("Level.BIN"), b"original").unwrap();

        let mut files = Files::new(Some(game.clone()), Some(write.clone()));
        let handle = files
            .open("C:\\TOY2\\DATA\\level.bin", false, Disposition::OpenExisting)
            .unwrap();
        assert_eq!(files.read(handle, 4), Some(&b"orig"[..]));
        assert_eq!(files.read(handle, 100), Some(&b"inal"[..]));
        assert!(files.write(handle, b"x").is_none());
        assert!(files.close(handle));
        assert_eq!(files.open("missing.dat", false, Disposition::OpenExisting), None);

        let handle = files
            .open("data\\level.bin", true, Disposition::CreateAlways)
            .unwrap();
        files.write(handle, b"changed").unwrap();
        files.close(handle);
        assert_eq!(
            fs::read(game.join("Data").join("Level.BIN")).unwrap(),
            b"original"
        );
        // The new copy shadows the original from now on.
        let handle = files
            .open("DATA\\LEVEL.BIN", false, Disposition::OpenExisting)
            .unwrap();
        assert_eq!(files.read(handle, 100), Some(&b"changed"[..]));
        fs::remove_dir_all(root).unwrap();
    }
}
