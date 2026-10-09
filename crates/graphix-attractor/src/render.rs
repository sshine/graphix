//! Projection and accumulation: the swarm turned about its system's symmetry
//! axis and splatted additively into an RGB image, which graphix then turns
//! into terminal cells.
//!
//! There is no z-buffer and no depth test. Strands are light rather than
//! solid, so a crossing should add up rather than hide what is behind it,
//! which is the same reason the original animation draws its lines with
//! additive blending. Depth survives as a fog term instead: a far strand
//! contributes less, and what it fades toward is the background of the image.

use std::f32::consts::TAU;

use graphix::image::{Rgb, RgbImage};

use crate::color::{Palette, tone_map};
use crate::swarm::Swarm;
use crate::system::{System, trace};

/// Camera distance, as a multiple of the attractor's radius. This sets how
/// strongly the near side flares: at 5 the nearest point is drawn a quarter
/// larger than the centre, enough to read as depth without the near strands
/// bursting out of the frame.
const CAMERA_MULT: f32 = 5.0;

/// The share of the half-frame the attractor's radius reaches at the centre
/// depth. The near-side flare of [`CAMERA_MULT`] eats the rest of the margin.
const FIT: f32 = 0.76;

/// How much of its brightness the furthest strand keeps.
const FOG_FLOOR: f32 = 0.25;

/// Brightness at the tail of a trail, relative to its head. The original holds
/// one opacity down the whole strand, but at this resolution a trail without a
/// gradient reads as a static loop rather than something in motion.
const TAIL_WEIGHT: f32 = 0.45;

/// Light deposited per unit length of strand before exposure.
const GAIN: f32 = 0.35;

/// How far one orbit is followed to find out how big the attractor is. A trail
/// holds a few time units, which is a fraction of a circuit; the framing has to
/// hold for every circuit, so the measurement runs far past what is on screen.
const FIT_STEPS: usize = 60_000;

/// The most sub-samples one segment is split into, so that a jump across the
/// frame cannot cost an unbounded amount of work.
const MAX_SEGMENT_SAMPLES: usize = 512;

/// Where the attractor sits and how big it is.
///
/// Measured rather than written down. These systems differ by more than a
/// factor of two in extent and none of them is centred where you would guess:
/// Thomas sits around `(1.31, 1.31, 1.31)`, not the origin. Nudged parameters
/// move both centre and radius again.
///
/// It is measured by following a single orbit rather than by reading the
/// swarm's trails. Where a system's orbits converge, the swarm at any instant
/// occupies a narrow tube nowhere near the full extent of the shape it is
/// winding around.
#[derive(Debug, Clone, Copy)]
pub struct Fit {
    /// The midpoint of the attractor's bounding box.
    pub center: [f32; 3],
    /// The furthest the attractor reaches from that midpoint.
    pub radius: f32,
}

impl Fit {
    /// Measure `system` by tracing one orbit from `start`.
    pub fn measure<S: System>(system: &S, start: [f64; 3]) -> Self {
        let fallback = Fit {
            center: [0.0; 3],
            radius: 1.0,
        };

        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for p in trace(system, start, FIT_STEPS) {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        if !lo.iter().chain(hi.iter()).all(|v| v.is_finite()) {
            return fallback;
        }

        let center = [0, 1, 2].map(|i| ((lo[i] + hi[i]) / 2.0) as f32);
        let radius = trace(system, start, FIT_STEPS)
            .map(|p| {
                let d = [0, 1, 2].map(|i| p[i] as f32 - center[i]);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .fold(0.0f32, f32::max);

        Fit {
            center,
            radius: radius.max(1e-3),
        }
    }
}

/// An orthonormal frame with the symmetry axis standing up on the screen.
///
/// The view turns about that axis rather than about a fixed coordinate, so a
/// cyclically symmetric system can be shown down its `(1, 1, 1)` diagonal,
/// where its three-fold symmetry is visible, instead of from a corner.
#[derive(Debug, Clone, Copy)]
struct Frame {
    across: [f32; 3],
    into: [f32; 3],
    up: [f32; 3],
}

impl Frame {
    fn new(axis: [f64; 3]) -> Self {
        let up = normalize([axis[0] as f32, axis[1] as f32, axis[2] as f32]);
        // Cross with whichever coordinate axis is least aligned with `up`, so
        // the product never comes out near zero. Ties go to y, which leaves an
        // axis of z giving back the plain x, y, z frame.
        let (a, b, c) = (up[0].abs(), up[1].abs(), up[2].abs());
        let helper = if b <= a && b <= c {
            [0.0, 1.0, 0.0]
        } else if a <= c {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let across = normalize(cross(helper, up));
        Frame {
            across,
            into: cross(up, across),
            up,
        }
    }
}

/// A swarm turning at a fixed rate, rendered into square frames.
///
/// This is not a pure function of time the way the `flake` renderer is: a
/// chaotic system has to be integrated in sequence, so `frame` advances the
/// swarm as well as drawing it. The sequence is still reproducible, since
/// everything follows from the seed the swarm was built with.
#[derive(Debug, Clone)]
pub struct Renderer<S> {
    swarm: Swarm<S>,
    palette: Palette,
    fit: Fit,
    view: Frame,
    size: u32,
    period: f32,
    tilt: f32,
    exposure: f32,
    steps_per_frame: u32,
    elapsed: f32,
}

impl<S: System> Renderer<S> {
    /// A renderer producing `size`x`size` frames, one revolution per `period`
    /// seconds, viewed from `tilt` degrees above the attractor's equator.
    pub fn new(swarm: Swarm<S>, palette: Palette, size: u32, period: f32, tilt: f32) -> Self {
        let start = swarm.orbits().first().map_or([0.1, 0.0, 0.0], |o| o.head());
        Renderer {
            fit: Fit::measure(swarm.system(), start),
            view: Frame::new(swarm.system().symmetry_axis()),
            swarm,
            palette,
            size: size.max(1),
            period: period.max(0.1),
            tilt: tilt.to_radians(),
            exposure: 1.0,
            steps_per_frame: 3,
            elapsed: 0.0,
        }
    }

    /// Set how hard the tone curve climbs.
    pub fn with_exposure(mut self, exposure: f32) -> Self {
        self.exposure = exposure.max(0.0);
        self
    }

    /// Set how many integration steps each frame advances by, which is what
    /// holds a system's motion to the same speed across frame rates.
    pub fn with_steps_per_frame(mut self, steps: u32) -> Self {
        self.steps_per_frame = steps.max(1);
        self
    }

    /// Seconds per revolution, which is also the animation's loop length.
    pub fn period_secs(&self) -> f32 {
        self.period
    }

    /// The swarm being drawn.
    pub fn swarm(&self) -> &Swarm<S> {
        &self.swarm
    }

    /// Where the attractor was measured to sit and how far it reaches.
    pub fn fit(&self) -> Fit {
        self.fit
    }

    /// Advance the swarm by `dt` seconds and draw it.
    pub fn frame(&mut self, dt: f32) -> RgbImage {
        for _ in 0..self.steps_per_frame {
            self.swarm.step();
        }
        self.elapsed += dt;
        self.frame_at(TAU * self.elapsed / self.period)
    }

    /// Draw the swarm where it stands, turned `angle` radians about the
    /// symmetry axis.
    pub fn frame_at(&self, angle: f32) -> RgbImage {
        let n = (self.size * self.size) as usize;
        let mut acc = vec![[0.0f32; 3]; n];
        let (cos, sin) = (angle.cos(), angle.sin());
        let (tcos, tsin) = (self.tilt.cos(), self.tilt.sin());

        for orbit in self.swarm.orbits() {
            let color = self.palette.tint(orbit.tint());
            let len = orbit.len().max(2);
            let mut previous: Option<((f32, f32), f32)> = None;

            for (i, p) in orbit.points().enumerate() {
                // Newest point first, so the weight falls off down the trail.
                let along = i as f32 / (len - 1) as f32;
                let weight = 1.0 + (TAIL_WEIGHT - 1.0) * along;

                let (vx, vy, vz) = self.view_space(p, cos, sin, tcos, tsin);
                let here = self
                    .project(vx, vy, vz)
                    .map(|at| (at, weight * self.fog(vz)));

                // The trail is drawn as the segments between its points, not
                // as the points themselves. One step carries some of these
                // systems several pixels across the frame, and plotting only
                // the points leaves every strand a dotted line.
                if let (Some((from, w0)), Some((to, w1))) = (previous, here) {
                    let amount = (w0 + w1) * 0.5 * GAIN;
                    draw_segment(&mut acc, self.size, from, to, color, amount);
                }
                previous = here;
            }
        }

        let mut img = RgbImage::new(self.size, self.size);
        for (i, px) in acc.iter().enumerate() {
            let (x, y) = (i as u32 % self.size, i as u32 / self.size);
            img.put_pixel(x, y, Rgb(px.map(|v| tone_map(v, self.exposure))));
        }
        img
    }

    /// Centre a state-space point, turn it about the symmetry axis, and tip the
    /// camera above the equator.
    fn view_space(&self, p: [f64; 3], cos: f32, sin: f32, tcos: f32, tsin: f32) -> (f32, f32, f32) {
        let d = [0, 1, 2].map(|i| p[i] as f32 - self.fit.center[i]);
        let (a, b, h) = (
            dot(d, self.view.across),
            dot(d, self.view.into),
            dot(d, self.view.up),
        );
        let (ra, rb) = (a * cos - b * sin, a * sin + b * cos);
        (ra, h * tcos - rb * tsin, h * tsin + rb * tcos)
    }

    /// Perspective-project a view-space point to pixel coordinates, or `None`
    /// when it falls behind the camera.
    fn project(&self, vx: f32, vy: f32, vz: f32) -> Option<(f32, f32)> {
        let camera = CAMERA_MULT * self.fit.radius;
        let depth = vz + camera;
        if depth <= self.fit.radius * 0.01 {
            return None;
        }
        let half = self.size as f32 / 2.0;
        let focal = half * FIT * camera / self.fit.radius;
        // Screen Y grows downward, so the up axis is negated.
        Some((half + vx * focal / depth, half - vy * focal / depth))
    }

    /// How much of its brightness a strand at view depth `vz` keeps, falling
    /// from full at the near edge to [`FOG_FLOOR`] at the far one.
    fn fog(&self, vz: f32) -> f32 {
        let r = self.fit.radius;
        let near = ((r - vz) / (2.0 * r)).clamp(0.0, 1.0);
        FOG_FLOOR + (1.0 - FOG_FLOOR) * near
    }
}

/// Draw the line between two projected points, depositing `amount` at roughly
/// one sample per pixel of its length.
///
/// Sampling by length rather than a fixed count is what keeps brightness even:
/// a strand crossing the frame quickly covers more pixels, and each of them
/// should come out as bright as the pixels under a strand that is barely
/// moving, not share one segment's light between them.
fn draw_segment(
    acc: &mut [[f32; 3]],
    size: u32,
    from: (f32, f32),
    to: (f32, f32),
    color: [f32; 3],
    amount: f32,
) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    if !length.is_finite() {
        return;
    }
    let samples = (length.ceil() as usize).clamp(1, MAX_SEGMENT_SAMPLES);

    for i in 0..samples {
        let t = (i as f32 + 0.5) / samples as f32;
        splat(acc, size, from.0 + dx * t, from.1 + dy * t, color, amount);
    }
}

/// Add `amount` of `color` at a fractional pixel position, split across the
/// four pixels around it.
///
/// Splatting bilinearly rather than rounding to the nearest pixel is what keeps
/// a strand continuous: at this resolution a rounded trail breaks into a dotted
/// line as soon as it runs near a diagonal.
fn splat(acc: &mut [[f32; 3]], size: u32, px: f32, py: f32, color: [f32; 3], amount: f32) {
    let (x0, y0) = (px.floor(), py.floor());
    let (fx, fy) = (px - x0, py - y0);

    for (dx, dy, w) in [
        (0, 0, (1.0 - fx) * (1.0 - fy)),
        (1, 0, fx * (1.0 - fy)),
        (0, 1, (1.0 - fx) * fy),
        (1, 1, fx * fy),
    ] {
        let (x, y) = (x0 as i64 + dx, y0 as i64 + dy);
        if x < 0 || y < 0 || x >= i64::from(size) || y >= i64::from(size) {
            continue;
        }
        let cell = &mut acc[(y as u32 * size + x as u32) as usize];
        for c in 0..3 {
            cell[c] += color[c] * amount * w;
        }
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        v.map(|c| c / len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit circle in the xy plane, whose extent is known exactly.
    #[derive(Clone)]
    struct Circle;

    impl System for Circle {
        fn derivative(&self, [x, y, _]: [f64; 3]) -> [f64; 3] {
            [-y, x, 0.0]
        }
        fn step(&self) -> f64 {
            0.05
        }
        fn seed_radius(&self) -> f64 {
            1.0
        }
    }

    /// The same circle, offset so its centre is nowhere near the origin.
    #[derive(Clone)]
    struct OffsetCircle;

    impl System for OffsetCircle {
        fn derivative(&self, [x, y, _]: [f64; 3]) -> [f64; 3] {
            [-(y - 7.0), x - 3.0, 0.0]
        }
        fn step(&self) -> f64 {
            0.05
        }
        fn seed_radius(&self) -> f64 {
            1.0
        }
    }

    #[test]
    fn the_fit_finds_a_known_centre_and_radius() {
        let fit = Fit::measure(&Circle, [2.0, 0.0, 0.0]);
        assert!(
            fit.center.iter().all(|c| c.abs() < 1e-2),
            "{:?}",
            fit.center
        );
        assert!((fit.radius - 2.0).abs() < 1e-2, "radius {}", fit.radius);
    }

    #[test]
    fn the_fit_centres_an_attractor_that_is_nowhere_near_the_origin() {
        // Hardcoding a centre of zero is exactly the error this guards.
        let fit = Fit::measure(&OffsetCircle, [3.0 + 2.0, 7.0, 0.0]);
        assert!((fit.center[0] - 3.0).abs() < 1e-2, "{:?}", fit.center);
        assert!((fit.center[1] - 7.0).abs() < 1e-2, "{:?}", fit.center);
        assert!((fit.radius - 2.0).abs() < 1e-2, "radius {}", fit.radius);
    }

    #[test]
    fn the_default_axis_gives_the_plain_coordinate_frame() {
        let f = Frame::new([0.0, 0.0, 1.0]);
        assert_eq!(f.up, [0.0, 0.0, 1.0]);
        assert_eq!(f.across, [1.0, 0.0, 0.0]);
        assert_eq!(f.into, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_diagonal_axis_still_gives_an_orthonormal_frame() {
        let f = Frame::new([1.0, 1.0, 1.0]);
        for v in [f.across, f.into, f.up] {
            assert!((dot(v, v) - 1.0).abs() < 1e-5, "not unit length: {v:?}");
        }
        assert!(dot(f.across, f.into).abs() < 1e-5);
        assert!(dot(f.across, f.up).abs() < 1e-5);
        assert!(dot(f.into, f.up).abs() < 1e-5);
        // Up really is the diagonal.
        let d = 1.0 / 3.0f32.sqrt();
        assert!(f.up.iter().all(|c| (c - d).abs() < 1e-5), "{:?}", f.up);
    }

    #[test]
    fn a_splat_conserves_what_it_deposits() {
        let mut acc = vec![[0.0f32; 3]; 16];
        splat(&mut acc, 4, 1.25, 1.75, [1.0, 1.0, 1.0], 1.0);
        let total: f32 = acc.iter().map(|p| p[0]).sum();
        assert!((total - 1.0).abs() < 1e-5, "deposited {total}");
        assert!(acc[4 + 1][0] > 0.0);
        assert!(acc[2 * 4 + 2][0] > 0.0);
    }

    #[test]
    fn a_splat_off_the_edge_is_dropped_rather_than_wrapped() {
        let mut acc = vec![[0.0f32; 3]; 16];
        splat(&mut acc, 4, -5.0, 2.0, [1.0, 1.0, 1.0], 1.0);
        splat(&mut acc, 4, 50.0, 2.0, [1.0, 1.0, 1.0], 1.0);
        assert!(acc.iter().all(|p| p[0] == 0.0));
    }

    #[test]
    fn a_long_segment_is_as_bright_per_pixel_as_a_short_one() {
        // The invariant that matters for a strand crossing the frame quickly:
        // a pixel fully under the line gets the same light however long the
        // line is, and the total scales with length rather than being shared
        // out along it.
        let stats = |from: (f32, f32), to: (f32, f32)| {
            let mut acc = vec![[0.0f32; 3]; 64 * 64];
            draw_segment(&mut acc, 64, from, to, [1.0, 1.0, 1.0], 1.0);
            let peak = acc.iter().map(|p| p[0]).fold(0.0f32, f32::max);
            (peak, acc.iter().map(|p| p[0]).sum::<f32>())
        };
        let (short_peak, short_total) = stats((10.0, 32.0), (14.0, 32.0));
        let (long_peak, long_total) = stats((10.0, 32.0), (54.0, 32.0));

        assert!(
            (short_peak - long_peak).abs() < 0.05,
            "peak {short_peak} vs {long_peak}"
        );
        // Eleven times the length should deposit about eleven times the light.
        let ratio = long_total / short_total;
        assert!((ratio - 11.0).abs() < 1.0, "total light ratio {ratio}");
    }
}
