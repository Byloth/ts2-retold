//! DSOUND: DirectSound.
//!
//! One output device, the default one, which Windows lists first under the
//! name "Primary Sound Driver" with a null identifier. Mixing is done in
//! software, as it was on most sound cards of the time, so no hardware
//! buffers are advertised.
//!
//! Sound buffers keep their samples in the program's memory, where `Lock`
//! exposes them. This module holds the state the program can observe
//! (formats, play cursors, volumes); turning playing buffers into sound is
//! the job of an audio output that reads the same state. Play cursors
//! advance with the machine's clock.

use super::com::{E_INVALIDARG, E_NOINTERFACE, Guid, IID_IUNKNOWN, S_OK, guid, interface};
use super::{HostResult, functions};
use crate::Machine;
use crate::audio::{Frame, OUTPUT_RATE};
use crate::threads::Object;

pub const DS_OK: u32 = S_OK;
const DSERR_INVALIDPARAM: u32 = E_INVALIDARG;
const DSERR_CONTROLUNAVAIL: u32 = ds_error(30);
const DSERR_INVALIDCALL: u32 = ds_error(50);
const DSERR_PRIOLEVELNEEDED: u32 = ds_error(70);
const DSERR_BADFORMAT: u32 = ds_error(100);
const DSERR_ALREADYINITIALIZED: u32 = ds_error(130);
const CLASS_E_NOAGGREGATION: u32 = 0x8004_0110;

const fn ds_error(code: u32) -> u32 {
    0x8878_0000 | code
}

const IID_IDIRECTSOUND: Guid =
    guid(0x279AFA83, 0x4981, 0x11CE, [0xA5, 0x21, 0x00, 0x20, 0xAF, 0x0B, 0xE5, 0x60]);
const IID_IDIRECTSOUNDNOTIFY: Guid =
    guid(0xB0210783, 0x89CD, 0x11D0, [0xAF, 0x08, 0x00, 0xA0, 0xC9, 0x25, 0xCD, 0x16]);
const IID_IDIRECTSOUNDBUFFER: Guid =
    guid(0x279AFA85, 0x4981, 0x11CE, [0xA5, 0x21, 0x00, 0x20, 0xAF, 0x0B, 0xE5, 0x60]);

const DSSCL_NORMAL: u32 = 1;
const DSSCL_PRIORITY: u32 = 2;
const DSSCL_WRITEPRIMARY: u32 = 4;

const DSBCAPS_PRIMARYBUFFER: u32 = 0x0001;
const DSBCAPS_LOCSOFTWARE: u32 = 0x0008;
const DSBCAPS_CTRLFREQUENCY: u32 = 0x0020;
const DSBCAPS_CTRLPAN: u32 = 0x0040;
const DSBCAPS_CTRLVOLUME: u32 = 0x0080;
const DSBCAPS_CTRLPOSITIONNOTIFY: u32 = 0x0100;
/// In place of an offset: the event is signalled when the buffer stops.
const DSBPN_OFFSETSTOP: u32 = u32::MAX;

const DSBPLAY_LOOPING: u32 = 1;
const DSBSTATUS_PLAYING: u32 = 1;
const DSBSTATUS_LOOPING: u32 = 4;
const DSBLOCK_FROMWRITECURSOR: u32 = 1;
const DSBLOCK_ENTIREBUFFER: u32 = 2;

const DSBSIZE_MIN: u32 = 4;
const DSBSIZE_MAX: u32 = 0x0FFF_FFFF;
const DSBFREQUENCY_MIN: u32 = 100;
const DSBFREQUENCY_MAX: u32 = 100_000;
const DSSPEAKER_STEREO: u32 = 4;
const WAVE_FORMAT_PCM: u16 = 1;
/// How far ahead of the play cursor it is safe to write: 15 ms, the usual
/// figure for a software mixer.
const WRITE_LEAD_MS: u64 = 15;

/// `DSCAPS`: 24 words.
const CAPS_WORDS: usize = 24;
// Primary and secondary buffers in mono or stereo, 8 or 16 bits, at any rate.
const DEVICE_CAPS: u32 = 0x0001 | 0x0002 | 0x0004 | 0x0008 | 0x0010 | 0x0100 | 0x0200 | 0x0400 | 0x0800;

/// Uncompressed sample format (`PCMWAVEFORMAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveFormat {
    pub channels: u16,
    pub samples_per_second: u32,
    pub bits_per_sample: u16,
}

impl WaveFormat {
    /// What a primary buffer plays until the program changes it.
    const DEFAULT_PRIMARY: WaveFormat =
        WaveFormat { channels: 2, samples_per_second: 22_050, bits_per_sample: 8 };

    pub fn block_align(&self) -> u32 {
        self.channels as u32 * self.bits_per_sample as u32 / 8
    }

    /// The 18 bytes of `WAVEFORMATEX`.
    fn to_bytes(self) -> [u8; 18] {
        let mut bytes = [0; 18];
        let block = self.block_align();
        bytes[0..2].copy_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.channels.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.samples_per_second.to_le_bytes());
        bytes[8..12].copy_from_slice(&(self.samples_per_second * block).to_le_bytes());
        bytes[12..14].copy_from_slice(&(block as u16).to_le_bytes());
        bytes[14..16].copy_from_slice(&self.bits_per_sample.to_le_bytes());
        bytes
    }

    /// Parses the first 16 bytes of a wave format; `None` unless it is PCM
    /// with a consistent layout.
    fn from_bytes(bytes: &[u8]) -> Option<WaveFormat> {
        let word = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let format = WaveFormat {
            channels: word(2),
            samples_per_second: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            bits_per_sample: word(14),
        };
        let valid = word(0) == WAVE_FORMAT_PCM
            && matches!(format.channels, 1 | 2)
            && matches!(format.bits_per_sample, 8 | 16)
            && (DSBFREQUENCY_MIN..=DSBFREQUENCY_MAX).contains(&format.samples_per_second)
            && word(12) as u32 == format.block_align();
        valid.then_some(format)
    }
}

#[derive(Debug)]
struct Device {
    references: u32,
    pointer: u32,
    cooperative_level: u32,
}

#[derive(Debug, Clone, Copy)]
struct Playback {
    started_ms: u32,
    /// Play cursor when playback started, in bytes.
    from: u32,
    looping: bool,
}

#[derive(Debug)]
pub struct Buffer {
    references: u32,
    pointer: u32,
    device: usize,
    /// `DSBCAPS` flags given at creation.
    flags: u32,
    pub format: WaveFormat,
    /// Guest address and size of the samples; a primary buffer has none
    /// until the program takes over mixing.
    pub memory: u32,
    pub size: u32,
    playback: Option<Playback>,
    /// Play cursor while stopped.
    position: u32,
    /// Attenuation in hundredths of a decibel, 0 (full) to -10000 (silent).
    pub volume: i32,
    /// -10000 (left) to 10000 (right).
    pub pan: i32,
    /// Playback rate in hertz; differs from the format's rate to shift pitch.
    pub frequency: u32,
    /// Its `IDirectSoundNotify` interface pointer, once asked for.
    notify_pointer: u32,
    /// Offsets at which the play cursor signals an event, with the event.
    notifications: Vec<(u32, u32)>,
    /// While it plays: the play cursor up to which notifications have been
    /// signalled.
    notified_to: Option<u32>,
    /// While it plays: where the mixer is in it.
    mix: Option<Mix>,
}

/// The mixer's place in a playing buffer. The play cursor the program is
/// told follows the clock in whole milliseconds; the mixer keeps its own,
/// exact to a fraction of a sample, so that what is heard has no steps.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mix {
    /// In sample frames from the start of the buffer.
    at: f64,
    looping: bool,
}

impl Buffer {
    fn is_primary(&self) -> bool {
        self.flags & DSBCAPS_PRIMARYBUFFER != 0
    }

    fn bytes_per_second(&self) -> u64 {
        self.frequency as u64 * self.format.block_align() as u64
    }

    /// The play cursor at time `now_ms`, and whether the buffer still plays.
    pub fn cursor(&self, now_ms: u32) -> (u32, bool) {
        let Some(playback) = self.playback else {
            return (self.position, false);
        };
        if self.size == 0 {
            return (0, true);
        }
        let block = self.format.block_align() as u64;
        let elapsed = now_ms.wrapping_sub(playback.started_ms) as u64;
        let advanced = elapsed * self.bytes_per_second() / 1000 / block * block;
        let cursor = playback.from as u64 + advanced;
        if playback.looping {
            ((cursor % self.size as u64) as u32, true)
        } else if cursor >= self.size as u64 {
            // Played to the end: stopped, cursor back at the start.
            (0, false)
        } else {
            (cursor as u32, true)
        }
    }

    /// Folds elapsed playback into the stored state, so that it can be
    /// changed (rate, position) from a known point.
    fn settle(&mut self, now_ms: u32) {
        let (cursor, playing) = self.cursor(now_ms);
        self.position = cursor;
        self.playback = match (playing, self.playback) {
            (true, Some(playback)) => Some(Playback { started_ms: now_ms, from: cursor, ..playback }),
            _ => None,
        };
    }
}

impl Buffer {
    /// How loud the buffer is in the left and the right channel, from its
    /// volume and pan: both are attenuations in hundredths of a decibel,
    /// and a pan to one side attenuates the other.
    fn gains(&self) -> [f32; 2] {
        let gain = |hundredths: i32| 10f32.powf(hundredths as f32 / 2000.0);
        [gain(self.volume - self.pan.max(0)), gain(self.volume + self.pan.min(0))]
    }

    /// Adds what the buffer plays next to `out`, a stretch of the mixed
    /// stream, and moves the mixer's place on. `samples` reads the bytes of
    /// the buffer from an offset. Between two of its samples the value is
    /// taken on the straight line from one to the other.
    fn mix_into(&mut self, out: &mut [[f32; 2]], mut samples: impl FnMut(u32, u32) -> Vec<u8>) {
        let Some(Mix { mut at, looping }) = self.mix else { return };
        let block = self.format.block_align();
        let frames = (self.size / block.max(1)) as u64;
        if frames == 0 {
            self.mix = None;
            return;
        }
        let step = self.frequency as f64 / OUTPUT_RATE as f64;
        // The sample frames this stretch touches, read in one go (in two
        // when it goes round the end of a looping buffer).
        let first = at as u64;
        let wanted = (at + step * out.len() as f64) as u64 + 2 - first;
        let available = if looping { wanted } else { wanted.min(frames.saturating_sub(first)) };
        let mut bytes = Vec::with_capacity((available * block as u64) as usize);
        let mut frame = first % frames;
        while (bytes.len() as u64) < available * block as u64 {
            let count = (available - bytes.len() as u64 / block as u64).min(frames - frame);
            bytes.extend(samples(frame as u32 * block, count as u32 * block));
            frame = 0;
        }
        let (wide, channels) = (self.format.bits_per_sample == 16, self.format.channels as usize);
        // A frame of the buffer as left and right, from -1 to 1; silence
        // past the end of a buffer that plays once.
        let frame_at = |index: u64| -> [f32; 2] {
            let sample = |channel: usize| {
                let at = (index - first) as usize * block as usize + channel * if wide { 2 } else { 1 };
                match (wide, bytes.get(at..at + 2), bytes.get(at)) {
                    (true, Some(pair), _) => i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0,
                    // Eight-bit samples are unsigned, silence at 128.
                    (false, _, Some(&byte)) => (byte as f32 - 128.0) / 128.0,
                    _ => 0.0,
                }
            };
            [sample(0), sample(channels - 1)]
        };
        let [left, right] = self.gains();
        for mixed in out.iter_mut() {
            if !looping && at >= frames as f64 {
                break;
            }
            let (index, between) = (at as u64, at.fract() as f32);
            let (from, to) = (frame_at(index), frame_at(index + 1));
            mixed[0] += (from[0] + (to[0] - from[0]) * between) * left;
            mixed[1] += (from[1] + (to[1] - from[1]) * between) * right;
            at += step;
        }
        self.mix = if looping {
            Some(Mix { at: at % frames as f64, looping })
        } else {
            (at < frames as f64).then_some(Mix { at, looping })
        };
    }

    /// The events of the notification positions the play cursor has passed
    /// since the last call, and how long, in milliseconds, until it passes
    /// the next one. Between calls the cursor must not go round a looping
    /// buffer more than once; the scheduler calls every millisecond and
    /// never lets the clock jump past the time returned.
    fn pass_notifications(&mut self, now_ms: u32, events: &mut Vec<u32>) -> Option<u64> {
        let from = self.notified_to?;
        let (cursor, playing) = self.cursor(now_ms);
        let size = self.size as u64;
        if !playing {
            // Played to its end: the positions that were left, and the stop.
            events.extend(self.notifications.iter().filter(|&&(at, _)| at > from).map(|&(_, event)| event));
            self.settle(now_ms);
            self.notified_to = None;
            return None;
        }
        // How far along the buffer a position is from the last one signalled.
        let ahead = |position: u32| (position as u64 + size - from as u64 - 1) % size + 1;
        let advanced = if cursor == from { 0 } else { ahead(cursor) };
        let positions = self.notifications.iter().filter(|&&(at, _)| at != DSBPN_OFFSETSTOP);
        events.extend(positions.clone().filter(|&&(at, _)| ahead(at) <= advanced).map(|&(_, event)| event));
        self.notified_to = Some(cursor);
        let looping = self.playback.is_some_and(|playback| playback.looping);
        // The next position, or the end of a buffer that plays once.
        let next = positions
            .map(|&(at, _)| (at as u64 + size - cursor as u64 - 1) % size + 1)
            .filter(|&distance| looping || distance <= size - cursor as u64)
            .chain((!looping).then_some(size - cursor as u64))
            .min()?;
        Some((next * 1000).div_ceil(self.bytes_per_second()).max(1))
    }
}

#[derive(Debug, Default)]
pub struct State {
    /// Guest strings describing the driver, created on first enumeration.
    driver_strings: Option<(u32, u32)>,
    devices: Vec<Device>,
    pub buffers: Vec<Buffer>,
    /// Frames of the mixed stream handed on so far.
    mixed_frames: u64,
}

impl Machine {
    fn sound_device(&mut self, this: u32) -> Option<&mut Device> {
        let index = self.object_index(this)?;
        self.os.dsound.devices.get_mut(index).filter(|d| d.references > 0 && d.pointer == this)
    }

    /// Mixes what the buffers play, up to the present of the machine's
    /// clock, and hands it to the receiver of the sound. Called as the
    /// clock advances and before anything about a buffer changes, so that
    /// every change is heard from its own moment on. The samples are read
    /// from the program's memory as they are when the mixer reaches them,
    /// which is how a program that keeps refilling a looping buffer is heard.
    pub(crate) fn mix_sound(&mut self) {
        let Some(mut audio) = self.audio.take() else { return };
        let target = crate::audio::frames_at(self.clock_ms());
        let count = target.saturating_sub(self.os.dsound.mixed_frames) as usize;
        if count > 0 {
            self.os.dsound.mixed_frames = target;
            let mut mixed = vec![[0f32; 2]; count];
            let memory = &mut self.memory;
            for buffer in self.os.dsound.buffers.iter_mut().filter(|buffer| buffer.mix.is_some()) {
                let start = buffer.memory;
                buffer.mix_into(&mut mixed, |offset, len| {
                    memory.dump(start + offset, len).unwrap_or_else(|| vec![0; len as usize])
                });
            }
            // The movies play their sound through the same mix.
            self.os.amstream.mix_into(&mut mixed);
            // Louder than the output can carry is cut off at its limits.
            let frames: Vec<Frame> = mixed
                .iter()
                .map(|frame| frame.map(|sample| (sample * 32768.0).clamp(-32768.0, 32767.0) as i16))
                .collect();
            audio.play(&frames);
        }
        self.audio = Some(audio);
    }

    fn sound_buffer(&mut self, this: u32) -> Option<&mut Buffer> {
        self.mix_sound();
        let index = self.object_index(this)?;
        self.os.dsound.buffers.get_mut(index).filter(|b| b.references > 0 && b.pointer == this)
    }

    /// The buffer behind `this`, brought up to date with the clock.
    fn settled_buffer(&mut self, this: u32) -> Option<&mut Buffer> {
        let now = self.os.now_ms;
        let buffer = self.sound_buffer(this)?;
        buffer.settle(now);
        Some(buffer)
    }

    /// The buffer whose `IDirectSoundNotify` interface `this` is.
    fn notifying_buffer(&mut self, this: u32) -> Option<&mut Buffer> {
        let index = self.object_index(this)?;
        self.os.dsound.buffers.get_mut(index).filter(|b| b.references > 0 && b.notify_pointer == this)
    }

    /// Signals the events of the notification positions that playing
    /// buffers have passed, and returns the time in milliseconds until the
    /// next one. The scheduler calls this as the clock advances.
    pub(crate) fn sound_notifications(&mut self) -> Option<u64> {
        self.mix_sound();
        let now = self.clock_ms() as u32;
        let mut events = Vec::new();
        let next = self
            .os
            .dsound
            .buffers
            .iter_mut()
            .filter(|buffer| buffer.notified_to.is_some())
            .filter_map(|buffer| buffer.pass_notifications(now, &mut events))
            .min();
        self.signal(&events);
        next
    }

    fn signal(&mut self, events: &[u32]) {
        for &event in events {
            if let Some(Object::Event { signaled, .. }) = self.threads.object(event) {
                *signaled = true;
            }
        }
    }

    fn new_buffer(&mut self, buffer: Buffer) -> u32 {
        let index = self.os.dsound.buffers.len() as u32;
        let pointer = self.interface_pointer(&SOUND_BUFFER, index);
        self.os.dsound.buffers.push(Buffer { pointer, ..buffer });
        pointer
    }
}

functions! { "DSOUND.dll";
    DirectSoundEnumerateA(2) => |m, a| -> HostResult {
        let (description, module) = match m.os.dsound.driver_strings {
            Some(strings) => strings,
            None => {
                let strings =
                    (m.os.string(&mut m.memory, b"Primary Sound Driver"), m.os.string(&mut m.memory, b""));
                m.os.dsound.driver_strings = Some(strings);
                strings
            }
        };
        m.call_guest(a[0], &[0, description, module, a[1]])?;
        Ok(DS_OK)
    },
    DirectSoundCreate(3) => |m, a| {
        // A null identifier asks for the default device, the only one there is.
        let (identifier, out, outer) = (a[0], a[1], a[2]);
        if outer != 0 {
            return CLASS_E_NOAGGREGATION;
        }
        if identifier != 0 {
            return DSERR_INVALIDPARAM;
        }
        let index = m.os.dsound.devices.len() as u32;
        let pointer = m.interface_pointer(&SOUND_DEVICE, index);
        m.os.dsound.devices.push(Device { references: 1, pointer, cooperative_level: 0 });
        match m.memory.write(out, 4, pointer) {
            Some(()) => DS_OK,
            None => DSERR_INVALIDPARAM,
        }
    },
}

// --- IDirectSound ---------------------------------------------------------

fn device_query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(wanted) = m.read_guid(a[1]) else {
        return DSERR_INVALIDPARAM;
    };
    let Some(device) = m.sound_device(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if wanted != IID_IUNKNOWN && wanted != IID_IDIRECTSOUND {
        let _ = m.memory.write(a[2], 4, 0);
        return E_NOINTERFACE;
    }
    device.references += 1;
    if m.memory.write(a[2], 4, a[0]).is_some() { S_OK } else { DSERR_INVALIDPARAM }
}

fn device_add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    m.sound_device(a[0]).map_or(0, |device| {
        device.references += 1;
        device.references
    })
}

fn device_release(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(index) = m.object_index(a[0]) else {
        return 0;
    };
    let Some(device) = m.sound_device(a[0]) else {
        return 0;
    };
    device.references -= 1;
    let remaining = device.references;
    if remaining == 0 {
        // The device takes its buffers with it.
        for buffer in m.os.dsound.buffers.iter_mut().filter(|b| b.device == index) {
            buffer.references = 0;
            buffer.playback = None;
        }
    }
    remaining
}

fn create_sound_buffer(m: &mut Machine, a: &[u32]) -> u32 {
    let (description, out, outer) = (a[1], a[2], a[3]);
    if outer != 0 {
        return CLASS_E_NOAGGREGATION;
    }
    let Some(device) = m.object_index(a[0]).filter(|_| m.sound_device(a[0]).is_some()) else {
        return DSERR_INVALIDPARAM;
    };
    // DSBUFFERDESC: size, flags, buffer bytes, reserved, format pointer.
    let [size, flags, bytes, _, format_address]: [u32; 5] = m.read_words(description);
    if size != 20 && size != 36 {
        return DSERR_INVALIDPARAM;
    }
    let primary = flags & DSBCAPS_PRIMARYBUFFER != 0;
    let (format, memory, bytes) = if primary {
        // The primary buffer belongs to the device: no size, no format here.
        if bytes != 0 || format_address != 0 {
            return DSERR_INVALIDPARAM;
        }
        if let Some(existing) =
            m.os.dsound.buffers.iter_mut().find(|b| b.device == device && b.is_primary() && b.references > 0)
        {
            // There is one primary buffer; asking again returns it.
            existing.references += 1;
            let pointer = existing.pointer;
            return if m.memory.write(out, 4, pointer).is_some() { DS_OK } else { DSERR_INVALIDPARAM };
        }
        (WaveFormat::DEFAULT_PRIMARY, 0, 0)
    } else {
        if !(DSBSIZE_MIN..=DSBSIZE_MAX).contains(&bytes) {
            return DSERR_INVALIDPARAM;
        }
        let Some(raw) = m.memory.dump(format_address, 16) else {
            return DSERR_INVALIDPARAM;
        };
        let Some(format) = WaveFormat::from_bytes(&raw) else {
            return DSERR_BADFORMAT;
        };
        let memory = m.os.allocate(&mut m.memory, bytes);
        if memory == 0 {
            return 0x8007_000E; // E_OUTOFMEMORY
        }
        (format, memory, bytes)
    };
    m.note(|| {
        format!(
            "{} buffer, {bytes} bytes, {} Hz, {} bit, {} channel(s), flags {flags:#x}",
            if primary { "primary" } else { "secondary" },
            format.samples_per_second,
            format.bits_per_sample,
            format.channels
        )
    });
    let pointer = m.new_buffer(Buffer {
        references: 1,
        pointer: 0,
        device,
        flags: flags | DSBCAPS_LOCSOFTWARE,
        format,
        memory,
        size: bytes,
        playback: None,
        position: 0,
        volume: 0,
        pan: 0,
        frequency: format.samples_per_second,
        notify_pointer: 0,
        notifications: Vec::new(),
        notified_to: None,
        mix: None,
    });
    if m.memory.write(out, 4, pointer).is_some() { DS_OK } else { DSERR_INVALIDPARAM }
}

fn device_get_caps(m: &mut Machine, a: &[u32]) -> u32 {
    if m.sound_device(a[0]).is_none() || m.memory.read(a[1], 4) != Some(4 * CAPS_WORDS as u32) {
        return DSERR_INVALIDPARAM;
    }
    let mut words = [0; CAPS_WORDS];
    words[0] = 4 * CAPS_WORDS as u32;
    words[1] = DEVICE_CAPS;
    words[2] = DSBFREQUENCY_MIN;
    words[3] = DSBFREQUENCY_MAX;
    words[4] = 1; // primary buffers
    m.write_words(a[1], &words);
    DS_OK
}

fn duplicate_sound_buffer(m: &mut Machine, a: &[u32]) -> u32 {
    if m.sound_device(a[0]).is_none() {
        return DSERR_INVALIDPARAM;
    }
    let Some(original) = m.sound_buffer(a[1]) else {
        return DSERR_INVALIDPARAM;
    };
    if original.is_primary() {
        return DSERR_INVALIDCALL;
    }
    // The copy shares the samples and starts with the original's settings,
    // stopped at the beginning.
    let copy = Buffer {
        references: 1,
        pointer: 0,
        device: original.device,
        flags: original.flags,
        format: original.format,
        memory: original.memory,
        size: original.size,
        playback: None,
        position: 0,
        volume: original.volume,
        pan: original.pan,
        frequency: original.frequency,
        notify_pointer: 0,
        notifications: Vec::new(),
        notified_to: None,
        mix: None,
    };
    let pointer = m.new_buffer(copy);
    if m.memory.write(a[2], 4, pointer).is_some() { DS_OK } else { DSERR_INVALIDPARAM }
}

fn device_set_cooperative_level(m: &mut Machine, a: &[u32]) -> u32 {
    let level = a[2];
    if !(DSSCL_NORMAL..=DSSCL_WRITEPRIMARY).contains(&level) {
        return DSERR_INVALIDPARAM;
    }
    let Some(device) = m.sound_device(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    device.cooperative_level = level;
    DS_OK
}

fn get_speaker_config(m: &mut Machine, a: &[u32]) -> u32 {
    if m.sound_device(a[0]).is_some() && m.memory.write(a[1], 4, DSSPEAKER_STEREO).is_some() {
        DS_OK
    } else {
        DSERR_INVALIDPARAM
    }
}

interface! { static SOUND_DEVICE = "IDirectSound";
    QueryInterface(3) => device_query_interface,
    AddRef(1) => device_add_ref,
    Release(1) => device_release,
    CreateSoundBuffer(4) => create_sound_buffer,
    GetCaps(2) => device_get_caps,
    DuplicateSoundBuffer(3) => duplicate_sound_buffer,
    SetCooperativeLevel(3) => device_set_cooperative_level,
    Compact(1) => |_, _| DS_OK,
    GetSpeakerConfig(2) => get_speaker_config,
    SetSpeakerConfig(2) => |_, _| DS_OK,
    Initialize(2) => |_, _| DSERR_ALREADYINITIALIZED,
}

// --- IDirectSoundBuffer ---------------------------------------------------

/// `QueryInterface` of a buffer, reached through either of its interfaces.
/// The notification interface exists only on a buffer created for it.
fn query_buffer(m: &mut Machine, index: usize, wanted_at: u32, out: u32) -> u32 {
    let Some(wanted) = m.read_guid(wanted_at) else {
        return DSERR_INVALIDPARAM;
    };
    let buffer = &m.os.dsound.buffers[index];
    let pointer = match wanted {
        IID_IUNKNOWN | IID_IDIRECTSOUNDBUFFER => buffer.pointer,
        IID_IDIRECTSOUNDNOTIFY if buffer.flags & DSBCAPS_CTRLPOSITIONNOTIFY != 0 => {
            if buffer.notify_pointer == 0 {
                let pointer = m.interface_pointer(&SOUND_NOTIFY, index as u32);
                m.os.dsound.buffers[index].notify_pointer = pointer;
            }
            m.os.dsound.buffers[index].notify_pointer
        }
        _ => {
            let _ = m.memory.write(out, 4, 0);
            return E_NOINTERFACE;
        }
    };
    m.os.dsound.buffers[index].references += 1;
    if m.memory.write(out, 4, pointer).is_some() { S_OK } else { DSERR_INVALIDPARAM }
}

fn buffer_query_interface(m: &mut Machine, a: &[u32]) -> u32 {
    match m.object_index(a[0]).filter(|_| m.sound_buffer(a[0]).is_some()) {
        Some(index) => query_buffer(m, index, a[1], a[2]),
        None => DSERR_INVALIDPARAM,
    }
}

fn buffer_add_ref(m: &mut Machine, a: &[u32]) -> u32 {
    m.sound_buffer(a[0]).map_or(0, |buffer| {
        buffer.references += 1;
        buffer.references
    })
}

fn buffer_release(m: &mut Machine, a: &[u32]) -> u32 {
    m.sound_buffer(a[0]).map_or(0, |buffer| {
        buffer.references -= 1;
        if buffer.references == 0 {
            buffer.playback = None;
            buffer.mix = None;
            buffer.notified_to = None;
            buffer.notifications.clear();
        }
        buffer.references
    })
}

fn buffer_get_caps(m: &mut Machine, a: &[u32]) -> u32 {
    // DSBCAPS: size, flags, buffer bytes, unlock transfer rate, play CPU overhead.
    let Some((flags, size)) = m.sound_buffer(a[0]).map(|b| (b.flags, b.size)) else {
        return DSERR_INVALIDPARAM;
    };
    if m.memory.read(a[1], 4) != Some(20) {
        return DSERR_INVALIDPARAM;
    }
    m.write_words(a[1], &[20, flags, size, 0, 0]);
    DS_OK
}

fn get_current_position(m: &mut Machine, a: &[u32]) -> u32 {
    let now = m.os.now_ms;
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    let (play, _) = buffer.cursor(now);
    let block = buffer.format.block_align().max(1) as u64;
    let lead = (buffer.bytes_per_second() * WRITE_LEAD_MS / 1000 / block * block) as u32;
    let write = if buffer.size == 0 { 0 } else { (play + lead) % buffer.size };
    for (out, value) in [(a[1], play), (a[2], write)] {
        if out != 0 && m.memory.write(out, 4, value).is_none() {
            return DSERR_INVALIDPARAM;
        }
    }
    DS_OK
}

fn get_format(m: &mut Machine, a: &[u32]) -> u32 {
    let (out, capacity, written_out) = (a[1], a[2] as usize, a[3]);
    let Some(format) = m.sound_buffer(a[0]).map(|b| b.format) else {
        return DSERR_INVALIDPARAM;
    };
    let bytes = format.to_bytes();
    if out != 0 {
        // As much of the structure as the caller has room for.
        let len = capacity.min(bytes.len());
        if m.memory.load(out, &bytes[..len]).is_none() {
            return DSERR_INVALIDPARAM;
        }
        if written_out != 0 {
            let _ = m.memory.write(written_out, 4, len as u32);
        }
    } else if written_out != 0 {
        let _ = m.memory.write(written_out, 4, bytes.len() as u32);
    } else {
        return DSERR_INVALIDPARAM;
    }
    DS_OK
}

fn get_value(m: &mut Machine, a: &[u32], control: u32, value: fn(&Buffer) -> u32) -> u32 {
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if control != 0 && buffer.flags & control == 0 {
        return DSERR_CONTROLUNAVAIL;
    }
    let value = value(buffer);
    if m.memory.write(a[1], 4, value).is_some() { DS_OK } else { DSERR_INVALIDPARAM }
}

fn get_status(m: &mut Machine, a: &[u32]) -> u32 {
    let now = m.os.now_ms;
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    let (_, playing) = buffer.cursor(now);
    let looping = playing && buffer.playback.is_some_and(|p| p.looping);
    let status = if playing { DSBSTATUS_PLAYING } else { 0 } | if looping { DSBSTATUS_LOOPING } else { 0 };
    if m.memory.write(a[1], 4, status).is_some() { DS_OK } else { DSERR_INVALIDPARAM }
}

fn lock(m: &mut Machine, a: &[u32]) -> u32 {
    let (mut offset, mut bytes, flags) = (a[1], a[2], a[7]);
    let (first_out, first_len_out, second_out, second_len_out) = (a[3], a[4], a[5], a[6]);
    let now = m.os.now_ms;
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if buffer.memory == 0 {
        // Writing to the primary buffer needs the write-primary level.
        return DSERR_PRIOLEVELNEEDED;
    }
    let (memory, size) = (buffer.memory, buffer.size);
    if flags & DSBLOCK_FROMWRITECURSOR != 0 {
        let (play, _) = buffer.cursor(now);
        let block = buffer.format.block_align().max(1) as u64;
        let lead = (buffer.bytes_per_second() * WRITE_LEAD_MS / 1000 / block * block) as u32;
        offset = (play + lead) % size;
    }
    if flags & DSBLOCK_ENTIREBUFFER != 0 {
        bytes = size;
    }
    if offset >= size || bytes == 0 || bytes > size || first_out == 0 || first_len_out == 0 {
        return DSERR_INVALIDPARAM;
    }
    // A range that runs past the end continues at the start of the buffer.
    let first_len = bytes.min(size - offset);
    let second_len = bytes - first_len;
    let mut ok = m.memory.write(first_out, 4, memory + offset).is_some()
        && m.memory.write(first_len_out, 4, first_len).is_some();
    if second_out != 0 {
        ok &= m.memory.write(second_out, 4, if second_len > 0 { memory } else { 0 }).is_some();
    }
    if second_len_out != 0 {
        ok &= m.memory.write(second_len_out, 4, second_len).is_some();
    }
    if ok { DS_OK } else { DSERR_INVALIDPARAM }
}

fn play(m: &mut Machine, a: &[u32]) -> u32 {
    let now = m.os.now_ms;
    let looping = a[3] & DSBPLAY_LOOPING != 0;
    let Some(buffer) = m.settled_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if buffer.is_primary() && !looping {
        // The primary buffer only ever plays looping.
        return DSERR_INVALIDPARAM;
    }
    // A buffer already playing goes on from where it is.
    let at = match (buffer.playback, buffer.mix) {
        (Some(_), Some(mix)) => mix.at,
        _ => (buffer.position / buffer.format.block_align().max(1)) as f64,
    };
    buffer.mix = (!buffer.is_primary()).then_some(Mix { at, looping });
    buffer.playback = Some(Playback { started_ms: now, from: buffer.position, looping });
    // Notifications count from where playback starts, unless it was
    // already playing.
    if !buffer.notifications.is_empty() && buffer.notified_to.is_none() {
        buffer.notified_to = Some(buffer.position);
    }
    DS_OK
}

fn stop(m: &mut Machine, a: &[u32]) -> u32 {
    let now = m.os.now_ms;
    let mut events = Vec::new();
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    // What the cursor passed up to now, then the stop itself.
    if buffer.pass_notifications(now, &mut events).is_some() || buffer.notified_to.take().is_some() {
        events
            .extend(buffer.notifications.iter().filter(|&&(at, _)| at == DSBPN_OFFSETSTOP).map(|&(_, e)| e));
    }
    buffer.settle(now);
    buffer.playback = None;
    buffer.mix = None;
    buffer.notified_to = None;
    m.signal(&events);
    DS_OK
}

fn set_current_position(m: &mut Machine, a: &[u32]) -> u32 {
    let now = m.os.now_ms;
    let Some(buffer) = m.settled_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if buffer.is_primary() {
        return DSERR_INVALIDCALL;
    }
    if a[1] >= buffer.size {
        return DSERR_INVALIDPARAM;
    }
    let block = buffer.format.block_align().max(1);
    buffer.position = a[1] / block * block;
    if let Some(playback) = &mut buffer.playback {
        *playback = Playback { started_ms: now, from: buffer.position, ..*playback };
    }
    if let Some(mix) = &mut buffer.mix {
        mix.at = (buffer.position / block) as f64;
    }
    DS_OK
}

fn set_format(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(raw) = m.memory.dump(a[1], 16) else {
        return DSERR_INVALIDPARAM;
    };
    let Some(index) = m.object_index(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    let Some(device) = m.sound_buffer(a[0]).map(|b| b.device) else {
        return DSERR_INVALIDPARAM;
    };
    if !m.os.dsound.buffers[index].is_primary() {
        // Only the primary buffer's format can be changed.
        return DSERR_INVALIDCALL;
    }
    if m.os.dsound.devices[device].cooperative_level < DSSCL_PRIORITY {
        return DSERR_PRIOLEVELNEEDED;
    }
    let Some(format) = WaveFormat::from_bytes(&raw) else {
        return DSERR_BADFORMAT;
    };
    m.note(|| {
        format!(
            "{} Hz, {} bit, {} channel(s)",
            format.samples_per_second, format.bits_per_sample, format.channels
        )
    });
    let buffer = &mut m.os.dsound.buffers[index];
    buffer.format = format;
    buffer.frequency = format.samples_per_second;
    DS_OK
}

fn set_volume(m: &mut Machine, a: &[u32]) -> u32 {
    let volume = a[1] as i32;
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if buffer.flags & DSBCAPS_CTRLVOLUME == 0 {
        return DSERR_CONTROLUNAVAIL;
    }
    if !(-10_000..=0).contains(&volume) {
        return DSERR_INVALIDPARAM;
    }
    buffer.volume = volume;
    DS_OK
}

fn set_pan(m: &mut Machine, a: &[u32]) -> u32 {
    let pan = a[1] as i32;
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if buffer.flags & DSBCAPS_CTRLPAN == 0 {
        return DSERR_CONTROLUNAVAIL;
    }
    if !(-10_000..=10_000).contains(&pan) {
        return DSERR_INVALIDPARAM;
    }
    buffer.pan = pan;
    DS_OK
}

fn set_frequency(m: &mut Machine, a: &[u32]) -> u32 {
    let Some(buffer) = m.settled_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    if buffer.flags & DSBCAPS_CTRLFREQUENCY == 0 {
        return DSERR_CONTROLUNAVAIL;
    }
    // Zero restores the rate of the buffer's format.
    let frequency = if a[1] == 0 { buffer.format.samples_per_second } else { a[1] };
    if !(DSBFREQUENCY_MIN..=DSBFREQUENCY_MAX).contains(&frequency) {
        return DSERR_INVALIDPARAM;
    }
    buffer.frequency = frequency;
    DS_OK
}

fn unlock(m: &mut Machine, a: &[u32]) -> u32 {
    // The locked ranges are the buffer's own memory: nothing to copy back.
    let Some(buffer) = m.sound_buffer(a[0]) else {
        return DSERR_INVALIDPARAM;
    };
    let inside = a[1] >= buffer.memory && a[1] - buffer.memory + a[2] <= buffer.size;
    if buffer.memory != 0 && inside { DS_OK } else { DSERR_INVALIDPARAM }
}

interface! { static SOUND_BUFFER = "IDirectSoundBuffer";
    QueryInterface(3) => buffer_query_interface,
    AddRef(1) => buffer_add_ref,
    Release(1) => buffer_release,
    GetCaps(2) => buffer_get_caps,
    GetCurrentPosition(3) => get_current_position,
    GetFormat(4) => get_format,
    GetVolume(2) => |m, a| get_value(m, a, DSBCAPS_CTRLVOLUME, |b| b.volume as u32),
    GetPan(2) => |m, a| get_value(m, a, DSBCAPS_CTRLPAN, |b| b.pan as u32),
    GetFrequency(2) => |m, a| get_value(m, a, DSBCAPS_CTRLFREQUENCY, |b| b.frequency),
    GetStatus(2) => get_status,
    Initialize(3) => |_, _| DSERR_ALREADYINITIALIZED,
    Lock(8) => lock,
    Play(4) => play,
    SetCurrentPosition(2) => set_current_position,
    SetFormat(2) => set_format,
    SetVolume(2) => set_volume,
    SetPan(2) => set_pan,
    SetFrequency(2) => set_frequency,
    Stop(1) => stop,
    Unlock(5) => unlock,
    Restore(1) => |_, _| DS_OK,
}

// --- IDirectSoundNotify -----------------------------------------------------

/// `SetNotificationPositions`: a count and that many pairs of an offset and
/// an event. Only while the buffer is stopped.
fn set_notification_positions(m: &mut Machine, a: &[u32]) -> u32 {
    let (count, positions) = (a[1], a[2]);
    let now = m.os.now_ms;
    let Some((size, playing)) = m.notifying_buffer(a[0]).map(|buffer| (buffer.size, buffer.cursor(now).1))
    else {
        return DSERR_INVALIDPARAM;
    };
    if playing {
        return DSERR_INVALIDCALL;
    }
    if count > 0x1_0000 {
        return DSERR_INVALIDPARAM;
    }
    let mut notifications = Vec::with_capacity(count as usize);
    for n in 0..count {
        let [offset, event]: [u32; 2] = m.read_words(positions + 8 * n);
        let is_event = matches!(m.threads.object(event), Some(Object::Event { .. }));
        if !is_event || (offset >= size && offset != DSBPN_OFFSETSTOP) {
            return DSERR_INVALIDPARAM;
        }
        notifications.push((offset, event));
    }
    m.notifying_buffer(a[0]).expect("checked above").notifications = notifications;
    DS_OK
}

interface! { static SOUND_NOTIFY = "IDirectSoundNotify";
    QueryInterface(3) => |m, a| match m.object_index(a[0]).filter(|_| m.notifying_buffer(a[0]).is_some()) {
        Some(index) => query_buffer(m, index, a[1], a[2]),
        None => DSERR_INVALIDPARAM,
    },
    // The interface shares the buffer's reference count.
    AddRef(1) => |m, a| m.notifying_buffer(a[0]).map_or(0, |buffer| {
        buffer.references += 1;
        buffer.references
    }),
    Release(1) => |m, a| m.notifying_buffer(a[0]).map_or(0, |buffer| {
        buffer.references -= 1;
        buffer.references
    }),
    SetNotificationPositions(3) => set_notification_positions,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(size: u32, playback: Option<Playback>) -> Buffer {
        // 22050 Hz, 16 bit, stereo: 88200 bytes per second, 4 bytes per frame.
        let format = WaveFormat { channels: 2, samples_per_second: 22_050, bits_per_sample: 16 };
        Buffer {
            references: 1,
            pointer: 0,
            device: 0,
            flags: 0,
            format,
            memory: 0x1000,
            size,
            playback,
            position: 40,
            volume: 0,
            pan: 0,
            frequency: format.samples_per_second,
            notify_pointer: 0,
            notifications: Vec::new(),
            notified_to: None,
            mix: None,
        }
    }

    #[test]
    fn wave_formats_round_trip_and_reject_nonsense() {
        let format = WaveFormat { channels: 2, samples_per_second: 44_100, bits_per_sample: 16 };
        let bytes = format.to_bytes();
        assert_eq!(&bytes[..16], [1, 0, 2, 0, 0x44, 0xAC, 0, 0, 0x10, 0xB1, 2, 0, 4, 0, 16, 0]);
        assert_eq!(WaveFormat::from_bytes(&bytes), Some(format));
        let mut wrong_align = bytes;
        wrong_align[12] = 2;
        assert_eq!(WaveFormat::from_bytes(&wrong_align), None);
        let mut compressed = bytes;
        compressed[0] = 2;
        assert_eq!(WaveFormat::from_bytes(&compressed), None);
    }

    /// Mixes `frames` of output from a buffer holding `samples`.
    fn mixed(buffer: &mut Buffer, samples: &[u8], frames: usize) -> Vec<[f32; 2]> {
        let mut out = vec![[0.0; 2]; frames];
        buffer.mix_into(&mut out, |offset, len| samples[offset as usize..][..len as usize].to_vec());
        out
    }

    #[test]
    fn the_mixer_plays_a_buffer_at_its_rate_volume_and_pan() {
        // Sixteen bits, one channel, a quarter of the output's rate: a ramp
        // of four samples comes out as a straight line sixteen frames long.
        let ramp: Vec<u8> =
            [0i16, 8192, 16384, 24576].iter().flat_map(|sample| sample.to_le_bytes()).collect();
        let mut sound = buffer(8, None);
        sound.format = WaveFormat { channels: 1, samples_per_second: OUTPUT_RATE / 4, bits_per_sample: 16 };
        sound.frequency = OUTPUT_RATE / 4;
        sound.mix = Some(Mix { at: 0.0, looping: false });
        let out = mixed(&mut sound, &ramp, 20);
        for (frame, value) in out.iter().enumerate().take(12) {
            assert_eq!(*value, [frame as f32 / 16.0; 2], "frame {frame}");
        }
        // After the last sample it fades to nothing and has ended.
        assert_eq!(out[13], [0.75 * 0.75; 2]);
        assert_eq!(out[16..], [[0.0; 2]; 4]);
        assert_eq!(sound.mix, None);

        // Looping it goes round, and remembers where it is between calls.
        sound.mix = Some(Mix { at: 0.0, looping: true });
        let first = mixed(&mut sound, &ramp, 10);
        let second = mixed(&mut sound, &ramp, 10);
        assert_eq!(sound.mix, Some(Mix { at: 1.0, looping: true }));
        assert_eq!((first[4], second[2], second[6]), ([0.25; 2], [0.75; 2], [0.0; 2]));
        // Between the last sample and the first it goes from one to the other.
        assert_eq!(second[4], [0.375; 2]);

        // Twenty decibels down is a tenth; a pan to the right leaves the
        // right as it is and attenuates the left.
        (sound.volume, sound.pan) = (-2000, 0);
        assert!(sound.gains().iter().all(|gain| (gain - 0.1).abs() < 1e-6));
        (sound.volume, sound.pan) = (0, 2000);
        let [left, right] = sound.gains();
        assert!((left - 0.1).abs() < 1e-6 && right == 1.0);
        (sound.volume, sound.pan) = (0, -10000);
        assert_eq!(sound.gains(), [1.0, 1e-5]);
    }

    #[test]
    fn the_mixer_reads_eight_bits_and_two_channels() {
        // Eight bits are unsigned around 128; two channels stay apart.
        let mut sound = buffer(4, None);
        sound.format = WaveFormat { channels: 2, samples_per_second: OUTPUT_RATE, bits_per_sample: 8 };
        sound.frequency = OUTPUT_RATE;
        sound.mix = Some(Mix { at: 0.0, looping: false });
        let out = mixed(&mut sound, &[128, 192, 64, 0], 3);
        assert_eq!(out, [[0.0, 0.5], [-0.5, -1.0], [0.0, 0.0]]);
    }

    #[test]
    fn play_cursor_follows_the_clock() {
        assert_eq!(buffer(1000, None).cursor(5000), (40, false));
        let once = Some(Playback { started_ms: 1000, from: 0, looping: false });
        // 10 ms at 88200 bytes per second is 882 bytes, rounded down to a frame.
        assert_eq!(buffer(100_000, once).cursor(1010), (880, true));
        assert_eq!(buffer(100_000, once).cursor(1000), (0, true));
        // A buffer that is not looping stops at its end.
        assert_eq!(buffer(100_000, once).cursor(3000), (0, false));
        let looping = Some(Playback { started_ms: 1000, from: 400, looping: true });
        assert_eq!(buffer(1000, looping).cursor(1010), ((400 + 880) % 1000, true));
        // Changing the rate changes how fast the cursor moves.
        let mut fast = buffer(100_000, once);
        fast.frequency = 44_100;
        assert_eq!(fast.cursor(1010), (1764, true));
    }

    #[test]
    fn settling_keeps_the_cursor_continuous() {
        let mut playing = buffer(100_000, Some(Playback { started_ms: 0, from: 0, looping: false }));
        playing.settle(10);
        assert_eq!(playing.position, 880);
        assert_eq!(playing.cursor(20), (1760, true));
        playing.settle(5000);
        assert!(playing.playback.is_none());
        assert_eq!(playing.cursor(6000), (0, false));
    }

    #[test]
    fn notification_positions_are_signalled_as_the_cursor_passes_them() {
        // 88200 bytes per second; a looping buffer of one second.
        let mut b = buffer(88_200, None);
        b.position = 0;
        b.notifications = vec![(22_048, 1), (44_100, 2), (88_196, 3), (DSBPN_OFFSETSTOP, 4)];
        let mut events = Vec::new();
        // Not playing: nothing to pass.
        assert_eq!(b.pass_notifications(0, &mut events), None);
        b.playback = Some(Playback { started_ms: 1000, from: 0, looping: true });
        b.notified_to = Some(0);
        // A quarter of a second to the first position.
        assert_eq!(b.pass_notifications(1000, &mut events), Some(250));
        assert_eq!(b.pass_notifications(1249, &mut events), Some(1));
        assert!(events.is_empty());
        assert_eq!(b.pass_notifications(1250, &mut events), Some(251));
        assert_eq!(std::mem::take(&mut events), [1]);
        // Two positions in one step, then round the end of the buffer.
        assert_eq!(b.pass_notifications(1999, &mut events), Some(1));
        assert_eq!(std::mem::take(&mut events), [2]);
        assert_eq!(b.pass_notifications(2100, &mut events), Some(150));
        assert_eq!(std::mem::take(&mut events), [3]);

        // A buffer that plays once ends with the positions that were left;
        // the stop event is the caller's to add.
        let mut b = buffer(88_200, Some(Playback { started_ms: 0, from: 0, looping: false }));
        b.notifications = vec![(44_100, 2), (88_196, 3)];
        b.notified_to = Some(0);
        assert_eq!(b.pass_notifications(100, &mut events), Some(400));
        assert_eq!(b.pass_notifications(600, &mut events), Some(400));
        assert_eq!(std::mem::take(&mut events), [2]);
        assert_eq!(b.pass_notifications(1000, &mut events), None);
        assert_eq!(std::mem::take(&mut events), [3]);
        assert_eq!((b.notified_to, b.playback.is_some()), (None, false));
    }
}
