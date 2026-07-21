//! File outputs: PNG frame dumps and animated GIF export.

use std::fs::File;
use std::path::Path;

use graphix::image::codecs::gif::{GifEncoder, Repeat};
use graphix::image::{Delay, Frame, RgbImage, RgbaImage};

use crate::Error;
use crate::render::Renderer;

/// Nearest-neighbor integer upscale — the only scaling that keeps pixel art
/// crisp.
pub fn upscale(img: &RgbImage, factor: u32) -> RgbImage {
    let factor = factor.max(1);
    let (w, h) = img.dimensions();
    RgbImage::from_fn(w * factor, h * factor, |x, y| {
        *img.get_pixel(x / factor, y / factor)
    })
}

/// Render `n` frames at `fps` and write them as `frame_0000.png`… into
/// `dir` (created if missing), upscaled by `scale`.
pub fn dump_frames(
    renderer: &Renderer,
    n: u32,
    fps: f32,
    dir: &Path,
    scale: u32,
) -> Result<(), Error> {
    std::fs::create_dir_all(dir)?;
    let dt = 1.0 / fps.max(f32::MIN_POSITIVE);
    for i in 0..n {
        let img = upscale(&renderer.frame(i as f32 * dt), scale);
        img.save(dir.join(format!("frame_{i:04}.png")))?;
    }
    Ok(())
}

/// Export one full surface rotation as an infinitely looping GIF at `fps`,
/// upscaled by `scale`. The surface loops seamlessly; the evolving cloud
/// field seams slightly at the loop point.
pub fn export_gif(renderer: &Renderer, fps: f32, path: &Path, scale: u32) -> Result<(), Error> {
    let fps = fps.max(1.0);
    let frames = (fps * renderer.period_secs()).round().max(1.0) as u32;
    let mut encoder = GifEncoder::new(File::create(path)?);
    encoder.set_repeat(Repeat::Infinite)?;
    let delay = Delay::from_numer_denom_ms(1000.0 as u32, fps as u32);
    for i in 0..frames {
        let t = i as f32 / fps;
        let rgb = upscale(&renderer.frame(t), scale);
        let (w, h) = rgb.dimensions();
        let rgba = RgbaImage::from_fn(w, h, |x, y| {
            let p = rgb.get_pixel(x, y).0;
            graphix::image::Rgba([p[0], p[1], p[2], 255])
        });
        encoder.encode_frame(Frame::from_parts(rgba, 0, 0, delay))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upscale_replicates_pixels() {
        let mut img = RgbImage::new(2, 1);
        img.put_pixel(0, 0, graphix::image::Rgb([10, 20, 30]));
        img.put_pixel(1, 0, graphix::image::Rgb([40, 50, 60]));
        let up = upscale(&img, 3);
        assert_eq!(up.dimensions(), (6, 3));
        assert_eq!(up.get_pixel(2, 2).0, [10, 20, 30]);
        assert_eq!(up.get_pixel(3, 0).0, [40, 50, 60]);
    }

    #[test]
    fn upscale_factor_zero_is_clamped() {
        let img = RgbImage::new(2, 2);
        assert_eq!(upscale(&img, 0).dimensions(), (2, 2));
    }
}
