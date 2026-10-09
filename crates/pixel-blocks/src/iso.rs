//! The 2:1 dimetric projection: where a cube's pixels are, which face and
//! texel each one belongs to, and the baked sprite that answers both at once.
//!
//! Everything here is exact integer arithmetic on doubled pixel coordinates,
//! so a pixel centre never lands on a face boundary and the three faces
//! partition the silhouette with no gaps and no overwrites. The mapping
//! depends only on [`BlockSize`], which is why it can be baked once and reused
//! for every block of that size.

use std::ops::Range;

use graphix::image::RgbaImage;

use crate::color::Color;
use crate::face::{Face, Texel};

/// How big a block is drawn: `texels` per cube edge, `scale` pixels per texel.
///
/// With `n = texels * scale` the sprite is `4n` square: the top diamond is
/// `4n` by `2n`, and each side face hangs `2n` below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSize {
    texels: u32,
    scale: u32,
}

impl Default for BlockSize {
    fn default() -> Self {
        BlockSize::new(16, 4)
    }
}

impl BlockSize {
    /// A size of `texels` per cube edge at `scale` pixels per texel, each
    /// clamped to at least one.
    pub fn new(texels: u32, scale: u32) -> Self {
        BlockSize {
            texels: texels.max(1),
            scale: scale.max(1),
        }
    }

    /// Texels per cube edge.
    pub fn texels(self) -> u32 {
        self.texels
    }

    /// Pixels per texel.
    pub fn scale(self) -> u32 {
        self.scale
    }

    /// A quarter of the sprite's width: `texels * scale`. Every offset in the
    /// projection is a small multiple of this.
    pub fn quarter(self) -> u32 {
        self.texels * self.scale
    }

    /// The sprite's side length in pixels, `4 * quarter`.
    pub fn sprite(self) -> u32 {
        4 * self.quarter()
    }

    /// Horizontal offset between a block and its neighbour one step along `x`
    /// (the neighbour along `y` moves by the negation).
    pub fn step_x(self) -> i64 {
        2 * i64::from(self.quarter())
    }

    /// Vertical offset between a block and its neighbour one step along `x`
    /// or `y`.
    pub fn step_y(self) -> i64 {
        i64::from(self.quarter())
    }

    /// Vertical offset between a block and the one directly above it, which is
    /// the height of a side face.
    pub fn step_z(self) -> i64 {
        2 * i64::from(self.quarter())
    }

    /// The horizontal range of sprite row `sy` that the cube covers.
    ///
    /// The silhouette is a hexagon bounded by four sloped edges, so the range
    /// is the intersection of four half-planes. Rows outside the sprite give
    /// an empty range.
    pub fn row_span(self, sy: u32) -> Range<u32> {
        let q = i64::from(self.quarter());
        let (yy, side) = (2 * i64::from(sy) + 1, 4 * q);
        let start = 0.max(2 * q - yy).max(yy - 6 * q);
        let end = side.min(yy + 2 * q).min(10 * q - yy);
        if end <= start {
            return 0..0;
        }
        start as u32..end as u32
    }

    /// Which face and texel sprite pixel `(sx, sy)` belongs to, or `None` when
    /// the pixel falls outside the silhouette.
    pub fn locate(self, sx: u32, sy: u32) -> Option<Texel> {
        let (q, n) = (i64::from(self.quarter()), self.texels);
        let (xx, yy) = (2 * i64::from(sx) + 1, 2 * i64::from(sy) + 1);
        // Face-local coordinates scaled by 8 * scale, so dividing by that
        // gives the texel index and the bounds test is a comparison with 8q.
        let (lim, unit) = (8 * q, 8 * i64::from(self.scale));
        let (a, b) = (2 * yy + xx - 4 * q, 2 * yy - xx + 4 * q);

        let texel = |face, u: i64, v: i64| {
            Some(Texel {
                face,
                u: (u / unit) as u32,
                v: (v / unit) as u32,
                n,
            })
        };
        if (0..lim).contains(&a) && (0..lim).contains(&b) {
            return texel(Face::Top, a, b);
        }
        // Below the top face's lower-left edge the left face begins, and below
        // its lower-right edge the right face does; the two split at mid-width.
        if xx < 4 * q {
            let v = b - lim;
            (0..lim)
                .contains(&v)
                .then(|| texel(Face::Left, 2 * xx, v))?
        } else {
            let v = a - lim;
            (0..lim)
                .contains(&v)
                .then(|| texel(Face::Right, 2 * (xx - 4 * q), v))?
        }
    }
}

/// A baked block: its sprite, ready to copy into a scene row by row.
///
/// The pixels are stored in [`RgbaImage`]'s own layout so a blit is one
/// `copy_from_slice` per row, and nothing outside the silhouette is ever read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledBlock {
    size: BlockSize,
    pixels: Vec<u8>,
}

impl CompiledBlock {
    /// Rasterize three baked faces, each `n * n` colors in row-major order,
    /// into a sprite. Faces shorter than that repeat their last color.
    pub fn from_faces(size: BlockSize, top: &[Color], left: &[Color], right: &[Color]) -> Self {
        let sprite = size.sprite();
        let mut pixels = vec![0u8; (sprite * sprite * 4) as usize];
        for sy in 0..sprite {
            for sx in size.row_span(sy) {
                let Some(t) = size.locate(sx, sy) else {
                    continue;
                };
                let face = match t.face {
                    Face::Top => top,
                    Face::Left => left,
                    Face::Right => right,
                };
                let i = (t.v * t.n + t.u) as usize;
                let [r, g, b] = face
                    .get(i)
                    .or_else(|| face.last())
                    .copied()
                    .unwrap_or([0; 3]);
                let o = ((sy * sprite + sx) * 4) as usize;
                pixels[o..o + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        CompiledBlock { size, pixels }
    }

    /// The size this block was baked at.
    pub fn size(&self) -> BlockSize {
        self.size
    }

    /// The block as a standalone image with a transparent background.
    pub fn to_image(&self) -> RgbaImage {
        let sprite = self.size.sprite();
        RgbaImage::from_raw(sprite, sprite, self.pixels.clone())
            .unwrap_or_else(|| RgbaImage::new(sprite, sprite))
    }

    /// Copy the sprite into `dst` with its top-left corner at `(ox, oy)`,
    /// clipped to the target. Only covered pixels are written, so a nearer
    /// block drawn afterwards replaces exactly the part it hides.
    pub fn blit(&self, dst: &mut RgbaImage, ox: i64, oy: i64) {
        let (dw, dh) = (i64::from(dst.width()), i64::from(dst.height()));
        let sprite = i64::from(self.size.sprite());
        let buf: &mut [u8] = dst;
        for sy in 0..self.size.sprite() {
            let dy = oy + i64::from(sy);
            if !(0..dh).contains(&dy) {
                continue;
            }
            let span = self.size.row_span(sy);
            let lo = (i64::from(span.start) + ox).max(0);
            let hi = (i64::from(span.end) + ox).min(dw);
            if hi <= lo {
                continue;
            }
            let src = ((i64::from(sy) * sprite + lo - ox) * 4) as usize;
            let dst_at = ((dy * dw + lo) * 4) as usize;
            let len = ((hi - lo) * 4) as usize;
            buf[dst_at..dst_at + len].copy_from_slice(&self.pixels[src..src + len]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sizes worth checking: `texels = 1` and `2` exercise the degenerate
    /// cases, and the scales cover both an odd and an even pixel grid.
    fn sizes() -> Vec<BlockSize> {
        let mut out = Vec::new();
        for texels in [1, 2, 3, 16] {
            for scale in [1, 2, 4] {
                out.push(BlockSize::new(texels, scale));
            }
        }
        out
    }

    #[test]
    fn a_sprite_is_four_quarters_square() {
        let size = BlockSize::new(16, 4);
        assert_eq!(size.sprite(), 4 * 16 * 4);
        assert_eq!(BlockSize::default(), size);
        // Both arguments are clamped rather than allowed to produce an empty
        // sprite.
        assert_eq!(BlockSize::new(0, 0), BlockSize::new(1, 1));
    }

    #[test]
    fn every_pixel_belongs_to_at_most_one_face() {
        // `locate` returns a single texel, so the partition is only in doubt
        // where a face's own bounds might overlap another's. Checking the
        // covered count against the hexagon's area pins it from both sides.
        for size in sizes() {
            let sprite = size.sprite();
            let covered = (0..sprite)
                .flat_map(|y| (0..sprite).map(move |x| (x, y)))
                .filter(|&(x, y)| size.locate(x, y).is_some())
                .count();
            let q = size.quarter() as usize;
            assert_eq!(covered, 12 * q * q, "wrong coverage at {size:?}");
        }
    }

    #[test]
    fn row_spans_agree_with_face_membership() {
        for size in sizes() {
            let sprite = size.sprite();
            for y in 0..sprite {
                let span = size.row_span(y);
                let inside: Vec<u32> = (0..sprite)
                    .filter(|&x| size.locate(x, y).is_some())
                    .collect();
                match inside.as_slice() {
                    [] => assert!(span.is_empty(), "row {y} of {size:?} should be empty"),
                    [first, .., last] => {
                        assert_eq!(span, *first..*last + 1, "row {y} of {size:?}");
                        // The span must be solid, not merely bounded correctly.
                        assert_eq!(inside.len() as u32, span.end - span.start);
                    }
                    [only] => assert_eq!(span, *only..*only + 1),
                }
            }
        }
    }

    #[test]
    fn every_texel_of_every_face_is_drawn() {
        // No texel may be skipped by rounding, or a pattern would have pixels
        // it can never reach.
        for size in sizes() {
            let (sprite, n) = (size.sprite(), size.texels());
            let mut seen = std::collections::HashSet::new();
            for y in 0..sprite {
                for x in size.row_span(y) {
                    if let Some(t) = size.locate(x, y) {
                        assert!(t.u < n && t.v < n, "texel {t:?} out of range at {size:?}");
                        seen.insert((t.face, t.u, t.v));
                    }
                }
            }
            assert_eq!(
                seen.len(),
                3 * (n * n) as usize,
                "missing texels at {size:?}"
            );
        }
    }

    #[test]
    fn the_silhouette_is_left_right_symmetric() {
        for size in sizes() {
            let sprite = size.sprite();
            for y in 0..sprite {
                let span = size.row_span(y);
                let mirrored = sprite - span.end..sprite - span.start;
                assert_eq!(span, mirrored, "row {y} of {size:?} is lopsided");
            }
        }
    }

    #[test]
    fn the_faces_meet_at_the_expected_vertices() {
        let size = BlockSize::new(2, 2);
        let q = size.quarter();
        // Just below the top vertex is the top face; the lower-left quadrant
        // is the left face and the lower-right quadrant the right face.
        assert_eq!(size.locate(2 * q, 0).map(|t| t.face), Some(Face::Top));
        assert_eq!(size.locate(q, 3 * q).map(|t| t.face), Some(Face::Left));
        assert_eq!(size.locate(3 * q, 3 * q).map(|t| t.face), Some(Face::Right));
        // The four corners of the bounding box fall outside the hexagon.
        for (x, y) in [
            (0, 0),
            (4 * q - 1, 0),
            (0, 4 * q - 1),
            (4 * q - 1, 4 * q - 1),
        ] {
            assert_eq!(size.locate(x, y), None, "corner ({x}, {y}) is covered");
        }
    }

    #[test]
    fn a_baked_block_paints_each_face_its_own_color() {
        let size = BlockSize::new(4, 2);
        let n = (size.texels() * size.texels()) as usize;
        let block = CompiledBlock::from_faces(
            size,
            &vec![[255, 0, 0]; n],
            &vec![[0, 255, 0]; n],
            &vec![[0, 0, 255]; n],
        );
        let img = block.to_image();
        let q = size.quarter();
        assert_eq!(img.dimensions(), (size.sprite(), size.sprite()));
        assert_eq!(img.get_pixel(2 * q, q).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(q, 3 * q).0, [0, 255, 0, 255]);
        assert_eq!(img.get_pixel(3 * q, 3 * q).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 0]);
    }

    #[test]
    fn blitting_writes_the_silhouette_and_nothing_else() {
        let size = BlockSize::new(2, 2);
        let n = (size.texels() * size.texels()) as usize;
        let block = CompiledBlock::from_faces(
            size,
            &vec![[9, 9, 9]; n],
            &vec![[9, 9, 9]; n],
            &vec![[9, 9, 9]; n],
        );
        let sprite = size.sprite();
        let mut img = RgbaImage::new(sprite, sprite);
        block.blit(&mut img, 0, 0);
        for y in 0..sprite {
            for x in 0..sprite {
                let opaque = img.get_pixel(x, y).0[3] == 255;
                assert_eq!(opaque, size.locate(x, y).is_some(), "pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn blitting_off_the_edge_clips_instead_of_panicking() {
        let size = BlockSize::new(2, 2);
        let n = (size.texels() * size.texels()) as usize;
        let block = CompiledBlock::from_faces(
            size,
            &vec![[1, 2, 3]; n],
            &vec![[1, 2, 3]; n],
            &vec![[1, 2, 3]; n],
        );
        let mut img = RgbaImage::new(8, 8);
        let far = i64::from(size.sprite()) + 4;
        for (ox, oy) in [(-far, 0), (far, 0), (0, -far), (0, far), (-3, -3), (5, 5)] {
            block.blit(&mut img, ox, oy);
        }
        // The two overlapping offsets touched the image; the four distant ones
        // could not have.
        assert!(img.pixels().any(|p| p.0[3] == 255));
    }
}
