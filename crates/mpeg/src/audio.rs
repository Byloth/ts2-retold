//! The audio stream: MPEG-1 Layer II (ISO/IEC 11172-3).
//!
//! A stream is a run of frames of 1152 samples each. A frame says, for
//! each of 32 subbands of the spectrum, how finely its samples are
//! quantised and how they are scaled; the samples come in twelve groups of
//! three per subband, and a filter bank turns each group's 32 subband
//! samples into 32 output samples.
//!
//! The samples and the filter bank are `f32`, whose sums, products and
//! quotients are the same on every machine when done in the same order,
//! as they are here: a stream decodes to the same samples everywhere.
//!
//! **Requantisation follows the standard, not pl_mpeg.** A sample coded
//! with `n` levels stands for `(2 * code - (n - 1)) / n` (the standard's
//! `C * (s + D)`, table 3-B.4, in one expression). pl_mpeg, after kjmp2,
//! computes it in integers with `n + 1` for the divisor, which makes
//! every subband quieter by `n / (n + 1)`: a quarter for the coarsest
//! quantiser, three levels, and so most at the low bit rates the game's
//! movies use. It also divides its output by 65/64 more than the filter
//! bank's scale.

use std::sync::Arc;

use crate::audio_tables::{
    BIT_RATE, QUANT_LUT_STEP_1, QUANT_LUT_STEP_2, QUANT_LUT_STEP_3, QUANT_LUT_STEP_4, QUANTISERS,
    SAMPLE_RATE, SYNTHESIS_WINDOW, idct36,
};
use crate::bits::Bits;

/// Samples in a frame.
pub const FRAME: usize = 1152;

const SYNC: u32 = 0x7FF;
const MPEG_1: u32 = 3;
const LAYER_II: u32 = 2;
const JOINT_STEREO: u32 = 1;
const MONO: u32 = 3;
/// The first three scale factors (the standard's table 3-B.1): 2, the cube
/// root of 4 and the cube root of 2. Each further three are half the three
/// before, which is exact in floating point.
const SCALE_FACTORS: [f64; 3] = [2.0, 1.587_401_051_968_199_4, 1.259_921_049_894_873_2];
/// pl_mpeg's window is the standard's times 32768.
const OUTPUT_SCALE: f32 = 32768.0;

/// The scale factor of an index from 0 to 62; index 63 is not one, and is
/// taken as silence.
fn scale_factor(index: u32) -> f32 {
    if index == 63 { 0.0 } else { (SCALE_FACTORS[index as usize % 3] / (1u64 << (index / 3)) as f64) as f32 }
}

/// The decoder of one audio stream.
pub struct Decoder {
    bits: Bits,
    /// What the stream's frames have in common, from the first header:
    /// the indices of the bit rate and the sample rate, and the mode.
    stream: Option<(usize, usize, u32)>,
    /// From which subband on the two channels share their samples.
    bound: usize,
    /// The bytes of the frame whose header was read last; zero if none.
    frame_bytes: usize,
    /// Per channel and subband: one more than the quantiser's index, or
    /// zero for a subband without samples.
    allocation: [[u8; 32]; 2],
    scale_factor_info: [[u8; 32]; 2],
    scale_factor: [[[u32; 3]; 32]; 2],
    sample: [[[f32; 3]; 32]; 2],
    /// The filter bank: its window twice over, its vector per channel and
    /// where the vector's newest values are.
    window: Box<[f32; 1024]>,
    vector: Box<[[f32; 1024]; 2]>,
    vector_position: usize,
    samples: Box<[[f32; 2]; FRAME]>,
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "audio::Decoder {{ {} Hz, {} channel(s) }}", self.rate(), self.channels())
    }
}

impl Decoder {
    /// A decoder at the start of a stream. `None` if no frame of Layer II
    /// is found in it.
    pub fn new(stream: impl Into<Arc<[u8]>>) -> Option<Decoder> {
        let mut window = Box::new([0.0; 1024]);
        window[..512].copy_from_slice(&SYNTHESIS_WINDOW);
        window[512..].copy_from_slice(&SYNTHESIS_WINDOW);
        let mut decoder = Decoder {
            bits: Bits::new(stream),
            stream: None,
            bound: 0,
            frame_bytes: 0,
            allocation: [[0; 32]; 2],
            scale_factor_info: [[0; 32]; 2],
            scale_factor: [[[0; 3]; 32]; 2],
            sample: [[[0.0; 3]; 32]; 2],
            window,
            vector: Box::new([[0.0; 1024]; 2]),
            vector_position: 0,
            samples: Box::new([[0.0; 2]; FRAME]),
        };
        while decoder.stream.is_none() && decoder.bits.has(48) {
            decoder.frame_bytes = decoder.read_header();
        }
        decoder.stream.is_some().then_some(decoder)
    }

    /// Samples a second.
    pub fn rate(&self) -> u32 {
        self.stream.map_or(0, |(_, rate, _)| SAMPLE_RATE[rate])
    }

    /// One or two. A stream of one channel still decodes to two equal ones.
    pub fn channels(&self) -> u32 {
        if self.stream.is_some_and(|(_, _, mode)| mode == MONO) { 1 } else { 2 }
    }

    /// Decodes the next frame: its samples as left and right, from -1 to
    /// 1. `None` at the end of the stream.
    pub fn decode(&mut self) -> Option<&[[f32; 2]; FRAME]> {
        loop {
            if self.frame_bytes == 0 {
                if !self.bits.has(48) {
                    return None;
                }
                // Without a header here, the search goes on further along.
                self.frame_bytes = self.read_header();
                continue;
            }
            if !self.bits.has(8 * self.frame_bytes) {
                return None;
            }
            self.read_frame();
            self.frame_bytes = 0;
            return Some(&self.samples);
        }
    }

    /// Moves to just after the next frame sync from here on, if there is one.
    fn find_sync(&mut self) -> bool {
        let bytes = self.bits.bytes();
        let from = self.bits.byte_position();
        let found = (from..bytes.len().saturating_sub(1))
            .find(|&at| bytes[at] == 0xFF && bytes[at + 1] & 0xFE == 0xFC);
        match found {
            Some(at) => {
                // Eleven bits of sync: a byte and three bits.
                self.bits.seek(at + 1);
                self.bits.skip(3);
                true
            }
            None => {
                self.bits.seek(bytes.len());
                false
            }
        }
    }

    /// Reads a frame's header and returns the bytes of the rest of the
    /// frame, or zero if there is no usable header here.
    ///
    /// The sync pattern may also turn up inside a frame, so after losing
    /// the place a header only counts if it agrees with the stream's
    /// first one.
    fn read_header(&mut self) -> usize {
        if !self.bits.has(48) {
            return 0;
        }
        self.bits.skip_bytes(0x00);
        if self.bits.read(11) != SYNC && !self.find_sync() {
            return 0;
        }
        let version = self.bits.read(2);
        let layer = self.bits.read(2);
        let has_crc = !self.bits.read_bit();
        if version != MPEG_1 || layer != LAYER_II {
            return 0;
        }
        // Index 0 is "free format", 15 is forbidden.
        let bit_rate = self.bits.read(4);
        if !(1..=14).contains(&bit_rate) {
            return 0;
        }
        let bit_rate = bit_rate as usize - 1;
        let rate = self.bits.read(2) as usize;
        if rate == 3 {
            return 0;
        }
        let padding = self.bits.read(1) as usize;
        self.bits.skip(1); // private
        let mode = self.bits.read(2);
        if self.stream.is_some_and(|stream| stream != (bit_rate, rate, mode)) {
            return 0;
        }
        self.stream = Some((bit_rate, rate, mode));
        // The mode extension: where joint stereo begins.
        if mode == JOINT_STEREO {
            self.bound = (self.bits.read(2) as usize + 1) << 2;
        } else {
            self.bits.skip(2);
            self.bound = if mode == MONO { 0 } else { 32 };
        }
        self.bits.skip(4); // copyright, original, emphasis
        if has_crc {
            self.bits.skip(16);
        }
        let frame = 144_000 * BIT_RATE[bit_rate] as usize / SAMPLE_RATE[rate] as usize + padding;
        frame - if has_crc { 6 } else { 4 }
    }

    fn read_allocation(&mut self, subband: usize, table: usize) -> u8 {
        let entry = QUANT_LUT_STEP_3[table][subband];
        QUANT_LUT_STEP_4[(entry & 15) as usize][self.bits.read((entry >> 4) as u32) as usize]
    }

    /// Reads the three samples of a subband and gives them their values.
    fn read_samples(&mut self, channel: usize, subband: usize, part: usize) {
        let allocation = self.allocation[channel][subband];
        if allocation == 0 {
            self.sample[channel][subband] = [0.0; 3];
            return;
        }
        let (levels, grouped, bits) = QUANTISERS[allocation as usize - 1];
        let factor = scale_factor(self.scale_factor[channel][subband][part]);
        let mut codes = [0i32; 3];
        if grouped {
            // Three samples in one number, in the base of the levels.
            let mut value = self.bits.read(bits) as i32;
            codes[0] = value % levels;
            value /= levels;
            codes[1] = value % levels;
            codes[2] = value / levels;
        } else {
            for code in &mut codes {
                *code = self.bits.read(bits) as i32;
            }
        }
        self.sample[channel][subband] =
            codes.map(|code| (2 * code - (levels - 1)) as f32 / levels as f32 * factor);
    }

    fn read_frame(&mut self) {
        let (bit_rate, rate, mode) = self.stream.expect("a header was read");
        let mono = mode == MONO;
        let class = QUANT_LUT_STEP_1[if mono { 0 } else { 1 }][bit_rate] as usize;
        let entry = QUANT_LUT_STEP_2[class][rate] as usize;
        let (subbands, table) = (entry & 63, entry >> 6);
        self.bound = self.bound.min(subbands);
        let bound = self.bound;

        for subband in 0..bound {
            self.allocation[0][subband] = self.read_allocation(subband, table);
            self.allocation[1][subband] = self.read_allocation(subband, table);
        }
        for subband in bound..subbands {
            let allocation = self.read_allocation(subband, table);
            self.allocation[0][subband] = allocation;
            self.allocation[1][subband] = allocation;
        }

        // Which of a subband's three scale factors are given.
        let channels = if mono { 1 } else { 2 };
        for subband in 0..subbands {
            for channel in 0..channels {
                if self.allocation[channel][subband] != 0 {
                    self.scale_factor_info[channel][subband] = self.bits.read(2) as u8;
                }
            }
            if mono {
                self.scale_factor_info[1][subband] = self.scale_factor_info[0][subband];
            }
        }
        for subband in 0..subbands {
            for channel in 0..channels {
                if self.allocation[channel][subband] != 0 {
                    let mut next = || self.bits.read(6);
                    self.scale_factor[channel][subband] = match self.scale_factor_info[channel][subband] {
                        0 => [next(), next(), next()],
                        1 => {
                            let (first, last) = (next(), next());
                            [first, first, last]
                        }
                        2 => [next(); 3],
                        _ => {
                            let (first, last) = (next(), next());
                            [first, last, last]
                        }
                    };
                }
            }
            if mono {
                self.scale_factor[1][subband] = self.scale_factor[0][subband];
            }
        }

        let mut out = 0;
        for part in 0..3 {
            for _granule in 0..4 {
                for subband in 0..bound {
                    self.read_samples(0, subband, part);
                    self.read_samples(1, subband, part);
                }
                for subband in bound..subbands {
                    self.read_samples(0, subband, part);
                    self.sample[1][subband] = self.sample[0][subband];
                }
                for subband in subbands..32 {
                    self.sample[0][subband] = [0.0; 3];
                    self.sample[1][subband] = [0.0; 3];
                }

                // The filter bank, once for each of the three samples.
                for third in 0..3 {
                    self.vector_position = self.vector_position.wrapping_sub(64) & 1023;
                    let position = self.vector_position;
                    for channel in 0..2 {
                        let vector = &mut self.vector[channel];
                        idct36(&self.sample[channel], third, vector, position);
                        // The vector under the window, summed into 32 samples.
                        let mut sums = [0f32; 32];
                        let mut window_at = 512 - (position >> 1);
                        let mut vector_at = (position % 128) >> 1;
                        let mut window_under_vector = |window_at: &mut usize, vector_at: &mut usize| {
                            while *vector_at < 1024 {
                                for sum in &mut sums {
                                    *sum += self.window[*window_at] * vector[*vector_at];
                                    *window_at += 1;
                                    *vector_at += 1;
                                }
                                *vector_at += 128 - 32;
                                *window_at += 64 - 32;
                            }
                        };
                        window_under_vector(&mut window_at, &mut vector_at);
                        // And once more over the values in between.
                        window_at -= 512 - 32;
                        vector_at = (128 - 32 + 1024) - vector_at;
                        window_under_vector(&mut window_at, &mut vector_at);
                        for (sample, sum) in self.samples[out..out + 32].iter_mut().zip(sums) {
                            sample[channel] = sum / OUTPUT_SCALE;
                        }
                    }
                    out += 32;
                }
            }
        }
        self.bits.align();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_factors_are_twice_a_power_of_the_cube_root_of_a_half() {
        // The constants are what they are said to be.
        assert!((SCALE_FACTORS[1].powi(3) - 4.0).abs() < 1e-14);
        assert!((SCALE_FACTORS[2].powi(3) - 2.0).abs() < 1e-14);
        assert_eq!(
            (scale_factor(0), scale_factor(3), scale_factor(6), scale_factor(63)),
            (2.0, 1.0, 0.5, 0.0)
        );
        assert_eq!(scale_factor(62), (SCALE_FACTORS[2] / (1u64 << 20) as f64) as f32);
        for index in 0..62 {
            assert!(scale_factor(index) > scale_factor(index + 1));
        }
    }

    /// A frame of 48 kbit/s, 32 kHz, one channel: twelve subbands, of
    /// which only the first has samples, three levels, all at the given
    /// code and scale factor index.
    fn frame(code: u32, factor: u32) -> Vec<u8> {
        let mut bits: Vec<bool> = Vec::new();
        let mut put = |value: u32, count: u32| bits.extend((0..count).rev().map(|bit| value >> bit & 1 == 1));
        put(0x7FF, 11); // sync
        put(3, 2); // MPEG-1
        put(2, 2); // Layer II
        put(1, 1); // no CRC
        put(2, 4); // 48 kbit/s
        put(2, 2); // 32 kHz
        put(0, 2); // no padding, private
        put(3, 2); // one channel
        put(0, 6); // mode extension, copyright, original, emphasis
        // Allocations: four bits for the first two subbands, three for the
        // other ten. In this table's row an allocation of 1 is three levels.
        put(1, 4);
        put(0, 4 + 3 * 10);
        put(2, 2); // one scale factor for all three parts
        put(factor, 6);
        // Twelve groups of three samples, each group one number of five bits.
        for _ in 0..12 {
            put(code * 9 + code * 3 + code, 5);
        }
        let mut bytes = vec![0u8; 144_000 * 48 / 32_000];
        for (at, bit) in bits.iter().enumerate() {
            bytes[at / 8] |= u8::from(*bit) << (7 - at % 8);
        }
        bytes
    }

    #[test]
    fn a_frame_is_found_read_and_turned_into_sound() {
        // Garbage first: the decoder finds the frame after it.
        let mut stream = vec![0x12, 0xFF, 0x00];
        stream.extend(frame(2, 3));
        stream.extend(frame(2, 3));
        let mut decoder = Decoder::new(stream).unwrap();
        assert_eq!((decoder.rate(), decoder.channels()), (32_000, 1));
        // The lowest subband held at one value: a subband sample that does
        // not change is the bottom of the subband, which for the lowest is
        // a steady level. Once the filter has filled, the output is that
        // level, 2/3 for the top code of three at a scale factor of one
        // (the filter passes it within a few parts in a thousand); both
        // channels the same.
        decoder.decode().unwrap();
        let samples = decoder.decode().unwrap();
        assert!(samples.iter().all(|sample| sample[0] == sample[1]));
        let (low, high) = samples
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), sample| (low.min(sample[0]), high.max(sample[0])));
        assert!((low - 2.0 / 3.0).abs() < 0.005 && (high - 2.0 / 3.0).abs() < 0.005, "{low} to {high}");
        assert!(decoder.decode().is_none());
        // The middle code of three is silence; so is the scale factor 63.
        for (code, factor) in [(1, 3), (2, 63)] {
            let mut decoder = Decoder::new(frame(code, factor)).unwrap();
            assert!(decoder.decode().unwrap().iter().all(|sample| *sample == [0.0; 2]));
        }
        assert!(Decoder::new(vec![0u8; 100]).is_none());
    }
}
