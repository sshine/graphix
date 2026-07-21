//! Biome classification and pixel-art palettes.
//!
//! A biome is chosen from the derived surface fields (elevation, temperature,
//! moisture) by a top-to-bottom threshold table. Each biome paints with three
//! banded colors indexed by fine elevation detail, giving the contour-like
//! shading of classic pixel-art planets.

use crate::planet::PlanetParams;

/// Macro-level surface classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Biome {
    /// Open ocean well below sea level.
    DeepOcean,
    /// Regular ocean.
    Ocean,
    /// Coastal water, host to algal blooms and bioluminescence.
    Shallows,
    /// Frozen ocean surface.
    SeaIce,
    /// Land cold enough for permanent snow.
    Snow,
    /// Cold, barren land fringing the snow line.
    Tundra,
    /// Hot or rain-starved land.
    Desert,
    /// Dry grassland between desert and greener biomes.
    Steppe,
    /// Temperate open land.
    Grassland,
    /// Temperate forest.
    Forest,
    /// Hot, wet, dense growth.
    Jungle,
    /// High-elevation rock (snow-capped when cold).
    Mountain,
}

/// Classify a surface point from elevation `e`, temperature `t`, and moisture
/// `m` (all in `0..=1`), under the planet's parameters.
pub fn classify(e: f32, t: f32, m: f32, p: &PlanetParams) -> Biome {
    let t_ice = p.ice_threshold();
    if e < p.water {
        if t < t_ice {
            Biome::SeaIce
        } else if p.water - e < 0.04 {
            Biome::Shallows
        } else if p.water - e > 0.15 {
            Biome::DeepOcean
        } else {
            Biome::Ocean
        }
    } else if e > p.water + 0.26 {
        Biome::Mountain
    } else if t < t_ice {
        Biome::Snow
    } else if t < t_ice + 0.12 {
        Biome::Tundra
    } else if m < 0.28 - 0.10 * p.vegetation {
        Biome::Desert
    } else if m < 0.42 {
        Biome::Steppe
    } else if t > 0.72 && m > 0.60 {
        Biome::Jungle
    } else if m > 0.55 + 0.15 * (1.0 - p.vegetation) {
        Biome::Forest
    } else {
        Biome::Grassland
    }
}

/// An RGB triple.
pub type Color = [u8; 3];

/// Channel-wise linear interpolation between two colors.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (f32::from(a[i]) + t * (f32::from(b[i]) - f32::from(a[i]))).round() as u8)
}

/// The sere brown vegetation fades toward as vigor drops.
const SERE: [Color; 3] = [[110, 96, 54], [128, 112, 64], [146, 128, 76]];

/// The teal an algal super-bloom pushes ocean colors toward.
pub const BLOOM_TEAL: Color = [24, 140, 108];

/// The biome's three banded colors, dark to light, adjusted by the planet's
/// vegetation vigor (greens fade toward sere brown as it drops).
pub fn palette(b: Biome, p: &PlanetParams) -> [Color; 3] {
    let sere = 1.0 - p.vegetation;
    let vegetated = |colors: [Color; 3]| [0, 1, 2].map(|i| mix(colors[i], SERE[i], sere * 0.6));
    match b {
        Biome::DeepOcean => [[8, 24, 66], [12, 32, 88], [16, 40, 104]],
        Biome::Ocean => [[18, 46, 120], [24, 58, 140], [32, 72, 158]],
        Biome::Shallows => [[52, 110, 170], [72, 140, 186], [96, 168, 200]],
        Biome::SeaIce => [[176, 196, 212], [200, 216, 228], [224, 238, 246]],
        Biome::Snow => [[200, 214, 224], [222, 232, 240], [244, 250, 255]],
        Biome::Tundra => [[112, 122, 110], [130, 140, 128], [150, 158, 146]],
        Biome::Desert => [[178, 150, 96], [198, 172, 112], [216, 192, 132]],
        Biome::Steppe => vegetated([[124, 118, 62], [140, 132, 72], [158, 148, 84]]),
        Biome::Grassland => vegetated([[74, 112, 52], [96, 140, 62], [122, 168, 76]]),
        Biome::Forest => vegetated([[38, 84, 44], [52, 104, 54], [70, 126, 64]]),
        Biome::Jungle => vegetated([[22, 72, 40], [32, 92, 48], [46, 112, 58]]),
        Biome::Mountain => [[110, 104, 100], [134, 128, 122], [160, 154, 148]],
    }
}

/// Whether a biome is water (subject to bloom tinting).
pub fn is_water(b: Biome) -> bool {
    matches!(b, Biome::DeepOcean | Biome::Ocean | Biome::Shallows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> PlanetParams {
        PlanetParams::default()
    }

    #[test]
    fn sea_and_land_split_at_water_level() {
        let p = params();
        assert!(is_water(classify(p.water - 0.01, 0.5, 0.5, &p)));
        assert!(!is_water(classify(p.water + 0.01, 0.5, 0.5, &p)));
    }

    #[test]
    fn ocean_depth_bands() {
        let p = params();
        assert_eq!(classify(p.water - 0.02, 0.5, 0.5, &p), Biome::Shallows);
        assert_eq!(classify(p.water - 0.10, 0.5, 0.5, &p), Biome::Ocean);
        assert_eq!(classify(p.water - 0.20, 0.5, 0.5, &p), Biome::DeepOcean);
    }

    #[test]
    fn cold_freezes_sea_and_land() {
        let p = params();
        let t_ice = p.ice_threshold();
        assert_eq!(
            classify(p.water - 0.10, t_ice - 0.01, 0.5, &p),
            Biome::SeaIce
        );
        assert_eq!(classify(p.water + 0.05, t_ice - 0.01, 0.5, &p), Biome::Snow);
        assert_eq!(
            classify(p.water + 0.05, t_ice + 0.05, 0.5, &p),
            Biome::Tundra
        );
    }

    #[test]
    fn high_elevation_is_mountain() {
        let p = params();
        assert_eq!(classify(p.water + 0.30, 0.5, 0.5, &p), Biome::Mountain);
    }

    #[test]
    fn moisture_gradient_dry_to_wet() {
        let p = params();
        let e = p.water + 0.05;
        assert_eq!(classify(e, 0.5, 0.10, &p), Biome::Desert);
        assert_eq!(classify(e, 0.5, 0.35, &p), Biome::Steppe);
        assert_eq!(classify(e, 0.5, 0.50, &p), Biome::Grassland);
        assert_eq!(classify(e, 0.5, 0.70, &p), Biome::Forest);
        assert_eq!(classify(e, 0.80, 0.70, &p), Biome::Jungle);
    }

    #[test]
    fn vegetation_pushes_back_deserts() {
        let lush = PlanetParams {
            vegetation: 1.0,
            ..Default::default()
        };
        let barren = PlanetParams {
            vegetation: 0.0,
            ..Default::default()
        };
        let e = lush.water + 0.05;
        // Moisture at the boundary: desert on a barren world, not on a lush one.
        assert_eq!(classify(e, 0.5, 0.24, &barren), Biome::Desert);
        assert_ne!(classify(e, 0.5, 0.24, &lush), Biome::Desert);
    }
}
