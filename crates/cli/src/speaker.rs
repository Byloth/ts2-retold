//! The machine's sound on the sound device.
//!
//! Two clocks meet here. The machine produces sound at the pace of its own
//! clock, which the window keeps in step with the computer's; the sound
//! device takes it at the pace of its own crystal, which is never quite
//! the same. Between them is a short queue ([`Feed`]), and the device is
//! fed from it a little faster or slower (by half a percent at most, too
//! little to hear) so that the queue stays about as long as it should:
//! long enough to cover the unevenness of the machine's turns, short
//! enough that sound is not late.
//!
//! The speaker is opened once the window is on the screen
//! (`crate::play`): what makes the sound exists before the sound does.
//!
//! On Linux the device is reached through the PulseAudio server (which
//! PipeWire and WSLg also provide), spoken to directly: no system library
//! is needed. On Windows it is reached through the system's own interface
//! (the `cpal` crate), in the format the device mixes in: the queue then
//! also fits the machine's rate to the device's.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use andy_runtime::audio::{Audio, Frame, OUTPUT_RATE};

/// How much sound the queue holds when all is well, in frames: 60 ms.
const TARGET: usize = OUTPUT_RATE as usize * 60 / 1000;
/// More than this (200 ms) is sound that would come late: the oldest is
/// dropped. It happens when the machine has run ahead of the device, as
/// after the computer was busy with something else.
const LIMIT: usize = OUTPUT_RATE as usize * 200 / 1000;
/// How far the pace of feeding may differ from one frame for one frame.
const BEND: f64 = 0.005;

/// The queue between the machine and the sound device.
#[derive(Debug, Default)]
pub struct Feed {
    queue: VecDeque<Frame>,
    /// How far the device is between the first frame of the queue and the
    /// second, from 0 to 1.
    between: f64,
    /// Whether the device is being fed. It is not until the queue has
    /// filled to its target, at the start and after it ran dry: silence is
    /// better than sound in shreds.
    flowing: bool,
    /// Frames of silence given because the queue was dry, and frames
    /// dropped because it was too long.
    pub starved: u64,
    pub dropped: u64,
    /// Frames the device has taken, sound or silence.
    pub taken: u64,
    /// How many frames of the machine's sound go into one of the
    /// device's, less one: zero for a device that takes the machine's
    /// rate, and for one of another rate the machine's rate over the
    /// device's, less one.
    pub stretch: f64,
}

impl Feed {
    /// The machine's next frames.
    pub fn push(&mut self, frames: &[Frame]) {
        self.queue.extend(frames);
        if self.queue.len() > LIMIT {
            let excess = self.queue.len() - TARGET;
            self.queue.drain(..excess);
            self.dropped += excess as u64;
        }
    }

    /// The device's next frames.
    pub fn pull(&mut self, out: &mut [Frame]) {
        self.taken += out.len() as u64;
        if !self.flowing && self.queue.len() >= TARGET {
            self.flowing = true;
        }
        // A queue longer than its target is fed faster, a shorter one slower.
        let off_target = (self.queue.len() as f64 - TARGET as f64) / TARGET as f64;
        let pace = (1.0 + self.stretch) * (1.0 + BEND * off_target.clamp(-1.0, 1.0));
        for frame in out {
            if self.flowing && self.queue.len() < 2 {
                self.flowing = false;
            }
            if !self.flowing {
                *frame = [0; 2];
                self.starved += 1;
                continue;
            }
            let (from, to) = (self.queue[0], self.queue[1]);
            *frame = std::array::from_fn(|channel| {
                let (from, to) = (from[channel] as f64, to[channel] as f64);
                (from + (to - from) * self.between).round() as i16
            });
            self.between += pace;
            while self.between >= 1.0 && self.queue.len() >= 2 {
                self.queue.pop_front();
                self.between -= 1.0;
            }
        }
    }
}

/// The sound device, as the receiver of the machine's sound.
pub struct Speaker {
    feed: Arc<Mutex<Feed>>,
    losses: Losses,
    // The stream lives as long as the speaker.
    _stream: Stream,
}

/// The stream to the sound device: through the PulseAudio server, with
/// the connection to it.
#[cfg(unix)]
type Stream = (pulseaudio::PlaybackStream, pulseaudio::Client);
/// The stream to the sound device: through the system's own interface
/// (WASAPI on Windows).
#[cfg(not(unix))]
type Stream = cpal::Stream;

/// What a speaker lost, readable after the speaker itself has been handed
/// to the machine.
#[derive(Clone)]
pub struct Losses {
    feed: Arc<Mutex<Feed>>,
}

impl Losses {
    /// Frames of silence given for lack of sound, and frames dropped for
    /// being late, so far.
    pub fn count(&self) -> (u64, u64) {
        self.feed.lock().map_or((0, 0), |feed| (feed.starved, feed.dropped))
    }
}

#[cfg(not(unix))]
impl Speaker {
    /// Opens the default sound device, in the format it mixes in: the
    /// machine's sound is fitted to its rate and to its channels on the
    /// way. `UNVERIFIED`: written and compiled for Windows, never run.
    pub fn open() -> Result<Speaker, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let device = cpal::default_host().default_output_device().ok_or("no sound device")?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("no format of the sound device: {error}"))?;
        let (format, channels) = (supported.sample_format(), supported.channels() as usize);
        let config: cpal::StreamConfig = supported.into();
        let stretch = OUTPUT_RATE as f64 / config.sample_rate.0 as f64 - 1.0;
        let feed = Arc::new(Mutex::new(Feed { stretch, ..Feed::default() }));
        // The device's next frames, each as its channels' samples: the
        // machine's left and right on the first two (their mean on a
        // device of one), nothing on the others.
        fn fill<S: Copy>(
            feed: &Mutex<Feed>,
            channels: usize,
            out: &mut [S],
            silence: S,
            sample: fn(i16) -> S,
        ) {
            let mut frames = vec![[0i16; 2]; out.len() / channels.max(1)];
            // A poisoned lock means the other side panicked: silence.
            if let Ok(mut feed) = feed.lock() {
                feed.pull(&mut frames);
            }
            out.fill(silence);
            for (out, frame) in out.chunks_exact_mut(channels.max(1)).zip(&frames) {
                match out {
                    [only] => *only = sample(((frame[0] as i32 + frame[1] as i32) / 2) as i16),
                    [left, right, ..] => (*left, *right) = (sample(frame[0]), sample(frame[1])),
                    [] => {}
                }
            }
        }
        let failed = |error: cpal::StreamError| eprintln!("sound: {error}");
        let source = feed.clone();
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    fill(&source, channels, out, 0.0, |sample| sample as f32 / 32768.0)
                },
                failed,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config,
                move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    fill(&source, channels, out, 0, |sample| sample)
                },
                failed,
                None,
            ),
            other => return Err(format!("the sound device mixes in {other:?}, which is not handled yet")),
        };
        let stream = stream.map_err(|error| format!("no sound stream: {error}"))?;
        stream.play().map_err(|error| format!("the sound stream does not start: {error}"))?;
        Ok(Speaker { losses: Losses { feed: feed.clone() }, feed, _stream: stream })
    }
}

#[cfg(unix)]
impl Speaker {
    /// Opens the default sound device.
    pub fn open() -> Result<Speaker, String> {
        use pulseaudio::AsPlaybackSource;
        use pulseaudio::protocol::stream::{BufferAttr, StreamFlags};
        use pulseaudio::protocol::{
            ChannelMap, ChannelVolume, PlaybackStreamParams, SampleFormat, SampleSpec,
        };

        let client = pulseaudio::Client::from_env(c"Toy Story 2: Retold")
            .map_err(|error| format!("no PulseAudio server: {error}"))?;
        let feed = Arc::new(Mutex::new(Feed::default()));
        let source = feed.clone();
        let callback = move |bytes: &mut [u8]| {
            let mut frames = vec![[0i16; 2]; bytes.len() / 4];
            // A poisoned lock means the other side panicked: silence.
            if let Ok(mut feed) = source.lock() {
                feed.pull(&mut frames);
            }
            for (bytes, frame) in bytes.chunks_exact_mut(4).zip(&frames) {
                bytes[..2].copy_from_slice(&frame[0].to_le_bytes());
                bytes[2..].copy_from_slice(&frame[1].to_le_bytes());
            }
            // Nothing written would mean the sound has ended.
            bytes.len() - bytes.len() % 4
        };
        let params = PlaybackStreamParams {
            sample_spec: SampleSpec { format: SampleFormat::S16Le, channels: 2, sample_rate: OUTPUT_RATE },
            channel_map: ChannelMap::stereo(),
            cvolume: Some(ChannelVolume::norm(2)),
            // The server keeps 40 ms of its own, counted with the device's.
            buffer_attr: BufferAttr { target_length: OUTPUT_RATE * 4 * 40 / 1000, ..Default::default() },
            flags: StreamFlags { adjust_latency: true, ..Default::default() },
            ..Default::default()
        };
        let stream = pollster::block_on(client.create_playback_stream(params, callback.as_playback_source()))
            .map_err(|error| format!("no sound stream: {error}"))?;
        Ok(Speaker { losses: Losses { feed: feed.clone() }, feed, _stream: (stream, client) })
    }
}

impl Speaker {
    pub fn losses(&self) -> Losses {
        self.losses.clone()
    }
}

impl Audio for Speaker {
    fn play(&mut self, frames: &[Frame]) {
        if let Ok(mut feed) = self.feed.lock() {
            feed.push(frames);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rising line, so that every frame tells where in the stream it is.
    fn line(from: usize, count: usize) -> Vec<Frame> {
        (from..from + count).map(|at| [at as i16; 2]).collect()
    }

    #[test]
    fn the_device_is_fed_once_the_queue_has_filled() {
        let mut feed = Feed::default();
        let mut out = [[7; 2]; 4];
        feed.push(&line(0, TARGET - 1));
        feed.pull(&mut out);
        assert_eq!((out, feed.starved), ([[0; 2]; 4], 4));
        // At its target the queue is fed frame for frame.
        feed.push(&line(TARGET - 1, 5));
        feed.pull(&mut out);
        assert_eq!(out, [[0; 2], [1; 2], [2; 2], [3; 2]]);
    }

    #[test]
    fn a_long_queue_is_fed_faster_and_a_short_one_slower() {
        let mut out = vec![[0; 2]; 1000];
        // Twice the target: half a percent faster, five frames in a thousand.
        let mut feed = Feed::default();
        feed.push(&line(0, 2 * TARGET));
        feed.pull(&mut out);
        assert_eq!(feed.queue.len(), 2 * TARGET - 1004);
        assert_eq!(out[999][0], 1004);
        // Half the target: a quarter of a percent slower.
        let mut feed = Feed { flowing: true, ..Feed::default() };
        feed.push(&line(0, TARGET / 2));
        feed.pull(&mut out);
        assert_eq!(feed.queue.len(), TARGET / 2 - 997);
    }

    #[test]
    fn a_dry_queue_gives_silence_until_it_has_filled_again() {
        let mut feed = Feed { flowing: true, ..Feed::default() };
        feed.push(&line(1, 3));
        let mut out = [[9; 2]; 4];
        feed.pull(&mut out);
        // The last frame is reached, and then there is nothing to go on to.
        assert_eq!(out[..3], [[1; 2], [2; 2], [3; 2]]);
        assert_eq!((out[3], feed.starved), ([0; 2], 1));
        feed.push(&line(0, 10));
        feed.pull(&mut out);
        assert_eq!(out, [[0; 2]; 4]);
    }

    /// Plays a tone for three seconds on the sound device, fed at the pace
    /// of the computer's clock, and expects next to nothing lost:
    /// `cargo test -p andy speaker -- --ignored`.
    #[test]
    #[ignore]
    fn a_tone_on_the_sound_device() {
        let mut speaker = Speaker::open().unwrap();
        let losses = speaker.losses();
        let started = std::time::Instant::now();
        let mut sent = 0u64;
        while started.elapsed().as_secs() < 3 {
            let due = started.elapsed().as_millis() as u64 * OUTPUT_RATE as u64 / 1000;
            let frames: Vec<Frame> = (sent..due)
                .map(|at| {
                    [((at as f64 * 440.0 * std::f64::consts::TAU / OUTPUT_RATE as f64).sin() * 6000.0) as i16;
                        2]
                })
                .collect();
            speaker.play(&frames);
            sent = due;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let (starved, dropped) = losses.count();
        assert!(starved < OUTPUT_RATE as u64 / 5 && dropped == 0, "starved {starved}, dropped {dropped}");
    }

    #[test]
    fn sound_that_would_come_late_is_dropped() {
        let mut feed = Feed::default();
        feed.push(&line(0, LIMIT + 1));
        assert_eq!((feed.queue.len(), feed.dropped), (TARGET, (LIMIT + 1 - TARGET) as u64));
        // What is left is the newest.
        assert_eq!(feed.queue[0], [(LIMIT + 1 - TARGET) as i16; 2]);
    }
}
