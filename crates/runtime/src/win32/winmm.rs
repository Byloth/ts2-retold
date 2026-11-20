//! WINMM: the multimedia timer and RIFF file reading (`mmio*`).
//!
//! The game reads its `.wav` files with the wave-loading routines of the
//! DirectX SDK samples, which use both ways of reading that this API offers:
//! `mmioRead`, and direct access to the file's I/O buffer (`mmioGetInfo`,
//! `mmioAdvance`, `mmioSetInfo`). Writing RIFF files is not implemented.

use super::{HostResult, functions};
use crate::files::Disposition;
use crate::{Machine, Stop};

const MMSYSERR_INVALHANDLE: u32 = 5;
const MMSYSERR_INVALPARAM: u32 = 11;
const MMIOERR_CANNOTREAD: u32 = 261;
const MMIOERR_CHUNKNOTFOUND: u32 = 265;

const MMIO_WRITE: u32 = 0x0001;
const MMIO_READWRITE: u32 = 0x0002;
const MMIO_CREATE: u32 = 0x1000;
const MMIO_FINDCHUNK: u32 = 0x0010;
const MMIO_FINDRIFF: u32 = 0x0020;
const MMIO_FINDLIST: u32 = 0x0040;
/// `mmioAdvance`: fill the buffer by reading.
const MMIO_READ: u32 = 0;
const MMIO_DIRTY: u32 = 0x1000_0000;

const FOURCC_RIFF: u32 = u32::from_le_bytes(*b"RIFF");
const FOURCC_LIST: u32 = u32::from_le_bytes(*b"LIST");
/// Size of the I/O buffer Windows allocates by default.
const DEFAULT_BUFFER: u32 = 8192;
const FIRST_HANDLE: u32 = 0x0500_0000;

// MMIOINFO, in words: flags, I/O procedure code and address, error (16 bits)
// and task (16 bits, with padding), buffer size, buffer, next byte, end of
// valid data, end of buffer, buffer's file offset, disk offset, ...
const INFO_ERROR: u32 = 3;
const INFO_BUFFER_SIZE: u32 = 5;
const INFO_BUFFER: u32 = 6;
const INFO_NEXT: u32 = 7;
const INFO_END_READ: u32 = 8;
const INFO_END_WRITE: u32 = 9;
const INFO_BUFFER_OFFSET: u32 = 10;
const INFO_DISK_OFFSET: u32 = 11;
const INFO_WORDS: usize = 18;

/// One open multimedia file.
#[derive(Debug)]
struct Stream {
    /// Handle of the underlying file.
    file: u32,
    /// Logical read position, when no buffer contents are handed out.
    position: u32,
    /// The I/O buffer in guest memory.
    buffer: u32,
    /// File offset of the buffer's first byte, and how many bytes are valid.
    buffer_offset: u32,
    buffer_filled: u32,
}

#[derive(Debug, Default)]
pub struct State {
    streams: Vec<Option<Stream>>,
}

impl Machine {
    fn stream(&mut self, handle: u32) -> Option<&mut Stream> {
        let index = handle.checked_sub(FIRST_HANDLE)? as usize;
        self.os.mmio.streams.get_mut(index)?.as_mut()
    }

    /// Reads up to `len` bytes at the stream's position and advances it.
    fn stream_read(&mut self, handle: u32, len: u32) -> Option<Vec<u8>> {
        let (file, position) = self.stream(handle).map(|s| (s.file, s.position))?;
        self.os.files.get(file)?.position = position as u64;
        let bytes = self.os.files.read(file, len)?.to_vec();
        self.stream(handle)?.position += bytes.len() as u32;
        Some(bytes)
    }

    /// Loads the I/O buffer with the file's contents at the current position.
    fn fill_buffer(&mut self, handle: u32) -> Option<()> {
        let (position, buffer) = self.stream(handle).map(|s| (s.position, s.buffer))?;
        let bytes = self.stream_read(handle, DEFAULT_BUFFER)?;
        self.memory.load(buffer, &bytes)?;
        let stream = self.stream(handle)?;
        // Handing out the buffer does not consume the data: the position
        // moves when the program reports how far it read.
        stream.position = position;
        stream.buffer_offset = position;
        stream.buffer_filled = bytes.len() as u32;
        Some(())
    }

    /// Writes the buffer fields of an `MMIOINFO`.
    fn write_buffer_info(&mut self, handle: u32, info: u32) -> Option<()> {
        let stream = self.stream(handle)?;
        let (buffer, offset, filled) = (stream.buffer, stream.buffer_offset, stream.buffer_filled);
        let fields = [
            (INFO_BUFFER_SIZE, DEFAULT_BUFFER),
            (INFO_BUFFER, buffer),
            (INFO_NEXT, buffer),
            (INFO_END_READ, buffer + filled),
            (INFO_END_WRITE, buffer + DEFAULT_BUFFER),
            (INFO_BUFFER_OFFSET, offset),
            (INFO_DISK_OFFSET, offset + filled),
        ];
        for (field, value) in fields {
            self.memory.write(info + 4 * field, 4, value)?;
        }
        Some(())
    }

    /// Takes the position the program reached in the buffer from its `MMIOINFO`.
    fn adopt_buffer_position(&mut self, handle: u32, info: u32) -> Option<()> {
        let next = self.memory.read(info + 4 * INFO_NEXT, 4)?;
        let stream = self.stream(handle)?;
        let consumed = next.checked_sub(stream.buffer).filter(|&n| n <= stream.buffer_filled)?;
        stream.position = stream.buffer_offset + consumed;
        Some(())
    }
}

fn not_implemented(what: &str) -> HostResult {
    Err(Stop::Unimplemented { dll: "WINMM.dll".into(), name: what.into() })
}

fn open(m: &mut Machine, a: &[u32]) -> HostResult {
    let (name_address, info, flags) = (a[0], a[1], a[2]);
    if flags & (MMIO_WRITE | MMIO_READWRITE | MMIO_CREATE) != 0 {
        return not_implemented("mmioOpenA for writing");
    }
    let name = m.text(name_address);
    let file = m.os.files.open(&name, false, Disposition::OpenExisting);
    m.note(|| format!("{name:?}{}", if file.is_some() { "" } else { " -> not found" }));
    let Some(file) = file else {
        if info != 0 {
            // The error goes in the low half of the fourth word.
            const MMIOERR_FILENOTFOUND: u32 = 257;
            let _ = m.memory.write(info + 4 * INFO_ERROR, 2, MMIOERR_FILENOTFOUND);
        }
        return Ok(0);
    };
    let buffer = m.os.allocate(&mut m.memory, DEFAULT_BUFFER);
    let stream = Stream { file, position: 0, buffer, buffer_offset: 0, buffer_filled: 0 };
    let streams = &mut m.os.mmio.streams;
    let index = streams.iter().position(Option::is_none).unwrap_or_else(|| {
        streams.push(None);
        streams.len() - 1
    });
    streams[index] = Some(stream);
    Ok(FIRST_HANDLE + index as u32)
}

fn close(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = a[0].checked_sub(FIRST_HANDLE).map(|i| i as usize) else {
        return MMSYSERR_INVALHANDLE;
    };
    match m.os.mmio.streams.get_mut(index).and_then(Option::take) {
        Some(stream) => {
            m.os.files.close(stream.file);
            0
        }
        None => MMSYSERR_INVALHANDLE,
    }
}

fn read(m: &mut Machine, a: &[u32]) -> u32 {
    let (handle, out, len) = (a[0], a[1], a[2]);
    // A negative count is an error; the result is a signed byte count.
    if len as i32 >= 0
        && let Some(bytes) = m.stream_read(handle, len)
        && m.memory.load(out, &bytes).is_some()
    {
        return bytes.len() as u32;
    }
    u32::MAX
}

fn seek(m: &mut Machine, a: &[u32]) -> u32 {
    let (handle, offset, origin) = (a[0], a[1] as i32 as i64, a[2]);
    let Some((file, position)) = m.stream(handle).map(|s| (s.file, s.position)) else {
        return u32::MAX;
    };
    let Some(size) = m.os.files.get(file).map(|f| f.data.len() as i64) else {
        return u32::MAX;
    };
    let base = match origin {
        0 => 0,
        1 => position as i64,
        2 => size,
        _ => return u32::MAX,
    };
    match u32::try_from(base + offset) {
        Ok(target) if (target as i64) <= size => {
            m.stream(handle).expect("checked above").position = target;
            target
        }
        _ => u32::MAX,
    }
}

/// `mmioDescend`: enters the chunk at the current position, or searches
/// forward for one with a given identifier.
fn descend(m: &mut Machine, a: &[u32]) -> u32 {
    let (handle, chunk, parent, flags) = (a[0], a[1], a[2], a[3]);
    let Some(start) = m.stream(handle).map(|s| s.position) else {
        return MMSYSERR_INVALHANDLE;
    };
    // MMCKINFO: identifier, size, form type, data offset, flags.
    let [wanted_id, _, wanted_type, _, _]: [u32; 5] = m.read_words(chunk);
    let (search_id, search_type) = match flags & (MMIO_FINDCHUNK | MMIO_FINDRIFF | MMIO_FINDLIST) {
        0 => (None, None),
        MMIO_FINDCHUNK => (Some(wanted_id), None),
        MMIO_FINDRIFF => (Some(FOURCC_RIFF), Some(wanted_type)),
        MMIO_FINDLIST => (Some(FOURCC_LIST), Some(wanted_type)),
        _ => return MMSYSERR_INVALPARAM,
    };
    // A search inside a parent chunk does not leave it.
    let limit = if parent != 0 {
        let [_, size, _, data_offset, _]: [u32; 5] = m.read_words(parent);
        Some(data_offset.saturating_add(size))
    } else {
        None
    };
    loop {
        let position = m.stream(handle).expect("checked above").position;
        let header = m.stream_read(handle, 8).filter(|h| h.len() == 8);
        let within = limit.is_none_or(|limit| position.saturating_add(8) <= limit);
        let Some(header) = header.filter(|_| within) else {
            m.stream(handle).expect("checked above").position = start;
            return MMIOERR_CHUNKNOTFOUND;
        };
        let id = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let data_offset = position + 8;
        // RIFF and LIST chunks start with a four-character form type.
        let is_container = id == FOURCC_RIFF || id == FOURCC_LIST;
        let form_type = if is_container {
            match m.stream_read(handle, 4).filter(|t| t.len() == 4) {
                Some(t) => u32::from_le_bytes([t[0], t[1], t[2], t[3]]),
                None => {
                    m.stream(handle).expect("checked above").position = start;
                    return MMIOERR_CHUNKNOTFOUND;
                }
            }
        } else {
            0
        };
        let matches = search_id.is_none_or(|wanted| wanted == id)
            && search_type.is_none_or(|wanted| wanted == form_type);
        if matches {
            m.write_words(chunk, &[id, size, form_type, data_offset, 0]);
            return 0;
        }
        // Chunks are padded to an even size.
        let next = data_offset.saturating_add(size.saturating_add(1) & !1);
        m.stream(handle).expect("checked above").position = next;
    }
}

/// `mmioAscend`: moves past the end of a chunk entered with `mmioDescend`.
fn ascend(m: &mut Machine, a: &[u32]) -> HostResult {
    let [_, size, _, data_offset, flags]: [u32; 5] = m.read_words(a[1]);
    if flags & MMIO_DIRTY != 0 {
        return not_implemented("mmioAscend on a chunk being written");
    }
    let Some(stream) = m.stream(a[0]) else {
        return Ok(MMSYSERR_INVALHANDLE);
    };
    stream.position = data_offset.saturating_add(size.saturating_add(1) & !1);
    Ok(0)
}

fn get_info(m: &mut Machine, a: &[u32]) -> u32 {
    let (handle, info) = (a[0], a[1]);
    if m.stream(handle).is_none() {
        return MMSYSERR_INVALHANDLE;
    }
    let ok = m.memory.load(info, &[0; 4 * INFO_WORDS]).is_some()
        && m.fill_buffer(handle).is_some()
        && m.write_buffer_info(handle, info).is_some()
        && m.memory.write(info + 4 * (INFO_WORDS as u32 - 1), 4, handle).is_some();
    if ok { 0 } else { MMSYSERR_INVALPARAM }
}

fn set_info(m: &mut Machine, a: &[u32]) -> u32 {
    if m.stream(a[0]).is_none() {
        return MMSYSERR_INVALHANDLE;
    }
    if m.adopt_buffer_position(a[0], a[1]).is_some() { 0 } else { MMSYSERR_INVALPARAM }
}

fn advance(m: &mut Machine, a: &[u32]) -> HostResult {
    let (handle, info, flags) = (a[0], a[1], a[2]);
    if flags != MMIO_READ {
        return not_implemented("mmioAdvance for writing");
    }
    if m.stream(handle).is_none() {
        return Ok(MMSYSERR_INVALHANDLE);
    }
    let ok = m.adopt_buffer_position(handle, info).is_some()
        && m.fill_buffer(handle).is_some()
        && m.write_buffer_info(handle, info).is_some();
    Ok(if ok { 0 } else { MMIOERR_CANNOTREAD })
}

functions! { "WINMM.dll";
    timeGetTime(0) => |m, _| m.os.now_ms,
    mmioOpenA(3) => open,
    mmioClose(2) => close,
    mmioRead(3) => read,
    mmioSeek(3) => seek,
    mmioDescend(4) => descend,
    mmioAscend(3) => ascend,
    mmioGetInfo(3) => get_info,
    mmioSetInfo(3) => set_info,
    mmioAdvance(3) => advance,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    /// A RIFF file with a format chunk of odd size (to test padding), an
    /// unrelated chunk and 20,000 bytes of data.
    fn wave_file() -> Vec<u8> {
        let mut body = b"WAVE".to_vec();
        let mut chunk = |id: &[u8; 4], data: &[u8]| {
            body.extend_from_slice(id);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        };
        chunk(b"fmt ", &[1, 0, 1, 0, 0x22, 0x56, 0, 0, 0x22, 0x56, 0, 0, 1, 0, 8, 0, 0]);
        chunk(b"fact", &[9, 9, 9, 9]);
        let samples: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        chunk(b"data", &samples);
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn reads_a_wave_file_the_way_the_sdk_loader_does() {
        let mut m = testing::machine();
        m.os.files.inject("sfx/test.wav", wave_file());
        let name = m.os.string(&mut m.memory, b"C:\\TOY2\\sfx\\TEST.WAV");
        let (riff, chunk, info, out) = (m.scratch(20), m.scratch(20), m.scratch(72), m.scratch(64));

        let missing = m.os.string(&mut m.memory, b"nothing.wav");
        assert_eq!(m.host("WINMM.dll", "mmioOpenA", &[missing, 0, 0]), 0);
        let handle = m.host("WINMM.dll", "mmioOpenA", &[name, 0, 0x1_0000]);
        assert_ne!(handle, 0);

        // The outer RIFF chunk, then the format chunk inside it.
        assert_eq!(m.host("WINMM.dll", "mmioDescend", &[handle, riff, 0, 0]), 0);
        let [id, _, form, data_offset, _]: [u32; 5] = m.read_words(riff);
        assert_eq!((id, form, data_offset), (FOURCC_RIFF, u32::from_le_bytes(*b"WAVE"), 8));
        m.memory.write(chunk, 4, u32::from_le_bytes(*b"fmt ")).unwrap();
        assert_eq!(m.host("WINMM.dll", "mmioDescend", &[handle, chunk, riff, MMIO_FINDCHUNK]), 0);
        assert_eq!(m.read_words::<5>(chunk)[1], 17);
        assert_eq!(m.host("WINMM.dll", "mmioRead", &[handle, out, 16]), 16);
        assert_eq!(m.memory.read(out + 4, 4), Some(22_050));
        assert_eq!(m.host("WINMM.dll", "mmioAscend", &[handle, chunk, 0]), 0);

        // Back to the start of the form, then find the data, skipping "fact".
        assert_eq!(m.host("WINMM.dll", "mmioSeek", &[handle, data_offset + 4, 0]), 12);
        m.memory.write(chunk, 4, u32::from_le_bytes(*b"data")).unwrap();
        assert_eq!(m.host("WINMM.dll", "mmioDescend", &[handle, chunk, riff, MMIO_FINDCHUNK]), 0);
        let [_, size, _, samples_at, _]: [u32; 5] = m.read_words(chunk);
        assert_eq!(size, 20_000);

        // Direct buffer access: consume everything, 8192 bytes at a time.
        assert_eq!(m.host("WINMM.dll", "mmioGetInfo", &[handle, info, 0]), 0);
        let mut total = 0u32;
        while total < size {
            let [next, end]: [u32; 2] = m.read_words(info + 4 * INFO_NEXT);
            if next == end {
                assert_eq!(m.host("WINMM.dll", "mmioAdvance", &[handle, info, MMIO_READ]), 0);
                continue;
            }
            let take = (end - next).min(size - total);
            for i in [0, take - 1] {
                assert_eq!(m.memory.read_u8(next + i), Some(((total + i) % 251) as u8));
            }
            m.memory.write(info + 4 * INFO_NEXT, 4, next + take).unwrap();
            total += take;
        }
        assert_eq!(m.host("WINMM.dll", "mmioSetInfo", &[handle, info, 0]), 0);
        assert_eq!(m.host("WINMM.dll", "mmioSeek", &[handle, 0, 1]), samples_at + size);

        // A chunk that is not there leaves the position where it was.
        m.memory.write(chunk, 4, u32::from_le_bytes(*b"cue ")).unwrap();
        assert_eq!(
            m.host("WINMM.dll", "mmioDescend", &[handle, chunk, riff, MMIO_FINDCHUNK]),
            MMIOERR_CHUNKNOTFOUND
        );
        assert_eq!(m.host("WINMM.dll", "mmioSeek", &[handle, 0, 1]), samples_at + size);
        assert_eq!(m.host("WINMM.dll", "mmioClose", &[handle, 0]), 0);
        assert_eq!(m.host("WINMM.dll", "mmioClose", &[handle, 0]), MMSYSERR_INVALHANDLE);
    }
}
