//! # thomas
//!
//! The Thomas cyclically symmetric attractor traced in the terminal (via
//! [`graphix`] and [`graphix_attractor`]): a swarm of orbits each trailing its
//! recent path, turning about the body diagonal the system's symmetry lies on.
//!
//! ```sh
//! thomas                      # live, turning attractor
//! thomas --still              # one frame
//! thomas --b 0.1 --tilt 20    # looser damping, seen from nearer the equator
//! thomas --gif thomas.gif     # one revolution, looping
//! ```
//!
//! Where the Aizawa demo next door draws a system whose orbits converge, this
//! one is chaotic: strands separate, so they stay distinct however long the
//! warm-up runs and the picture is the denser for it.

use graphix_attractor::cli::Defaults;
use graphix_attractor::system::System;

/// The Thomas system, cyclically symmetric in x, y and z.
///
/// ```text
/// dx/dt = sin(y) - b x
/// dy/dt = sin(z) - b y
/// dz/dt = sin(x) - b z
/// ```
///
/// One parameter, a damping term: at `b` 0 the orbit never contracts, and as
/// `b` rises the attractor shrinks toward the origin and the motion goes
/// periodic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thomas {
    pub b: f64,
}

impl Default for Thomas {
    fn default() -> Self {
        Thomas { b: Thomas::CHAOTIC }
    }
}

impl Thomas {
    /// The damping the demo runs at, measuring a largest Lyapunov exponent of
    /// about 0.036.
    ///
    /// Not the 0.208186 the system is usually quoted with. That value sits
    /// right at the onset of chaos and measures about 0.009, near enough to
    /// periodic that the strands stay bunched and the picture comes out as a
    /// single thin loop rather than a filled lattice.
    pub const CHAOTIC: f64 = 0.1998;
}

impl System for Thomas {
    fn derivative(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        [
            y.sin() - self.b * x,
            z.sin() - self.b * y,
            x.sin() - self.b * z,
        ]
    }

    fn step(&self) -> f64 {
        0.03
    }

    // The integrator is left at the default of RK4, which this system needs.
    // Under forward Euler the measured largest Lyapunov exponent swings from
    // -0.002 at a step of 0.01 to +0.052 at 0.03, while RK4 holds 0.034 to
    // 0.039 across the same range. The bounding box barely moves either way,
    // so the shape alone does not give the problem away: what Euler gets wrong
    // is how fast neighbouring orbits separate, which is the one thing the
    // strands in this picture are showing.

    /// A ball wide enough to clear the fixed point at the origin. An orbit
    /// started too near it has nothing to push it away and sits there.
    fn seed_radius(&self) -> f64 {
        2.0
    }

    /// The body diagonal. Cycling x into y into z leaves the system unchanged,
    /// so the diagonal is its three-fold symmetry axis; turning the view about
    /// anything else shows the lattice from a corner.
    fn symmetry_axis(&self) -> [f64; 3] {
        [1.0, 1.0, 1.0]
    }
}

/// Orbit count, trail length and view this system reads best at.
///
/// The warm-up is generous, unlike the Aizawa demo's. This system is chaotic,
/// so a longer one only spreads the strands further across the attractor.
pub const DEFAULTS: Defaults = Defaults {
    orbits: 180,
    trail: 900,
    warmup_extra: 8000,
    period: 24.0,
    // Near enough to looking straight down the diagonal for the three-fold
    // symmetry to read, but off it far enough that the turn still shows depth
    // rather than spinning the picture in its own plane.
    tilt: 80.0,
    exposure: 1.2,
    steps: 2,
};

#[cfg(test)]
mod tests {
    use super::*;
    use graphix_attractor::system::Integrator;
    use graphix_attractor::{Fit, Swarm, trace};

    /// The largest Lyapunov exponent, by Benettin renormalisation: follow a
    /// second orbit a hair away from the first and average how fast the gap
    /// grows, pulling it back to its starting size each step.
    ///
    /// Both durations are in time units rather than steps, because comparing
    /// two step sizes means comparing the same stretch of the orbit. Counting
    /// steps instead would measure a different span of the attractor for each
    /// and the exponents would not be comparable. This exponent is small, so
    /// the average needs a long run to hold still.
    fn lyapunov<S: System>(system: &S, start: [f64; 3], warmup: f64, measure: f64) -> f64 {
        let steps = |t: f64| (t / system.step()).round() as usize;
        let mut a = trace(system, start, steps(warmup)).last().unwrap_or(start);
        let mut b = a;
        let d0 = 1e-9;
        b[0] += d0;

        let n = steps(measure);
        let mut sum = 0.0;
        for _ in 0..n {
            a = graphix_attractor::advance(system, a);
            b = graphix_attractor::advance(system, b);
            let gap = (0..3).map(|i| (b[i] - a[i]).powi(2)).sum::<f64>().sqrt();
            sum += (gap / d0).ln();
            for i in 0..3 {
                b[i] = a[i] + (b[i] - a[i]) * (d0 / gap);
            }
        }
        sum / (n as f64 * system.step())
    }

    #[test]
    fn the_system_is_cyclically_symmetric() {
        // Cycling the coordinates should cycle the derivative with them.
        let t = Thomas::default();
        let [a, b, c] = t.derivative([0.3, -1.2, 2.5]);
        let [a2, b2, c2] = t.derivative([-1.2, 2.5, 0.3]);
        assert!((a2 - b).abs() < 1e-12, "{a2} vs {b}");
        assert!((b2 - c).abs() < 1e-12, "{b2} vs {c}");
        assert!((c2 - a).abs() < 1e-12, "{c2} vs {a}");
    }

    #[test]
    fn the_origin_is_a_fixed_point_and_the_seed_ball_clears_it() {
        assert_eq!(Thomas::default().derivative([0.0; 3]), [0.0; 3]);
        // Seeds are drawn per axis from +/- this, so a ball of 0.5 like the
        // Aizawa demo's would leave orbits stranded on the fixed point.
        assert!(Thomas::default().seed_radius() >= 2.0);
    }

    #[test]
    fn it_runs_on_rk4() {
        assert_eq!(Thomas::default().integrator(), Integrator::Rk4);
    }

    #[test]
    fn the_chosen_damping_is_chaotic_and_the_usual_one_is_barely_so() {
        // 0.208186 is the value normally quoted; this is why it is not used.
        let at = |b| lyapunov(&Thomas { b }, [1.1, 1.1, -0.01], 300.0, 1500.0);
        let chosen = at(Thomas::CHAOTIC);
        let quoted = at(0.208186);
        assert!(chosen > 0.025, "chosen damping measured {chosen}");
        assert!(quoted < chosen / 2.0, "quoted {quoted} vs chosen {chosen}");
    }

    #[test]
    fn rk4_keeps_its_exponent_as_the_step_changes_where_euler_does_not() {
        /// The same system forced onto a given step and integrator.
        struct Forced(f64, Integrator);
        impl System for Forced {
            fn derivative(&self, p: [f64; 3]) -> [f64; 3] {
                Thomas::default().derivative(p)
            }
            fn step(&self) -> f64 {
                self.0
            }
            fn integrator(&self) -> Integrator {
                self.1
            }
            fn seed_radius(&self) -> f64 {
                2.0
            }
        }

        let at = |h, i| lyapunov(&Forced(h, i), [1.1, 1.1, -0.01], 300.0, 1500.0);
        let rk4 = (at(0.03, Integrator::Rk4), at(0.01, Integrator::Rk4));
        let euler = (at(0.03, Integrator::Euler), at(0.01, Integrator::Euler));

        let spread = |p: (f64, f64)| (p.0 - p.1).abs();
        assert!(
            spread(rk4) < 0.012,
            "rk4 should hold its exponent across steps: {rk4:?}"
        );
        assert!(
            spread(euler) > 3.0 * spread(rk4),
            "euler was expected to disagree far more: euler {euler:?}, rk4 {rk4:?}"
        );
    }

    #[test]
    fn the_measured_extent_is_centred_away_from_the_origin() {
        // The attractor sits around (1.31, 1.31, 1.31), so a renderer that
        // assumed the origin would frame it off to one side.
        let fit = Fit::measure(&Thomas::default(), [1.1, 1.1, -0.01]);
        for c in fit.center {
            assert!((1.0..1.7).contains(&c), "centre {:?}", fit.center);
        }
        assert!((3.4..4.1).contains(&fit.radius), "radius {}", fit.radius);
    }

    #[test]
    fn the_long_warm_up_leaves_the_strands_spread_out() {
        // The point of using a long warm-up on a chaotic system: orbits that
        // started close together end up far apart.
        let trail = 64;
        let swarm = Swarm::new(
            Thomas::default(),
            24,
            trail,
            5,
            trail + DEFAULTS.warmup_extra,
        );
        let heads: Vec<_> = swarm.orbits().iter().map(|o| o.head()).collect();
        let mut total = 0.0;
        let mut pairs = 0;
        for (i, a) in heads.iter().enumerate() {
            for b in &heads[i + 1..] {
                total += (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt();
                pairs += 1;
            }
        }
        let mean = total / f64::from(pairs);
        assert!(mean > 1.5, "strands only {mean} apart on average");
    }
}
