//! Saving what the game shows: a video backend that passes everything on to
//! another one and writes every so many presented frames as PNG files.
//!
//! The files are derived from the game: they belong under `out/`, never in
//! the repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use andy_runtime::video::{Draw, PixelFormat, Rect, SurfaceId, Video};

pub struct Capture {
    inner: Box<dyn Video>,
    directory: PathBuf,
    /// Every how many frames one is written.
    step: u64,
    frames: u64,
    /// Size and format of the surfaces that exist.
    surfaces: HashMap<SurfaceId, (u32, u32, PixelFormat)>,
    /// The first failure to write a file, if any.
    pub error: Option<String>,
    pub written: u64,
}

impl Capture {
    pub fn new(inner: Box<dyn Video>, directory: PathBuf, step: u64) -> std::io::Result<Capture> {
        std::fs::create_dir_all(&directory)?;
        let surfaces = HashMap::new();
        Ok(Capture { inner, directory, step: step.max(1), frames: 0, surfaces, error: None, written: 0 })
    }

    /// The backend everything is passed on to.
    pub fn inner(&self) -> &dyn Video {
        self.inner.as_ref()
    }
}

/// Writes rows of red, green, blue and alpha bytes as a PNG file.
pub fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

impl Video for Capture {
    fn create_surface(&mut self, width: u32, height: u32, format: PixelFormat) -> SurfaceId {
        let surface = self.inner.create_surface(width, height, format);
        self.surfaces.insert(surface, (width, height, format));
        surface
    }

    fn destroy_surface(&mut self, surface: SurfaceId) {
        self.surfaces.remove(&surface);
        self.inner.destroy_surface(surface);
    }

    fn upload(&mut self, surface: SurfaceId, area: Rect, pitch: usize, pixels: &[u8]) {
        self.inner.upload(surface, area, pitch, pixels);
    }

    fn download(&mut self, surface: SurfaceId, area: Rect, pitch: usize, pixels: &mut [u8]) {
        self.inner.download(surface, area, pitch, pixels);
    }

    fn fill(&mut self, surface: SurfaceId, area: Rect, value: u32) {
        self.inner.fill(surface, area, value);
    }

    fn copy(
        &mut self,
        source: SurfaceId,
        from: Rect,
        target: SurfaceId,
        to: Rect,
        color_key: Option<(u32, u32)>,
    ) {
        self.inner.copy(source, from, target, to, color_key);
    }

    fn draw(&mut self, draw: &Draw) {
        self.inner.draw(draw);
    }

    fn present(&mut self, surface: SurfaceId) {
        self.inner.present(surface);
        self.frames += 1;
        if !(self.frames - 1).is_multiple_of(self.step) || self.error.is_some() {
            return;
        }
        let (width, height, format) = self.surfaces[&surface];
        let size = format.bytes_per_pixel() as usize;
        let pitch = width as usize * size;
        let mut pixels = vec![0; pitch * height as usize];
        self.inner.download(surface, Rect::whole(width, height), pitch, &mut pixels);
        // The alpha of what is shown means nothing: the picture is opaque.
        let rgba: Vec<u8> = pixels
            .chunks(size)
            .flat_map(|raw| {
                let raw = raw.iter().rev().fold(0, |value, &byte| value << 8 | byte as u32);
                let [blue, green, red, _] = format.to_argb(raw).unwrap_or(0).to_le_bytes();
                [red, green, blue, 0xFF]
            })
            .collect();
        let path = self.directory.join(format!("frame-{:06}.png", self.frames));
        match write_png(&path, width, height, &rgba) {
            Ok(()) => self.written += 1,
            Err(error) => self.error = Some(format!("{}: {error}", path.display())),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
