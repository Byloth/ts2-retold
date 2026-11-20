//! The video layer (ISO/IEC 11172-2): pictures from a video stream.

use crate::bits::Bits;
use crate::tables;

const START_PICTURE: u8 = 0x00;
const START_SLICE_FIRST: u8 = 0x01;
const START_SLICE_LAST: u8 = 0xAF;
const START_USER_DATA: u8 = 0xB2;
const START_SEQUENCE: u8 = 0xB3;
const START_EXTENSION: u8 = 0xB5;

/// Pictures per second for each code of the sequence header, as a fraction.
const PICTURE_RATES: [(u32, u32); 9] =
    [(0, 1), (24000, 1001), (24, 1), (25, 1), (30000, 1001), (30, 1), (50, 1), (60000, 1001), (60, 1)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PictureType {
    Intra,
    Predictive,
    Bidirectional,
}

/// A decoded picture: one byte of luminance per pixel, and two planes of
/// chrominance at half the size each way. The planes cover whole
/// macroblocks, so they may be larger than the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The picture's size in pixels.
    pub width: usize,
    pub height: usize,
    /// Bytes per row of the luminance plane; the chrominance planes have half.
    pub stride: usize,
    pub y: Vec<u8>,
    pub cb: Vec<u8>,
    pub cr: Vec<u8>,
}

impl Frame {
    fn new(width: usize, height: usize, macroblocks: (usize, usize)) -> Frame {
        let (luma, chroma) = (256 * macroblocks.0 * macroblocks.1, 64 * macroblocks.0 * macroblocks.1);
        Frame {
            width,
            height,
            stride: 16 * macroblocks.0,
            y: vec![0; luma],
            cb: vec![0; chroma],
            cr: vec![0; chroma],
        }
    }

    /// The picture as rows of red, green, blue bytes followed by one of
    /// padding per pixel (the ITU-R BT.601 conversion, in integers).
    pub fn to_rgbx(&self) -> Vec<u8> {
        let clamp = |value: i32| value.clamp(0, 255) as u8;
        let mut out = Vec::with_capacity(4 * self.width * self.height);
        for row in 0..self.height {
            for column in 0..self.width {
                let chroma = (row / 2) * (self.stride / 2) + column / 2;
                let (cb, cr) = (self.cb[chroma] as i32 - 128, self.cr[chroma] as i32 - 128);
                let y = ((self.y[row * self.stride + column] as i32 - 16) * 76309) >> 16;
                let red = (cr * 104597) >> 16;
                let green = (cb * 25674 + cr * 53278) >> 16;
                let blue = (cb * 132201) >> 16;
                out.extend_from_slice(&[clamp(y + red), clamp(y - green), clamp(y + blue), 0xFF]);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Motion {
    /// Vectors are in whole pixels rather than halves.
    full_pixel: bool,
    /// Whether the macroblock has a vector of this kind.
    is_set: bool,
    r_size: u32,
    h: i32,
    v: i32,
}

/// Why a stream cannot be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The stream has no sequence header, or one without a size or rate.
    NoSequenceHeader,
}

/// A decoder of one video stream held in memory.
pub struct Decoder<'a> {
    bits: Bits<'a>,
    width: usize,
    height: usize,
    /// Macroblocks across and down.
    mb_width: usize,
    mb_height: usize,
    rate: (u32, u32),
    intra_quant_matrix: [u8; 64],
    non_intra_quant_matrix: [u8; 64],
    /// The picture being decoded and the two it may be predicted from. In
    /// display order `forward` comes before it and `backward` after.
    current: Frame,
    forward: Frame,
    backward: Frame,
    /// Whether `backward` holds a picture not yet given out.
    has_reference: bool,
    /// The last start code read, when it has not been acted on yet.
    start_code: Option<u8>,
    picture_type: PictureType,
    motion_forward: Motion,
    motion_backward: Motion,
    quantizer_scale: i32,
    slice_begin: bool,
    macroblock_address: isize,
    mb_row: usize,
    mb_col: usize,
    macroblock_intra: bool,
    dc_predictor: [i32; 3],
    block: [i32; 64],
    frames_decoded: u64,
}

impl<'a> Decoder<'a> {
    /// Reads the sequence header of a video stream.
    pub fn new(stream: &'a [u8]) -> Result<Decoder<'a>, Error> {
        let mut bits = Bits::new(stream);
        if !bits.find_start_code(START_SEQUENCE) || !bits.has(64) {
            return Err(Error::NoSequenceHeader);
        }
        let (width, height) = (bits.read(12) as usize, bits.read(12) as usize);
        bits.skip(4); // the shape of a pixel
        let rate = PICTURE_RATES.get(bits.read(4) as usize).copied().unwrap_or((0, 1));
        if width == 0 || height == 0 || rate.0 == 0 {
            return Err(Error::NoSequenceHeader);
        }
        bits.skip(18 + 1 + 10 + 1); // bit rate, marker, buffer size, constrained flag
        let mut matrix = |default: &[u8; 64]| {
            let mut matrix = *default;
            if bits.read_bit() {
                for &position in &tables::ZIG_ZAG {
                    matrix[position as usize] = bits.read(8) as u8;
                }
            }
            matrix
        };
        let intra_quant_matrix = matrix(&tables::INTRA_QUANT_MATRIX);
        let non_intra_quant_matrix = matrix(&tables::NON_INTRA_QUANT_MATRIX);
        let macroblocks = (width.div_ceil(16), height.div_ceil(16));
        let frame = Frame::new(width, height, macroblocks);
        Ok(Decoder {
            bits,
            width,
            height,
            mb_width: macroblocks.0,
            mb_height: macroblocks.1,
            rate,
            intra_quant_matrix,
            non_intra_quant_matrix,
            current: frame.clone(),
            forward: frame.clone(),
            backward: frame,
            has_reference: false,
            start_code: None,
            picture_type: PictureType::Intra,
            motion_forward: Motion::default(),
            motion_backward: Motion::default(),
            quantizer_scale: 0,
            slice_begin: false,
            macroblock_address: 0,
            mb_row: 0,
            mb_col: 0,
            macroblock_intra: false,
            dc_predictor: [128; 3],
            block: [0; 64],
            frames_decoded: 0,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Pictures per second, as a fraction.
    pub fn rate(&self) -> (u32, u32) {
        self.rate
    }

    /// How many pictures have been given out.
    pub fn frames_decoded(&self) -> u64 {
        self.frames_decoded
    }

    /// The next picture in display order; `None` at the end of the stream.
    ///
    /// Pictures are stored in the order they are needed for prediction: a
    /// picture others are predicted from is given out when the next such
    /// picture has been decoded, or at the end.
    pub fn decode(&mut self) -> Option<&Frame> {
        #[derive(Clone, Copy)]
        enum Which {
            Current,
            Forward,
            Backward,
        }
        let which = loop {
            if self.start_code != Some(START_PICTURE) {
                if !self.bits.find_start_code(START_PICTURE) {
                    self.start_code = None;
                    // The last reference picture is still to be shown.
                    if std::mem::take(&mut self.has_reference) {
                        break Which::Backward;
                    }
                    return None;
                }
                self.start_code = Some(START_PICTURE);
            }
            self.decode_picture();
            if self.picture_type == PictureType::Bidirectional {
                break Which::Current;
            } else if self.has_reference {
                break Which::Forward;
            }
            self.has_reference = true;
        };
        self.frames_decoded += 1;
        Some(match which {
            Which::Current => &self.current,
            Which::Forward => &self.forward,
            Which::Backward => &self.backward,
        })
    }

    fn decode_picture(&mut self) {
        self.start_code = None;
        self.bits.skip(10); // temporal reference
        self.picture_type = match self.bits.read(3) {
            1 => PictureType::Intra,
            2 => PictureType::Predictive,
            3 => PictureType::Bidirectional,
            _ => return,
        };
        self.bits.skip(16); // buffer delay
        if self.picture_type != PictureType::Intra {
            self.motion_forward.full_pixel = self.bits.read_bit();
            match self.bits.read(3) {
                0 => return,
                f_code => self.motion_forward.r_size = f_code - 1,
            }
        }
        if self.picture_type == PictureType::Bidirectional {
            self.motion_backward.full_pixel = self.bits.read_bit();
            match self.bits.read(3) {
                0 => return,
                f_code => self.motion_backward.r_size = f_code - 1,
            }
        }
        // A picture others are predicted from follows, in display order,
        // the last such picture: that one becomes the forward reference.
        let reference = self.picture_type != PictureType::Bidirectional;
        if reference {
            std::mem::swap(&mut self.forward, &mut self.backward);
        }

        self.start_code = self.bits.next_start_code();
        while matches!(self.start_code, Some(START_EXTENSION | START_USER_DATA)) {
            self.start_code = self.bits.next_start_code();
        }
        let last = (self.mb_width * self.mb_height) as isize - 1;
        while let Some(slice @ START_SLICE_FIRST..=START_SLICE_LAST) = self.start_code {
            self.decode_slice(slice as usize);
            if self.macroblock_address >= last {
                break;
            }
            self.start_code = self.bits.next_start_code();
        }
        // The decoded picture becomes the backward reference; the buffer
        // that held the picture before the forward one is free again.
        if reference {
            std::mem::swap(&mut self.backward, &mut self.current);
        }
    }

    fn decode_slice(&mut self, slice: usize) {
        self.slice_begin = true;
        self.macroblock_address = ((slice - 1) * self.mb_width) as isize - 1;
        (self.motion_forward.h, self.motion_forward.v) = (0, 0);
        (self.motion_backward.h, self.motion_backward.v) = (0, 0);
        self.dc_predictor = [128; 3];
        self.quantizer_scale = self.bits.read(5) as i32;
        // Extra information: bytes each announced by a set bit.
        while self.bits.read_bit() {
            self.bits.skip(8);
        }
        let last = (self.mb_width * self.mb_height) as isize - 1;
        loop {
            self.decode_macroblock();
            if self.macroblock_address >= last || !self.bits.peek_non_zero(23) {
                break;
            }
        }
    }

    fn decode_macroblock(&mut self) {
        // How many macroblocks on from the last one this one is.
        let table = &tables::MACROBLOCK_ADDRESS_INCREMENT;
        let mut code = self.bits.read_vlc(table);
        while code == 34 {
            code = self.bits.read_vlc(table); // stuffing
        }
        let mut increment = 0;
        while code == 35 {
            increment += 33; // escape
            code = self.bits.read_vlc(table);
        }
        increment += code as isize;

        let count = (self.mb_width * self.mb_height) as isize;
        if std::mem::take(&mut self.slice_begin) {
            // The first increment of a slice is its position in the row.
            self.macroblock_address += increment;
        } else {
            if self.macroblock_address + increment >= count {
                return; // a damaged stream
            }
            if increment > 1 {
                self.dc_predictor = [128; 3];
                if self.picture_type == PictureType::Predictive {
                    (self.motion_forward.h, self.motion_forward.v) = (0, 0);
                }
            }
            // Skipped macroblocks are predicted with the vectors as they are.
            while increment > 1 {
                self.macroblock_address += 1;
                self.set_position();
                self.predict_macroblock();
                increment -= 1;
            }
            self.macroblock_address += 1;
        }
        if !(0..count).contains(&self.macroblock_address) {
            return; // a damaged stream
        }
        self.set_position();

        let table: &[(i16, i16)] = match self.picture_type {
            PictureType::Intra => &tables::MACROBLOCK_TYPE_INTRA,
            PictureType::Predictive => &tables::MACROBLOCK_TYPE_PREDICTIVE,
            PictureType::Bidirectional => &tables::MACROBLOCK_TYPE_B,
        };
        // Bits of the type: intra, has a block pattern, backward vector,
        // forward vector, new quantiser scale.
        let macroblock_type = self.bits.read_vlc(table);
        self.macroblock_intra = macroblock_type & 0x01 != 0;
        self.motion_forward.is_set = macroblock_type & 0x08 != 0;
        self.motion_backward.is_set = macroblock_type & 0x04 != 0;
        if macroblock_type & 0x10 != 0 {
            self.quantizer_scale = self.bits.read(5) as i32;
        }
        if self.macroblock_intra {
            (self.motion_forward.h, self.motion_forward.v) = (0, 0);
            (self.motion_backward.h, self.motion_backward.v) = (0, 0);
        } else {
            self.dc_predictor = [128; 3];
            self.decode_motion_vectors();
            self.predict_macroblock();
        }
        let pattern = if macroblock_type & 0x02 != 0 {
            self.bits.read_vlc(&tables::CODE_BLOCK_PATTERN)
        } else if self.macroblock_intra {
            0x3F
        } else {
            0
        };
        for block in 0..6 {
            if pattern & (0x20 >> block) != 0 {
                self.decode_block(block);
            }
        }
    }

    fn set_position(&mut self) {
        self.mb_row = self.macroblock_address as usize / self.mb_width;
        self.mb_col = self.macroblock_address as usize % self.mb_width;
    }

    fn decode_motion_vectors(&mut self) {
        if self.motion_forward.is_set {
            let r_size = self.motion_forward.r_size;
            self.motion_forward.h = self.decode_motion_vector(r_size, self.motion_forward.h);
            self.motion_forward.v = self.decode_motion_vector(r_size, self.motion_forward.v);
        } else if self.picture_type == PictureType::Predictive {
            (self.motion_forward.h, self.motion_forward.v) = (0, 0);
        }
        if self.motion_backward.is_set {
            let r_size = self.motion_backward.r_size;
            self.motion_backward.h = self.decode_motion_vector(r_size, self.motion_backward.h);
            self.motion_backward.v = self.decode_motion_vector(r_size, self.motion_backward.v);
        }
    }

    /// One component of a vector: a difference from the previous one,
    /// wrapped into the range the picture's `f_code` allows.
    fn decode_motion_vector(&mut self, r_size: u32, motion: i32) -> i32 {
        let scale = 1i32 << r_size;
        let code = self.bits.read_vlc(&tables::MOTION) as i32;
        let difference = if code != 0 && scale != 1 {
            let residual = self.bits.read(r_size) as i32;
            let magnitude = ((code.abs() - 1) << r_size) + residual + 1;
            if code < 0 { -magnitude } else { magnitude }
        } else {
            code
        };
        let motion = motion + difference;
        if motion > (scale << 4) - 1 {
            motion - (scale << 5)
        } else if motion < -(scale << 4) {
            motion + (scale << 5)
        } else {
            motion
        }
    }

    fn predict_macroblock(&mut self) {
        let vector = |motion: &Motion| {
            let shift = motion.full_pixel as i32;
            (motion.h << shift, motion.v << shift)
        };
        let (forward, backward) = (vector(&self.motion_forward), vector(&self.motion_backward));
        let position = (self.mb_row, self.mb_col, self.mb_width, self.mb_height);
        if self.picture_type == PictureType::Bidirectional {
            if self.motion_forward.is_set {
                predict(&self.forward, &mut self.current, position, forward, false);
                if self.motion_backward.is_set {
                    predict(&self.backward, &mut self.current, position, backward, true);
                }
            } else {
                predict(&self.backward, &mut self.current, position, backward, false);
            }
        } else {
            predict(&self.forward, &mut self.current, position, forward, false);
        }
    }

    fn decode_block(&mut self, block: usize) {
        let mut n = 0;
        let quant_matrix = if self.macroblock_intra {
            // The first coefficient is a difference from the one of the
            // previous block of the same plane.
            let plane = block.saturating_sub(3);
            let table = if plane == 0 { &tables::DCT_SIZE_LUMINANCE } else { &tables::DCT_SIZE_CHROMINANCE };
            let size = self.bits.read_vlc(table) as u32;
            let mut dc = self.dc_predictor[plane];
            if size > 0 {
                let differential = self.bits.read(size) as i32;
                dc += if differential & (1 << (size - 1)) != 0 {
                    differential
                } else {
                    -(1 << size) | (differential + 1)
                };
            }
            self.dc_predictor[plane] = dc;
            self.block[0] = dc << (3 + 5);
            n = 1;
            &self.intra_quant_matrix
        } else {
            &self.non_intra_quant_matrix
        };

        loop {
            let coefficient = self.bits.read_vlc(&tables::DCT_COEFF);
            // The code "1" ends the block, except as the first coefficient,
            // where "1" followed by a sign is the level 1.
            if coefficient == 0x0001 && n > 0 && !self.bits.read_bit() {
                break;
            }
            let (run, mut level);
            if coefficient == -1 {
                // An escape: the run and the level are written out.
                run = self.bits.read(6) as usize;
                level = self.bits.read(8) as i32;
                if level == 0 {
                    level = self.bits.read(8) as i32;
                } else if level == 128 {
                    level = self.bits.read(8) as i32 - 256;
                } else if level > 128 {
                    level -= 256;
                }
            } else {
                run = (coefficient >> 8) as usize;
                level = (coefficient & 0xFF) as i32;
                if self.bits.read_bit() {
                    level = -level;
                }
            }
            n += run;
            if n >= 64 {
                return; // a damaged stream
            }
            let position = tables::ZIG_ZAG[n] as usize;
            n += 1;

            // Reconstruction, with the "oddification" the standard uses
            // against the build-up of transform errors.
            level *= 2;
            if !self.macroblock_intra {
                level += if level < 0 { -1 } else { 1 };
            }
            level = (level * self.quantizer_scale * quant_matrix[position] as i32) >> 4;
            if level & 1 == 0 {
                level -= if level > 0 { 1 } else { -1 };
            }
            level = level.clamp(-2048, 2047);
            self.block[position] = level * tables::PREMULTIPLIER_MATRIX[position] as i32;
        }

        let (plane, stride, start) = if block < 4 {
            let stride = 16 * self.mb_width;
            let mut start = (self.mb_row * stride + self.mb_col) * 16;
            start += if block & 1 != 0 { 8 } else { 0 };
            start += if block & 2 != 0 { 8 * stride } else { 0 };
            (&mut self.current.y, stride, start)
        } else {
            let stride = 8 * self.mb_width;
            let plane = if block == 4 { &mut self.current.cb } else { &mut self.current.cr };
            (plane, stride, (self.mb_row * stride + self.mb_col) * 8)
        };
        // A block with only its first coefficient is one flat value.
        let flat = n == 1;
        if flat {
            let value = (self.block[0] + 128) >> 8;
            self.block = [value; 64];
        } else {
            idct(&mut self.block);
        }
        for (row, values) in self.block.chunks_exact(8).enumerate() {
            let pixels = &mut plane[start + row * stride..][..8];
            for (pixel, &value) in pixels.iter_mut().zip(values) {
                let value = if self.macroblock_intra { value } else { *pixel as i32 + value };
                *pixel = value.clamp(0, 255) as u8;
            }
        }
        self.block = [0; 64];
    }
}

/// Copies one macroblock from a reference picture, displaced by a vector in
/// half pixels, or averages it with what is there.
fn predict(
    source: &Frame,
    target: &mut Frame,
    (row, column, mb_width, mb_height): (usize, usize, usize, usize),
    (h, v): (i32, i32),
    average: bool,
) {
    let position = (row, column, mb_width, mb_height);
    predict_plane(&source.y, &mut target.y, position, 16, (h, v), average);
    predict_plane(&source.cr, &mut target.cr, position, 8, (h / 2, v / 2), average);
    predict_plane(&source.cb, &mut target.cb, position, 8, (h / 2, v / 2), average);
}

fn predict_plane(
    source: &[u8],
    target: &mut [u8],
    (row, column, mb_width, mb_height): (usize, usize, usize, usize),
    size: usize,
    (h, v): (i32, i32),
    average: bool,
) {
    let stride = mb_width * size;
    let (odd_h, odd_v) = (h & 1 != 0, v & 1 != 0);
    let from = ((row * size) as isize + (v >> 1) as isize) * stride as isize
        + (column * size) as isize
        + (h >> 1) as isize;
    let to = (row * stride + column) * size;
    // The last address a block of this size can start at.
    let limit = (stride * (mb_height * size - size + 1) - size) as isize;
    if !(0..=limit).contains(&from) || to as isize > limit {
        return; // a vector pointing outside the picture: a damaged stream
    }
    let mut from = from as usize;
    let mut to = to;
    let at = |index: usize| source.get(index).copied().unwrap_or(0) as u32;
    for _ in 0..size {
        for x in 0..size {
            let s = from + x;
            let value = match (odd_h, odd_v) {
                (false, false) => at(s),
                (false, true) => (at(s) + at(s + stride) + 1) >> 1,
                (true, false) => (at(s) + at(s + 1) + 1) >> 1,
                (true, true) => (at(s) + at(s + 1) + at(s + stride) + at(s + stride + 1) + 2) >> 2,
            };
            let pixel = &mut target[to + x];
            *pixel = if average { ((*pixel as u32 + value + 1) >> 1) as u8 } else { value as u8 };
        }
        from += stride;
        to += stride;
    }
}

/// The inverse discrete cosine transform of a block of premultiplied
/// coefficients, in integers (pl_mpeg's, itself after jsmpeg's).
fn idct(block: &mut [i32; 64]) {
    // One pass over columns, one over rows; the second rounds to pixels.
    for (step, stride, last) in [(1, 8, false), (8, 1, true)] {
        for line in 0..8 {
            let at = |n: usize| line * step + n * stride;
            let b1 = block[at(4)];
            let b3 = block[at(2)] + block[at(6)];
            let b4 = block[at(5)] - block[at(3)];
            let tmp1 = block[at(1)] + block[at(7)];
            let tmp2 = block[at(3)] + block[at(5)];
            let b6 = block[at(1)] - block[at(7)];
            let b7 = tmp1 + tmp2;
            let m0 = block[at(0)];
            let x4 = ((b6 * 473 - b4 * 196 + 128) >> 8) - b7;
            let x0 = x4 - (((tmp1 - tmp2) * 362 + 128) >> 8);
            let x1 = m0 - b1;
            let x2 = (((block[at(2)] - block[at(6)]) * 362 + 128) >> 8) - b3;
            let x3 = m0 + b1;
            let y3 = x1 + x2;
            let y4 = x3 + b3;
            let y5 = x1 - x2;
            let y6 = x3 - b3;
            let y7 = -x0 - ((b4 * 473 + b6 * 196 + 128) >> 8);
            let out = [b7 + y4, x4 + y3, y5 - x0, y6 - y7, y6 + y7, x0 + y5, y3 - x4, y4 - b7];
            for (n, value) in out.into_iter().enumerate() {
                block[at(n)] = if last { (value + 128) >> 8 } else { value };
            }
        }
    }
}
