//! Where the machine's sound goes.
//!
//! The runtime mixes what the program plays (`win32::dsound`) into one
//! stream, as the sound card's mixer did, and hands it on in pieces as the
//! machine's clock advances. What receives it, a file or a sound device,
//! is outside the runtime, behind [`Audio`].

/// Frames a second of the mixed stream.
pub const OUTPUT_RATE: u32 = 44_100;

/// One moment of the mixed stream: the left and the right sample.
pub type Frame = [i16; 2];

/// The receiver of the machine's sound.
pub trait Audio {
    /// The next frames of the stream, [`OUTPUT_RATE`] of them for every
    /// second of the machine's clock.
    fn play(&mut self, frames: &[Frame]);
}

/// The frames of the stream the machine's clock has reached at `clock_ms`.
pub(crate) fn frames_at(clock_ms: u64) -> u64 {
    clock_ms * OUTPUT_RATE as u64 / 1000
}
