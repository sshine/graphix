//! The pattern primitives a recipe builds a face's texture from.
//!
//! A pattern is a pure function from a texel to a color: it reads the texel's
//! position on the cube and the face's ramp, never neighbouring results, so
//! texels can be evaluated in any order and a block is reproducible from its
//! seed alone. Patterns that leave some texels alone return `None` there and
//! stack under [`Pattern::Layer`].
//!
//! Positions come from [`Texel::key`], the texel's centre on the cube rather
//! than its `(u, v)` on one face, so a texture runs on across the cube's edges
//! instead of restarting three times.

use graphix_noise::{Noise, hash_to_unit, hash3, splitmix64};
use serde::{Deserialize, Serialize};

use crate::color::{Color, ColorSpec, Ramp, band};
use crate::face::Texel;

/// Noise channel tag, so speckle does not correlate with the smooth fields.
const CH_SPECKLE: u64 = 0x5EEC_41ED;

/// How a face's texels are colored.
///
/// In TOML a pattern is a table named after its variant, so the fields with
/// defaults can be left out: `[pattern.speckle] contrast = 1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Pattern {
    /// One flat color: the ramp stop at `index`, clamped to the ramp.
    Solid {
        /// Which ramp stop to use, counting from the darkest.
        index: usize,
    },
    /// Independent per-texel noise across the whole ramp: the grainy look of
    /// stone and of a grass top.
    Speckle {
        /// How much of the ramp to spread across, in `0..=1`.
        contrast: f32,
    },
    /// Correlated noise, giving patches several texels across rather than the
    /// single-texel grain of [`Pattern::Speckle`]. Sand and other materials
    /// that clump.
    Blotch {
        /// Patches per cube edge; larger is finer.
        scale: f32,
        /// How much of the ramp to spread across, in `0..=1`.
        contrast: f32,
        /// Layers of detail, each twice as fine and half as strong. One gives
        /// plain patches; three roughens them without breaking the clumping.
        #[serde(default = "one")]
        octaves: u32,
    },
    /// Sparse inclusions on a coarse grid: the grey pebbles in dirt, or the
    /// nuggets in an ore.
    Scatter {
        /// Side of the grid cell, in texels, that holds at most one inclusion.
        cell: u32,
        /// Chance a given cell holds one, in `0..=1`.
        density: f32,
        /// Side of one inclusion, in texels.
        #[serde(default = "one")]
        size: u32,
        /// The inclusion's color.
        color: ColorSpec,
    },
    /// A band along the top of the side faces with a ragged lower edge: grass
    /// hanging over dirt. The band's depth varies along the cube's side, so it
    /// does not step where two faces meet. Never covers the top face.
    Fringe {
        /// Mean depth of the band, in texels.
        depth: u32,
        /// How far the depth strays from the mean, in texels.
        #[serde(default = "one")]
        jitter: u32,
        /// Columns between changes of direction; smaller is more ragged.
        #[serde(default = "two")]
        span: u32,
        /// The band's own palette.
        ramp: Ramp,
        /// The band's own pattern, evaluated against `ramp`.
        pattern: Box<Pattern>,
    },
    /// Mortar lines dividing the face into a grid: brick courses, or the seams
    /// between planks.
    Lattice {
        /// Horizontal lines per face; `0` for none.
        rows: u32,
        /// Vertical lines per face; `0` for none.
        cols: u32,
        /// Whether alternate courses are offset by half a cell.
        #[serde(default)]
        stagger: bool,
        /// The lines' color.
        mortar: ColorSpec,
    },
    /// Patterns stacked back to front: the last one that colors a texel wins.
    Layer(Vec<Pattern>),
}

impl Pattern {
    /// The color this pattern gives `t`, or `None` where it leaves the texel
    /// to whatever lies under it in a [`Pattern::Layer`].
    pub fn sample(&self, noise: &Noise, t: Texel, ramp: &[Color]) -> Option<Color> {
        match self {
            Pattern::Solid { index } => Some(band(ramp, stop(*index, ramp.len()))),
            Pattern::Speckle { contrast } => {
                let h = unit_at(&noise.channel(CH_SPECKLE), t);
                Some(band(ramp, spread(h, *contrast)))
            }
            Pattern::Blotch {
                scale,
                contrast,
                octaves,
            } => {
                let p = sample_point(t, *scale);
                let v = match octaves {
                    0 | 1 => noise.value3(p),
                    n => noise.fbm3(p, *n, 2.0, 0.5),
                };
                Some(band(ramp, spread(v * 0.5 + 0.5, *contrast)))
            }
            Pattern::Scatter {
                cell,
                density,
                size,
                color,
            } => scatter(noise, t, *cell, *density, *size).then(|| color.resolve()),
            Pattern::Fringe {
                depth,
                jitter,
                span,
                ramp,
                pattern,
            } => {
                let r = t.ring()?;
                let d = fringe_depth(noise, r, *depth, *jitter, *span);
                (i64::from(t.v) < d).then(|| pattern.sample(noise, t, &ramp.resolve()))?
            }
            Pattern::Lattice {
                rows,
                cols,
                stagger,
                mortar,
            } => lattice(t, *rows, *cols, *stagger).then(|| mortar.resolve()),
            Pattern::Layer(layers) => layers.iter().rev().find_map(|p| p.sample(noise, t, ramp)),
        }
    }

    /// The color this pattern gives `t`, falling back to the middle of the
    /// ramp where every layer leaves the texel untouched.
    pub fn shade(&self, noise: &Noise, t: Texel, ramp: &[Color]) -> Color {
        self.sample(noise, t, ramp)
            .unwrap_or_else(|| band(ramp, 0.5))
    }
}

/// Serde defaults for the counts a hand-written recipe usually omits.
fn one() -> u32 {
    1
}

/// Serde default for the fringe's lattice spacing.
fn two() -> u32 {
    2
}

/// The ramp position of stop `index` of `len`, clamped to the last stop.
fn stop(index: usize, len: usize) -> f32 {
    if len == 0 {
        return 0.0;
    }
    (index.min(len - 1) as f32 + 0.5) / len as f32
}

/// Center a unit value on the ramp and squeeze it by `contrast`, so a low
/// contrast keeps to the middle stops instead of clipping to the ends.
fn spread(unit: f32, contrast: f32) -> f32 {
    0.5 + (unit - 0.5) * contrast.clamp(0.0, 1.0)
}

/// A uniform value in `[0, 1)` for one texel, keyed on its place on the cube.
fn unit_at(noise: &Noise, t: Texel) -> f32 {
    let [x, y, z] = t.key();
    hash_to_unit(hash3(noise.seed, x, y, z))
}

/// The texel's centre in noise space, where one unit is one lattice cell and
/// `scale` cells span the cube.
fn sample_point(t: Texel, scale: f32) -> [f32; 3] {
    let f = scale.max(0.0) / (2.0 * t.n.max(1) as f32);
    t.key().map(|c| c as f32 * f)
}

/// Whether an inclusion covers `t`: each `cell` by `cell` block of texels
/// rolls once for one, then places it at a position drawn from the same hash.
fn scatter(noise: &Noise, t: Texel, cell: u32, density: f32, size: u32) -> bool {
    let cell = cell.max(1);
    let anchor = Texel {
        u: t.u / cell * cell,
        v: t.v / cell * cell,
        ..t
    };
    let [x, y, z] = anchor.key();
    let h = hash3(noise.seed, x, y, z);
    if hash_to_unit(h) >= density {
        return false;
    }
    let size = size.clamp(1, cell);
    let free = cell - size + 1;
    let place = |tag: u64| (hash_to_unit(splitmix64(h ^ tag)) * free as f32) as u32;
    let (ou, ov) = (place(1), place(2));
    let (du, dv) = (t.u % cell, t.v % cell);
    (ou..ou + size).contains(&du) && (ov..ov + size).contains(&dv)
}

/// The depth of the fringe at column `r` of the cube's side band.
///
/// The depth is interpolated between lattice points `span` columns apart, so
/// it varies smoothly along the band and, crucially, is one function of one
/// coordinate: the column where two faces meet is no different from any other.
fn fringe_depth(noise: &Noise, r: u32, depth: u32, jitter: u32, span: u32) -> i64 {
    let span = span.max(1);
    let (i, f) = (i64::from(r / span), (r % span) as f32 / span as f32);
    let at = |k: i64| hash_to_unit(splitmix64(noise.seed ^ k as u64)) * 2.0 - 1.0;
    let wobble = at(i) + (at(i + 1) - at(i)) * f;
    (depth as f32 + wobble * jitter as f32).round() as i64
}

/// Whether `t` falls on a mortar line of a `rows` by `cols` lattice.
fn lattice(t: Texel, rows: u32, cols: u32, stagger: bool) -> bool {
    let n = t.n.max(1);
    if divides(t.v, rows, n) {
        return true;
    }
    if cols == 0 {
        return false;
    }
    // Vertical lines are indexed around the cube's side band so a course runs
    // unbroken past the front edge, and staggered courses shift by half a cell.
    let course = t.v * rows.max(1) / n;
    let shift = if stagger && !course.is_multiple_of(2) {
        n / (2 * cols)
    } else {
        0
    };
    divides((t.ring().unwrap_or(t.u) + shift) % n, cols, n)
}

/// Whether `i` is the first of its share when `0..n` is cut into `parts`.
fn divides(i: u32, parts: u32, n: u32) -> bool {
    parts > 0 && n > 0 && (i * parts) % n < parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::Face;

    const N: u32 = 16;

    fn ramp() -> Vec<Color> {
        Ramp::rgb([[40, 40, 40], [80, 80, 80], [120, 120, 120], [160, 160, 160]]).resolve()
    }

    fn texels(n: u32) -> Vec<Texel> {
        let mut out = Vec::new();
        for face in [Face::Top, Face::Left, Face::Right] {
            for v in 0..n {
                for u in 0..n {
                    out.push(Texel { face, u, v, n });
                }
            }
        }
        out
    }

    #[test]
    fn a_pattern_is_a_pure_function_of_its_texel() {
        let (noise, ramp) = (Noise::new(7), ramp());
        let pattern = Pattern::Layer(vec![
            Pattern::Speckle { contrast: 0.7 },
            Pattern::Scatter {
                cell: 4,
                density: 0.4,
                size: 1,
                color: ColorSpec::Rgb([1, 2, 3]),
            },
        ]);
        for t in texels(N) {
            assert_eq!(
                pattern.sample(&noise, t, &ramp),
                pattern.sample(&noise, t, &ramp)
            );
        }
    }

    #[test]
    fn base_patterns_stay_inside_their_ramp() {
        let (noise, ramp) = (Noise::new(3), ramp());
        for pattern in [
            Pattern::Solid { index: 1 },
            Pattern::Speckle { contrast: 1.0 },
            Pattern::Blotch {
                scale: 3.0,
                contrast: 0.8,
                octaves: 1,
            },
        ] {
            for t in texels(N) {
                let c = pattern.shade(&noise, t, &ramp);
                assert!(ramp.contains(&c), "{c:?} is off-palette for {pattern:?}");
            }
        }
    }

    #[test]
    fn contrast_narrows_the_range_of_stops_used() {
        // Full contrast must reach both ends of the ramp; low contrast must
        // keep to the middle, which is what stops sand looking like static.
        let (noise, ramp) = (Noise::new(11), ramp());
        let stops = |contrast| {
            texels(N)
                .into_iter()
                .map(|t| Pattern::Speckle { contrast }.shade(&noise, t, &ramp))
                .collect::<std::collections::HashSet<_>>()
        };
        assert_eq!(stops(1.0).len(), 4);
        assert!(stops(0.2).len() < 4);
    }

    #[test]
    fn a_solid_pattern_clamps_to_the_last_stop() {
        let (noise, ramp) = (Noise::new(1), ramp());
        let t = Texel {
            face: Face::Top,
            u: 0,
            v: 0,
            n: N,
        };
        assert_eq!(Pattern::Solid { index: 0 }.shade(&noise, t, &ramp), ramp[0]);
        assert_eq!(
            Pattern::Solid { index: 99 }.shade(&noise, t, &ramp),
            ramp[3]
        );
    }

    #[test]
    fn blotches_are_continuous_across_the_cube_edges() {
        // Texels touching across the front vertical edge must differ no more
        // than texels touching within a face, or the seam would be visible.
        let noise = Noise::new(5);
        let pattern = Pattern::Blotch {
            scale: 3.0,
            contrast: 1.0,
            octaves: 1,
        };
        let ramp = ramp();
        let at = |face, u, v| pattern.shade(&noise, Texel { face, u, v, n: N }, &ramp)[0] as i32;
        let mut within = 0;
        let mut across = 0;
        for v in 0..N {
            for u in 0..N - 1 {
                within = within.max((at(Face::Left, u, v) - at(Face::Left, u + 1, v)).abs());
            }
            across = across.max((at(Face::Left, N - 1, v) - at(Face::Right, 0, v)).abs());
        }
        assert!(
            across <= within,
            "seam step {across} exceeds in-face step {within}"
        );
    }

    #[test]
    fn scatter_honours_its_density_and_never_touches_two_cells() {
        let noise = Noise::new(13);
        let (cell, density) = (4, 0.5);
        let pattern = Pattern::Scatter {
            cell,
            density,
            size: 1,
            color: ColorSpec::Rgb([0, 0, 0]),
        };
        let hits = texels(N)
            .into_iter()
            .filter(|&t| pattern.sample(&noise, t, &ramp()).is_some())
            .count();
        // One texel per populated cell, and about `density` of the cells.
        let cells = 3 * (N / cell) * (N / cell);
        let expected = f64::from(cells) * f64::from(density);
        assert!(hits > 0 && (hits as f64) < f64::from(cells));
        assert!(
            (hits as f64 - expected).abs() < expected,
            "{hits} hits vs {expected} expected"
        );
    }

    #[test]
    fn scatter_at_zero_density_places_nothing() {
        let noise = Noise::new(2);
        let pattern = Pattern::Scatter {
            cell: 4,
            density: 0.0,
            size: 1,
            color: ColorSpec::Rgb([0; 3]),
        };
        assert!(
            texels(N)
                .into_iter()
                .all(|t| pattern.sample(&noise, t, &ramp()).is_none())
        );
    }

    #[test]
    fn the_fringe_covers_the_sides_and_spares_the_top() {
        let noise = Noise::new(17);
        let pattern = Pattern::Fringe {
            depth: 4,
            jitter: 2,
            span: 3,
            ramp: Ramp::rgb([[0, 255, 0]]),
            pattern: Box::new(Pattern::Solid { index: 0 }),
        };
        let ramp = ramp();
        for t in texels(N) {
            let got = pattern.sample(&noise, t, &ramp);
            match t.face {
                Face::Top => assert_eq!(got, None, "the fringe reached the top face"),
                _ if t.v < 2 => assert_eq!(got, Some([0, 255, 0]), "gap at {t:?}"),
                _ if t.v > 6 => assert_eq!(got, None, "the fringe hangs too low at {t:?}"),
                _ => {}
            }
        }
    }

    #[test]
    fn the_fringe_does_not_step_where_two_faces_meet() {
        // The depth is one function of the band column, so the change across
        // the front edge must be no larger than the largest change within a
        // face. This is the whole reason the band coordinate exists.
        let noise = Noise::new(23);
        let (depth, jitter, span) = (4, 2, 3);
        let d = |r| fringe_depth(&noise, r, depth, jitter, span);
        let within = (0..N - 1)
            .map(|r| (d(r) - d(r + 1)).abs())
            .max()
            .unwrap_or(0);
        let across = (d(N - 1) - d(N)).abs();
        assert!(
            across <= within.max(1),
            "seam step {across} exceeds in-face step {within}"
        );
        for r in 0..2 * N {
            let bound = i64::from(depth + jitter) + 1;
            assert!((0..=bound).contains(&d(r)), "depth {} at column {r}", d(r));
        }
    }

    #[test]
    fn a_lattice_draws_the_courses_it_promises() {
        // Four rows of mortar on a 16-texel face means a line every 4 texels.
        let lines: Vec<u32> = (0..N).filter(|&v| divides(v, 4, N)).collect();
        assert_eq!(lines, vec![0, 4, 8, 12]);
        assert!((0..N).all(|v| !divides(v, 0, N)));

        let t = |u, v| Texel {
            face: Face::Left,
            u,
            v,
            n: N,
        };
        assert!(lattice(t(3, 0), 4, 0, false), "the course line is missing");
        assert!(
            !lattice(t(3, 1), 4, 0, false),
            "the course line is too thick"
        );
    }

    #[test]
    fn layers_paint_over_one_another_in_order() {
        let (noise, ramp) = (Noise::new(4), ramp());
        let t = Texel {
            face: Face::Top,
            u: 0,
            v: 0,
            n: N,
        };
        let solid = |i| Pattern::Solid { index: i };
        assert_eq!(
            Pattern::Layer(vec![solid(0), solid(2)]).shade(&noise, t, &ramp),
            ramp[2]
        );
        assert_eq!(Pattern::Layer(vec![]).shade(&noise, t, &ramp), ramp[2]);
    }

    #[test]
    fn patterns_survive_a_one_texel_cube() {
        // Every divisor and cell size must clamp rather than divide by zero.
        let (noise, ramp) = (Noise::new(8), ramp());
        let pattern = Pattern::Layer(vec![
            Pattern::Blotch {
                scale: 4.0,
                contrast: 1.0,
                octaves: 3,
            },
            Pattern::Scatter {
                cell: 8,
                density: 1.0,
                size: 4,
                color: ColorSpec::Rgb([1; 3]),
            },
            Pattern::Lattice {
                rows: 4,
                cols: 4,
                stagger: true,
                mortar: ColorSpec::Rgb([2; 3]),
            },
            Pattern::Fringe {
                depth: 2,
                jitter: 4,
                span: 0,
                ramp: Ramp::rgb([[3; 3]]),
                pattern: Box::new(Pattern::Speckle { contrast: 1.0 }),
            },
        ]);
        for n in [1, 2, 3] {
            for t in texels(n) {
                pattern.shade(&noise, t, &ramp);
            }
        }
    }
}
