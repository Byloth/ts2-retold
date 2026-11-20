//! AMSTREAM: DirectShow's multimedia streams, which the game uses to play
//! its movies onto a DirectDraw surface.
//!
//! The object exists and is set up as the game sets it up. **No movie is
//! decoded yet**: opening a file that is there stops the machine. Opening a
//! file that is not there fails as it does on Windows, and the game carries
//! on without the movie.

use super::HostResult;
use super::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use crate::files::Disposition;
use crate::{Machine, Stop};

const E_POINTER: u32 = 0x8000_4003;
const E_INVALIDARG: u32 = 0x8007_0057;
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`: what the filter graph gives
/// for a source file that does not exist (UNVERIFIED for this interface;
/// the game only looks at the sign).
const FILE_NOT_FOUND: u32 = 0x8007_0002;

const CLSID_AM_MULTI_MEDIA_STREAM: Guid =
    guid(0x49C47CE5, 0x9BA4, 0x11D0, [0x82, 0x12, 0x00, 0xC0, 0x4F, 0xC3, 0x2C, 0x45]);
const IID_IMULTI_MEDIA_STREAM: Guid =
    guid(0xB502D1BC, 0x9A57, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const IID_IAM_MULTI_MEDIA_STREAM: Guid =
    guid(0xBEBE595C, 0x9A6F, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const MSPID_PRIMARY_VIDEO: Guid =
    guid(0xA35FF56A, 0x9FDA, 0x11D0, [0x8F, 0xDF, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const MSPID_PRIMARY_AUDIO: Guid =
    guid(0xA35FF56B, 0x9FDA, 0x11D0, [0x8F, 0xDF, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const STREAMTYPE_READ: u32 = 0;

#[derive(Debug)]
struct Stream {
    references: u32,
    pointer: u32,
    /// Whether it has a video and an audio stream.
    video: bool,
    audio: bool,
}

#[derive(Debug, Default)]
pub struct State {
    streams: Vec<Stream>,
}

fn not_implemented(what: String) -> Stop {
    Stop::Unimplemented { dll: "IAMMultiMediaStream".into(), name: what }
}

impl Machine {
    fn media_stream(&mut self, this: u32) -> Option<&mut Stream> {
        let index = self.object_index(this)?;
        self.os
            .amstream
            .streams
            .get_mut(index)
            .filter(|stream| stream.references > 0 && stream.pointer == this)
    }
}

/// `CoCreateInstance` for the classes of this module: class, outer object,
/// context, interface, result. `None` if the class is not one of them.
pub(super) fn create_instance(m: &mut Machine, class: &Guid, a: &[u32]) -> Option<u32> {
    let (outer, interface, out) = (a[1], a[3], a[4]);
    if *class != CLSID_AM_MULTI_MEDIA_STREAM {
        return None;
    }
    if outer != 0 {
        return Some(CLASS_E_NOAGGREGATION);
    }
    let interface = m.read_guid(interface)?;
    if ![IID_IUNKNOWN, IID_IMULTI_MEDIA_STREAM, IID_IAM_MULTI_MEDIA_STREAM].contains(&interface) {
        return Some(E_NOINTERFACE);
    }
    let index = m.os.amstream.streams.len() as u32;
    let pointer = m.interface_pointer(&MULTI_MEDIA_STREAM, index);
    m.os.amstream.streams.push(Stream { references: 1, pointer, video: false, audio: false });
    Some(if m.memory.write(out, 4, pointer).is_some() { S_OK } else { E_POINTER })
}

fn query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return E_INVALIDARG };
    let Some(stream) = m.media_stream(a[0]) else { return E_INVALIDARG };
    if ![IID_IUNKNOWN, IID_IMULTI_MEDIA_STREAM, IID_IAM_MULTI_MEDIA_STREAM].contains(&wanted) {
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    stream.references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { E_POINTER }
}

fn initialize(m: &mut Machine, a: &[u32]) -> HostResult {
    // Stream type, flags, a filter graph to use.
    if m.media_stream(a[0]).is_none() {
        return Ok(E_INVALIDARG);
    }
    if a[1] != STREAMTYPE_READ || a[3] != 0 {
        return Err(not_implemented(format!("Initialize for stream type {} or with a graph", a[1])));
    }
    Ok(S_OK)
}

fn add_media_stream(m: &mut Machine, a: &[u32]) -> HostResult {
    // The object the stream draws on or plays through, its purpose, flags,
    // where to put the new stream.
    let (purpose, out) = (a[2], a[4]);
    let Some(purpose) = m.read_guid(purpose) else { return Ok(E_POINTER) };
    let Some(stream) = m.media_stream(a[0]) else { return Ok(E_INVALIDARG) };
    match purpose {
        MSPID_PRIMARY_VIDEO => stream.video = true,
        MSPID_PRIMARY_AUDIO => stream.audio = true,
        _ => return Ok(E_INVALIDARG),
    }
    if out != 0 {
        return Err(not_implemented("AddMediaStream returning the stream".into()));
    }
    Ok(S_OK)
}

fn open_file(m: &mut Machine, a: &[u32]) -> HostResult {
    if a[1] == 0 {
        return Ok(E_POINTER);
    }
    if m.media_stream(a[0]).is_none() {
        return Ok(E_INVALIDARG);
    }
    // The name is in UTF-16.
    let mut name = Vec::new();
    while let Some(unit) = m.memory.read(a[1] + 2 * name.len() as u32, 2).filter(|&unit| unit != 0) {
        name.push(unit as u16);
    }
    let name = String::from_utf16_lossy(&name);
    m.note(|| format!("{name:?}"));
    match m.os.files.open(&name, false, Disposition::OpenExisting) {
        None => Ok(FILE_NOT_FOUND),
        Some(file) => {
            m.os.files.close(file);
            Err(not_implemented(format!("playing the movie {name}")))
        }
    }
}

interface! { static MULTI_MEDIA_STREAM = "IAMMultiMediaStream";
    QueryInterface(3) => query_interface,
    AddRef(1) => |m, a| m.media_stream(a[0]).map_or(0, |stream| {
        stream.references += 1;
        stream.references
    }),
    Release(1) => |m, a| m.media_stream(a[0]).map_or(0, |stream| {
        stream.references -= 1;
        stream.references
    }),
    GetInformation(3),
    GetMediaStream(3),
    EnumMediaStreams(3),
    GetState(2),
    SetState(2),
    GetTime(2),
    GetDuration(2),
    Seek(3),
    GetEndOfStreamEventHandle(2),
    Initialize(4) => initialize,
    GetFilterGraph(2),
    GetFilter(2),
    AddMediaStream(5) => add_media_stream,
    OpenFile(3) => open_file,
    OpenMoniker(4),
    Render(2),
}
