//! Minimal PE32 loader.
//!
//! Parses exactly what the recompiler needs from a 32-bit Windows executable:
//! sections, imports and base relocations. It works on a byte slice and does no
//! I/O, so it can run unchanged inside a browser.

use std::fmt;

/// Section characteristic: contains executable code.
pub const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
/// Section characteristic: writable.
pub const SCN_MEM_WRITE: u32 = 0x8000_0000;

const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
const PE32_MAGIC: u16 = 0x010B;
const DIR_IMPORT: usize = 1;
const DIR_RESOURCE: usize = 2;
const DIR_BASERELOC: usize = 5;
const REL_BASED_ABSOLUTE: u16 = 0;
const REL_BASED_HIGHLOW: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeError {
    /// The file ends before a structure that the headers say is there.
    Truncated(&'static str),
    /// A signature or magic number is wrong.
    BadMagic(&'static str),
    /// Valid PE, but not something this loader handles.
    Unsupported(String),
}

impl fmt::Display for PeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeError::Truncated(what) => write!(f, "truncated file while reading {what}"),
            PeError::BadMagic(what) => write!(f, "bad {what}"),
            PeError::Unsupported(what) => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for PeError {}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    /// Virtual address of the first byte (image base included).
    pub va: u32,
    /// Size in memory. Bytes past `data.len()` are zero-initialised.
    pub virtual_size: u32,
    pub flags: u32,
    /// Initialised bytes, never longer than `virtual_size`.
    pub data: Vec<u8>,
}

impl Section {
    pub fn contains(&self, va: u32) -> bool {
        va >= self.va && va - self.va < self.virtual_size
    }

    pub fn is_executable(&self) -> bool {
        self.flags & SCN_MEM_EXECUTE != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportName {
    Name(String),
    Ordinal(u16),
}

impl fmt::Display for ImportName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportName::Name(n) => f.write_str(n),
            ImportName::Ordinal(o) => write!(f, "#{o}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Import {
    pub dll: String,
    pub name: ImportName,
    /// Virtual address of the import address table slot.
    pub iat_va: u32,
}

/// A loaded executable image, addressed by virtual address.
#[derive(Debug, Clone, Default)]
pub struct Image {
    pub base: u32,
    pub entry: u32,
    pub timestamp: u32,
    pub linker_version: (u8, u8),
    pub sections: Vec<Section>,
    pub imports: Vec<Import>,
    /// Virtual addresses of every 32-bit absolute fixup, sorted ascending.
    pub relocs: Vec<u32>,
    /// Every resource, in directory order.
    pub resources: Vec<Resource>,
}

impl Image {
    pub fn parse(file: &[u8]) -> Result<Image, PeError> {
        let r = Reader(file);
        if r.u16(0, "DOS header")? != 0x5A4D {
            return Err(PeError::BadMagic("DOS signature"));
        }
        let pe = r.u32(0x3C, "DOS header")? as usize;
        if r.u32(pe, "PE signature")? != 0x0000_4550 {
            return Err(PeError::BadMagic("PE signature"));
        }
        let coff = pe + 4;
        if r.u16(coff, "COFF header")? != IMAGE_FILE_MACHINE_I386 {
            return Err(PeError::Unsupported("machine is not i386".into()));
        }
        let section_count = r.u16(coff + 2, "COFF header")? as usize;
        let timestamp = r.u32(coff + 4, "COFF header")?;
        let opt_size = r.u16(coff + 16, "COFF header")? as usize;
        let opt = coff + 20;
        if r.u16(opt, "optional header")? != PE32_MAGIC {
            return Err(PeError::Unsupported("not a PE32 optional header".into()));
        }
        let linker_version = (
            r.u8(opt + 2, "optional header")?,
            r.u8(opt + 3, "optional header")?,
        );
        let entry_rva = r.u32(opt + 16, "optional header")?;
        let base = r.u32(opt + 28, "optional header")?;
        let dir_count = r.u32(opt + 92, "optional header")? as usize;
        let dir = |index: usize| -> Result<(u32, u32), PeError> {
            if index >= dir_count {
                return Ok((0, 0));
            }
            let at = opt + 96 + index * 8;
            Ok((r.u32(at, "data directory")?, r.u32(at + 4, "data directory")?))
        };

        let mut sections = Vec::with_capacity(section_count);
        let table = opt + opt_size;
        for i in 0..section_count {
            let at = table + i * 40;
            let raw_name = r.bytes(at, 8, "section table")?;
            let name_len = raw_name.iter().position(|&b| b == 0).unwrap_or(8);
            let name = String::from_utf8_lossy(&raw_name[..name_len]).into_owned();
            let declared_vsize = r.u32(at + 8, "section table")?;
            let rva = r.u32(at + 12, "section table")?;
            let raw_size = r.u32(at + 16, "section table")?;
            let raw_ptr = r.u32(at + 20, "section table")? as usize;
            let flags = r.u32(at + 36, "section table")?;
            // A zero virtual size means "use the raw size" (old linkers).
            let virtual_size = if declared_vsize == 0 {
                raw_size
            } else {
                declared_vsize
            };
            let init_len = raw_size.min(virtual_size) as usize;
            let data = r.bytes(raw_ptr, init_len, "section data")?.to_vec();
            let va = base
                .checked_add(rva)
                .filter(|va| va.checked_add(virtual_size).is_some())
                .ok_or_else(|| PeError::Unsupported(format!("section {name} wraps the address space")))?;
            sections.push(Section {
                name,
                va,
                virtual_size,
                flags,
                data,
            });
        }

        let mut image = Image {
            base,
            entry: base.wrapping_add(entry_rva),
            timestamp,
            linker_version,
            sections,
            imports: Vec::new(),
            relocs: Vec::new(),
            resources: Vec::new(),
        };
        let (import_rva, _) = dir(DIR_IMPORT)?;
        if import_rva != 0 {
            image.parse_imports(base.wrapping_add(import_rva))?;
        }
        let (resource_rva, _) = dir(DIR_RESOURCE)?;
        if resource_rva != 0 {
            image.parse_resources(base.wrapping_add(resource_rva))?;
        }
        let (reloc_rva, reloc_size) = dir(DIR_BASERELOC)?;
        if reloc_rva != 0 {
            image.parse_relocs(base.wrapping_add(reloc_rva), reloc_size)?;
        }
        Ok(image)
    }

    fn parse_imports(&mut self, mut descriptor: u32) -> Result<(), PeError> {
        const WHAT: &str = "import table";
        loop {
            let lookup_rva = self.need_u32(descriptor, WHAT)?;
            let name_rva = self.need_u32(descriptor + 12, WHAT)?;
            let iat_rva = self.need_u32(descriptor + 16, WHAT)?;
            if name_rva == 0 {
                return Ok(());
            }
            let dll = self
                .c_string(self.base.wrapping_add(name_rva))
                .ok_or(PeError::Truncated(WHAT))?;
            // The lookup table survives binding; fall back to the IAT without one.
            let thunks = self
                .base
                .wrapping_add(if lookup_rva != 0 { lookup_rva } else { iat_rva });
            for index in 0u32.. {
                let thunk = self.need_u32(thunks + index * 4, WHAT)?;
                if thunk == 0 {
                    break;
                }
                let name = if thunk & 0x8000_0000 != 0 {
                    ImportName::Ordinal(thunk as u16)
                } else {
                    // Skip the 16-bit hint that precedes the name.
                    let va = self.base.wrapping_add(thunk).wrapping_add(2);
                    ImportName::Name(self.c_string(va).ok_or(PeError::Truncated(WHAT))?)
                };
                let iat_va = self.base.wrapping_add(iat_rva).wrapping_add(index * 4);
                self.imports.push(Import {
                    dll: dll.clone(),
                    name,
                    iat_va,
                });
            }
            descriptor += 20;
        }
    }

    fn parse_relocs(&mut self, start: u32, size: u32) -> Result<(), PeError> {
        const WHAT: &str = "relocation table";
        let end = start.checked_add(size).ok_or(PeError::Truncated(WHAT))?;
        let mut block = start;
        while block + 8 <= end {
            let page = self.need_u32(block, WHAT)?;
            let block_size = self.need_u32(block + 4, WHAT)?;
            if block_size < 8 || block_size % 2 != 0 || block + block_size > end {
                if block_size == 0 {
                    break; // zero padding at the end of the directory
                }
                return Err(PeError::Unsupported(format!(
                    "relocation block of size {block_size}"
                )));
            }
            for at in (block + 8..block + block_size).step_by(2) {
                let entry = u16::from_le_bytes([
                    self.read_u8(at).ok_or(PeError::Truncated(WHAT))?,
                    self.read_u8(at + 1).ok_or(PeError::Truncated(WHAT))?,
                ]);
                match entry >> 12 {
                    REL_BASED_ABSOLUTE => {}
                    REL_BASED_HIGHLOW => {
                        self.relocs
                            .push(self.base.wrapping_add(page).wrapping_add((entry & 0xFFF) as u32));
                    }
                    other => {
                        return Err(PeError::Unsupported(format!("relocation type {other}")));
                    }
                }
            }
            block += block_size;
        }
        self.relocs.sort_unstable();
        self.relocs.dedup();
        Ok(())
    }

    pub fn section_at(&self, va: u32) -> Option<&Section> {
        self.sections.iter().find(|s| s.contains(va))
    }

    /// The section holding the entry point, i.e. the program's code.
    pub fn code_section(&self) -> Option<&Section> {
        self.section_at(self.entry).filter(|s| s.is_executable())
    }

    /// Reads one byte; zero for the uninitialised tail of a section.
    pub fn read_u8(&self, va: u32) -> Option<u8> {
        let section = self.section_at(va)?;
        Some(section.data.get((va - section.va) as usize).copied().unwrap_or(0))
    }

    /// Reads a little-endian `u32` that lies entirely inside one section.
    pub fn read_u32(&self, va: u32) -> Option<u32> {
        let section = self.section_at(va)?;
        let offset = va - section.va;
        if section.virtual_size - offset < 4 {
            return None;
        }
        let mut bytes = [0u8; 4];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = section.data.get(offset as usize + i).copied().unwrap_or(0);
        }
        Some(u32::from_le_bytes(bytes))
    }

    fn need_u32(&self, va: u32, what: &'static str) -> Result<u32, PeError> {
        self.read_u32(va).ok_or(PeError::Truncated(what))
    }

    /// Reads a NUL-terminated string (lossy for non-UTF-8 bytes).
    pub fn c_string(&self, va: u32) -> Option<String> {
        let section = self.section_at(va)?;
        let tail = section.data.get((va - section.va) as usize..)?;
        let len = tail.iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&tail[..len]).into_owned())
    }

    /// True if a 32-bit absolute fixup is applied exactly at `va`.
    pub fn is_reloc(&self, va: u32) -> bool {
        self.relocs.binary_search(&va).is_ok()
    }

    /// Fixups whose address lies in `start..end`.
    pub fn relocs_in(&self, start: u32, end: u32) -> &[u32] {
        let lo = self.relocs.partition_point(|&r| r < start);
        let hi = self.relocs.partition_point(|&r| r < end);
        &self.relocs[lo..hi]
    }

    pub fn import_at(&self, iat_va: u32) -> Option<&Import> {
        self.imports.iter().find(|i| i.iat_va == iat_va)
    }

    /// Walks the three-level resource directory: type, name, language.
    fn parse_resources(&mut self, root: u32) -> Result<(), PeError> {
        const WHAT: &str = "resource directory";
        let mut found = Vec::new();
        for (kind, types) in self.resource_entries(root, root)? {
            let Some(names) = types else { continue };
            for (name, languages) in self.resource_entries(root, names)? {
                let Some(languages) = languages else { continue };
                for (language, _) in self.resource_entries(root, languages)? {
                    // At the last level the entry points at a data descriptor.
                    let descriptor = self.resource_leaf(root, languages, &language)?;
                    let rva = self.need_u32(descriptor, WHAT)?;
                    let size = self.need_u32(descriptor + 4, WHAT)?;
                    let language = match language {
                        ResourceId::Id(id) => id,
                        ResourceId::Name(_) => 0,
                    };
                    found.push(Resource {
                        kind: kind.clone(),
                        name: name.clone(),
                        language,
                        va: self.base.wrapping_add(rva),
                        size,
                    });
                }
            }
        }
        self.resources = found;
        Ok(())
    }

    /// Entries of one directory: identifier, and the address of the
    /// subdirectory it leads to (`None` for a data entry).
    fn resource_entries(&self, root: u32, directory: u32) -> Result<Vec<(ResourceId, Option<u32>)>, PeError> {
        const WHAT: &str = "resource directory";
        let counts = self.need_u32(directory + 12, WHAT)?;
        let count = (counts & 0xFFFF) + (counts >> 16);
        if count > 0x1_0000 {
            return Err(PeError::Unsupported(
                "resource directory with too many entries".into(),
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for n in 0..count {
            let entry = directory + 16 + 8 * n;
            let id = self.need_u32(entry, WHAT)?;
            let target = self.need_u32(entry + 4, WHAT)?;
            let id = if id & 0x8000_0000 != 0 {
                // A length-prefixed UTF-16 string.
                let at = root.wrapping_add(id & 0x7FFF_FFFF);
                let len = self.need_u32(at, WHAT)? & 0xFFFF;
                let mut units = Vec::with_capacity(len as usize);
                for i in 0..len {
                    units.push(self.need_u32(at + 2 + 2 * i, WHAT)? as u16);
                }
                ResourceId::Name(String::from_utf16_lossy(&units))
            } else {
                ResourceId::Id(id as u16)
            };
            let subdirectory = (target & 0x8000_0000 != 0).then(|| root.wrapping_add(target & 0x7FFF_FFFF));
            entries.push((id, subdirectory));
        }
        Ok(entries)
    }

    fn resource_leaf(&self, root: u32, directory: u32, wanted: &ResourceId) -> Result<u32, PeError> {
        const WHAT: &str = "resource directory";
        let counts = self.need_u32(directory + 12, WHAT)?;
        let count = (counts & 0xFFFF) + (counts >> 16);
        for (n, (id, _)) in self
            .resource_entries(root, directory)?
            .iter()
            .enumerate()
            .take(count as usize)
        {
            if id == wanted {
                let target = self.need_u32(directory + 16 + 8 * n as u32 + 4, WHAT)?;
                return Ok(root.wrapping_add(target & 0x7FFF_FFFF));
            }
        }
        Err(PeError::Truncated(WHAT))
    }

    /// Finds a resource by type and name, in any language. Names compare
    /// without regard to case, as the resource functions do.
    pub fn resource(&self, kind: &ResourceId, name: &ResourceId) -> Option<&Resource> {
        self.resources
            .iter()
            .find(|r| r.kind.matches(kind) && r.name.matches(name))
    }
}

/// Identifier of a resource type or of a resource: a number or a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceId {
    Id(u16),
    Name(String),
}

impl ResourceId {
    pub fn matches(&self, other: &ResourceId) -> bool {
        match (self, other) {
            (ResourceId::Id(a), ResourceId::Id(b)) => a == b,
            (ResourceId::Name(a), ResourceId::Name(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceId::Id(id) => write!(f, "#{id}"),
            ResourceId::Name(name) => write!(f, "{name:?}"),
        }
    }
}

/// One resource: its data stays in the image, addressed by `va`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub kind: ResourceId,
    pub name: ResourceId,
    pub language: u16,
    pub va: u32,
    pub size: u32,
}

/// Standard resource types (`RT_*`).
pub mod resource_type {
    pub const CURSOR: u16 = 1;
    pub const BITMAP: u16 = 2;
    pub const ICON: u16 = 3;
    pub const MENU: u16 = 4;
    pub const DIALOG: u16 = 5;
    pub const STRING: u16 = 6;
    pub const ACCELERATOR: u16 = 9;
    pub const GROUP_CURSOR: u16 = 12;
    pub const GROUP_ICON: u16 = 14;

    pub fn name(kind: u16) -> &'static str {
        match kind {
            CURSOR => "cursor",
            BITMAP => "bitmap",
            ICON => "icon",
            MENU => "menu",
            DIALOG => "dialog",
            STRING => "string table",
            ACCELERATOR => "accelerators",
            GROUP_CURSOR => "cursor group",
            GROUP_ICON => "icon group",
            16 => "version",
            _ => "other",
        }
    }
}

/// Bounds-checked little-endian reads on the raw file.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn bytes(&self, at: usize, len: usize, what: &'static str) -> Result<&'a [u8], PeError> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or(PeError::Truncated(what))
    }

    fn u8(&self, at: usize, what: &'static str) -> Result<u8, PeError> {
        Ok(self.bytes(at, 1, what)?[0])
    }

    fn u16(&self, at: usize, what: &'static str) -> Result<u16, PeError> {
        let b = self.bytes(at, 2, what)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&self, at: usize, what: &'static str) -> Result<u32, PeError> {
        let b = self.bytes(at, 4, what)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage() {
        assert_eq!(Image::parse(b"").unwrap_err(), PeError::Truncated("DOS header"));
        assert_eq!(
            Image::parse(&[0u8; 64]).unwrap_err(),
            PeError::BadMagic("DOS signature")
        );
        let mut dos = vec![0u8; 0x80];
        dos[0] = b'M';
        dos[1] = b'Z';
        dos[0x3C] = 0x40;
        assert_eq!(Image::parse(&dos).unwrap_err(), PeError::BadMagic("PE signature"));
    }

    #[test]
    fn walks_the_resource_directory() {
        // One resource: type #5 (dialog), name "MAIN", language 0x409, 3 bytes at RVA 0x2080.
        let mut rsrc = vec![0u8; 0x100];
        let mut put = |at: usize, value: u32| rsrc[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(0x0C, 1 << 16); // root: one id entry
        put(0x10, 5);
        put(0x14, 0x8000_0020);
        put(0x2C, 1); // type directory: one named entry
        put(0x30, 0x8000_0060);
        put(0x34, 0x8000_0040);
        put(0x4C, 1 << 16); // name directory: one id entry
        put(0x50, 0x409);
        put(0x54, 0x70);
        put(0x70, 0x2080); // data descriptor: rva, size
        put(0x74, 3);
        rsrc[0x60..0x6A].copy_from_slice(&[4, 0, b'M', 0, b'A', 0, b'I', 0, b'N', 0]);
        let mut image = Image {
            base: 0x40_0000,
            sections: vec![Section {
                name: ".rsrc".into(),
                va: 0x40_2000,
                virtual_size: 0x100,
                flags: 0,
                data: rsrc,
            }],
            ..Image::default()
        };
        image.parse_resources(0x40_2000).unwrap();
        assert_eq!(
            image.resources,
            [Resource {
                kind: ResourceId::Id(5),
                name: ResourceId::Name("MAIN".into()),
                language: 0x409,
                va: 0x40_2080,
                size: 3,
            }]
        );
        let found = image.resource(
            &ResourceId::Id(resource_type::DIALOG),
            &ResourceId::Name("main".into()),
        );
        assert_eq!(found.map(|r| r.va), Some(0x40_2080));
        assert!(
            image
                .resource(&ResourceId::Id(4), &ResourceId::Name("main".into()))
                .is_none()
        );
    }

    #[test]
    fn reads_zero_fill_and_respects_section_bounds() {
        let image = Image {
            base: 0x40_0000,
            sections: vec![Section {
                name: ".data".into(),
                va: 0x40_1000,
                virtual_size: 8,
                flags: SCN_MEM_WRITE,
                data: vec![0x11, 0x22, 0x33, 0x44, 0x55],
            }],
            relocs: vec![0x40_1000, 0x40_1004],
            ..Image::default()
        };
        assert_eq!(image.read_u32(0x40_1000), Some(0x4433_2211));
        assert_eq!(image.read_u32(0x40_1004), Some(0x0000_0055));
        assert_eq!(image.read_u32(0x40_1005), None);
        assert_eq!(image.read_u8(0x40_1007), Some(0));
        assert_eq!(image.read_u8(0x40_1008), None);
        assert!(image.is_reloc(0x40_1004) && !image.is_reloc(0x40_1001));
        assert_eq!(image.relocs_in(0x40_1001, 0x40_1008), &[0x40_1004]);
    }
}
