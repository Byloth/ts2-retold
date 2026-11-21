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
const DELETED_SUFFIX: &str = ".andy-deleted";

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
    /// Another language's game directory: the files that are language
    /// ([`is_language_file`]) are read from it where it has them.
    language_dir: Option<PathBuf>,
    write_dir: Option<PathBuf>,
    open: HashMap<u32, OpenFile>,
    next_handle: u32,
    /// Files that exist only for this run, by normalised lower-case name.
    /// They take precedence over both directories.
    injected: HashMap<String, Vec<u8>>,
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

/// Whether a file of the game, named relative to the install directory in
/// lower case with `/` between the parts, carries language: speech, a
/// movie, a screen with words on it, or the textures that hold the
/// typefaces. The discs of the other languages are also a later revision
/// of the levels' data (terrain, placement, paths); those files are not
/// language and stay the reference's (ADR 0016).
pub fn is_language_file(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    match parts.as_slice() {
        // The movies.
        ["rtlibs", file] => file.ends_with(".dll"),
        // Speech (and the other sounds, which are the same on every disc).
        ["data", "sfx", file] => file.ends_with(".wav"),
        // The loading, bonus and result screens.
        ["data", "gfx", file] => file.ends_with(".ngn"),
        // The front end: its textures and the level map's.
        ["data", "level00", file] => file.ends_with(".ngn"),
        // Each level's textures, the typefaces among them. Not `level1.ngn`
        // and the rest, which are the level itself.
        ["data", level, "level.ngn"] => level.starts_with("level"),
        _ => false,
    }
}

impl Files {
    /// Has the files that are language read from another language's game
    /// directory.
    pub fn set_language_dir(&mut self, dir: PathBuf) {
        self.language_dir = Some(dir);
    }

    pub fn new(game_dir: Option<PathBuf>, write_dir: Option<PathBuf>) -> Files {
        Files { game_dir, write_dir, next_handle: 0x100, ..Files::default() }
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

    /// Makes a file appear in the install directory for this run only.
    pub fn inject(&mut self, name: &str, data: Vec<u8>) {
        self.injected.insert(name.to_ascii_lowercase(), data);
    }

    /// Where a file the program wrote lives in the write directory.
    fn written_path(root: &Path, name: &str) -> PathBuf {
        root.join(name.to_ascii_lowercase())
    }

    /// Marker left in the write directory when the program deletes a file
    /// of the game directory, which itself is never touched.
    fn deletion_marker(root: &Path, name: &str) -> PathBuf {
        root.join(format!("{}{DELETED_SUFFIX}", name.to_ascii_lowercase()))
    }

    fn read_existing(&self, parts: &[String]) -> Option<Vec<u8>> {
        let name = parts.join("/");
        if let Some(data) = self.injected.get(&name.to_ascii_lowercase()) {
            return Some(data.clone());
        }
        if let Some(root) = &self.write_dir {
            if let Ok(data) = fs::read(Self::written_path(root, &name)) {
                return Some(data);
            }
            if Self::deletion_marker(root, &name).exists() {
                return None;
            }
        }
        let language = self.language_dir.as_ref().filter(|_| is_language_file(&name.to_ascii_lowercase()));
        let in_language = language.and_then(|root| Self::resolve(root, parts)).filter(|path| path.is_file());
        let path = match in_language {
            Some(path) => path,
            None => Self::resolve(self.game_dir.as_ref()?, parts)?,
        };
        if path.is_file() { fs::read(path).ok() } else { None }
    }

    /// Deletes a file as the program sees it. Returns whether it existed.
    pub fn delete(&mut self, guest_path: &str) -> bool {
        let Some(parts) = Self::components(guest_path) else {
            return false;
        };
        if self.read_existing(&parts).is_none() {
            return false;
        }
        let name = parts.join("/");
        self.injected.remove(&name.to_ascii_lowercase());
        if let Some(root) = &self.write_dir {
            let _ = fs::remove_file(Self::written_path(root, &name));
            let in_game_dir = self.game_dir.as_ref().and_then(|game| Self::resolve(game, &parts)).is_some();
            if in_game_dir {
                let marker = Self::deletion_marker(root, &name);
                if let Some(parent) = marker.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(marker, b"");
            }
        }
        true
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
            Some(OpenFile { name: parts.join("/"), data, position: 0, writable, dirty: created })
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
            let path = Self::written_path(root, &file.name);
            let _ = fs::remove_file(Self::deletion_marker(root, &file.name));
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
    #[test]
    fn the_files_that_are_language() {
        for name in [
            "rtlibs/l 01 in.dll",
            "data/sfx/usbuz20.wav",
            "data/gfx/loading.ngn",
            "data/level00/level.ngn",
            "data/level00/levelt2.ngn",
            "data/level07/level.ngn",
        ] {
            assert!(super::is_language_file(name), "{name}");
        }
        for name in [
            "toy2.exe",
            "data/level07/level1.ngn",
            "data/level05/terrain.all",
            "data/level01/level1.raw",
            "data/pad/path02.bin",
            "data/gfx/level4b.raw",
            "audio/house.wav",
            "data/sfx/camera.txt",
        ] {
            assert!(!super::is_language_file(name), "{name}");
        }
    }

    use super::*;

    #[test]
    fn paths_are_confined_to_the_install_directory() {
        let parts = |path| Files::components(path).map(|p| p.join("/"));
        assert_eq!(parts("C:\\TOY2\\data\\LEVEL01\\level.ngn").as_deref(), Some("data/LEVEL01/level.ngn"));
        assert_eq!(parts("c:\\toy2\\toy2.cfg").as_deref(), Some("toy2.cfg"));
        assert_eq!(parts("toy2.err").as_deref(), Some("toy2.err"));
        assert_eq!(parts(".\\data\\..\\validate.tta").as_deref(), Some("validate.tta"));
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
        let handle = files.open("C:\\TOY2\\DATA\\level.bin", false, Disposition::OpenExisting).unwrap();
        assert_eq!(files.read(handle, 4), Some(&b"orig"[..]));
        assert_eq!(files.read(handle, 100), Some(&b"inal"[..]));
        assert!(files.write(handle, b"x").is_none());
        assert!(files.close(handle));
        assert_eq!(files.open("missing.dat", false, Disposition::OpenExisting), None);
        files.inject("Extra.INI", b"LOG".to_vec());
        let handle = files.open("C:\\TOY2\\extra.ini", false, Disposition::OpenExisting).unwrap();
        assert_eq!(files.read(handle, 10), Some(&b"LOG"[..]));

        let handle = files.open("data\\level.bin", true, Disposition::CreateAlways).unwrap();
        files.write(handle, b"changed").unwrap();
        files.close(handle);
        assert_eq!(fs::read(game.join("Data").join("Level.BIN")).unwrap(), b"original");
        // The new copy shadows the original from now on.
        let handle = files.open("DATA\\LEVEL.BIN", false, Disposition::OpenExisting).unwrap();
        assert_eq!(files.read(handle, 100), Some(&b"changed"[..]));
        files.close(handle);

        // Deleting hides the original without touching it, until the file is written again.
        assert!(files.delete("data\\level.bin"));
        assert!(!files.delete("data\\level.bin"));
        assert_eq!(files.open("data\\level.bin", false, Disposition::OpenExisting), None);
        assert_eq!(fs::read(game.join("Data").join("Level.BIN")).unwrap(), b"original");
        let handle = files.open("data\\level.bin", true, Disposition::OpenAlways).unwrap();
        assert_eq!(files.read(handle, 100), Some(&b""[..]));
        files.write(handle, b"new").unwrap();
        files.close(handle);
        let handle = files.open("data\\level.bin", false, Disposition::OpenExisting).unwrap();
        assert_eq!(files.read(handle, 100), Some(&b"new"[..]));
        fs::remove_dir_all(root).unwrap();
    }
}
