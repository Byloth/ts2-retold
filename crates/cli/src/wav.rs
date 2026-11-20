//! The machine's sound written to a file: a WAVE file of 16-bit stereo
//! samples.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use andy_runtime::audio::{Audio, Frame, OUTPUT_RATE};

/// The bytes before the samples.
const HEADER: u32 = 44;
const BYTES_PER_FRAME: u32 = 4;

pub struct WaveFile {
    file: BufWriter<File>,
    frames: u32,
}

fn header(frames: u32) -> Vec<u8> {
    let data = frames * BYTES_PER_FRAME;
    let mut bytes = Vec::with_capacity(HEADER as usize);
    bytes.extend(b"RIFF");
    bytes.extend((HEADER - 8 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    // Uncompressed, two channels.
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(OUTPUT_RATE.to_le_bytes());
    bytes.extend((OUTPUT_RATE * BYTES_PER_FRAME).to_le_bytes());
    bytes.extend((BYTES_PER_FRAME as u16).to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes
}

impl WaveFile {
    pub fn create(path: &Path) -> std::io::Result<WaveFile> {
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(&header(0))?;
        Ok(WaveFile { file, frames: 0 })
    }
}

impl Audio for WaveFile {
    fn play(&mut self, frames: &[Frame]) {
        for frame in frames {
            let [left, right] = frame.map(i16::to_le_bytes);
            // A failed write shows as a short file; the lengths in the
            // header are those of what was written.
            if self.file.write_all(&[left[0], left[1], right[0], right[1]]).is_err() {
                return;
            }
            self.frames += 1;
        }
    }
}

impl Drop for WaveFile {
    /// The lengths can only be written once the sound has ended.
    fn drop(&mut self) {
        let header = header(self.frames);
        let _ = self.file.seek(SeekFrom::Start(0)).and_then(|_| self.file.write_all(&header));
        let _ = self.file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_has_its_lengths_once_it_is_closed() {
        let path = std::env::temp_dir().join(format!("andy-wave-test-{}.wav", std::process::id()));
        let mut wave = WaveFile::create(&path).unwrap();
        wave.play(&[[1, -1], [0x1234, -0x8000]]);
        drop(wave);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(bytes.len(), 52);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes[4..8], 44u32.to_le_bytes());
        assert_eq!(bytes[24..28], 44_100u32.to_le_bytes());
        assert_eq!(bytes[40..44], 8u32.to_le_bytes());
        assert_eq!(bytes[44..], [1, 0, 0xFF, 0xFF, 0x34, 0x12, 0x00, 0x80]);
    }
}
