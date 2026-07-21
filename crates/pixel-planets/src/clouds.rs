//! The cloud layer: realistic planetary-scale flow or cartoonish puffs.
//!
//! Clouds are sampled in the tilted-but-unrotated frame, so their motion is
//! decoupled from the surface rotation by construction: the surface spins at
//! the rotation period while clouds advect at their own latitude-dependent
//! speeds.

use std::f32::consts::{PI, TAU};

use crate::noise::{Noise, hash_to_unit, splitmix64};

/// Which cloud model to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum CloudMode {
    /// Planetary-scale flow: zonal jets shear an evolving, domain-warped
    /// noise field into bands and swirls.
    #[default]
    Realistic,
    /// Puffy flat-white metaball clusters drifting at their own speeds.
    Cartoon,
    /// A cloudless sky.
    None,
}

/// What the cloud layer contributes at one point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudSample {
    /// No cloud; the surface shows through.
    Clear,
    /// Cloud body: flat white.
    Lit,
    /// Cloud edge or underside: light grey.
    Shaded,
}

/// One cartoon cloud cluster: a few overlapping discs drifting zonally.
#[derive(Debug, Clone)]
pub struct Puff {
    /// Disc centers (unit vectors) and angular radii (radians).
    discs: Vec<([f32; 3], f32)>,
    /// Zonal angular drift speed (radians per second).
    drift: f32,
}

/// The instantiated cloud layer of one planet.
pub enum CloudField {
    /// See [`CloudMode::Realistic`].
    Realistic {
        /// The seeded density field.
        noise: Noise,
    },
    /// See [`CloudMode::Cartoon`].
    Cartoon {
        /// The baked puff clusters.
        puffs: Vec<Puff>,
    },
    /// See [`CloudMode::None`].
    None,
}

/// Rotate `v` about the Y axis by `angle` radians.
pub fn rot_y(v: [f32; 3], angle: f32) -> [f32; 3] {
    let (sin, cos) = angle.sin_cos();
    [v[0] * cos + v[2] * sin, v[1], -v[0] * sin + v[2] * cos]
}

/// The base angular speed shared by surface rotation and cloud drift.
fn base_speed(period_secs: f32) -> f32 {
    TAU / period_secs.max(f32::MIN_POSITIVE)
}

/// Latitude-dependent zonal wind: the layer leads the surface by ~15%, with
/// gentle alternating jets. The jet amplitude is kept small — the shear rate
/// within a jet is `dw/dlat · t`, so even 0.08 stretches a cloud noticeably
/// over a rotation; larger values smear the whole layer into streaks.
fn zonal_speed(lat: f32, period_secs: f32) -> f32 {
    base_speed(period_secs) * (1.15 + 0.08 * (3.0 * lat).cos())
}

impl CloudField {
    /// Instantiate the cloud layer for a seed, mode, and coverage.
    pub fn new(seed: u64, mode: CloudMode, cloudiness: f32, period_secs: f32) -> Self {
        let noise = Noise::new(seed).channel(0xC10D);
        match mode {
            CloudMode::Realistic => CloudField::Realistic { noise },
            CloudMode::Cartoon => CloudField::Cartoon {
                puffs: bake_puffs(noise.seed, cloudiness, period_secs),
            },
            CloudMode::None => CloudField::None,
        }
    }

    /// Sample the layer at unit vector `v` in the tilted (unrotated) frame,
    /// at absolute time `t` seconds. `spin` (`+1.0`/`-1.0`) is the planet's
    /// rotation direction, which the clouds drift along.
    pub fn sample(
        &self,
        v: [f32; 3],
        t: f32,
        cloudiness: f32,
        period_secs: f32,
        spin: f32,
    ) -> CloudSample {
        match self {
            CloudField::None => CloudSample::Clear,
            CloudField::Realistic { noise } => {
                let lat = v[1].clamp(-1.0, 1.0).asin();
                // Zonal advection: rotating the sample point backward in time
                // by the local wind is exactly wind-driven drift on the sphere.
                let q = rot_y(v, -spin * zonal_speed(lat, period_secs) * t);
                // Shapes evolve by drifting through the 3D field over time.
                let p = [q[0] * 3.0, q[1] * 3.0 + 0.05 * t, q[2] * 3.0];
                // fBm mass concentrates near zero; stretch it so cloud fields
                // separate cleanly from clear sky at the threshold.
                let d = 0.5 + 0.8 * noise.warped_fbm3(p, 4, 0.6);
                // Calibrated so cloudiness 0.5 covers roughly 40% of the sky.
                let cut = 0.86 - 0.6 * cloudiness;
                if d > cut + 0.06 {
                    CloudSample::Lit
                } else if d > cut {
                    CloudSample::Shaded
                } else {
                    CloudSample::Clear
                }
            }
            CloudField::Cartoon { puffs } => {
                for puff in puffs {
                    let q = rot_y(v, -spin * puff.drift * t);
                    if puff.contains(q) {
                        // Underside rim: a point slightly below (toward -y)
                        // that falls outside the cluster shades the edge.
                        let below = [q[0], q[1] - 0.05, q[2]];
                        return if puff.contains(normalize(below)) {
                            CloudSample::Lit
                        } else {
                            CloudSample::Shaded
                        };
                    }
                }
                CloudSample::Clear
            }
        }
    }
}

impl Puff {
    /// Whether unit vector `q` lies inside any disc of this cluster.
    fn contains(&self, q: [f32; 3]) -> bool {
        self.discs.iter().any(|(c, r)| dot(q, *c) > r.cos())
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt().max(f32::MIN_POSITIVE);
    v.map(|c| c / len)
}

/// Bake the cartoon puff clusters: mid-latitude biased centers, each cluster
/// 2–4 discs elongated along longitude, drifting zonally at its own speed.
fn bake_puffs(seed: u64, cloudiness: f32, period_secs: f32) -> Vec<Puff> {
    let count = (4.0 + 14.0 * cloudiness).round() as u64;
    let mut puffs = Vec::with_capacity(count as usize);
    for i in 0..count {
        let h = |tag: u64| hash_to_unit(splitmix64(seed ^ (i << 8) ^ tag));
        let lat = (h(1) - 0.5) * PI * 0.8;
        let lon = h(2) * TAU;
        let discs_n = 2 + (h(3) * 3.0) as usize; // 2..=4
        let radius = 0.06 + 0.10 * h(4);
        let mut discs = Vec::with_capacity(discs_n);
        for d in 0..discs_n {
            let hd = |tag: u64| hash_to_unit(splitmix64(seed ^ (i << 8) ^ (d as u64) << 16 ^ tag));
            // Offsets elongated along longitude for the classic silhouette.
            let dlon = (hd(5) - 0.5) * 3.0 * radius;
            let dlat = (hd(6) - 0.5) * 1.0 * radius;
            let (clat, clon) = (lat + dlat, lon + dlon);
            discs.push((
                [clat.cos() * clon.cos(), clat.sin(), clat.cos() * clon.sin()],
                radius * (0.7 + 0.6 * hd(7)),
            ));
        }
        puffs.push(Puff {
            discs,
            drift: base_speed(period_secs) * (1.2 + 0.3 * h(8)),
        });
    }
    puffs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic scatter of unit vectors.
    fn sphere_points(n: u64) -> Vec<[f32; 3]> {
        (0..n)
            .map(|i| {
                let u = hash_to_unit(splitmix64(i)) * 2.0 - 1.0;
                let lon = hash_to_unit(splitmix64(i ^ 0xF00D)) * TAU;
                let r = (1.0 - u * u).sqrt();
                [r * lon.cos(), u, r * lon.sin()]
            })
            .collect()
    }

    fn coverage(mode: CloudMode, cloudiness: f32) -> f32 {
        let field = CloudField::new(7, mode, cloudiness, 12.0);
        let points = sphere_points(2000);
        let cloudy = points
            .iter()
            .filter(|&&v| field.sample(v, 3.0, cloudiness, 12.0, 1.0) != CloudSample::Clear)
            .count();
        cloudy as f32 / points.len() as f32
    }

    #[test]
    fn none_mode_is_always_clear() {
        assert_eq!(coverage(CloudMode::None, 1.0), 0.0);
    }

    #[test]
    fn realistic_coverage_grows_with_cloudiness() {
        let (low, mid, high) = (
            coverage(CloudMode::Realistic, 0.1),
            coverage(CloudMode::Realistic, 0.5),
            coverage(CloudMode::Realistic, 0.9),
        );
        assert!(low <= mid && mid <= high, "{low} <= {mid} <= {high}");
        assert!(high > low, "coverage must respond to cloudiness");
    }

    #[test]
    fn cartoon_coverage_grows_with_cloudiness() {
        let (low, high) = (
            coverage(CloudMode::Cartoon, 0.1),
            coverage(CloudMode::Cartoon, 0.9),
        );
        assert!(high > low, "{high} > {low} violated");
    }

    #[test]
    fn cartoon_puff_count_matches_formula() {
        for cloudiness in [0.0, 0.5, 1.0] {
            let field = CloudField::new(7, CloudMode::Cartoon, cloudiness, 12.0);
            let CloudField::Cartoon { puffs } = field else {
                panic!("expected cartoon field");
            };
            assert_eq!(puffs.len(), (4.0 + 14.0 * cloudiness).round() as usize);
        }
    }

    #[test]
    fn clouds_move_over_time() {
        let field = CloudField::new(7, CloudMode::Realistic, 0.5, 12.0);
        let points = sphere_points(500);
        let changed = points
            .iter()
            .filter(|&&v| {
                field.sample(v, 0.0, 0.5, 12.0, 1.0) != field.sample(v, 3.0, 0.5, 12.0, 1.0)
            })
            .count();
        assert!(changed > 0, "cloud field must evolve between t=0 and t=3");
    }

    #[test]
    fn sampling_is_deterministic() {
        let a = CloudField::new(9, CloudMode::Realistic, 0.5, 12.0);
        let b = CloudField::new(9, CloudMode::Realistic, 0.5, 12.0);
        for v in sphere_points(200) {
            assert_eq!(
                a.sample(v, 1.5, 0.5, 12.0, 1.0),
                b.sample(v, 1.5, 0.5, 12.0, 1.0)
            );
        }
    }
}
