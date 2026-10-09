//! A set of orbits of one system, each keeping the trail of its recent path.

use graphix_noise::{hash_to_unit, splitmix64};

use crate::system::{System, advance};

/// Past this an orbit has left for infinity and its trail is reseeded. Both
/// systems here are bounded, so this only catches parameters tuned off the CLI.
const ESCAPE: f64 = 1e4;

/// One orbit, with the trail of recent points behind it.
///
/// The trail is a ring buffer: the head moves backwards through it so that
/// reading forwards from the cursor walks newest point to oldest, which is the
/// order the renderer fades along. Shifting every point back one slot per tick
/// instead, the way the original animation does, is a few hundred megabytes of
/// copying per second and puts a long warm-up out of reach.
#[derive(Debug, Clone)]
pub struct Orbit {
    trail: Vec<[f64; 3]>,
    cursor: usize,
    tint: f32,
}

impl Orbit {
    /// Where this orbit sits on the palette, in `0..=1`. Fixed per orbit, so a
    /// strand keeps its color for as long as it lives.
    pub fn tint(&self) -> f32 {
        self.tint
    }

    /// The trail from the newest point to the oldest.
    pub fn points(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        (0..self.trail.len()).map(move |i| self.trail[(self.cursor + i) % self.trail.len()])
    }

    /// The point the orbit has reached.
    pub fn head(&self) -> [f64; 3] {
        self.trail[self.cursor]
    }

    /// How many points the trail holds.
    pub fn len(&self) -> usize {
        self.trail.len()
    }

    /// Whether the trail holds nothing, which a constructed orbit never does.
    pub fn is_empty(&self) -> bool {
        self.trail.is_empty()
    }

    /// Collapse the whole trail onto `point`, so no segment is drawn across the
    /// gap between an old position and a fresh one.
    fn reseed(&mut self, point: [f64; 3]) {
        self.trail.fill(point);
        self.cursor = 0;
    }
}

/// Orbits of one system, advanced together.
#[derive(Debug, Clone)]
pub struct Swarm<S> {
    system: S,
    orbits: Vec<Orbit>,
    rng: u64,
}

impl<S: System> Swarm<S> {
    /// `count` orbits of `system`, each carrying a trail of `trail` points,
    /// started from `seed` and advanced `warmup` steps.
    ///
    /// The warm-up has to outlast the trail, or the first frame shows strands
    /// still collapsed on their starting points. How much further it should run
    /// depends on the system: where orbits separate, more warm-up only spreads
    /// them better, but where they converge it ends with every strand tracing
    /// the same curve.
    pub fn new(system: S, count: usize, trail: usize, seed: u64, warmup: usize) -> Self {
        let trail = trail.max(2);
        let radius = system.seed_radius();
        let mut rng = seed;
        let mut next = move || {
            rng = splitmix64(rng);
            rng
        };

        let orbits = (0..count.max(1))
            .map(|_| {
                let start = [(); 3].map(|()| f64::from(hash_to_unit(next()) - 0.5) * 2.0 * radius);
                Orbit {
                    trail: vec![start; trail],
                    cursor: 0,
                    tint: hash_to_unit(next()),
                }
            })
            .collect();

        let mut swarm = Swarm {
            system,
            orbits,
            rng: next(),
        };
        for _ in 0..warmup {
            swarm.step();
        }
        swarm
    }

    /// The orbits, for the renderer to walk.
    pub fn orbits(&self) -> &[Orbit] {
        &self.orbits
    }

    /// The system being integrated.
    pub fn system(&self) -> &S {
        &self.system
    }

    /// Advance every orbit one step.
    pub fn step(&mut self) {
        for orbit in &mut self.orbits {
            let next = advance(&self.system, orbit.trail[orbit.cursor]);

            if next.iter().any(|v| !v.is_finite() || v.abs() > ESCAPE) {
                self.rng = splitmix64(self.rng);
                let mut h = self.rng;
                let radius = self.system.seed_radius();
                let fresh = [(); 3].map(|()| {
                    h = splitmix64(h);
                    f64::from(hash_to_unit(h) - 0.5) * 2.0 * radius
                });
                orbit.reseed(fresh);
                continue;
            }

            // Walking forwards from the cursor reads newest to oldest, so the
            // new head belongs one slot behind it.
            orbit.cursor = (orbit.cursor + orbit.trail.len() - 1) % orbit.trail.len();
            orbit.trail[orbit.cursor] = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Integrator;

    /// A system whose orbits run away, for exercising the escape guard.
    #[derive(Clone)]
    struct Runaway;

    impl System for Runaway {
        fn derivative(&self, p: [f64; 3]) -> [f64; 3] {
            p.map(|v| v * 4.0)
        }
        fn step(&self) -> f64 {
            0.1
        }
        fn integrator(&self) -> Integrator {
            Integrator::Euler
        }
        fn seed_radius(&self) -> f64 {
            1.0
        }
    }

    /// Plain rotation, which stays put and is easy to reason about.
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

    #[test]
    fn a_trail_runs_newest_to_oldest() {
        let mut swarm = Swarm::new(Circle, 1, 8, 1, 10);
        let before = swarm.orbits()[0].head();
        swarm.step();
        let after: Vec<_> = swarm.orbits()[0].points().collect();
        assert_eq!(after[0], swarm.orbits()[0].head());
        assert_eq!(
            after[1], before,
            "the previous head should follow the new one"
        );
        assert_ne!(after[0], before);
    }

    #[test]
    fn a_fresh_trail_is_collapsed_onto_its_starting_point() {
        let swarm = Swarm::new(Circle, 4, 16, 7, 0);
        for orbit in swarm.orbits() {
            let points: Vec<_> = orbit.points().collect();
            assert!(
                points.iter().all(|p| *p == points[0]),
                "an unwarmed trail should be a single point repeated"
            );
        }
    }

    #[test]
    fn seeds_are_drawn_from_the_system_s_own_ball() {
        let swarm = Swarm::new(Circle, 64, 4, 3, 0);
        for orbit in swarm.orbits() {
            // Each coordinate is drawn independently, so the bound is per axis.
            assert!(orbit.head().iter().all(|v| v.abs() <= Circle.seed_radius()));
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_swarm() {
        let a = Swarm::new(Circle, 4, 32, 99, 30);
        let b = Swarm::new(Circle, 4, 32, 99, 30);
        let c = Swarm::new(Circle, 4, 32, 100, 30);
        assert_eq!(a.orbits()[0].head(), b.orbits()[0].head());
        assert_ne!(a.orbits()[0].head(), c.orbits()[0].head());
    }

    #[test]
    fn an_escaped_orbit_is_pulled_back_rather_than_left_as_nan() {
        let mut swarm = Swarm::new(Runaway, 4, 16, 3, 0);
        for _ in 0..2000 {
            swarm.step();
        }
        assert!(
            swarm
                .orbits()
                .iter()
                .flat_map(|o| o.points())
                .all(|p| p.iter().all(|v| v.is_finite())),
            "the escape guard let a non-finite point through"
        );
    }

    #[test]
    fn a_trail_is_as_long_as_asked_and_never_shorter_than_two() {
        assert_eq!(Swarm::new(Circle, 1, 300, 1, 0).orbits()[0].len(), 300);
        assert_eq!(Swarm::new(Circle, 1, 0, 1, 0).orbits()[0].len(), 2);
    }
}
