//! Blocks on an integer grid, drawn so the near ones hide the far ones.
//!
//! Cubes on a lattice viewed along `(-1, -1, -1)` are the case where a painter
//! needs no depth buffer. If block A hides any part of block B then A is at
//! least as far along each of the three axes and further along one, so
//! `x + y + z` orders them; blocks with equal sums cannot overlap at all.
//! Drawing in ascending order of that sum is therefore exact, and each block
//! costs one row copy per scanline.
//!
//! On top of that, a block whose three near neighbours are all present is
//! covered completely and never drawn. That holds even when those neighbours
//! are themselves culled, since whatever covers them covers this block too,
//! and the chain ends at the largest sum in the scene.

use std::collections::HashMap;

use graphix::image::{Rgb, RgbImage, RgbaImage};

use crate::color::Color;
use crate::iso::{BlockSize, CompiledBlock};

/// A grid position holding a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Placement {
    /// Steps along the axis drawn down and to the right.
    pub x: i32,
    /// Steps along the axis drawn down and to the left.
    pub y: i32,
    /// Steps upward.
    pub z: i32,
    /// Which of the scene's baked blocks sits here.
    pub block: usize,
}

impl Placement {
    /// The painter's-algorithm sort key: blocks with a smaller sum are drawn
    /// first because nothing with a smaller sum can hide them.
    pub fn depth(self) -> i32 {
        self.x + self.y + self.z
    }
}

/// A grid of blocks, together with the baked sprites they are drawn from.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    size: Option<BlockSize>,
    palette: Vec<CompiledBlock>,
    grid: HashMap<(i32, i32, i32), usize>,
}

impl Scene {
    /// An empty scene.
    pub fn new() -> Self {
        Scene::default()
    }

    /// Add a baked block to the scene's palette and return the handle that
    /// [`Scene::place`] refers to it by. Blocks baked at different sizes
    /// cannot share a scene, so the first one added fixes the size.
    pub fn add(&mut self, block: CompiledBlock) -> usize {
        self.size.get_or_insert(block.size());
        self.palette.push(block);
        self.palette.len() - 1
    }

    /// Put `block` at `(x, y, z)`, replacing whatever was there.
    pub fn place(&mut self, x: i32, y: i32, z: i32, block: usize) {
        self.grid.insert((x, y, z), block);
    }

    /// The size every block in the scene is baked at, if any block is.
    pub fn size(&self) -> Option<BlockSize> {
        self.size
    }

    /// How many blocks the scene holds, drawn or not.
    pub fn len(&self) -> usize {
        self.grid.len()
    }

    /// Whether the scene holds no blocks.
    pub fn is_empty(&self) -> bool {
        self.grid.is_empty()
    }

    /// Whether the block at `(x, y, z)` is covered by its three near
    /// neighbours and so need not be drawn.
    pub fn hidden(&self, x: i32, y: i32, z: i32) -> bool {
        [(x + 1, y, z), (x, y + 1, z), (x, y, z + 1)]
            .iter()
            .all(|p| self.grid.contains_key(p))
    }

    /// The blocks that actually get drawn, in the order they are drawn.
    pub fn visible(&self) -> Vec<Placement> {
        let mut out: Vec<Placement> = self
            .grid
            .iter()
            .filter(|((x, y, z), _)| !self.hidden(*x, *y, *z))
            .map(|(&(x, y, z), &block)| Placement { x, y, z, block })
            .collect();
        // Ties cannot overlap, so any order within a depth will do; sorting on
        // the whole placement just keeps the output reproducible.
        out.sort_by_key(|p| (p.depth(), *p));
        out
    }

    /// Where a block at `(x, y, z)` puts the top-left corner of its sprite,
    /// before the scene is shifted to start at the origin.
    fn origin(&self, size: BlockSize, p: Placement) -> (i64, i64) {
        let (x, y, z) = (i64::from(p.x), i64::from(p.y), i64::from(p.z));
        (
            size.step_x() * (x - y),
            size.step_y() * (x + y) - size.step_z() * z,
        )
    }

    /// The scene drawn onto a transparent background, cropped to what it
    /// covers. An empty scene gives a one-pixel image.
    pub fn render(&self) -> RgbaImage {
        let (Some(size), false) = (self.size, self.grid.is_empty()) else {
            return RgbaImage::new(1, 1);
        };
        let drawn = self.visible();
        let corners: Vec<(i64, i64)> = drawn.iter().map(|&p| self.origin(size, p)).collect();
        let sprite = i64::from(size.sprite());
        let (Some(x0), Some(y0)) = (
            corners.iter().map(|c| c.0).min(),
            corners.iter().map(|c| c.1).min(),
        ) else {
            return RgbaImage::new(1, 1);
        };
        let x1 = corners.iter().map(|c| c.0 + sprite).max().unwrap_or(x0);
        let y1 = corners.iter().map(|c| c.1 + sprite).max().unwrap_or(y0);

        let mut img = RgbaImage::new((x1 - x0) as u32, (y1 - y0) as u32);
        for (p, (ox, oy)) in drawn.iter().zip(corners) {
            if let Some(block) = self.palette.get(p.block) {
                block.blit(&mut img, ox - x0, oy - y0);
            }
        }
        img
    }
}

/// Drop the alpha channel, putting `background` behind the scene.
///
/// The terminal renderer works in RGB, so this is what a scene goes through on
/// its way to [`graphix::render_image`].
pub fn flatten(img: &RgbaImage, background: Color) -> RgbImage {
    RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        if p[3] == 0 {
            Rgb(background)
        } else {
            Rgb([p[0], p[1], p[2]])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin;

    fn size() -> BlockSize {
        BlockSize::new(4, 2)
    }

    /// A scene of `n` distinct blocks, all baked at the test size.
    fn scene() -> (Scene, Vec<usize>) {
        let mut s = Scene::new();
        let handles = builtin::all()
            .into_iter()
            .map(|r| s.add(r.bake(1, size()).expect("bake")))
            .collect();
        (s, handles)
    }

    /// A deterministic scatter of grid positions in a small cube.
    fn positions(seed: u64, count: usize, extent: i32) -> Vec<(i32, i32, i32)> {
        let span = extent as u64 * 2 + 1;
        (0..count as u64)
            .map(|i| {
                let h = |t: u64| {
                    (graphix_noise::splitmix64(seed ^ (i << 8) ^ t) % span) as i32 - extent
                };
                (h(1), h(2), h(3))
            })
            .collect()
    }

    /// The nearest block covering each pixel, found by asking every block
    /// rather than by ordering them. Panics if two blocks at the same depth
    /// both cover a pixel, which is the ordering assumption under test.
    fn reference(s: &Scene) -> RgbaImage {
        let size = s.size().expect("a size");
        let drawn = s.visible();
        let corners: Vec<(i64, i64)> = drawn.iter().map(|&p| s.origin(size, p)).collect();
        let sprite = i64::from(size.sprite());
        let x0 = corners.iter().map(|c| c.0).min().unwrap_or(0);
        let y0 = corners.iter().map(|c| c.1).min().unwrap_or(0);
        let w = corners.iter().map(|c| c.0 + sprite).max().unwrap_or(0) - x0;
        let h = corners.iter().map(|c| c.1 + sprite).max().unwrap_or(0) - y0;

        let mut img = RgbaImage::new(w as u32, h as u32);
        for py in 0..h {
            for px in 0..w {
                let mut best: Option<(i32, [u8; 4])> = None;
                for (p, &(ox, oy)) in drawn.iter().zip(&corners) {
                    let (lx, ly) = (px + x0 - ox, py + y0 - oy);
                    if !(0..sprite).contains(&lx) || !(0..sprite).contains(&ly) {
                        continue;
                    }
                    if size.locate(lx as u32, ly as u32).is_none() {
                        continue;
                    }
                    let block = &s.palette[p.block];
                    let color = block.to_image().get_pixel(lx as u32, ly as u32).0;
                    match best {
                        Some((d, _)) if d > p.depth() => {}
                        Some((d, _)) if d == p.depth() => {
                            panic!("two blocks at depth {d} both cover ({px}, {py})")
                        }
                        _ => best = Some((p.depth(), color)),
                    }
                }
                if let Some((_, color)) = best {
                    img.put_pixel(px as u32, py as u32, graphix::image::Rgba(color));
                }
            }
        }
        img
    }

    #[test]
    fn an_empty_scene_renders_without_panicking() {
        assert_eq!(Scene::new().render().dimensions(), (1, 1));
        assert!(Scene::new().is_empty());
    }

    #[test]
    fn a_lone_block_renders_exactly_its_sprite() {
        let (mut s, h) = scene();
        s.place(0, 0, 0, h[0]);
        let block = builtin::stone().bake(1, size()).expect("bake");
        assert_eq!(s.render(), block.to_image());
    }

    #[test]
    fn the_painter_agrees_with_a_depth_sorted_reference() {
        // Every pixel must come from the nearest block covering it, whichever
        // way that block is found. The reference also asserts that no two
        // blocks at one depth ever overlap, which is why the sort key works.
        let (_, h) = scene();
        for seed in 0..8u64 {
            let (mut s, _) = scene();
            for (i, (x, y, z)) in positions(seed, 10, 1).into_iter().enumerate() {
                s.place(x, y, z, h[i % h.len()]);
            }
            assert_eq!(s.render(), reference(&s), "seed {seed}");
        }
    }

    #[test]
    fn a_nearer_block_hides_the_one_behind_it() {
        let (mut s, h) = scene();
        s.place(0, 0, 0, h[0]);
        let alone = s.render();
        // One step along every axis puts the second block exactly in front,
        // where it covers the first entirely.
        s.place(1, 1, 1, h[3]);
        let pair = s.render();
        assert_ne!(pair, alone);
        assert!(!s.hidden(1, 1, 1), "the front block cannot be hidden");
        assert!(!s.hidden(0, 0, 0), "one neighbour is not three");
    }

    #[test]
    fn culling_never_changes_the_image() {
        // A solid cube of blocks: the interior is hidden and must be skipped,
        // and skipping it must be invisible.
        let (mut s, h) = scene();
        for x in 0..3 {
            for y in 0..3 {
                for z in 0..3 {
                    s.place(x, y, z, h[((x + y + z) as usize) % h.len()]);
                }
            }
        }
        assert_eq!(s.len(), 27);
        let drawn = s.visible().len();
        assert!(drawn < 27, "nothing was culled");
        assert_eq!(s.render(), reference(&s));

        // The block with all three near neighbours present is the one at the
        // low corner of the 2x2x2 sub-cube nearest the viewer.
        assert!(s.hidden(0, 0, 0));
        assert!(!s.hidden(2, 2, 2));
    }

    #[test]
    fn a_wall_of_blocks_tiles_without_gaps() {
        // Neighbours along x and y share a face edge exactly, so a flat layer
        // has no transparent pixel anywhere inside its outline.
        let (mut s, h) = scene();
        for x in 0..4 {
            for y in 0..4 {
                s.place(x, y, 0, h[0]);
            }
        }
        let img = s.render();
        let q = size().quarter();
        // The centre of the layer, well inside its outline.
        let (cx, cy) = (img.width() / 2, img.height() / 2);
        for dy in 0..q {
            for dx in 0..q {
                let p = img.get_pixel(cx + dx - q / 2, cy + dy - q / 2).0;
                assert_eq!(p[3], 255, "gap at ({dx}, {dy}) from the centre");
            }
        }
    }

    #[test]
    fn a_scene_is_cropped_to_what_it_covers() {
        let (mut s, h) = scene();
        s.place(0, 0, 0, h[0]);
        s.place(5, 5, 5, h[1]);
        let img = s.render();
        // Placing the same pair far from the origin must give the same image.
        let (mut t, g) = scene();
        t.place(100, 100, 100, g[0]);
        t.place(105, 105, 105, g[1]);
        assert_eq!(img, t.render());
    }

    #[test]
    fn flattening_puts_the_background_behind_the_scene() {
        let (mut s, h) = scene();
        s.place(0, 0, 0, h[0]);
        let rgb = flatten(&s.render(), [7, 8, 9]);
        assert_eq!(rgb.get_pixel(0, 0).0, [7, 8, 9]);
        let q = size().quarter();
        assert_ne!(rgb.get_pixel(2 * q, q).0, [7, 8, 9]);
    }
}
