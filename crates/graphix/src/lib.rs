//! # graphix
//!
//! Render PNG images as 24-bit ANSI block art in the terminal.
//!
//! `graphix` takes a PNG input image and produces artwork made of 24-bit ANSI
//! colors and the shading blocks `░▒▓█`, sized to fit the current terminal.
//!
//! ```sh
//! graphix image.png             # fit to the current terminal size
//! graphix image.png -w 80       # constrain to 80 columns
//! graphix image.png -w 80 -H 24 # constrain to 80x24 cells
//! ```
//!
//! Each terminal cell covers a rectangular region of source pixels. The region
//! is split into a dark and a light cluster by mean luminance; the dark cluster
//! becomes the ANSI background color, the light cluster the foreground color,
//! and the shading block is chosen so its foreground coverage (`░` 25%, `▒` 50%,
//! `▓` 75%, `█` 100%) approximates the light cluster's share of the region.
//!
//! ## Library
//!
//! Everything the binary does is exposed as a library; the CLI is argument
//! parsing plus one call:
//!
//! ```rust,no_run
//! # fn main() -> Result<(), graphix::Error> {
//! let (cols, rows) = graphix::terminal_grid();
//! let art = graphix::render_file("image.png", cols, rows)?;
//! print!("{art}");
//! # Ok(())
//! # }
//! ```
//!
//! The lower-level pipeline (`fit_grid` → `render_cells` → `to_ansi`) is also
//! public, and the `image` crate is re-exported for constructing images
//! without adding it as a separate dependency.
//!
//! ## Example
//!
//! ![irciii-logo.png](./irciii-logo.png)

use std::path::{Path, PathBuf};

pub use image;
use image::RgbImage;

/// Errors returned by the library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The input image could not be read or decoded.
    #[error("failed to read {path}: {source}")]
    Image {
        /// Path of the offending image file.
        path: PathBuf,
        /// The underlying decoding error.
        source: image::ImageError,
    },
}

/// A 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Perceptual luminance (Rec. 709 weights), in `0.0..=255.0`.
    pub fn luminance(self) -> f32 {
        0.2126 * f32::from(self.r) + 0.7152 * f32::from(self.g) + 0.0722 * f32::from(self.b)
    }
}

/// One rendered terminal cell: a shading block with foreground and background colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

/// The shading blocks and the fraction of the cell their foreground covers.
const SHADES: [(char, f32); 5] = [(' ', 0.0), ('░', 0.25), ('▒', 0.5), ('▓', 0.75), ('█', 1.0)];

/// Pick the shading block whose foreground coverage is closest to `ratio`.
pub fn shade_for_ratio(ratio: f32) -> char {
    let mut best = SHADES[0];
    for candidate in SHADES {
        if (candidate.1 - ratio).abs() < (best.1 - ratio).abs() {
            best = candidate;
        }
    }
    best.0
}

/// Compute the output grid size (columns, rows) that fits `width`x`height`
/// pixels into at most `max_cols`x`max_rows` terminal cells, preserving aspect
/// ratio. A terminal cell is assumed twice as tall as it is wide.
pub fn fit_grid(width: u32, height: u32, max_cols: u32, max_rows: u32) -> (u32, u32) {
    let scale = f32::max(
        width as f32 / max_cols as f32,
        height as f32 / (2.0 * max_rows as f32),
    )
    .max(f32::MIN_POSITIVE);
    let cols = ((width as f32 / scale).round() as u32).clamp(1, max_cols);
    let rows = ((height as f32 / (2.0 * scale)).round() as u32).clamp(1, max_rows);
    (cols, rows)
}

/// Reduce one region of source pixels to a single terminal cell.
///
/// Pixels are split into a dark and a light cluster around the mean luminance;
/// each cluster's average color becomes the background and foreground, and the
/// light cluster's share of the region picks the shading block.
fn cell_for_region(pixels: &[Rgb]) -> Cell {
    let mean = pixels.iter().map(|p| p.luminance()).sum::<f32>() / pixels.len().max(1) as f32;
    let (mut dark, mut light): (Vec<Rgb>, Vec<Rgb>) =
        pixels.iter().partition(|p| p.luminance() < mean);
    if dark.is_empty() {
        dark = light.clone();
    }
    if light.is_empty() {
        light = dark.clone();
    }
    let avg = |group: &[Rgb]| -> Rgb {
        let n = group.len().max(1) as u32;
        let (r, g, b) = group.iter().fold((0u32, 0u32, 0u32), |(r, g, b), p| {
            (r + u32::from(p.r), g + u32::from(p.g), b + u32::from(p.b))
        });
        Rgb {
            r: (r / n) as u8,
            g: (g / n) as u8,
            b: (b / n) as u8,
        }
    };
    let ratio = light.len() as f32 / pixels.len().max(1) as f32;
    Cell {
        ch: shade_for_ratio(ratio),
        fg: avg(&light),
        bg: avg(&dark),
    }
}

/// Render an image to a grid of cells, `cols` wide and `rows` tall.
pub fn render_cells(img: &RgbImage, cols: u32, rows: u32) -> Vec<Vec<Cell>> {
    let (w, h) = img.dimensions();
    let mut grid = Vec::with_capacity(rows as usize);
    for cy in 0..rows {
        let y0 = (u64::from(cy) * u64::from(h) / u64::from(rows)) as u32;
        let y1 =
            ((u64::from(cy) + 1) * u64::from(h) / u64::from(rows)).max(u64::from(y0) + 1) as u32;
        let mut row = Vec::with_capacity(cols as usize);
        for cx in 0..cols {
            let x0 = (u64::from(cx) * u64::from(w) / u64::from(cols)) as u32;
            let x1 = ((u64::from(cx) + 1) * u64::from(w) / u64::from(cols)).max(u64::from(x0) + 1)
                as u32;
            let mut pixels = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let p = img.get_pixel(x, y);
                    pixels.push(Rgb {
                        r: p.0[0],
                        g: p.0[1],
                        b: p.0[2],
                    });
                }
            }
            row.push(cell_for_region(&pixels));
        }
        grid.push(row);
    }
    grid
}

/// Serialize a cell grid to a string of 24-bit ANSI escape sequences.
///
/// Consecutive cells sharing colors reuse the active SGR state; every line
/// ends with a reset so the artwork doesn't bleed into the surrounding shell.
pub fn to_ansi(grid: &[Vec<Cell>]) -> String {
    let mut out = String::new();
    for row in grid {
        let mut current: Option<(Rgb, Rgb)> = None;
        for cell in row {
            let colors = (cell.fg, cell.bg);
            if current != Some(colors) {
                out.push_str(&format!(
                    "\x1b[38;2;{};{};{};48;2;{};{};{}m",
                    cell.fg.r, cell.fg.g, cell.fg.b, cell.bg.r, cell.bg.g, cell.bg.b
                ));
                current = Some(colors);
            }
            out.push(cell.ch);
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

/// Render an image into at most `max_cols`x`max_rows` terminal cells of
/// 24-bit ANSI art, preserving aspect ratio.
///
/// Composes [`fit_grid`], [`render_cells`] and [`to_ansi`].
pub fn render_image(img: &RgbImage, max_cols: u32, max_rows: u32) -> String {
    let (cols, rows) = fit_grid(img.width(), img.height(), max_cols.max(1), max_rows.max(1));
    to_ansi(&render_cells(img, cols, rows))
}

/// Load an image from `path` and render it like [`render_image`].
pub fn render_file(path: impl AsRef<Path>, max_cols: u32, max_rows: u32) -> Result<String, Error> {
    let path = path.as_ref();
    let img = image::open(path)
        .map_err(|source| Error::Image {
            path: path.to_path_buf(),
            source,
        })?
        .to_rgb8();
    Ok(render_image(&img, max_cols, max_rows))
}

/// The terminal size in cells usable for artwork: the current width, and the
/// height minus one row for the shell prompt. Falls back to 80x24 when there
/// is no terminal to ask (e.g. output is piped).
pub fn terminal_grid() -> (u32, u32) {
    match terminal_size::terminal_size() {
        Some((terminal_size::Width(w), terminal_size::Height(h))) => {
            (u32::from(w.max(1)), u32::from(h.saturating_sub(1).max(1)))
        }
        None => (80, 24),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_endpoints() {
        assert_eq!(shade_for_ratio(0.0), ' ');
        assert_eq!(shade_for_ratio(0.26), '░');
        assert_eq!(shade_for_ratio(0.5), '▒');
        assert_eq!(shade_for_ratio(0.74), '▓');
        assert_eq!(shade_for_ratio(1.0), '█');
    }

    #[test]
    fn fit_grid_wide_image_limited_by_columns() {
        // 800x100 image in an 80x24 terminal: width is the constraint.
        let (cols, rows) = fit_grid(800, 100, 80, 24);
        assert_eq!(cols, 80);
        assert_eq!(rows, 5);
    }

    #[test]
    fn fit_grid_tall_image_limited_by_rows() {
        // 100x960 image in an 80x24 terminal: height is the constraint.
        let (cols, rows) = fit_grid(100, 960, 80, 24);
        assert_eq!(rows, 24);
        assert_eq!(cols, 5);
    }

    #[test]
    fn fit_grid_never_exceeds_bounds() {
        for (w, h) in [(1, 1), (10_000, 3), (3, 10_000), (1920, 1080)] {
            let (cols, rows) = fit_grid(w, h, 80, 24);
            assert!((1..=80).contains(&cols));
            assert!((1..=24).contains(&rows));
        }
    }

    #[test]
    fn uniform_region_renders_solid_block() {
        let img = RgbImage::from_pixel(8, 8, image::Rgb([200, 10, 30]));
        let grid = render_cells(&img, 2, 2);
        for row in &grid {
            for cell in row {
                assert_eq!(cell.ch, '█');
                assert_eq!(
                    cell.fg,
                    Rgb {
                        r: 200,
                        g: 10,
                        b: 30
                    }
                );
            }
        }
    }

    #[test]
    fn half_and_half_region_uses_medium_shade() {
        // Left half black, right half white, in a single cell.
        let mut img = RgbImage::from_pixel(8, 4, image::Rgb([0, 0, 0]));
        for y in 0..4 {
            for x in 4..8 {
                img.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        let grid = render_cells(&img, 1, 1);
        let cell = grid[0][0];
        assert_eq!(cell.ch, '▒');
        assert_eq!(
            cell.fg,
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(cell.bg, Rgb { r: 0, g: 0, b: 0 });
    }

    #[test]
    fn render_image_stays_within_bounds() {
        let img = RgbImage::from_pixel(100, 100, image::Rgb([9, 9, 9]));
        let art = render_image(&img, 40, 12);
        assert_eq!(art.lines().count(), 12);
    }

    #[test]
    fn render_file_reports_missing_path() {
        let err = render_file("does-not-exist.png", 80, 24);
        assert!(matches!(err, Err(Error::Image { .. })));
    }

    #[test]
    fn ansi_output_has_reset_per_line() {
        let img = RgbImage::from_pixel(4, 4, image::Rgb([1, 2, 3]));
        let ansi = to_ansi(&render_cells(&img, 2, 2));
        assert_eq!(ansi.matches("\x1b[0m\n").count(), 2);
        assert!(ansi.contains("38;2;1;2;3"));
    }
}
