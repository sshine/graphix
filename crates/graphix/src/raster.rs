//! Rasterize a rendered cell grid back into a pixel image.
//!
//! Terminal emulators draw the glyphs graphix emits as geometric fills, so a
//! cell grid can be reproduced pixel-perfectly without any font: every glyph
//! maps back to the subcell pattern (or coverage fraction) it encodes. This
//! gives a faithful PNG preview of what the ANSI output looks like in a
//! terminal — useful for demos, tests, and environments without a terminal.

use std::collections::HashMap;

use image::RgbImage;

use crate::{Cell, Rgb, octant_char, sextant_char};

/// How a glyph distributes the cell's foreground over its area.
#[derive(Debug, Clone, Copy)]
enum Paint {
    /// A uniform mix of foreground into background (shade blocks `░▒▓`).
    Blend(f32),
    /// A solid `nx`x`ny` subcell matrix; bit `i` lights subcell `i` row-major.
    Fill { nx: u32, ny: u32, bits: u32 },
    /// A 2x4 dot matrix (Braille); bit `i` raises dot `i` row-major.
    Dots(u32),
}

/// Row-major subcell order of the Braille dot bits (see `BRAILLE_DOTS`).
const BRAILLE_DOTS: [u32; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];

/// Resolve a glyph to its paint by inverting the glyph mappings.
fn paint_for(ch: char) -> Paint {
    match ch {
        ' ' => return Paint::Blend(0.0),
        '░' => return Paint::Blend(0.25),
        '▒' => return Paint::Blend(0.5),
        '▓' => return Paint::Blend(0.75),
        _ => {}
    }
    if let Some(dots) = (ch as u32).checked_sub(0x2800).filter(|d| *d < 0x100) {
        let bits = BRAILLE_DOTS
            .iter()
            .enumerate()
            .filter(|(_, dot)| dots & **dot != 0)
            .fold(0u32, |acc, (i, _)| acc | 1 << i);
        return Paint::Dots(bits);
    }
    // The quadrant and quarter-block reuses are found via the octant search.
    if let Some(bits) = (0..256u32).find(|&bits| octant_char(bits) == ch) {
        return Paint::Fill { nx: 2, ny: 4, bits };
    }
    if let Some(bits) = (0..64u32).find(|&bits| sextant_char(bits) == ch) {
        return Paint::Fill { nx: 2, ny: 3, bits };
    }
    Paint::Blend(0.5)
}

/// Mix `fg` into `bg` by fraction `t`.
fn lerp(bg: Rgb, fg: Rgb, t: f32) -> Rgb {
    let mix = |a: u8, b: u8| (f32::from(a) + t * (f32::from(b) - f32::from(a))).round() as u8;
    Rgb {
        r: mix(bg.r, fg.r),
        g: mix(bg.g, fg.g),
        b: mix(bg.b, fg.b),
    }
}

/// The pixel color at `(px, py)` within a `cell_w`x`cell_h` cell.
fn pixel(paint: Paint, cell: &Cell, px: u32, py: u32, cell_w: u32, cell_h: u32) -> Rgb {
    match paint {
        Paint::Blend(t) => lerp(cell.bg, cell.fg, t),
        Paint::Fill { nx, ny, bits } => {
            let (sx, sy) = (px * nx / cell_w, py * ny / cell_h);
            if bits >> (sy * nx + sx) & 1 == 1 {
                cell.fg
            } else {
                cell.bg
            }
        }
        Paint::Dots(bits) => {
            let (nx, ny) = (2, 4);
            let (sx, sy) = (px * nx / cell_w, py * ny / cell_h);
            if bits >> (sy * nx + sx) & 1 == 0 {
                return cell.bg;
            }
            // A raised dot is a disc centered in its subcell.
            let (sw, sh) = (cell_w as f32 / nx as f32, cell_h as f32 / ny as f32);
            let (cx, cy) = ((sx as f32 + 0.5) * sw, (sy as f32 + 0.5) * sh);
            let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
            let radius = 0.4 * sw.min(sh);
            if dx * dx + dy * dy <= radius * radius {
                cell.fg
            } else {
                cell.bg
            }
        }
    }
}

/// Render a cell grid to an image, each cell drawn `cell_w`x`cell_h` pixels
/// (a 1:2 ratio like `8`x`16` matches the terminal cell aspect the rest of
/// the pipeline assumes).
pub fn rasterize(grid: &[Vec<Cell>], cell_w: u32, cell_h: u32) -> RgbImage {
    let rows = grid.len() as u32;
    let cols = grid.first().map_or(0, |row| row.len()) as u32;
    let mut paints: HashMap<char, Paint> = HashMap::new();
    let mut img = RgbImage::new((cols * cell_w).max(1), (rows * cell_h).max(1));
    for (cy, row) in grid.iter().enumerate() {
        for (cx, cell) in row.iter().enumerate() {
            let paint = *paints.entry(cell.ch).or_insert_with(|| paint_for(cell.ch));
            for py in 0..cell_h {
                for px in 0..cell_w {
                    let c = pixel(paint, cell, px, py, cell_w, cell_h);
                    img.put_pixel(
                        cx as u32 * cell_w + px,
                        cy as u32 * cell_h + py,
                        image::Rgb([c.r, c.g, c.b]),
                    );
                }
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Mode, render_cells};

    #[test]
    fn rasterize_round_trips_a_half_block_cell() {
        // Top half red, bottom half blue: rasterizing the `▀` cell reproduces it.
        let mut img = RgbImage::from_pixel(2, 4, image::Rgb([255, 0, 0]));
        for y in 2..4 {
            for x in 0..2 {
                img.put_pixel(x, y, image::Rgb([0, 0, 255]));
            }
        }
        let grid = render_cells(&img, 1, 1, Mode::HalfBlock);
        let out = rasterize(&grid, 8, 16);
        assert_eq!(out.dimensions(), (8, 16));
        assert_eq!(out.get_pixel(4, 4).0, [255, 0, 0]);
        assert_eq!(out.get_pixel(4, 12).0, [0, 0, 255]);
    }

    #[test]
    fn rasterize_octant_left_column() {
        // Left column white: `▌` fills the left half with foreground.
        let mut img = RgbImage::from_pixel(2, 4, image::Rgb([0, 0, 0]));
        for y in 0..4 {
            img.put_pixel(0, y, image::Rgb([255, 255, 255]));
        }
        let grid = render_cells(&img, 1, 1, Mode::Octant);
        let out = rasterize(&grid, 8, 16);
        assert_eq!(out.get_pixel(2, 8).0, [255, 255, 255]);
        assert_eq!(out.get_pixel(6, 8).0, [0, 0, 0]);
    }

    #[test]
    fn rasterize_braille_dot_has_background_gaps() {
        // A raised dot is a disc: subcell corners stay background-colored.
        let cell = Cell {
            ch: '⣿',
            fg: Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            bg: Rgb { r: 0, g: 0, b: 0 },
        };
        let out = rasterize(&[vec![cell]], 8, 16);
        assert_eq!(out.get_pixel(2, 2).0, [255, 255, 255]); // dot center
        assert_eq!(out.get_pixel(0, 0).0, [0, 0, 0]); // subcell corner
    }

    #[test]
    fn rasterize_shade_blends_colors() {
        let cell = Cell {
            ch: '▒',
            fg: Rgb {
                r: 200,
                g: 100,
                b: 0,
            },
            bg: Rgb { r: 0, g: 0, b: 0 },
        };
        let out = rasterize(&[vec![cell]], 4, 8);
        assert_eq!(out.get_pixel(0, 0).0, [100, 50, 0]);
    }
}
