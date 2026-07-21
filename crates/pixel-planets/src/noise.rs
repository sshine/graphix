//! Hand-rolled deterministic noise: an integer hash, 3D value noise, fractal
//! Brownian motion, and domain warping.
//!
//! Surface and cloud fields are sampled at points on the unit sphere (scaled
//! by a frequency), so 3D noise gives seamless longitude wrap and no pole
//! pinch for free. Time evolution moves the sample point through the field;
//! no 4D noise is needed.

/// SplitMix64: a fast, well-distributed 64-bit hash/PRNG step.
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Hash a seed and a 3D lattice coordinate into 64 bits.
fn hash3(seed: u64, x: i64, y: i64, z: i64) -> u64 {
    splitmix64(
        seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
    )
}

/// Map a hash to a uniform value in `[0, 1)`.
pub fn hash_to_unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Quintic fade `t³(t(6t − 15) + 10)`: zero first and second derivative at
/// the lattice points, so the trilinear blend has no visible grid creases.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Linear interpolation.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A seeded noise field.
#[derive(Debug, Clone, Copy)]
pub struct Noise {
    /// The seed defining this field.
    pub seed: u64,
}

impl Noise {
    /// A noise field seeded with `seed`.
    pub fn new(seed: u64) -> Self {
        Noise { seed }
    }

    /// Derive an independent noise field for a separate purpose (elevation
    /// vs. moisture vs. clouds) from the same planet seed.
    pub fn channel(self, tag: u64) -> Self {
        Noise {
            seed: splitmix64(self.seed ^ tag),
        }
    }

    /// The lattice value in `[-1, 1]` at integer coordinates.
    fn lattice(&self, x: i64, y: i64, z: i64) -> f32 {
        hash_to_unit(hash3(self.seed, x, y, z)) * 2.0 - 1.0
    }

    /// 3D value noise in `[-1, 1]`: trilinear interpolation of hashed lattice
    /// corner values with a quintic fade.
    pub fn value3(&self, p: [f32; 3]) -> f32 {
        let base = p.map(|c| c.floor());
        let [x0, y0, z0] = base.map(|c| c as i64);
        let [tx, ty, tz] = [
            fade(p[0] - base[0]),
            fade(p[1] - base[1]),
            fade(p[2] - base[2]),
        ];
        let corner = |dx: i64, dy: i64, dz: i64| self.lattice(x0 + dx, y0 + dy, z0 + dz);
        let face = |dz: i64| {
            lerp(
                lerp(corner(0, 0, dz), corner(1, 0, dz), tx),
                lerp(corner(0, 1, dz), corner(1, 1, dz), tx),
                ty,
            )
        };
        lerp(face(0), face(1), tz)
    }

    /// Fractal Brownian motion: `octaves` layers of [`value3`](Self::value3),
    /// each `lacunarity` times finer and `gain` times fainter, normalized so
    /// the sum stays in `[-1, 1]`.
    pub fn fbm3(&self, p: [f32; 3], octaves: u32, lacunarity: f32, gain: f32) -> f32 {
        let mut sum = 0.0;
        let mut total = 0.0;
        let mut freq = 1.0;
        let mut amp = 1.0;
        for octave in 0..octaves {
            // Each octave gets its own channel so layers don't correlate.
            let layer = self.channel(0x0C7A_0000 + u64::from(octave));
            sum += amp * layer.value3(p.map(|c| c * freq));
            total += amp;
            freq *= lacunarity;
            amp *= gain;
        }
        sum / total.max(f32::MIN_POSITIVE)
    }

    /// Domain-warped fBm: offset the sample point by a vector of three fBm
    /// evaluations before sampling again. Produces the swirled, sheared look
    /// of planetary cloud fields.
    pub fn warped_fbm3(&self, p: [f32; 3], octaves: u32, strength: f32) -> f32 {
        let warp = self.channel(0xAB0F_F5E7);
        let q = [
            warp.channel(1).fbm3(p, 3, 2.0, 0.5),
            warp.channel(2).fbm3(p, 3, 2.0, 0.5),
            warp.channel(3).fbm3(p, 3, 2.0, 0.5),
        ];
        self.fbm3(
            [
                p[0] + strength * q[0],
                p[1] + strength * q[1],
                p[2] + strength * q[2],
            ],
            octaves,
            2.0,
            0.5,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic scatter of sample points covering a few lattice cells.
    fn sample_points() -> Vec<[f32; 3]> {
        let mut points = Vec::new();
        for i in 0..200 {
            let u = hash_to_unit(splitmix64(i));
            let v = hash_to_unit(splitmix64(i ^ 0xFACE));
            let w = hash_to_unit(splitmix64(i ^ 0xBEEF));
            points.push([u * 8.0 - 4.0, v * 8.0 - 4.0, w * 8.0 - 4.0]);
        }
        points
    }

    #[test]
    fn same_seed_is_deterministic() {
        let (a, b) = (Noise::new(42), Noise::new(42));
        for p in sample_points() {
            assert_eq!(a.value3(p), b.value3(p));
            assert_eq!(a.fbm3(p, 4, 2.0, 0.5), b.fbm3(p, 4, 2.0, 0.5));
            assert_eq!(a.warped_fbm3(p, 4, 0.6), b.warped_fbm3(p, 4, 0.6));
        }
    }

    #[test]
    fn different_seeds_differ() {
        let (a, b) = (Noise::new(1), Noise::new(2));
        assert!(sample_points().iter().any(|&p| a.value3(p) != b.value3(p)));
    }

    #[test]
    fn channels_are_independent() {
        let n = Noise::new(7);
        let (a, b) = (n.channel(1), n.channel(2));
        assert_ne!(a.seed, b.seed);
        assert!(sample_points().iter().any(|&p| a.value3(p) != b.value3(p)));
    }

    #[test]
    fn outputs_stay_in_range() {
        let n = Noise::new(99);
        for p in sample_points() {
            for v in [
                n.value3(p),
                n.fbm3(p, 5, 2.0, 0.5),
                n.warped_fbm3(p, 4, 0.6),
            ] {
                assert!((-1.0..=1.0).contains(&v), "{v} out of range at {p:?}");
            }
        }
    }

    #[test]
    fn noise_is_continuous_across_lattice_edges() {
        // Sampling just below and above an integer coordinate should agree
        // closely (fade has zero derivative at the lattice).
        let n = Noise::new(5);
        for &[x, y, _] in &sample_points()[..50] {
            let lo = n.value3([x, y, 2.0 - 1e-4]);
            let hi = n.value3([x, y, 2.0 + 1e-4]);
            assert!((lo - hi).abs() < 1e-2, "discontinuity: {lo} vs {hi}");
        }
    }
}
