//! What a system has to provide to be traced, and the integrators that advance
//! it.
//!
//! State is held in `f64` throughout. A fixed-step integrator accumulates its
//! own error at every step and an orbit here runs for tens of thousands of
//! steps without ever being reset, so the integration is given the wider type
//! and only the projection narrows to `f32`.

/// Which fixed-step integrator a system is advanced with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrator {
    /// Forward Euler: one derivative per step, and error that grows with it.
    ///
    /// Worth reaching for only where a system has been checked to tolerate it.
    /// The check that matters is not whether the picture stays bounded but
    /// whether the largest Lyapunov exponent holds steady as the step changes.
    /// Where it does not, the separation on screen is the integrator's error
    /// rather than the system's own, and the strands are telling a lie.
    Euler,
    /// Classical fourth-order Runge-Kutta: four derivatives per step.
    Rk4,
}

/// A three-dimensional continuous-time system with an attractor worth drawing.
pub trait System {
    /// The velocity at a point in state space.
    fn derivative(&self, p: [f64; 3]) -> [f64; 3];

    /// The integration step.
    fn step(&self) -> f64;

    /// The integrator this system needs at that step.
    fn integrator(&self) -> Integrator {
        Integrator::Rk4
    }

    /// Radius of the ball starting points are drawn from.
    ///
    /// It has to clear whatever sits at the origin. Several of these systems
    /// have a fixed point there, and an orbit seeded too near one never
    /// reaches the attractor at all.
    fn seed_radius(&self) -> f64;

    /// The axis the view turns about, which is wherever the system's symmetry
    /// lies: the third coordinate for most, the `(1, 1, 1)` diagonal for the
    /// cyclically symmetric ones. It need not be normalized.
    fn symmetry_axis(&self) -> [f64; 3] {
        [0.0, 0.0, 1.0]
    }
}

/// Advance `p` by one step of `system`.
pub fn advance<S: System + ?Sized>(system: &S, p: [f64; 3]) -> [f64; 3] {
    let h = system.step();
    match system.integrator() {
        Integrator::Euler => {
            let k = system.derivative(p);
            offset(p, k, h)
        }
        Integrator::Rk4 => {
            let k1 = system.derivative(p);
            let k2 = system.derivative(offset(p, k1, h / 2.0));
            let k3 = system.derivative(offset(p, k2, h / 2.0));
            let k4 = system.derivative(offset(p, k3, h));
            [0, 1, 2].map(|i| p[i] + h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
        }
    }
}

/// Walk one orbit from `start`, yielding each point as it is reached.
///
/// Separate from the swarm, which carries trails and a reseeding guard. This is
/// for callers that want the shape of the attractor rather than something to
/// draw, such as measuring how far it reaches.
pub fn trace<S: System>(
    system: &S,
    start: [f64; 3],
    steps: usize,
) -> impl Iterator<Item = [f64; 3]> + '_ {
    let mut p = start;
    (0..steps).map(move |_| {
        p = advance(system, p);
        p
    })
}

/// `p` moved `h` along `d`.
fn offset(p: [f64; 3], d: [f64; 3], h: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| p[i] + h * d[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Circular motion in the xy plane at unit rate, whose exact solution is
    /// known, so an integrator's error can be read against it.
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

    struct EulerCircle;

    impl System for EulerCircle {
        fn derivative(&self, p: [f64; 3]) -> [f64; 3] {
            Circle.derivative(p)
        }
        fn step(&self) -> f64 {
            0.05
        }
        fn integrator(&self) -> Integrator {
            Integrator::Euler
        }
        fn seed_radius(&self) -> f64 {
            1.0
        }
    }

    /// Distance from the origin after one full turn of the unit circle.
    fn radius_after_a_turn<S: System>(system: &S) -> f64 {
        let steps = (std::f64::consts::TAU / system.step()).round() as usize;
        let end = trace(system, [1.0, 0.0, 0.0], steps)
            .last()
            .expect("a turn is more than one step");
        (end[0] * end[0] + end[1] * end[1]).sqrt()
    }

    #[test]
    fn rk4_holds_a_circle_and_euler_spirals_out() {
        // Euler always steps along the tangent, so it leaves the circle it is
        // meant to be following. This is the error that shows up as a false
        // Lyapunov exponent on a system where the orbits matter.
        let rk4 = radius_after_a_turn(&Circle);
        let euler = radius_after_a_turn(&EulerCircle);
        assert!((rk4 - 1.0).abs() < 1e-6, "rk4 left the circle at {rk4}");
        assert!(euler > 1.07, "euler only reached {euler}");
    }

    #[test]
    fn rk4_converges_far_faster_than_euler_as_the_step_shrinks() {
        struct Scaled(f64, Integrator);
        impl System for Scaled {
            fn derivative(&self, p: [f64; 3]) -> [f64; 3] {
                Circle.derivative(p)
            }
            fn step(&self) -> f64 {
                self.0
            }
            fn integrator(&self) -> Integrator {
                self.1
            }
            fn seed_radius(&self) -> f64 {
                1.0
            }
        }

        let err = |h: f64, i: Integrator| (radius_after_a_turn(&Scaled(h, i)) - 1.0).abs();
        // Halving the step should cut Euler's error about in half and RK4's by
        // about sixteen; allow plenty of slack and just check the orders differ.
        let euler_ratio = err(0.1, Integrator::Euler) / err(0.05, Integrator::Euler);
        let rk4_ratio = err(0.1, Integrator::Rk4) / err(0.05, Integrator::Rk4);
        assert!(euler_ratio < 4.0, "euler ratio {euler_ratio}");
        assert!(rk4_ratio > 8.0, "rk4 ratio {rk4_ratio}");
    }

    #[test]
    fn a_trace_yields_one_point_per_step() {
        assert_eq!(trace(&Circle, [1.0, 0.0, 0.0], 17).count(), 17);
    }

    #[test]
    fn the_default_axis_is_the_third_coordinate() {
        assert_eq!(Circle.symmetry_axis(), [0.0, 0.0, 1.0]);
    }
}
