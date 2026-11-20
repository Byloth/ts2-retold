//! The system layer: a file of packs and packets, taken apart into its
//! video stream and its audio stream.

use crate::bits::Bits;

const PACK: u8 = 0xBA;
const VIDEO: u8 = 0xE0;
const AUDIO: u8 = 0xC0;
/// Ticks per second of the time stamps.
const CLOCK: f64 = 90_000.0;

/// One of the streams of a file, with the packets' bytes joined together.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stream {
    pub bytes: Vec<u8>,
    /// Presentation times, in seconds, of the packets that carry one: where
    /// in `bytes` the packet starts, and its time.
    pub times: Vec<(usize, f64)>,
}

/// The first video stream and the first audio stream of a file.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Streams {
    pub video: Stream,
    pub audio: Stream,
}

/// A 33-bit time stamp in three pieces, each followed by a marker bit.
fn time(bits: &mut Bits) -> f64 {
    let mut clock = (bits.read(3) as u64) << 30;
    bits.skip(1);
    clock |= (bits.read(15) as u64) << 15;
    bits.skip(1);
    clock |= bits.read(15) as u64;
    bits.skip(1);
    clock as f64 / CLOCK
}

/// Takes an MPEG-1 system stream apart. `None` if the bytes do not start
/// one.
pub fn demux(file: &[u8]) -> Option<Streams> {
    let mut bits = Bits::new(file);
    // A system stream starts with a pack, whose header starts with 0010.
    if bits.next_start_code() != Some(PACK) || bits.read(4) != 0b0010 {
        return None;
    }
    let mut streams = Streams::default();
    while let Some(code) = bits.next_start_code() {
        let stream = match code {
            VIDEO => &mut streams.video,
            AUDIO => &mut streams.audio,
            _ => continue,
        };
        // Packet: length, stuffing bytes, an optional buffer size, and
        // time stamps or the four bits that say there are none.
        if !bits.has(16) {
            break;
        }
        let mut length = bits.read(16) as usize;
        let mut header = bits.skip_bytes(0xFF);
        // Two bits say whether a buffer size comes first: if so, its
        // remaining fourteen bits are skipped together with the two that
        // start the next field, which otherwise are the two just read.
        if bits.read(2) == 0b01 {
            bits.skip(16);
            header += 2;
        }
        let stamp = match bits.read(2) {
            0b11 => {
                let stamp = time(&mut bits);
                bits.skip(40); // the decoding time
                header += 10;
                Some(stamp)
            }
            0b10 => {
                header += 5;
                Some(time(&mut bits))
            }
            0b00 => {
                bits.skip(4);
                header += 1;
                None
            }
            _ => continue, // not a packet of this kind of stream
        };
        if length < header {
            continue;
        }
        length -= header;
        let start = bits.byte_position();
        let Some(payload) = file.get(start..start + length) else { break };
        if let Some(stamp) = stamp {
            stream.times.push((stream.bytes.len(), stamp));
        }
        stream.bytes.extend_from_slice(payload);
        bits = Bits::at(file, start + length);
    }
    Some(streams)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet of an MPEG-1 system stream.
    fn packet(stream: u8, stuffing: usize, buffer_size: bool, stamp: Option<u64>, payload: &[u8]) -> Vec<u8> {
        let mut header = vec![0xFF; stuffing];
        if buffer_size {
            header.extend_from_slice(&[0x40, 0x20]);
        }
        match stamp {
            Some(clock) => header.extend_from_slice(&[
                0x21 | (clock >> 29 & 0x0E) as u8,
                (clock >> 22) as u8,
                (clock >> 14) as u8 | 1,
                (clock >> 7) as u8,
                (clock << 1) as u8 | 1,
            ]),
            None => header.push(0x0F),
        }
        let length = (header.len() + payload.len()) as u16;
        let mut bytes = vec![0, 0, 1, stream];
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn packets_are_joined_per_stream_with_their_times() {
        let mut file = vec![0, 0, 1, PACK, 0x21, 0, 1, 0, 1, 0x80, 0, 1];
        file.extend(packet(VIDEO, 0, true, Some(90_000), &[1, 2, 3]));
        file.extend(packet(AUDIO, 3, false, Some(45_000), &[9, 8]));
        file.extend(packet(0xBE, 0, false, None, &[0xFF; 6])); // padding
        file.extend(packet(VIDEO, 2, false, None, &[4, 5]));
        file.extend(packet(VIDEO, 0, true, Some(0x1_2345_6789), &[6]));
        let streams = demux(&file).unwrap();
        assert_eq!(streams.video.bytes, [1, 2, 3, 4, 5, 6]);
        assert_eq!(streams.video.times, [(0, 1.0), (5, 0x1_2345_6789u64 as f64 / 90_000.0)]);
        assert_eq!(streams.audio.bytes, [9, 8]);
        assert_eq!(streams.audio.times, [(0, 0.5)]);
        assert_eq!(demux(&[0, 0, 1, 0xB3, 0, 0, 0, 0]), None);
    }
}
