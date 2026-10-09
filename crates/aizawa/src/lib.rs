//! # aizawa
//!
//! The Aizawa attractor traced in the terminal (via [`graphix`] and
//! [`graphix_attractor`]): the system behind the background animation on
//! zensical.org, a swarm of orbits each trailing its recent path, turning
//! about the attractor's axis.
//!
//! ```sh
//! aizawa                            # live, turning attractor
//! aizawa --preset published --still # one frame of the chaotic parameter set
//! aizawa --tilt 70 --orbits 200     # viewed from above, with more strands
//! aizawa --gif aizawa.gif           # one revolution, looping
//! ```
//!
//! The default parameters are the ones the zensical.org hero runs on, which
//! are not the ones the system is usually published with, and are not chaotic:
//! see [`Params::ZENSICAL`]. Pass `--preset published` for the strange
//! attractor proper.

use graphix_attractor::cli::Defaults;
use graphix_attractor::system::{Integrator, System};

/// The six parameters of the Aizawa system.
///
/// ```text
/// dx/dt = (z - beta) x - delta y
/// dy/dt = delta x + (z - beta) y
/// dz/dt = gamma + alpha z - z^3/3 - (x^2 + y^2)(1 + epsilon z) + zeta z x^3
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub delta: f64,
    pub epsilon: f64,
    pub zeta: f64,
}

/// Which set of parameters to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Preset {
    /// The values the zensical.org hero animation runs on.
    #[default]
    Zensical,
    /// The values the system is normally published with.
    Published,
}

impl Preset {
    /// The parameters this preset stands for.
    pub fn params(self) -> Params {
        match self {
            Preset::Zensical => Params::ZENSICAL,
            Preset::Published => Params::PUBLISHED,
        }
    }
}

impl Params {
    /// The parameters behind the zensical.org hero animation.
    ///
    /// Epsilon is 0.0625 rather than the 0.25 that site's own readout prints:
    /// it divides epsilon by the same factor it divides the step size by, so
    /// the running system sits at a quarter of the stated value. The quirk is
    /// kept because reproducing how the animation looks is the point.
    ///
    /// At these values the largest Lyapunov exponent measures zero, so this is
    /// a quasiperiodic orbit rather than a strange attractor: trails wind round
    /// a torus and slowly converge on one another instead of separating. Use
    /// [`Params::PUBLISHED`] for the chaotic one.
    pub const ZENSICAL: Params = Params {
        alpha: 0.95,
        beta: 0.2,
        gamma: 0.7,
        delta: 3.5,
        epsilon: 0.0625,
        zeta: 0.1,
    };

    /// The parameters the Aizawa system is normally quoted with, whose largest
    /// Lyapunov exponent measures about 0.089. Trails separate rather than
    /// converging, so the swarm stays spread across the attractor.
    pub const PUBLISHED: Params = Params {
        alpha: 0.95,
        beta: 0.7,
        gamma: 0.6,
        delta: 3.5,
        epsilon: 0.25,
        zeta: 0.1,
    };
}

impl System for Params {
    fn derivative(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        [
            (z - self.beta) * x - self.delta * y,
            self.delta * x + (z - self.beta) * y,
            self.gamma + self.alpha * z
                - z * z * z / 3.0
                - (x * x + y * y) * (1.0 + self.epsilon * z)
                + self.zeta * z * x * x * x,
        ]
    }

    /// The step the zensical.org hero advances by once per animation frame at
    /// 60fps. The demo takes several steps per terminal frame to hold that
    /// speed at a lower frame rate.
    fn step(&self) -> f64 {
        0.05 / 4.0
    }

    /// Euler, because it is what the original runs and this system tolerates
    /// it: the largest Lyapunov exponent keeps its value as the step changes,
    /// so what moves on screen is the system rather than the integrator's
    /// error.
    fn integrator(&self) -> Integrator {
        Integrator::Euler
    }

    fn seed_radius(&self) -> f64 {
        0.5
    }
}

/// Orbit count, trail length and view this system reads best at.
///
/// The warm-up is kept short on purpose. At the default preset the orbits
/// converge on one another, so running it longer leaves every strand tracing
/// the same curve and the swarm reads as a single wire.
pub const DEFAULTS: Defaults = Defaults {
    orbits: 120,
    trail: 1024,
    warmup_extra: 256,
    period: 16.0,
    tilt: 25.0,
    exposure: 1.8,
    steps: 3,
};

#[cfg(test)]
mod tests {
    use super::*;
    use graphix_attractor::{Fit, Swarm, trace};

    #[test]
    fn the_published_preset_differs_from_the_site_in_beta_gamma_and_epsilon() {
        let site = Preset::Zensical.params();
        let book = Preset::Published.params();
        assert_eq!(site.alpha, book.alpha);
        assert_eq!(site.delta, book.delta);
        assert_eq!(site.zeta, book.zeta);
        assert_ne!(site.beta, book.beta);
        assert_ne!(site.gamma, book.gamma);
        // The site runs epsilon at a quarter of the value it displays.
        assert_eq!(site.epsilon, book.epsilon / 4.0);
    }

    #[test]
    fn the_origin_is_not_a_fixed_point() {
        // gamma alone drives z off zero, which is what lets orbits seeded near
        // the origin reach the attractor at all.
        let d = Params::ZENSICAL.derivative([0.0, 0.0, 0.0]);
        assert_eq!([d[0], d[1]], [0.0, 0.0]);
        assert!(d[2] > 0.0);
    }

    #[test]
    fn the_xy_plane_rotates_at_delta() {
        // With z at beta the first two components reduce to a plain rotation,
        // which is the term that makes the attractor read as a spiral.
        let p = Params::ZENSICAL;
        let d = p.derivative([1.0, 0.0, p.beta]);
        assert!(d[0].abs() < 1e-12);
        assert!((d[1] - p.delta).abs() < 1e-12);
    }

    #[test]
    fn both_presets_stay_bounded() {
        for preset in [Preset::Zensical, Preset::Published] {
            let reach = trace(&preset.params(), [0.1, 0.0, 0.0], 60_000)
                .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
                .fold(0.0f64, f64::max);
            assert!(
                reach.is_finite() && reach < 5.0,
                "{preset:?} reached radius {reach}, which is off the attractor"
            );
        }
    }

    #[test]
    fn the_measured_extent_matches_what_the_system_reaches() {
        // Guards against anyone swapping the measurement for a constant: the
        // two presets differ by more than half again in how far they reach.
        let extent = |preset: Preset| Fit::measure(&preset.params(), [0.1, 0.0, 0.0]).radius;
        let site = extent(Preset::Zensical);
        let book = extent(Preset::Published);
        assert!((2.1..2.6).contains(&site), "zensical radius {site}");
        assert!((1.4..1.9).contains(&book), "published radius {book}");
        assert!(site > book * 1.2, "{site} should exceed {book}");
    }

    #[test]
    fn the_warm_up_leaves_every_orbit_on_the_attractor() {
        let trail = 128;
        let swarm = Swarm::new(
            Params::ZENSICAL,
            16,
            trail,
            7,
            trail + DEFAULTS.warmup_extra,
        );
        for orbit in swarm.orbits() {
            let [x, y, z] = orbit.head();
            // The seed ball has radius 0.5 and the attractor is wider than
            // that, so a point still inside it never left the start.
            assert!(
                (x * x + y * y + z * z).sqrt() > Params::ZENSICAL.seed_radius(),
                "an orbit is still sitting in the seed ball at {x},{y},{z}"
            );
        }
    }
}
