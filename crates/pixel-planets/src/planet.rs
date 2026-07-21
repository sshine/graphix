//! The planet model: macro-ecosystem parameters, derived surface fields, and
//! the baked equirectangular surface texture.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::Error;
use crate::biome::{self, Biome};
use crate::noise::{Noise, hash_to_unit, splitmix64};

/// Noise channel tags for the independent surface fields.
const CH_ELEVATION: u64 = 1;
const CH_MOISTURE: u64 = 2;
const CH_BLOOM: u64 = 3;
const CH_SPIN: u64 = 4;
const CH_TILT: u64 = 5;

/// The maximum magnitude of a seed-derived axial tilt, in degrees.
const MAX_SEED_TILT: f32 = 35.0;

/// The macro-ecosystem of a habitable planet.
///
/// A planet is stateless: the seed plus these parameters fully determine it.
/// Terraforming is rerunning with different values — every field here earns
/// its place by visibly changing the rendering.
#[derive(Debug, Clone)]
pub struct PlanetParams {
    /// Seed for all procedural fields.
    pub seed: u64,
    /// Sea level in `0..=1`: the elevation below which the surface floods.
    pub water: f32,
    /// Global temperature bias in `-1..=1` (negative cools, positive warms).
    pub temp: f32,
    /// Baseline atmospheric moisture in `0..=1`.
    pub humidity: f32,
    /// Plant vigor in `0..=1`: palette lushness and how far deserts recede.
    pub vegetation: f32,
    /// Polar ice cap extent in `0..=1`.
    pub ice: f32,
    /// Cloud coverage in `0..=1`.
    pub cloudiness: f32,
    /// Atmospheric haze strength at the limb, in `0..=1`.
    pub atmosphere: f32,
    /// Algal super-bloom in `0..=1`: oceans tint teal in swirling patches.
    pub bloom: f32,
    /// Bioluminescent shallows in `0..=1`: coastal water glows cyan on the
    /// night side, revealed as the planet rotates past the terminator.
    pub glow: f32,
    /// Axial tilt override in degrees (view plane). `None` derives a random
    /// tilt from the seed (in ±35°); `Some(d)` forces `d`.
    pub tilt: Option<f32>,
}

impl Default for PlanetParams {
    fn default() -> Self {
        PlanetParams {
            seed: 42,
            water: 0.60,
            temp: 0.0,
            humidity: 0.5,
            vegetation: 0.6,
            ice: 0.3,
            cloudiness: 0.5,
            atmosphere: 0.5,
            bloom: 0.0,
            glow: 0.0,
            tilt: None,
        }
    }
}

impl PlanetParams {
    /// Check every parameter against its documented range.
    pub fn validate(&self) -> Result<(), Error> {
        let unit = [
            ("water", self.water),
            ("humidity", self.humidity),
            ("vegetation", self.vegetation),
            ("ice", self.ice),
            ("cloudiness", self.cloudiness),
            ("atmosphere", self.atmosphere),
            ("bloom", self.bloom),
            ("glow", self.glow),
        ];
        for (name, value) in unit {
            if !(0.0..=1.0).contains(&value) {
                return Err(Error::InvalidParam {
                    name,
                    value,
                    range: "0..=1",
                });
            }
        }
        if !(-1.0..=1.0).contains(&self.temp) {
            return Err(Error::InvalidParam {
                name: "temp",
                value: self.temp,
                range: "-1..=1",
            });
        }
        if let Some(tilt) = self.tilt
            && !(-90.0..=90.0).contains(&tilt)
        {
            return Err(Error::InvalidParam {
                name: "tilt",
                value: tilt,
                range: "-90..=90 degrees",
            });
        }
        Ok(())
    }

    /// The effective axial tilt in degrees: the override if set, otherwise a
    /// seed-derived value in ±35°.
    pub fn tilt_deg(&self) -> f32 {
        self.tilt.unwrap_or_else(|| {
            (hash_to_unit(splitmix64(self.seed ^ CH_TILT)) * 2.0 - 1.0) * MAX_SEED_TILT
        })
    }

    /// The temperature below which water freezes and land snows over; a
    /// larger ice parameter pushes the caps toward the equator.
    ///
    /// Temperature falls as `1 − |lat_n|^1.6` (ignoring elevation and bias),
    /// so the cap edge sits where that equals the threshold. The range below
    /// puts the default (`ice = 0.3`) cap edge near 66° latitude — a visible
    /// polar cap — and reaches roughly 33° at `ice = 1.0` for a near-snowball.
    pub fn ice_threshold(&self) -> f32 {
        0.2 + 0.6 * self.ice
    }

    /// The planet's root noise field.
    pub fn noise(&self) -> Noise {
        Noise::new(self.seed)
    }

    /// Rotation direction, derived from the seed: `+1.0` for prograde
    /// (eastward) or `-1.0` for retrograde. The clouds follow the surface.
    pub fn spin(&self) -> f32 {
        if splitmix64(self.seed ^ CH_SPIN) & 1 == 0 {
            1.0
        } else {
            -1.0
        }
    }

    /// Elevation in roughly `0..=1` at unit-sphere point `p`.
    pub fn elevation(&self, p: [f32; 3]) -> f32 {
        let n = self.noise().channel(CH_ELEVATION);
        0.5 + 0.5 * n.fbm3(p.map(|c| c * 2.2), 5, 2.0, 0.5)
    }

    /// Temperature in `0..=1` at latitude `lat` (radians) and elevation `e`:
    /// hot equator, cold poles and mountains, shifted by the global bias.
    pub fn temperature(&self, lat: f32, e: f32) -> f32 {
        let lat_n = lat / FRAC_PI_2;
        (1.0 - lat_n.abs().powf(1.6) - 0.35 * (e - self.water).max(0.0) + 0.5 * self.temp)
            .clamp(0.0, 1.0)
    }

    /// Moisture in `0..=1` at unit-sphere point `p`.
    pub fn moisture(&self, p: [f32; 3]) -> f32 {
        let n = self.noise().channel(CH_MOISTURE);
        (0.5 + 0.3 * n.fbm3(p.map(|c| c * 3.1), 4, 2.0, 0.5) + 0.5 * (self.humidity - 0.5))
            .clamp(0.0, 1.0)
    }

    /// The patchy algal-bloom mask in `0..=1` at unit-sphere point `p`:
    /// zero outside bloom patches, ramping up inside them.
    pub fn bloom_mask(&self, p: [f32; 3]) -> f32 {
        if self.bloom <= 0.0 {
            return 0.0;
        }
        let n = self.noise().channel(CH_BLOOM);
        let d = 0.5 + 0.5 * n.fbm3(p.map(|c| c * 1.4), 3, 2.0, 0.5);
        ((d - 0.5) * 4.0).clamp(0.0, 1.0)
    }
}

/// A baked equirectangular surface texture: the planet's surface is static,
/// so all per-point noise is evaluated once here and per-frame surface cost
/// becomes a single nearest-neighbor lookup.
pub struct SurfaceMap {
    width: u32,
    height: u32,
    /// Final palette color per texel, row-major from the north pole.
    color: Vec<biome::Color>,
    /// Bioluminescence intensity per texel (shallows only, scaled by glow).
    glow: Vec<u8>,
}

impl SurfaceMap {
    /// Bake the surface at a resolution suited to `max_zoom`: 512 texels of
    /// width per zoom unit (clamped to 4096), plus one extra fBm octave per
    /// resolution doubling so close orbits show real detail.
    pub fn bake(params: &PlanetParams, max_zoom: f32) -> Self {
        let width = (512.0 * max_zoom.max(1.0).ceil()).min(4096.0) as u32;
        let height = width / 2;
        let extra_octaves = (width / 512).ilog2();
        let n_elev = params.noise().channel(CH_ELEVATION);
        let mut color = Vec::with_capacity((width * height) as usize);
        let mut glow = Vec::with_capacity((width * height) as usize);
        for ty in 0..height {
            // Texel-center latitude, +pi/2 at the top row.
            let lat = FRAC_PI_2 - PI * (ty as f32 + 0.5) / height as f32;
            for tx in 0..width {
                let lon = TAU * (tx as f32 + 0.5) / width as f32;
                let p = [lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin()];
                let e = 0.5 + 0.5 * n_elev.fbm3(p.map(|c| c * 2.2), 5 + extra_octaves, 2.0, 0.5);
                let t = params.temperature(lat, e);
                let m = params.moisture(p);
                let b = biome::classify(e, t, m, params);
                let bands = biome::palette(b, params);
                // Banded contour shading from the fine elevation detail.
                let idx = ((e * 6.0).fract() * 3.0) as usize;
                let mut c = bands[idx.min(2)];
                if biome::is_water(b) {
                    c = biome::mix(c, biome::BLOOM_TEAL, params.bloom * params.bloom_mask(p));
                }
                color.push(c);
                glow.push(if b == Biome::Shallows {
                    (params.glow * 255.0) as u8
                } else {
                    0
                });
            }
        }
        SurfaceMap {
            width,
            height,
            color,
            glow,
        }
    }

    /// Nearest-neighbor sample at unit-sphere point `p` (planet frame):
    /// the texel color and its bioluminescence intensity.
    pub fn sample(&self, p: [f32; 3]) -> (biome::Color, u8) {
        let lon = p[2].atan2(p[0]).rem_euclid(TAU);
        let lat = p[1].clamp(-1.0, 1.0).asin();
        let tx = ((lon / TAU * self.width as f32) as u32).min(self.width - 1);
        let ty = (((FRAC_PI_2 - lat) / PI * self.height as f32) as u32).min(self.height - 1);
        let i = (ty * self.width + tx) as usize;
        (self.color[i], self.glow[i])
    }

    /// Fraction of texels that are water-colored, for tests: counted during
    /// bake would be cleaner, but sampling the glow/color arrays suffices.
    #[cfg(test)]
    fn ocean_fraction(params: &PlanetParams) -> f32 {
        let mut sea = 0u32;
        let mut total = 0u32;
        let map = SurfaceMap::bake(params, 1.0);
        for ty in 0..map.height {
            let lat = FRAC_PI_2 - PI * (ty as f32 + 0.5) / map.height as f32;
            for tx in 0..map.width {
                let lon = TAU * (tx as f32 + 0.5) / map.width as f32;
                let p = [lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin()];
                total += 1;
                if params.elevation(p) < params.water {
                    sea += 1;
                }
            }
        }
        sea as f32 / total as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_params_validate() {
        assert!(PlanetParams::default().validate().is_ok());
    }

    #[test]
    fn spin_is_seed_derived_and_both_directions_occur() {
        let spin_of = |seed| {
            PlanetParams {
                seed,
                ..Default::default()
            }
            .spin()
        };
        // Deterministic per seed, and always ±1.
        for seed in 0..64 {
            assert_eq!(spin_of(seed), spin_of(seed));
            assert!(spin_of(seed).abs() == 1.0);
        }
        // Both prograde and retrograde planets exist across seeds.
        let spins: Vec<f32> = (0..64).map(spin_of).collect();
        assert!(spins.contains(&1.0) && spins.contains(&-1.0));
    }

    #[test]
    fn out_of_range_params_are_rejected() {
        let p = PlanetParams {
            water: 1.5,
            ..Default::default()
        };
        assert!(matches!(
            p.validate(),
            Err(Error::InvalidParam { name: "water", .. })
        ));
        let p = PlanetParams {
            temp: -2.0,
            ..Default::default()
        };
        assert!(matches!(
            p.validate(),
            Err(Error::InvalidParam { name: "temp", .. })
        ));
    }

    #[test]
    fn temperature_hot_equator_cold_poles() {
        let p = PlanetParams::default();
        assert!(p.temperature(0.0, p.water) > p.temperature(FRAC_PI_2, p.water));
        // Mountains are colder than lowlands at the same latitude.
        assert!(p.temperature(0.5, p.water + 0.3) < p.temperature(0.5, p.water));
    }

    #[test]
    fn ocean_fraction_grows_with_water_level() {
        let fraction = |water: f32| {
            let p = PlanetParams {
                water,
                ..Default::default()
            };
            SurfaceMap::ocean_fraction(&p)
        };
        let (low, mid, high) = (fraction(0.3), fraction(0.5), fraction(0.7));
        assert!(low < mid && mid < high, "{low} < {mid} < {high} violated");
    }

    #[test]
    fn glow_only_on_shallows_and_only_when_enabled() {
        let dark = SurfaceMap::bake(&PlanetParams::default(), 1.0);
        assert!(dark.glow.iter().all(|&g| g == 0));

        let p = PlanetParams {
            glow: 1.0,
            ..Default::default()
        };
        let lit = SurfaceMap::bake(&p, 1.0);
        assert!(lit.glow.iter().any(|&g| g > 0));
        // Glow texels must be shallows-colored (not deep ocean, not land):
        // spot-check that glowing texels carry the shallows palette hues.
        let shallows = biome::palette(Biome::Shallows, &p);
        for (c, g) in lit.color.iter().zip(&lit.glow) {
            if *g > 0 {
                assert!(shallows.contains(c), "glow on non-shallows texel {c:?}");
            }
        }
    }

    #[test]
    fn sample_round_trips_poles_and_seam() {
        let map = SurfaceMap::bake(&PlanetParams::default(), 1.0);
        // Poles and the lon = 0/tau seam must not panic or go out of bounds.
        for p in [
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, -1e-7],
        ] {
            let _ = map.sample(p);
        }
    }

    #[test]
    fn bake_resolution_scales_with_zoom() {
        let base = SurfaceMap::bake(&PlanetParams::default(), 1.0);
        let close = SurfaceMap::bake(&PlanetParams::default(), 3.0);
        let orbit = SurfaceMap::bake(&PlanetParams::default(), 100.0);
        assert_eq!(base.width, 512);
        assert_eq!(close.width, 1536);
        assert_eq!(orbit.width, 4096); // clamped
    }
}
