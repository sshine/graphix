//! File outputs: PNG frame dumps and animated GIF export. Both take whatever
//! frame sequence the caller hands them, so either can write the raw flake or
//! the graphix cell rasterization of it.

use std::fs::File;
use std::path::Path;

use graphix::image::codecs::gif::{GifEncoder, Repeat};
use graphix::image::{Delay, Frame, RgbImage, RgbaImage};

use crate::Error;

/// Write `frames` as `frame_0000.png`… into `dir` (created if missing).
pub fn dump_frames(frames: impl IntoIterator<Item = RgbImage>, dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir)?;
    for (i, img) in frames.into_iter().enumerate() {
        img.save(dir.join(format!("frame_{i:04}.png")))?;
    }
    Ok(())
}

/// Write `frames` as an infinitely looping GIF played at `fps`.
pub fn export_gif(
    frames: impl IntoIterator<Item = RgbImage>,
    fps: f32,
    path: &Path,
) -> Result<(), Error> {
    let mut encoder = GifEncoder::new(File::create(path)?);
    encoder.set_repeat(Repeat::Infinite)?;
    let delay = Delay::from_numer_denom_ms(1000, fps.max(1.0) as u32);
    for rgb in frames {
        let (w, h) = rgb.dimensions();
        let rgba = RgbaImage::from_fn(w, h, |x, y| {
            let p = rgb.get_pixel(x, y).0;
            graphix::image::Rgba([p[0], p[1], p[2], 255])
        });
        encoder.encode_frame(Frame::from_parts(rgba, 0, 0, delay))?;
    }
    Ok(())
}
