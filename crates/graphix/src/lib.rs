//! # graphix
//!
//! Render PNG images as 24-bit ANSI block art in the terminal.
//!
//! `graphix` takes a PNG input image and produces artwork made of 24-bit ANSI
//! colors and Unicode block or dot characters, sized to fit the current
//! terminal.
//!
//! ```sh
//! graphix image.png               # fit to the current terminal size
//! graphix image.png -w 80         # constrain to 80 columns
//! graphix image.png -w 80 -H 24   # constrain to 80x24 cells
//! graphix image.png -m half-block # square pixels instead of shade blocks
//! graphix image.png -m octant     # finest 2x4 solid-fill matrix per cell
//! ```
//!
//! ## Rendering modes
//!
//! Five granularity layers are supported, in increasing resolution, drawn from
//! four Unicode blocks. Every mode splits a cell's source pixels into a dark
//! and a light cluster by mean luminance; the two cluster averages become the
//! cell's background and foreground colors. What differs is how much detail
//! the glyph itself carries within the cell:
//!
//! - **`shade`** (default) — the shade characters `░▒▓█` from *Block Elements*
//!   (U+2580..U+259F). The shade is chosen so its foreground coverage (`░`
//!   25%, `▒` 50%, `▓` 75%, `█` 100%) approximates the light cluster's share
//!   of the region. One coverage value per cell; universal font support.
//! - **`half-block`** — the upper half block `▀` from *Block Elements*. Since
//!   a terminal cell is about twice as tall as it is wide, each cell shows two
//!   square pixels: the top half is the foreground color, the bottom half the
//!   background, each averaged exactly from its own pixels.
//! - **`sextant`** — *Block Sextants* (U+1FB00..=U+1FB3B), a 2x3 solid-fill
//!   matrix per cell. Broadly supported; subcells are slightly wider than tall.
//! - **`braille`** — *Braille Patterns* (U+2800..=U+28FF), a 2x4 *dot* matrix
//!   per cell. The 1:2 cell aspect makes each dot cover a square region; dots
//!   are raised where a subregion is nearer the light cluster. Universal font
//!   support, with visible gaps between the dots.
//! - **`octant`** — *Block Octants* (U+1CD00..=U+1CDE5, added in Unicode
//!   16.0), a 2x4 *solid-fill* matrix per cell — the finest layer, matching
//!   braille's resolution without the dot gaps. Font support is still sparse;
//!   unsupported glyphs render as tofu.
//!
//! Both `sextant` and `octant` reuse the *Block Elements* half and quadrant
//! glyphs for the patterns those blocks already encode, since Unicode omits
//! them from the sextant and octant ranges.
//!
//! ## Library
//!
//! Everything the binary does is exposed as a library; the CLI is argument
//! parsing plus one call:
//!
//! ```rust,no_run
//! # fn main() -> Result<(), graphix::Error> {
//! let (cols, rows) = graphix::terminal_grid();
//! let art = graphix::render_file("image.png", cols, rows, graphix::Mode::Shade)?;
//! print!("{art}");
//! # Ok(())
//! # }
//! ```
//!
//! The lower-level pipeline (`fit_grid` → `render_cells` → `to_ansi`) is also
//! public, and the `image` crate is re-exported for constructing images
//! without adding it as a separate dependency.
//!
//! ## Examples
//!
//! ![irciii-logo.png](./irciii-logo.png)
//! ![keen.png](./keen.png)

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

/// The rendering granularity: which Unicode characters approximate each
/// cell's region of source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Mode {
    /// Shade blocks `░▒▓█` from Block Elements (U+2580..U+259F): one
    /// foreground/background pair per cell, the shade picked by coverage.
    #[default]
    Shade,
    /// The upper half block `▀` from Block Elements: two square pixels per
    /// cell (top = foreground, bottom = background), each colored exactly.
    HalfBlock,
    /// Block Sextants (U+1FB00..=U+1FB3B): a 2x3 solid-fill matrix per cell.
    /// Widely supported, but subcells are slightly wider than tall.
    Sextant,
    /// Braille Patterns (U+2800..=U+28FF): a 2x4 dot matrix per cell, dots
    /// raised where the region is lighter than the cell's mean luminance.
    Braille,
    /// Block Octants (U+1CD00..=U+1CDE5, Unicode 16.0): a 2x4 solid-fill
    /// matrix per cell, the highest resolution offered. Needs a very recent
    /// terminal font; unsupported glyphs render as tofu.
    Octant,
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

/// A rectangular region of source pixels, in image coordinates.
#[derive(Debug, Clone, Copy)]
struct Region {
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
}

impl Region {
    /// The `i`-th of `n` equal spans of `start..end`, at least one unit wide;
    /// when the range is too small to subdivide, spans overlap.
    fn span(start: u32, end: u32, i: u32, n: u32) -> (u32, u32) {
        let len = u64::from(end - start);
        let a = start + (u64::from(i) * len / u64::from(n)) as u32;
        let b = start + ((u64::from(i) + 1) * len / u64::from(n)) as u32;
        if b > a { (a, b) } else { (a, a + 1) }
    }

    /// The subregion at `(sx, sy)` of an `nx`x`ny` subdivision of this region.
    fn sub(self, sx: u32, nx: u32, sy: u32, ny: u32) -> Region {
        let (x0, x1) = Self::span(self.x0, self.x1, sx, nx);
        let (y0, y1) = Self::span(self.y0, self.y1, sy, ny);
        Region { x0, x1, y0, y1 }
    }

    /// Collect the region's pixels, clipped to the image bounds.
    fn pixels(self, img: &RgbImage) -> Vec<Rgb> {
        let (w, h) = img.dimensions();
        let mut pixels =
            Vec::with_capacity((self.x1 - self.x0) as usize * (self.y1 - self.y0) as usize);
        for y in self.y0..self.y1.min(h) {
            for x in self.x0..self.x1.min(w) {
                let p = img.get_pixel(x, y);
                pixels.push(Rgb {
                    r: p.0[0],
                    g: p.0[1],
                    b: p.0[2],
                });
            }
        }
        pixels
    }
}

/// Average color of a group of pixels (black when the group is empty).
fn average(group: &[Rgb]) -> Rgb {
    let n = group.len().max(1) as u32;
    let (r, g, b) = group.iter().fold((0u32, 0u32, 0u32), |(r, g, b), p| {
        (r + u32::from(p.r), g + u32::from(p.g), b + u32::from(p.b))
    });
    Rgb {
        r: (r / n) as u8,
        g: (g / n) as u8,
        b: (b / n) as u8,
    }
}

/// Mean luminance of a group of pixels.
fn mean_luminance(pixels: &[Rgb]) -> f32 {
    pixels.iter().map(|p| p.luminance()).sum::<f32>() / pixels.len().max(1) as f32
}

/// Split pixels into a `(dark, light)` cluster pair around the mean luminance.
/// A one-sided split duplicates the populated side so neither cluster is empty.
fn split_clusters(pixels: &[Rgb]) -> (Vec<Rgb>, Vec<Rgb>) {
    let mean = mean_luminance(pixels);
    let (mut dark, mut light): (Vec<Rgb>, Vec<Rgb>) =
        pixels.iter().partition(|p| p.luminance() < mean);
    if dark.is_empty() {
        dark = light.clone();
    }
    if light.is_empty() {
        light = dark.clone();
    }
    (dark, light)
}

/// Reduce one region of source pixels to a shade-block cell.
///
/// Pixels are split into a dark and a light cluster around the mean luminance;
/// each cluster's average color becomes the background and foreground, and the
/// light cluster's share of the region picks the shading block.
fn shade_cell(pixels: &[Rgb]) -> Cell {
    let (dark, light) = split_clusters(pixels);
    let ratio = light.len() as f32 / pixels.len().max(1) as f32;
    Cell {
        ch: shade_for_ratio(ratio),
        fg: average(&light),
        bg: average(&dark),
    }
}

/// Reduce one region to a half-block cell: the top half of the region averaged
/// into the foreground, the bottom half into the background of a `▀`.
fn half_block_cell(img: &RgbImage, region: Region) -> Cell {
    Cell {
        ch: '▀',
        fg: average(&region.sub(0, 1, 0, 2).pixels(img)),
        bg: average(&region.sub(0, 1, 1, 2).pixels(img)),
    }
}

/// Subdivide `region` into an `nx`x`ny` grid and decide, for each subcell in
/// row-major order, whether it is lit: its mean luminance is at least as close
/// to the light cluster's as to the dark cluster's. Returns the lit mask along
/// with the light and dark cluster averages (the cell's foreground/background).
fn lit_subcells(img: &RgbImage, region: Region, nx: u32, ny: u32) -> (Vec<bool>, Rgb, Rgb) {
    let pixels = region.pixels(img);
    let (dark, light) = split_clusters(&pixels);
    let (dark_lum, light_lum) = (mean_luminance(&dark), mean_luminance(&light));
    let mut lit = Vec::with_capacity((nx * ny) as usize);
    for row in 0..ny {
        for col in 0..nx {
            let lum = mean_luminance(&region.sub(col, nx, row, ny).pixels(img));
            lit.push((lum - light_lum).abs() <= (lum - dark_lum).abs());
        }
    }
    (lit, average(&light), average(&dark))
}

/// Dot bits of the 2x4 Braille Patterns matrix, in row-major subcell order.
/// Unicode numbers the dots down the left column then down the right, so the
/// raster order used by [`lit_subcells`] is not the bit order.
const BRAILLE_DOTS: [u32; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];

/// Reduce one region to a Braille Patterns cell (2x4 dots).
fn braille_cell(img: &RgbImage, region: Region) -> Cell {
    let (lit, fg, bg) = lit_subcells(img, region, 2, 4);
    let bits = lit
        .iter()
        .zip(BRAILLE_DOTS)
        .filter(|(on, _)| **on)
        .fold(0u32, |acc, (_, dot)| acc | dot);
    Cell {
        // Always valid: 0x2800..=0x28FF is the Braille Patterns block.
        ch: char::from_u32(0x2800 + bits).unwrap_or('⣿'),
        fg,
        bg,
    }
}

/// The sixteen 2x2 quadrant patterns, as Block Elements, indexed by the
/// bits `upper-left | upper-right<<1 | lower-left<<2 | lower-right<<3`.
const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// If the 2x4 pattern `bits` collapses to a 2x2 quadrant (each column uniform
/// within the top half and within the bottom half), return its Block Element
/// glyph. Sextants and octants reuse these rather than encoding them twice.
fn quadrant_char(bits: u32) -> Option<char> {
    let bit = |n: u32| (bits >> n) & 1u32;
    let uniform = bit(0) == bit(2) && bit(1) == bit(3) && bit(4) == bit(6) && bit(5) == bit(7);
    uniform.then(|| QUADRANTS[(bit(0) | bit(1) << 1 | bit(4) << 2 | bit(5) << 3) as usize])
}

/// Pack a row-major lit mask into a bit pattern (subcell `i` at bit `i`).
fn pack(lit: &[bool]) -> u32 {
    lit.iter()
        .enumerate()
        .fold(0u32, |acc, (i, on)| acc | u32::from(*on) << i)
}

/// The glyph for a 2x3 sextant pattern (bits 0..=5, row-major).
///
/// The 60 sextant glyphs (U+1FB00..=U+1FB3B) cover every pattern except the
/// four already in Block Elements: blank, `▌`, `▐`, and `█`.
fn sextant_char(bits: u32) -> char {
    match bits {
        0 => ' ',
        0b010101 => '▌',
        0b101010 => '▐',
        0b111111 => '█',
        // Skip the two half-block patterns (21, 42) that precede `bits`.
        _ => char::from_u32(0x1FB00 + bits - 1 - (bits > 21) as u32 - (bits > 42) as u32)
            .unwrap_or('█'),
    }
}

/// The ten non-quadrant 2x4 patterns that Unicode encodes outside the octant
/// block, as `(pattern, glyph)`. Each subcell is half-wide and quarter-tall, so
/// these are the quarter/three-quarter rows and the half-of-a-quarter single
/// cells — shapes that already existed as Block Elements or elsewhere in the
/// Symbols for Legacy Computing blocks. (The sixteen 2x2 quadrant patterns are
/// handled separately by [`quadrant_char`].)
const OCTANT_EXTRAS: [(u32, char); 10] = [
    (0b0000_0001, '\u{1CEA8}'), // top-left cell     — LEFT HALF UPPER ONE QUARTER
    (0b0000_0010, '\u{1CEAB}'), // top-right cell    — RIGHT HALF UPPER ONE QUARTER
    (0b0000_0011, '\u{1FB82}'), // top row           — UPPER ONE QUARTER
    (0b0001_0100, '\u{1FBE6}'), // left middle pair  — MIDDLE LEFT ONE QUARTER
    (0b0010_1000, '\u{1FBE7}'), // right middle pair — MIDDLE RIGHT ONE QUARTER
    (0b0011_1111, '\u{1FB85}'), // top three rows    — UPPER THREE QUARTERS
    (0b0100_0000, '\u{1CEA3}'), // bottom-left cell  — LEFT HALF LOWER ONE QUARTER
    (0b1000_0000, '\u{1CEA0}'), // bottom-right cell — RIGHT HALF LOWER ONE QUARTER
    (0b1100_0000, '\u{2582}'),  // bottom row        — LOWER ONE QUARTER
    (0b1111_1100, '\u{2586}'),  // bottom three rows — LOWER THREE QUARTERS
];

/// If `bits` is one of the 26 patterns Unicode encodes outside the octant
/// block, return the pre-existing glyph: the sixteen 2x2 quadrants plus the ten
/// quarter blocks in [`OCTANT_EXTRAS`].
fn octant_reused(bits: u32) -> Option<char> {
    quadrant_char(bits).or_else(|| {
        OCTANT_EXTRAS
            .iter()
            .find_map(|(p, c)| (*p == bits).then_some(*c))
    })
}

/// The glyph for a 2x4 octant pattern (bits 0..=7, row-major).
///
/// The 230 octant glyphs (U+1CD00..=U+1CDE5, added in Unicode 16.0) run in
/// ascending pattern value, skipping the 26 patterns already encoded elsewhere
/// (see [`octant_reused`]). The block therefore does not start at pattern 1:
/// its first glyph, `BLOCK OCTANT-3` at U+1CD00, is pattern `0b100`.
fn octant_char(bits: u32) -> char {
    octant_reused(bits).unwrap_or_else(|| {
        // Rank among the patterns below `bits` that also earn an octant glyph.
        let rank = (0..bits).filter(|&p| octant_reused(p).is_none()).count() as u32;
        char::from_u32(0x1CD00 + rank).unwrap_or('█')
    })
}

/// Reduce one region to a Block Sextant cell (2x3 subcells).
fn sextant_cell(img: &RgbImage, region: Region) -> Cell {
    let (lit, fg, bg) = lit_subcells(img, region, 2, 3);
    Cell {
        ch: sextant_char(pack(&lit)),
        fg,
        bg,
    }
}

/// Reduce one region to a Block Octant cell (2x4 subcells).
fn octant_cell(img: &RgbImage, region: Region) -> Cell {
    let (lit, fg, bg) = lit_subcells(img, region, 2, 4);
    Cell {
        ch: octant_char(pack(&lit)),
        fg,
        bg,
    }
}

/// Render an image to a grid of cells, `cols` wide and `rows` tall.
pub fn render_cells(img: &RgbImage, cols: u32, rows: u32, mode: Mode) -> Vec<Vec<Cell>> {
    let (w, h) = img.dimensions();
    let full = Region {
        x0: 0,
        x1: w,
        y0: 0,
        y1: h,
    };
    let mut grid = Vec::with_capacity(rows as usize);
    for cy in 0..rows {
        let mut row = Vec::with_capacity(cols as usize);
        for cx in 0..cols {
            let region = full.sub(cx, cols, cy, rows);
            row.push(match mode {
                Mode::Shade => shade_cell(&region.pixels(img)),
                Mode::HalfBlock => half_block_cell(img, region),
                Mode::Sextant => sextant_cell(img, region),
                Mode::Braille => braille_cell(img, region),
                Mode::Octant => octant_cell(img, region),
            });
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
pub fn render_image(img: &RgbImage, max_cols: u32, max_rows: u32, mode: Mode) -> String {
    let (cols, rows) = fit_grid(img.width(), img.height(), max_cols.max(1), max_rows.max(1));
    to_ansi(&render_cells(img, cols, rows, mode))
}

/// Load an image from `path` and render it like [`render_image`].
pub fn render_file(
    path: impl AsRef<Path>,
    max_cols: u32,
    max_rows: u32,
    mode: Mode,
) -> Result<String, Error> {
    let path = path.as_ref();
    let img = image::open(path)
        .map_err(|source| Error::Image {
            path: path.to_path_buf(),
            source,
        })?
        .to_rgb8();
    Ok(render_image(&img, max_cols, max_rows, mode))
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
        let grid = render_cells(&img, 2, 2, Mode::Shade);
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
        let grid = render_cells(&img, 1, 1, Mode::Shade);
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
        let art = render_image(&img, 40, 12, Mode::Shade);
        assert_eq!(art.lines().count(), 12);
    }

    #[test]
    fn render_file_reports_missing_path() {
        let err = render_file("does-not-exist.png", 80, 24, Mode::Shade);
        assert!(matches!(err, Err(Error::Image { .. })));
    }

    #[test]
    fn ansi_output_has_reset_per_line() {
        let img = RgbImage::from_pixel(4, 4, image::Rgb([1, 2, 3]));
        let ansi = to_ansi(&render_cells(&img, 2, 2, Mode::Shade));
        assert_eq!(ansi.matches("\x1b[0m\n").count(), 2);
        assert!(ansi.contains("38;2;1;2;3"));
    }

    #[test]
    fn half_block_splits_cell_into_two_exact_pixels() {
        // Top half red, bottom half blue, in a single cell.
        let mut img = RgbImage::from_pixel(2, 4, image::Rgb([255, 0, 0]));
        for y in 2..4 {
            for x in 0..2 {
                img.put_pixel(x, y, image::Rgb([0, 0, 255]));
            }
        }
        let grid = render_cells(&img, 1, 1, Mode::HalfBlock);
        let cell = grid[0][0];
        assert_eq!(cell.ch, '▀');
        assert_eq!(cell.fg, Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(cell.bg, Rgb { r: 0, g: 0, b: 255 });
    }

    #[test]
    fn sextant_maps_onto_the_whole_block() {
        // Every 6-bit pattern maps to a distinct glyph; the 60 non-reused ones
        // land exactly on U+1FB00..=U+1FB3B, in ascending order.
        let mut seen = std::collections::HashSet::new();
        for bits in 0..64u32 {
            assert!(
                seen.insert(sextant_char(bits)),
                "duplicate glyph at {bits:#08b}"
            );
        }
        assert_eq!(seen.len(), 64);
        for reused in [' ', '▌', '▐', '█'] {
            assert!(seen.contains(&reused));
        }
        assert_eq!(sextant_char(0b000001), '\u{1FB00}');
        assert_eq!(sextant_char(0b111110), '\u{1FB3B}');
    }

    #[test]
    fn octant_maps_onto_the_whole_block() {
        // Every 8-bit pattern maps to a distinct glyph; the 230 non-reused ones
        // land exactly on U+1CD00..=U+1CDE5, in ascending pattern order. The 26
        // reused patterns (16 quadrants + 10 quarter blocks) fall outside it.
        let mut seen = std::collections::HashSet::new();
        for bits in 0..256u32 {
            assert!(
                seen.insert(octant_char(bits)),
                "duplicate glyph at {bits:#010b}"
            );
        }
        assert_eq!(seen.len(), 256);
        assert_eq!(
            seen.iter()
                .filter(|c| ('\u{1CD00}'..='\u{1CDE5}').contains(c))
                .count(),
            230
        );
        assert_eq!(octant_char(0b0000_0100), '\u{1CD00}'); // pattern 4 = BLOCK OCTANT-3
        assert_eq!(octant_char(0b1111_1110), '\u{1CDE5}'); // BLOCK OCTANT-2345678
    }

    #[test]
    fn sextant_uniform_region_fills_all_subcells() {
        let img = RgbImage::from_pixel(4, 6, image::Rgb([200, 10, 30]));
        let grid = render_cells(&img, 1, 1, Mode::Sextant);
        assert_eq!(grid[0][0].ch, '█'); // full 2x3 pattern folds back to a full block
    }

    #[test]
    fn sextant_left_half_reuses_left_half_block() {
        // Left column white, right column black: the three left subcells light.
        let mut img = RgbImage::from_pixel(2, 3, image::Rgb([0, 0, 0]));
        for y in 0..3 {
            img.put_pixel(0, y, image::Rgb([255, 255, 255]));
        }
        let grid = render_cells(&img, 1, 1, Mode::Sextant);
        assert_eq!(grid[0][0].ch, '▌');
    }

    #[test]
    fn sextant_top_left_is_first_block_glyph() {
        // Only the top-left subcell light: the lowest sextant codepoint.
        let mut img = RgbImage::from_pixel(2, 3, image::Rgb([0, 0, 0]));
        img.put_pixel(0, 0, image::Rgb([255, 255, 255]));
        let grid = render_cells(&img, 1, 1, Mode::Sextant);
        assert_eq!(grid[0][0].ch, '\u{1FB00}'); // 🬀 BLOCK SEXTANT-1
    }

    #[test]
    fn octant_uniform_region_fills_all_subcells() {
        let img = RgbImage::from_pixel(4, 8, image::Rgb([200, 10, 30]));
        let grid = render_cells(&img, 1, 1, Mode::Octant);
        assert_eq!(grid[0][0].ch, '█'); // full 2x4 pattern folds back to a full block
    }

    #[test]
    fn octant_left_half_reuses_left_half_block() {
        // Left column white, right column black: a 2x2-collapsible quadrant.
        let mut img = RgbImage::from_pixel(2, 4, image::Rgb([0, 0, 0]));
        for y in 0..4 {
            img.put_pixel(0, y, image::Rgb([255, 255, 255]));
        }
        let grid = render_cells(&img, 1, 1, Mode::Octant);
        assert_eq!(grid[0][0].ch, '▌');
    }

    #[test]
    fn octant_top_left_cell_reuses_a_quarter_block() {
        // Only the top-left subcell light. This 1/2-wide, 1/4-tall shape is not
        // in the octant block; Unicode encodes it as a half-of-a-quarter block.
        let mut img = RgbImage::from_pixel(2, 4, image::Rgb([0, 0, 0]));
        img.put_pixel(0, 0, image::Rgb([255, 255, 255]));
        let grid = render_cells(&img, 1, 1, Mode::Octant);
        assert_eq!(grid[0][0].ch, '\u{1CEA8}'); // LEFT HALF UPPER ONE QUARTER BLOCK
    }

    #[test]
    fn octant_second_row_left_is_first_block_glyph() {
        // Only the second-row left subcell (position 3) light: BLOCK OCTANT-3,
        // the first glyph in the octant block at U+1CD00.
        let mut img = RgbImage::from_pixel(2, 4, image::Rgb([0, 0, 0]));
        img.put_pixel(0, 1, image::Rgb([255, 255, 255]));
        let grid = render_cells(&img, 1, 1, Mode::Octant);
        assert_eq!(grid[0][0].ch, '\u{1CD00}'); // 𜴀 BLOCK OCTANT-3
    }

    #[test]
    fn braille_uniform_region_raises_all_dots() {
        let img = RgbImage::from_pixel(4, 8, image::Rgb([200, 10, 30]));
        let grid = render_cells(&img, 1, 1, Mode::Braille);
        assert_eq!(grid[0][0].ch, '\u{28FF}'); // ⣿ all eight dots
    }

    #[test]
    fn braille_raises_dots_on_the_light_side() {
        // Left half white, right half black, in a single cell: dots 1,2,3,7
        // (the left column) are raised.
        let mut img = RgbImage::from_pixel(4, 8, image::Rgb([0, 0, 0]));
        for y in 0..8 {
            for x in 0..2 {
                img.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        let grid = render_cells(&img, 1, 1, Mode::Braille);
        let cell = grid[0][0];
        assert_eq!(cell.ch, '\u{2847}'); // ⡇
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
}
