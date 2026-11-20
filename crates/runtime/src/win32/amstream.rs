//! AMSTREAM: DirectShow's multimedia streams, which the game uses to play
//! its movies onto a DirectDraw surface.
//!
//! A movie is an MPEG-1 system stream (ADR 0015). The program asks for one
//! picture at a time: the video stream's sample is `Update`d, which waits
//! until the next picture is due on the movie's clock and puts it in the
//! sample's surface. The clock is the machine's own, started when the stream
//! is set running, so a movie plays the same in every run.
//!
//! **The sound** is not asked for by the program at all: it adds the audio
//! stream with "the default renderer", which plays it by itself for as
//! long as the stream runs. Here that renderer is the runtime's mixer
//! (ADR 0019): while a stream runs, its movie's sound is part of the mix,
//! at the pace of the same clock the pictures are due by.

use std::collections::VecDeque;
use std::sync::Arc;

use andy_mpeg::video::Decoder;

use super::HostResult;
use super::com::{E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use super::ddraw::surface::{DDSCAPS_OFFSCREENPLAIN, DDSCAPS_SYSTEMMEMORY};
use crate::audio::OUTPUT_RATE;
use crate::files::Disposition;
use crate::threads::Waiting;
use crate::video::PixelFormat;
use crate::{Machine, Stop};

const E_POINTER: u32 = 0x8000_4003;
const E_INVALIDARG: u32 = 0x8007_0057;
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;
/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`: what the filter graph gives
/// for a source file that does not exist (UNVERIFIED for this interface;
/// the game only looks at the sign).
const FILE_NOT_FOUND: u32 = 0x8007_0002;
/// `MS_S_ENDOFSTREAM`, `MS_E_NOSTREAM`, `MS_E_NOTRUNNING`: codes of the
/// interface's own facility, successes from 0x40000 and errors from
/// 0x80040400.
const MS_S_ENDOFSTREAM: u32 = 0x0004_0003;
const MS_E_NOSTREAM: u32 = 0x8004_0403;
const MS_E_NOTRUNNING: u32 = 0x8004_040A;

const CLSID_AM_MULTI_MEDIA_STREAM: Guid =
    guid(0x49C47CE5, 0x9BA4, 0x11D0, [0x82, 0x12, 0x00, 0xC0, 0x4F, 0xC3, 0x2C, 0x45]);
const IID_IMULTI_MEDIA_STREAM: Guid =
    guid(0xB502D1BC, 0x9A57, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const IID_IMEDIA_STREAM: Guid =
    guid(0xB502D1BD, 0x9A57, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const IID_ISTREAM_SAMPLE: Guid =
    guid(0xB502D1BE, 0x9A57, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const IID_IAM_MULTI_MEDIA_STREAM: Guid =
    guid(0xBEBE595C, 0x9A6F, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const IID_IDIRECT_DRAW_MEDIA_STREAM: Guid =
    guid(0xF4104FCE, 0x9A70, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const IID_IDIRECT_DRAW_STREAM_SAMPLE: Guid =
    guid(0xF4104FCF, 0x9A70, 0x11D0, [0x8F, 0xDE, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const MSPID_PRIMARY_VIDEO: Guid =
    guid(0xA35FF56A, 0x9FDA, 0x11D0, [0x8F, 0xDF, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
const MSPID_PRIMARY_AUDIO: Guid =
    guid(0xA35FF56B, 0x9FDA, 0x11D0, [0x8F, 0xDF, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0x9D]);
/// `AMMSF_ADDDEFAULTRENDERER`: the stream plays through a renderer of its
/// own instead of giving its samples to the program.
const AMMSF_ADDDEFAULTRENDERER: u32 = 1;
const STREAMTYPE_READ: u32 = 0;
const STREAMSTATE_STOP: u32 = 0;
const STREAMSTATE_RUN: u32 = 1;
/// `MMSSF_*`: the stream has a clock, can seek, and works on its own thread.
const INFORMATION: u32 = 0x1 | 0x2 | 0x4;
const SSUPDATE_CONTINUOUS: u32 = 2;

/// An opened movie.
struct Movie {
    /// The video stream, kept to start again from.
    video: Arc<[u8]>,
    decoder: Decoder,
    /// How many pictures have been delivered.
    delivered: u64,
    /// Its sound, if it has any and the program asked for it to be played.
    sound: Option<Sound>,
}

/// Where the playing of a movie's sound is: the samples decoded and not
/// yet played, and the start, where sound and pictures may not begin at
/// the same moment.
#[derive(Debug, Clone, PartialEq)]
struct Track {
    /// Samples a second.
    rate: u32,
    /// Decoded samples, the first of them the one being played.
    pending: VecDeque<[f32; 2]>,
    /// How far the playing is from the first pending sample to the second.
    between: f64,
    /// Frames of the mix that pass before the sound begins, if it begins
    /// after the first picture.
    lead: u64,
    /// Samples left out at the start, if it begins before the first picture.
    skip: u64,
    ended: bool,
}

impl Track {
    /// A track at its start. `offset` is when the sound begins, in seconds
    /// after the first picture (negative: before it).
    fn new(rate: u32, offset: f64) -> Track {
        Track {
            rate,
            pending: VecDeque::new(),
            between: 0.0,
            lead: (offset.max(0.0) * OUTPUT_RATE as f64).round() as u64,
            skip: ((-offset).max(0.0) * rate as f64).round() as u64,
            ended: false,
        }
    }

    /// Adds the next stretch of the sound to `out`, a stretch of the mix.
    /// `more` gives the next decoded samples, or nothing at the end.
    /// Between two samples the value is taken on the straight line from
    /// one to the other, as the mixer does for sound buffers; at the mix's
    /// own rate the samples are played as they are.
    fn mix_into(&mut self, out: &mut [[f32; 2]], mut more: impl FnMut() -> Option<Vec<[f32; 2]>>) {
        let step = self.rate as f64 / OUTPUT_RATE as f64;
        for mixed in out {
            if self.lead > 0 {
                self.lead -= 1;
                continue;
            }
            while self.pending.len() < 2 && !self.ended {
                match more() {
                    Some(samples) => {
                        let skipped = (self.skip as usize).min(samples.len());
                        self.skip -= skipped as u64;
                        self.pending.extend(&samples[skipped..]);
                    }
                    None => self.ended = true,
                }
            }
            let Some(&from) = self.pending.front() else { return };
            // After the last sample comes silence.
            let to = self.pending.get(1).copied().unwrap_or_default();
            let between = self.between as f32;
            mixed[0] += from[0] + (to[0] - from[0]) * between;
            mixed[1] += from[1] + (to[1] - from[1]) * between;
            self.between += step;
            while self.between >= 1.0 && self.pending.pop_front().is_some() {
                self.between -= 1.0;
            }
        }
    }
}

/// A movie's sound: its stream, the decoder and the playing.
struct Sound {
    /// The audio stream, kept to start again from.
    stream: Arc<[u8]>,
    decoder: andy_mpeg::audio::Decoder,
    /// When the sound begins, in seconds after the first picture.
    offset: f64,
    track: Track,
}

impl Sound {
    fn new(stream: Arc<[u8]>, offset: f64) -> Option<Sound> {
        let decoder = andy_mpeg::audio::Decoder::new(stream.clone())?;
        let track = Track::new(decoder.rate(), offset);
        Some(Sound { stream, decoder, offset, track })
    }

    fn rewind(&mut self) {
        *self = Sound::new(self.stream.clone(), self.offset).expect("the stream was decoded before");
    }

    fn mix_into(&mut self, out: &mut [[f32; 2]]) {
        let decoder = &mut self.decoder;
        self.track.mix_into(out, || decoder.decode().map(|samples| samples.to_vec()));
    }
}

impl std::fmt::Debug for Movie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Movie {{ {} x {}, {} pictures delivered }}",
            self.decoder.width(),
            self.decoder.height(),
            self.delivered
        )
    }
}

#[derive(Debug)]
struct Stream {
    references: u32,
    pointer: u32,
    /// Whether it has a video and an audio stream.
    video: bool,
    audio: bool,
    movie: Option<Movie>,
    /// The machine's clock, in milliseconds, at the movie's time zero, while
    /// the stream is running.
    started_ms: Option<u64>,
    /// The video stream as an object of its own: its interface pointer, once
    /// asked for, and its reference count.
    video_pointer: u32,
    video_references: u32,
}

/// A picture of the video stream on a surface.
#[derive(Debug)]
struct Sample {
    references: u32,
    pointer: u32,
    stream: usize,
    surface: usize,
    /// Whether an `Update` is waiting for its picture's time.
    waiting: bool,
}

#[derive(Debug, Default)]
pub struct State {
    streams: Vec<Stream>,
    samples: Vec<Sample>,
}

impl State {
    /// Adds the sound of the movies that are running to `out`, a stretch
    /// of the mix. Called by the mixer as the machine's clock advances.
    pub(crate) fn mix_into(&mut self, out: &mut [[f32; 2]]) {
        for stream in &mut self.streams {
            if stream.references == 0 || !stream.audio || stream.started_ms.is_none() {
                continue;
            }
            if let Some(sound) = stream.movie.as_mut().and_then(|movie| movie.sound.as_mut()) {
                sound.mix_into(out);
            }
        }
    }
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

    /// The stream whose video stream an interface pointer names.
    fn video_stream(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let stream = self.os.amstream.streams.get(index)?;
        (stream.video_references > 0 && stream.video_pointer == this).then_some(index)
    }

    fn sample(&mut self, this: u32) -> Option<usize> {
        let index = self.object_index(this)?;
        let sample = self.os.amstream.samples.get(index)?;
        (sample.references > 0 && sample.pointer == this).then_some(index)
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
    m.os.amstream.streams.push(Stream {
        references: 1,
        pointer,
        video: false,
        audio: false,
        movie: None,
        started_ms: None,
        video_pointer: 0,
        video_references: 0,
    });
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
    let (object, purpose, flags, out) = (a[1], a[2], a[3], a[4]);
    let Some(purpose) = m.read_guid(purpose) else { return Ok(E_POINTER) };
    let Some(stream) = m.media_stream(a[0]) else { return Ok(E_INVALIDARG) };
    match purpose {
        MSPID_PRIMARY_VIDEO => stream.video = true,
        // The sound is only known played by its own renderer.
        MSPID_PRIMARY_AUDIO if object == 0 && flags == AMMSF_ADDDEFAULTRENDERER => stream.audio = true,
        MSPID_PRIMARY_AUDIO => {
            return Err(not_implemented(format!(
                "an audio stream without the default renderer (flags {flags:#x})"
            )));
        }
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
    let Some(file) = m.os.files.open(&name, false, Disposition::OpenExisting) else {
        m.note(|| format!("{name:?}: no such file"));
        return Ok(FILE_NOT_FOUND);
    };
    let bytes = m.os.files.read(file, u32::MAX).map(<[u8]>::to_vec).unwrap_or_default();
    m.os.files.close(file);
    let streams = andy_mpeg::demux::demux(bytes);
    // When the sound begins, counted from the first picture.
    let first = |stream: &andy_mpeg::demux::Stream| stream.times.first().map(|&(_, time)| time);
    let offset = streams
        .as_ref()
        .and_then(|streams| Some(first(&streams.audio)? - first(&streams.video)?))
        .unwrap_or(0.0);
    let (video, audio): (Option<Arc<[u8]>>, Option<Arc<[u8]>>) = match streams {
        Some(streams) => (Some(streams.video.bytes.into()), Some(streams.audio.bytes.into())),
        None => (None, None),
    };
    let Some((video, Ok(decoder))) = video.map(|video| (video.clone(), Decoder::new(video))) else {
        return Err(not_implemented(format!("the movie {name}, which is not an MPEG-1 system stream")));
    };
    // A movie without sound, or one whose sound nothing was asked to play.
    let wants_sound = m.media_stream(a[0]).is_some_and(|stream| stream.audio);
    let sound =
        audio.filter(|audio| wants_sound && !audio.is_empty()).and_then(|audio| Sound::new(audio, offset));
    m.mix_sound();
    let (rate, size) = (decoder.rate(), (decoder.width(), decoder.height()));
    m.note(|| format!("{name:?}: {} x {}, {}/{} pictures per second", size.0, size.1, rate.0, rate.1));
    let stream = m.media_stream(a[0]).expect("checked above");
    stream.movie = Some(Movie { video, decoder, delivered: 0, sound });
    Ok(S_OK)
}

fn get_information(m: &mut Machine, a: &[u32]) -> u32 {
    // Where to put the flags and the stream type; either may be null.
    if m.media_stream(a[0]).is_none() {
        return E_INVALIDARG;
    }
    let mut ok = true;
    if a[1] != 0 {
        ok &= m.memory.write(a[1], 4, INFORMATION).is_some();
    }
    if a[2] != 0 {
        ok &= m.memory.write(a[2], 4, STREAMTYPE_READ).is_some();
    }
    if ok { S_OK } else { E_POINTER }
}

fn get_media_stream(m: &mut Machine, a: &[u32]) -> HostResult {
    let Some(purpose) = m.read_guid(a[1]) else { return Ok(E_POINTER) };
    let Some(index) = m.object_index(a[0]).filter(|_| m.media_stream(a[0]).is_some()) else {
        return Ok(E_INVALIDARG);
    };
    let stream = &m.os.amstream.streams[index];
    match purpose {
        MSPID_PRIMARY_VIDEO if stream.video => {}
        MSPID_PRIMARY_AUDIO if stream.audio => {
            return Err(not_implemented("GetMediaStream for the audio".into()));
        }
        _ => return Ok(MS_E_NOSTREAM),
    }
    if stream.video_pointer == 0 {
        let pointer = m.interface_pointer(&VIDEO_STREAM, index as u32);
        m.os.amstream.streams[index].video_pointer = pointer;
    }
    let stream = &mut m.os.amstream.streams[index];
    stream.video_references += 1;
    let pointer = stream.video_pointer;
    Ok(if m.memory.write(a[2], 4, pointer).is_some() { S_OK } else { E_POINTER })
}

fn set_state(m: &mut Machine, a: &[u32]) -> HostResult {
    // What was heard up to now is mixed before anything changes.
    m.mix_sound();
    let now = m.clock_ms();
    let Some(stream) = m.media_stream(a[0]) else { return Ok(E_INVALIDARG) };
    match a[1] {
        STREAMSTATE_RUN => {
            // The movie's clock starts, or goes on from the pictures
            // already delivered.
            if stream.started_ms.is_none() {
                let position = stream.movie.as_ref().map_or(0, |movie| movie.due_ms(movie.delivered));
                stream.started_ms = Some(now.saturating_sub(position));
            }
        }
        STREAMSTATE_STOP => stream.started_ms = None,
        _ => return Ok(E_INVALIDARG),
    }
    Ok(S_OK)
}

/// `Seek`: to a time in units of 100 nanoseconds. Only the start can be
/// sought.
fn seek(m: &mut Machine, a: &[u32]) -> HostResult {
    m.mix_sound();
    let now = m.clock_ms();
    let Some(stream) = m.media_stream(a[0]) else { return Ok(E_INVALIDARG) };
    if (a[1], a[2]) != (0, 0) {
        return Err(not_implemented("Seek to anywhere but the start".into()));
    }
    if let Some(movie) = &mut stream.movie {
        movie.decoder = Decoder::new(movie.video.clone()).expect("the stream was decoded before");
        movie.delivered = 0;
        if let Some(sound) = &mut movie.sound {
            sound.rewind();
        }
        stream.started_ms = stream.started_ms.map(|_| now);
    }
    Ok(S_OK)
}

impl Movie {
    /// When, in milliseconds from the movie's start, a picture is due.
    fn due_ms(&self, picture: u64) -> u64 {
        let (pictures, seconds) = self.decoder.rate();
        picture * 1000 * seconds as u64 / pictures as u64
    }
}

interface! { static MULTI_MEDIA_STREAM = "IAMMultiMediaStream";
    QueryInterface(3) => query_interface,
    AddRef(1) => |m, a| m.media_stream(a[0]).map_or(0, |stream| {
        stream.references += 1;
        stream.references
    }),
    Release(1) => |m, a| {
        m.mix_sound();
        m.media_stream(a[0]).map_or(0, |stream| {
            stream.references -= 1;
            if stream.references == 0 {
                stream.movie = None;
            }
            stream.references
        })
    },
    GetInformation(3) => get_information,
    GetMediaStream(3) => get_media_stream,
    EnumMediaStreams(3),
    GetState(2),
    SetState(2) => set_state,
    GetTime(2),
    GetDuration(2),
    Seek(3) => seek,
    GetEndOfStreamEventHandle(2),
    Initialize(4) => initialize,
    GetFilterGraph(2),
    GetFilter(2),
    AddMediaStream(5) => add_media_stream,
    OpenFile(3) => open_file,
    OpenMoniker(4),
    Render(2),
}

// --- IDirectDrawMediaStream: the video stream ------------------------------

fn video_query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return E_INVALIDARG };
    let Some(index) = m.video_stream(a[0]) else { return E_INVALIDARG };
    if ![IID_IUNKNOWN, IID_IMEDIA_STREAM, IID_IDIRECT_DRAW_MEDIA_STREAM].contains(&wanted) {
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.amstream.streams[index].video_references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { E_POINTER }
}

/// `CreateSample`: a surface to receive pictures. Given none, the stream
/// makes one of the movie's size in the display's pixel format.
fn create_sample(m: &mut Machine, a: &[u32]) -> HostResult {
    let (surface, rect, out) = (a[1], a[2], a[4]);
    let Some(index) = m.video_stream(a[0]) else { return Ok(E_INVALIDARG) };
    if surface != 0 || rect != 0 {
        return Err(not_implemented("CreateSample on a surface of the program's".into()));
    }
    let Some(movie) = &m.os.amstream.streams[index].movie else {
        return Err(not_implemented("CreateSample before a file is opened".into()));
    };
    let (width, height) = (movie.decoder.width() as u32, movie.decoder.height() as u32);
    let format = PixelFormat::of_display(m.os.user.display.bits_per_pixel);
    if format == PixelFormat::Indexed8 {
        return Err(not_implemented("a movie on a display with a palette".into()));
    }
    let surface = m.new_surface(width, height, format, DDSCAPS_OFFSCREENPLAIN | DDSCAPS_SYSTEMMEMORY);
    let sample = m.os.amstream.samples.len();
    let pointer = m.interface_pointer(&STREAM_SAMPLE, sample as u32);
    m.os.amstream.samples.push(Sample { references: 1, pointer, stream: index, surface, waiting: false });
    Ok(if m.memory.write(out, 4, pointer).is_some() { S_OK } else { E_POINTER })
}

interface! { static VIDEO_STREAM = "IDirectDrawMediaStream";
    QueryInterface(3) => video_query_interface,
    AddRef(1) => |m, a| m.video_stream(a[0]).map_or(0, |index| {
        let stream = &mut m.os.amstream.streams[index];
        stream.video_references += 1;
        stream.video_references
    }),
    Release(1) => |m, a| m.video_stream(a[0]).map_or(0, |index| {
        let stream = &mut m.os.amstream.streams[index];
        stream.video_references -= 1;
        stream.video_references
    }),
    GetMultiMediaStream(2),
    GetInformation(3),
    SetSameFormat(3),
    AllocateSample(3),
    CreateSharedSample(4),
    SendEndOfStream(2),
    GetFormat(5),
    SetFormat(3),
    GetDirectDraw(2),
    SetDirectDraw(2),
    CreateSample(5) => create_sample,
    GetTimePerFrame(2),
}

// --- IDirectDrawStreamSample ------------------------------------------------

fn sample_query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else { return E_INVALIDARG };
    let Some(index) = m.sample(a[0]) else { return E_INVALIDARG };
    if ![IID_IUNKNOWN, IID_ISTREAM_SAMPLE, IID_IDIRECT_DRAW_STREAM_SAMPLE].contains(&wanted) {
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    m.os.amstream.samples[index].references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { E_POINTER }
}

/// `Update`: waits until the next picture is due and puts it on the
/// sample's surface. Pictures are never dropped: one that is late is
/// delivered at once.
fn update(m: &mut Machine, a: &[u32]) -> HostResult {
    let (flags, event, procedure) = (a[1], a[2], a[3]);
    let Some(index) = m.sample(a[0]) else { return Ok(E_INVALIDARG) };
    if flags & !SSUPDATE_CONTINUOUS != 0 || event != 0 || procedure != 0 {
        return Err(not_implemented(format!("an asynchronous Update (flags {flags:#x})")));
    }
    let sample = &m.os.amstream.samples[index];
    let (stream_index, surface, waiting) = (sample.stream, sample.surface, sample.waiting);
    let now = m.clock_ms();
    let stream = &m.os.amstream.streams[stream_index];
    let Some(started) = stream.started_ms else { return Ok(MS_E_NOTRUNNING) };
    let Some(movie) = &stream.movie else { return Ok(MS_S_ENDOFSTREAM) };
    let due = started + movie.due_ms(movie.delivered);
    if waiting || now < due {
        // A wait puts the thread to sleep and has this function called
        // again when the time has passed; that call ends the wait.
        m.os.amstream.samples[index].waiting = true;
        m.wait(Waiting::Time, due.saturating_sub(now) as u32)?;
        m.os.amstream.samples[index].waiting = false;
    }
    let movie = m.os.amstream.streams[stream_index].movie.as_mut().expect("checked above");
    let Some(frame) = movie.decoder.decode() else { return Ok(MS_S_ENDOFSTREAM) };
    movie.delivered += 1;
    let rgbx = frame.to_rgbx();
    let format = m.surface_format(surface);
    let size = format.bytes_per_pixel() as usize;
    let mut pixels = Vec::with_capacity(rgbx.len() / 4 * size);
    for pixel in rgbx.as_chunks::<4>().0 {
        let argb = u32::from_be_bytes([0xFF, pixel[0], pixel[1], pixel[2]]);
        pixels.extend_from_slice(&format.from_argb(argb).to_le_bytes()[..size]);
    }
    m.write_surface(surface, &pixels);
    Ok(S_OK)
}

/// `GetSurface`: the sample's surface and the rectangle of it the pictures
/// fill. The pointer is the surface's `IDirectDrawSurface4`, where the
/// interface asks for the first version; the game only asks it for the
/// fourth.
fn get_surface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.sample(a[0]) else { return E_INVALIDARG };
    let surface = m.os.amstream.samples[index].surface;
    let (pointer, width, height) = m.surface_pointer_and_size(surface);
    let mut ok = true;
    if a[1] != 0 {
        m.add_surface_reference(surface);
        ok &= m.memory.write(a[1], 4, pointer).is_some();
    }
    if a[2] != 0 {
        ok &= m.write_words(a[2], &[0, 0, width, height]);
    }
    if ok { S_OK } else { E_POINTER }
}

interface! { static STREAM_SAMPLE = "IDirectDrawStreamSample";
    QueryInterface(3) => sample_query_interface,
    AddRef(1) => |m, a| m.sample(a[0]).map_or(0, |index| {
        let sample = &mut m.os.amstream.samples[index];
        sample.references += 1;
        sample.references
    }),
    Release(1) => |m, a| m.sample(a[0]).map_or(0, |index| {
        let sample = &mut m.os.amstream.samples[index];
        sample.references -= 1;
        let (left, surface) = (sample.references, sample.surface);
        if left == 0 {
            m.release_surface(surface);
        }
        left
    }),
    GetMediaStream(2),
    GetSampleTimes(4),
    SetSampleTimes(3),
    Update(5) => update,
    CompletionStatus(3),
    GetSurface(3) => get_surface,
    SetRect(2),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rising line cut into pieces of four samples, `count` of them.
    fn pieces(count: usize) -> impl FnMut() -> Option<Vec<[f32; 2]>> {
        let mut given = 0;
        move || {
            (given < count).then(|| {
                given += 1;
                (4 * (given - 1)..4 * given).map(|at| [at as f32, -(at as f32)]).collect()
            })
        }
    }

    #[test]
    fn a_track_at_the_rate_of_the_mix_is_played_as_it_is() {
        let mut track = Track::new(OUTPUT_RATE, 0.0);
        let mut out = vec![[1.0; 2]; 6];
        let mut more = pieces(2);
        track.mix_into(&mut out, &mut more);
        // Added to what is in the mix already.
        assert_eq!(out, [[1.0, 1.0], [2.0, 0.0], [3.0, -1.0], [4.0, -2.0], [5.0, -3.0], [6.0, -4.0]]);
        // It goes on where it was, and after its end adds nothing.
        let mut out = vec![[0.0; 2]; 4];
        track.mix_into(&mut out, &mut more);
        assert_eq!(out, [[6.0, -6.0], [7.0, -7.0], [0.0; 2], [0.0; 2]]);
        assert!(track.ended);
    }

    #[test]
    fn sound_that_begins_before_or_after_the_first_picture() {
        // Three frames of the mix after the first picture: silence first.
        let mut track = Track::new(OUTPUT_RATE, 3.0 / OUTPUT_RATE as f64);
        let mut out = vec![[0.0; 2]; 5];
        track.mix_into(&mut out, pieces(2));
        assert_eq!(out.iter().map(|frame| frame[0]).collect::<Vec<_>>(), [0.0, 0.0, 0.0, 0.0, 1.0]);
        // Six samples before it: those are left out, across two pieces.
        let mut track = Track::new(OUTPUT_RATE, -6.0 / OUTPUT_RATE as f64);
        let mut out = vec![[0.0; 2]; 3];
        track.mix_into(&mut out, pieces(3));
        assert_eq!(out.iter().map(|frame| frame[0]).collect::<Vec<_>>(), [6.0, 7.0, 8.0]);
    }

    #[test]
    fn a_track_at_another_rate_is_drawn_out_or_pressed_together() {
        // Half the rate of the mix: every sample and the value half way
        // to the next.
        let mut track = Track::new(OUTPUT_RATE / 2, 0.0);
        let mut out = vec![[0.0; 2]; 5];
        track.mix_into(&mut out, pieces(1));
        assert_eq!(out.iter().map(|frame| frame[0]).collect::<Vec<_>>(), [0.0, 0.5, 1.0, 1.5, 2.0]);
        // Twice the rate: every other sample.
        let mut track = Track::new(OUTPUT_RATE * 2, 0.0);
        let mut out = vec![[0.0; 2]; 3];
        track.mix_into(&mut out, pieces(2));
        assert_eq!(out.iter().map(|frame| frame[0]).collect::<Vec<_>>(), [0.0, 2.0, 4.0]);
    }
}
