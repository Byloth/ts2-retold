//! The game's texture files (`*.ngn`), as far as their pictures go.
//!
//! A texture file begins with a block of pictures: the number `0x104`,
//! the size of the block, the number of pictures, and then each picture as
//! the length of its data, the length of its name, its name (`tex00`,
//! `tex31`, ...) and its data, which is a Windows bitmap file: 24 bits a
//! pixel for textures, 8 bits with a palette for some backgrounds. The
//! picture named `texNN` is the texture the game's sprite
//! sheets call number `NN`. What follows the block (models and the rest)
//! is not looked at here and is kept as it is.
//!
//! This is what the typefaces are rebuilt through when the game is built
//! (ADR 0016): the typefaces are pictures in these files.

/// The number a block of pictures begins with.
const PICTURES: u32 = 0x104;
/// The bytes of a bitmap file before its pixels, for 24 bits a pixel.
const BITMAP_HEADER: usize = 54;

/// A picture of a texture file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Blue, green, red for each pixel, rows from the top, no padding.
    pub pixels: Vec<u8>,
}

/// Why a texture file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// It does not begin with a block of pictures.
    NotATextureFile,
    /// It ends in the middle of the block.
    Truncated,
    /// A picture is not a bitmap of 24 bits a pixel, or of 8 with a
    /// palette, stored bottom row first without compression (the kinds
    /// the game's files have); or it cannot be replaced as asked.
    Bitmap(String),
}

fn word(bytes: &[u8], at: usize) -> Result<u32, Error> {
    let four = bytes.get(at..at + 4).ok_or(Error::Truncated)?;
    Ok(u32::from_le_bytes(four.try_into().expect("four bytes")))
}

/// Where each picture's bitmap file is in a texture file: its name, where
/// the bitmap begins and its length.
fn entries(file: &[u8]) -> Result<Vec<(String, usize, usize)>, Error> {
    if word(file, 0).map_err(|_| Error::NotATextureFile)? != PICTURES {
        return Err(Error::NotATextureFile);
    }
    let count = word(file, 8)?;
    let mut at = 12;
    let mut found = Vec::new();
    for _ in 0..count {
        let (length, name_length) = (word(file, at)? as usize, word(file, at + 4)? as usize);
        let name = file.get(at + 8..at + 8 + name_length).ok_or(Error::Truncated)?;
        let start = at + 8 + name_length;
        file.get(start..start + length).ok_or(Error::Truncated)?;
        found.push((name.iter().map(|&byte| byte as char).collect(), start, length));
        at = start + length;
    }
    Ok(found)
}

/// What is needed to find a bitmap's pixels: its width and height, the
/// bytes of a pixel (3, or 1 for an index into the palette), where the
/// pixels begin and the length of a row in bytes.
struct Shape {
    width: u32,
    height: u32,
    pixel: usize,
    pixels_at: usize,
    row: usize,
}

fn bitmap_shape(name: &str, bitmap: &[u8]) -> Result<Shape, Error> {
    let wrong = |what: &str| Error::Bitmap(format!("{name}: {what}"));
    if bitmap.get(..2) != Some(b"BM") {
        return Err(wrong("not a bitmap file"));
    }
    let pixels_at = word(bitmap, 10)? as usize;
    let (width, height) = (word(bitmap, 18)?, word(bitmap, 22)?);
    let bits = word(bitmap, 28)? & 0xFFFF;
    let compression = word(bitmap, 30)?;
    let pixel = match (bits, pixels_at) {
        (24, BITMAP_HEADER) => 3,
        (8, at) if at == BITMAP_HEADER + 4 * 256 => 1,
        _ => return Err(wrong("neither 24 bits a pixel nor 8 with a palette of 256")),
    };
    if compression != 0 || height > i32::MAX as u32 {
        return Err(wrong("compressed, or top row first"));
    }
    // Rows are padded to four bytes.
    let row = (pixel * width as usize).next_multiple_of(4);
    if bitmap.len() < pixels_at + row * height as usize {
        return Err(wrong("shorter than its size says"));
    }
    Ok(Shape { width, height, pixel, pixels_at, row })
}

/// The pictures of a texture file.
pub fn pictures(file: &[u8]) -> Result<Vec<Picture>, Error> {
    entries(file)?
        .into_iter()
        .map(|(name, start, length)| {
            let bitmap = &file[start..start + length];
            let shape = bitmap_shape(&name, bitmap)?;
            let mut pixels = Vec::with_capacity(3 * (shape.width * shape.height) as usize);
            for y in (0..shape.height as usize).rev() {
                let row = &bitmap[shape.pixels_at + y * shape.row..][..shape.pixel * shape.width as usize];
                if shape.pixel == 3 {
                    pixels.extend_from_slice(row);
                } else {
                    // The palette: blue, green, red and an unused byte.
                    for &index in row {
                        pixels.extend_from_slice(&bitmap[BITMAP_HEADER + 4 * index as usize..][..3]);
                    }
                }
            }
            Ok(Picture { name, width: shape.width, height: shape.height, pixels })
        })
        .collect()
}

/// The texture file with the pixels of one of its pictures replaced by
/// those of a picture of the same size. Nothing else in the file changes,
/// its length included. Only a picture of 24 bits a pixel can be replaced.
pub fn with_picture(file: &[u8], picture: &Picture) -> Result<Vec<u8>, Error> {
    let (name, start, length) = entries(file)?
        .into_iter()
        .find(|(name, _, _)| *name == picture.name)
        .ok_or_else(|| Error::Bitmap(format!("{}: no such picture", picture.name)))?;
    let shape = bitmap_shape(&name, &file[start..start + length])?;
    let width = shape.width as usize;
    if shape.pixel != 3 {
        return Err(Error::Bitmap(format!("{name}: has a palette, cannot be replaced")));
    }
    if (shape.width, shape.height) != (picture.width, picture.height)
        || picture.pixels.len() != 3 * width * shape.height as usize
    {
        return Err(Error::Bitmap(format!("{name}: the new picture is of another size")));
    }
    let mut file = file.to_vec();
    for y in 0..shape.height as usize {
        let from = &picture.pixels[3 * width * y..][..3 * width];
        let at = start + shape.pixels_at + (shape.height as usize - 1 - y) * shape.row;
        file[at..at + from.len()].copy_from_slice(from);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bitmap file of 24 bits a pixel from rows given top first.
    fn bitmap(width: u32, rows: &[&[u8]]) -> Vec<u8> {
        let row = (3 * width as usize).next_multiple_of(4);
        let mut file = vec![0u8; BITMAP_HEADER];
        file[..2].copy_from_slice(b"BM");
        file[10..14].copy_from_slice(&(BITMAP_HEADER as u32).to_le_bytes());
        file[14..18].copy_from_slice(&40u32.to_le_bytes());
        file[18..22].copy_from_slice(&width.to_le_bytes());
        file[22..26].copy_from_slice(&(rows.len() as u32).to_le_bytes());
        file[26..28].copy_from_slice(&1u16.to_le_bytes());
        file[28..30].copy_from_slice(&24u16.to_le_bytes());
        for pixels in rows.iter().rev() {
            file.extend_from_slice(pixels);
            file.resize(file.len() + row - pixels.len(), 0xEE);
        }
        file
    }

    /// A texture file with these pictures and something after them.
    fn texture_file(pictures: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut block = Vec::new();
        for (name, bitmap) in pictures {
            block.extend((bitmap.len() as u32).to_le_bytes());
            block.extend((name.len() as u32).to_le_bytes());
            block.extend(name.bytes());
            block.extend(bitmap);
        }
        let mut file = PICTURES.to_le_bytes().to_vec();
        file.extend((block.len() as u32 + 4).to_le_bytes());
        file.extend((pictures.len() as u32).to_le_bytes());
        file.extend(block);
        file.extend([0x06, 0x01, 0, 0, 9, 9, 9]);
        file
    }

    #[test]
    fn pictures_are_read_top_row_first_and_one_can_be_replaced() {
        let first = bitmap(1, &[&[1, 2, 3], &[4, 5, 6]]);
        let second = bitmap(2, &[&[10, 11, 12, 13, 14, 15]]);
        let file = texture_file(&[("tex00", first), ("tex31", second)]);
        let read = pictures(&file).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(
            read[0],
            Picture { name: "tex00".into(), width: 1, height: 2, pixels: vec![1, 2, 3, 4, 5, 6] }
        );
        assert_eq!((read[1].name.as_str(), read[1].width, read[1].height), ("tex31", 2, 1));
        assert_eq!(read[1].pixels, [10, 11, 12, 13, 14, 15]);

        let new = Picture { name: "tex00".into(), width: 1, height: 2, pixels: vec![7, 7, 7, 8, 8, 8] };
        let changed = with_picture(&file, &new).unwrap();
        assert_eq!(changed.len(), file.len());
        let read = pictures(&changed).unwrap();
        assert_eq!(read[0], new);
        assert_eq!(read[1].pixels, [10, 11, 12, 13, 14, 15]);
        // The rows' padding and what follows the pictures are as they were.
        assert_eq!(changed.iter().filter(|&&byte| byte == 0xEE).count(), 4);
        assert_eq!(changed[changed.len() - 7..], file[file.len() - 7..]);
    }

    #[test]
    fn a_picture_with_a_palette_is_read_through_it() {
        // Two pixels: palette entries 1 and 0.
        let mut paletted = vec![0u8; BITMAP_HEADER + 4 * 256];
        paletted[..2].copy_from_slice(b"BM");
        paletted[10..14].copy_from_slice(&((BITMAP_HEADER + 4 * 256) as u32).to_le_bytes());
        paletted[18..22].copy_from_slice(&2u32.to_le_bytes());
        paletted[22..26].copy_from_slice(&1u32.to_le_bytes());
        paletted[28..30].copy_from_slice(&8u16.to_le_bytes());
        paletted[BITMAP_HEADER..BITMAP_HEADER + 8].copy_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
        paletted.extend([1, 0, 0, 0]);
        let file = texture_file(&[("bgr36", paletted)]);
        let read = pictures(&file).unwrap();
        assert_eq!(read[0].pixels, [4, 5, 6, 1, 2, 3]);
        assert!(matches!(with_picture(&file, &read[0]), Err(Error::Bitmap(_))));
    }

    #[test]
    fn what_is_not_a_texture_file_is_said() {
        assert_eq!(pictures(b"BM"), Err(Error::NotATextureFile));
        assert_eq!(pictures(&[5, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Err(Error::NotATextureFile));
        let file = texture_file(&[("tex00", bitmap(1, &[&[1, 2, 3]]))]);
        assert_eq!(pictures(&file[..40]), Err(Error::Truncated));
        let other_size = Picture { name: "tex00".into(), width: 2, height: 1, pixels: vec![0; 6] };
        assert!(matches!(with_picture(&file, &other_size), Err(Error::Bitmap(_))));
        let missing = Picture { name: "tex99".into(), width: 1, height: 1, pixels: vec![0; 3] };
        assert!(matches!(with_picture(&file, &missing), Err(Error::Bitmap(_))));
        let mut sixteen_bits = file.clone();
        sixteen_bits[12 + 8 + 5 + 28] = 16;
        assert!(matches!(pictures(&sixteen_bits), Err(Error::Bitmap(_))));
    }
}
