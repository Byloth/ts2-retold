//! Reading bits, start codes and variable-length codes from a stream.

/// A position in a stream of bytes, read most significant bit first.
/// Reading past the end gives zero bits.
#[derive(Debug, Clone)]
pub struct Bits<'a> {
    bytes: &'a [u8],
    /// The next bit to read, counted from the start.
    position: usize,
}

impl<'a> Bits<'a> {
    pub fn new(bytes: &'a [u8]) -> Bits<'a> {
        Bits { bytes, position: 0 }
    }

    pub fn at(bytes: &'a [u8], byte: usize) -> Bits<'a> {
        Bits { bytes, position: 8 * byte }
    }

    /// The byte the next bit is in.
    pub fn byte_position(&self) -> usize {
        self.position / 8
    }

    /// Whether `count` more bits are there.
    pub fn has(&self, count: usize) -> bool {
        self.position + count <= 8 * self.bytes.len()
    }

    /// Reads up to 32 bits as a number.
    pub fn read(&mut self, count: u32) -> u32 {
        let mut value = 0u32;
        let mut left = count;
        while left > 0 {
            let byte = self.bytes.get(self.position / 8).copied().unwrap_or(0) as u32;
            let available = 8 - (self.position % 8) as u32;
            let take = available.min(left);
            let bits = byte >> (available - take) & ((1 << take) - 1);
            value = if take == 32 { bits } else { value << take | bits };
            self.position += take as usize;
            left -= take;
        }
        value
    }

    pub fn read_bit(&mut self) -> bool {
        self.read(1) != 0
    }

    pub fn skip(&mut self, count: usize) {
        self.position += count;
    }

    /// Moves to the next byte boundary.
    pub fn align(&mut self) {
        self.position = self.position.next_multiple_of(8);
    }

    /// Skips bytes equal to `value`; returns how many.
    pub fn skip_bytes(&mut self, value: u8) -> usize {
        self.align();
        let mut skipped = 0;
        while self.bytes.get(self.position / 8) == Some(&value) {
            self.position += 8;
            skipped += 1;
        }
        skipped
    }

    /// Moves past the next start code (`00 00 01 xx`) and returns its last
    /// byte; `None`, at the end of the stream, if there is none.
    pub fn next_start_code(&mut self) -> Option<u8> {
        self.align();
        let mut byte = self.position / 8;
        while byte + 4 <= self.bytes.len() {
            if self.bytes[byte..byte + 3] == [0, 0, 1] {
                self.position = 8 * (byte + 4);
                return Some(self.bytes[byte + 3]);
            }
            byte += 1;
        }
        self.position = 8 * self.bytes.len();
        None
    }

    /// Moves past the next start code with the given last byte.
    pub fn find_start_code(&mut self, code: u8) -> bool {
        loop {
            match self.next_start_code() {
                Some(found) if found == code => return true,
                Some(_) => {}
                None => return false,
            }
        }
    }

    /// Whether the next `count` bits are there and not all zero.
    pub fn peek_non_zero(&self, count: u32) -> bool {
        self.has(count as usize) && self.clone().read(count) != 0
    }

    /// Reads one variable-length code (see [`crate::tables`]).
    pub fn read_vlc(&mut self, table: &[(i16, i16)]) -> i16 {
        let mut index = 0;
        loop {
            let (next, value) = table[index + self.read(1) as usize];
            if next <= 0 {
                return value;
            }
            index = next as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;

    #[test]
    fn bits_are_read_most_significant_first_and_zero_past_the_end() {
        let mut bits = Bits::new(&[0b1010_0101, 0xFF, 0x12, 0x34, 0x56]);
        assert_eq!((bits.read(1), bits.read(3), bits.read(4)), (1, 0b010, 0b0101));
        assert_eq!(bits.read(12), 0xFF1);
        assert!(bits.has(20) && !bits.has(21));
        assert_eq!(bits.read(32), 0x2345_6000);
        assert!(!bits.has(1));
        let mut bits = Bits::new(&[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(bits.read(32), 0x1234_5678);
    }

    #[test]
    fn start_codes_are_found_on_byte_boundaries() {
        let stream = [0xFF, 0, 0, 1, 0xB3, 9, 0, 0, 0, 1, 0x00, 7, 0, 0, 1];
        let mut bits = Bits::new(&stream);
        bits.read(3);
        assert_eq!(bits.next_start_code(), Some(0xB3));
        assert_eq!(bits.read(8), 9);
        assert!(bits.find_start_code(0x00));
        assert_eq!(bits.read(8), 7);
        assert_eq!(bits.next_start_code(), None);
        assert!(!bits.has(1));
    }

    #[test]
    fn variable_length_codes_follow_the_tree() {
        // Macroblock address increment: "1" is 1, "011" is 2, "010" is 3.
        let mut bits = Bits::new(&[0b1011_0100]);
        let table = &tables::MACROBLOCK_ADDRESS_INCREMENT;
        assert_eq!((bits.read_vlc(table), bits.read_vlc(table), bits.read_vlc(table)), (1, 2, 3));
        // Motion: "1" is 0, "010" is 1, "011" is -1.
        let mut bits = Bits::new(&[0b1010_0110]);
        let table = &tables::MOTION;
        assert_eq!((bits.read_vlc(table), bits.read_vlc(table), bits.read_vlc(table)), (0, 1, -1));
    }
}
