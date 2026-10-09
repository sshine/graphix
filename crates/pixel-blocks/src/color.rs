//! The RGB triple everything works in, the palette ramps patterns index into,
//! and the OKLCH conversion recipes may specify colors with.

use serde::{Deserialize, Serialize};

/// An RGB triple.
pub type Color = [u8; 3];

/// Channel-wise linear interpolation between two colors.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (f32::from(a[i]) + t * (f32::from(b[i]) - f32::from(a[i]))).round() as u8)
}

/// Scale a color toward black by `factor` in `0..=1`, where `1.0` leaves it
/// untouched. This is how the projection darkens the two side faces.
pub fn shade(color: Color, factor: f32) -> Color {
    mix(color, [0, 0, 0], 1.0 - factor.clamp(0.0, 1.0))
}

/// A color as a recipe writes it: either sRGB bytes or OKLCH, which keeps a
/// ramp's steps perceptually even when they are generated rather than picked.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ColorSpec {
    /// sRGB bytes, written in TOML as `[128, 96, 64]`.
    Rgb(Color),
    /// OKLCH, written in TOML as `{ l = 0.62, c = 0.05, h = 90.0 }`.
    Oklch {
        /// Lightness in `0..=1`.
        l: f32,
        /// Chroma; roughly `0..=0.37` before the sRGB gamut clips.
        c: f32,
        /// Hue in degrees.
        h: f32,
    },
}

impl ColorSpec {
    /// The sRGB bytes this spec denotes.
    pub fn resolve(self) -> Color {
        match self {
            ColorSpec::Rgb(c) => c,
            ColorSpec::Oklch { l, c, h } => oklch(l, c, h),
        }
    }
}

/// An ordered palette, darkest first. Patterns pick a band rather than
/// interpolating, which is what keeps the output reading as pixel art.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ramp(Vec<ColorSpec>);

impl Ramp {
    /// A ramp from color specs, darkest first.
    pub fn new(stops: impl IntoIterator<Item = ColorSpec>) -> Self {
        Ramp(stops.into_iter().collect())
    }

    /// A ramp from sRGB bytes, darkest first.
    pub fn rgb(stops: impl IntoIterator<Item = Color>) -> Self {
        Ramp::new(stops.into_iter().map(ColorSpec::Rgb))
    }

    /// The ramp evaluated to sRGB, done once per bake so patterns index a
    /// plain slice.
    pub fn resolve(&self) -> Vec<Color> {
        self.0.iter().map(|s| s.resolve()).collect()
    }

    /// How many stops the ramp has.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the ramp has no stops, which no bakeable recipe may contain.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The band of `colors` at position `t` in `0..=1`, quantized rather than
/// interpolated. An empty ramp yields black; recipes are validated before
/// baking so that case does not arise in practice.
pub fn band(colors: &[Color], t: f32) -> Color {
    if colors.is_empty() {
        return [0, 0, 0];
    }
    let i = (t.clamp(0.0, 1.0) * colors.len() as f32) as usize;
    colors[i.min(colors.len() - 1)]
}

/// Convert OKLCH (lightness in `0..=1`, chroma, hue in degrees) to 8-bit sRGB,
/// clipping per channel when the color falls outside the sRGB gamut.
pub fn oklch(l: f32, c: f32, hue_deg: f32) -> Color {
    let h = hue_deg.to_radians();
    let (a, b) = (c * h.cos(), c * h.sin());

    let lms = [
        l + 0.396_337_78 * a + 0.215_803_76 * b,
        l - 0.105_561_346 * a - 0.063_854_17 * b,
        l - 0.089_484_18 * a - 1.291_485_5 * b,
    ]
    .map(|v| v * v * v);

    let linear = [
        4.076_741_7 * lms[0] - 3.307_711_6 * lms[1] + 0.230_969_94 * lms[2],
        -1.268_438 * lms[0] + 2.609_757_4 * lms[1] - 0.341_319_38 * lms[2],
        -0.004_196_086_3 * lms[0] - 0.703_418_6 * lms[1] + 1.707_614_7 * lms[2],
    ];

    linear.map(|v| (gamma(v.clamp(0.0, 1.0)) * 255.0).round() as u8)
}

/// The sRGB transfer function, linear light to encoded value.
fn gamma(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shading_darkens_monotonically() {
        let c = [200, 150, 100];
        let sum = |c: Color| c.iter().map(|&v| u32::from(v)).sum::<u32>();
        assert_eq!(shade(c, 1.0), c);
        assert!(sum(shade(c, 0.8)) < sum(c));
        assert!(sum(shade(c, 0.6)) < sum(shade(c, 0.8)));
        assert_eq!(shade(c, 0.0), [0, 0, 0]);
    }

    #[test]
    fn every_band_of_a_ramp_is_reachable() {
        // Sweeping t across 0..1 must visit each stop and never index past the
        // end, including at t = 1.0 exactly.
        let ramp = Ramp::rgb([[0, 0, 0], [85, 85, 85], [170, 170, 170], [255, 255, 255]]);
        let colors = ramp.resolve();
        let seen: std::collections::HashSet<Color> =
            (0..=100).map(|i| band(&colors, i as f32 / 100.0)).collect();
        assert_eq!(seen.len(), 4);
        assert_eq!(band(&colors, 1.0), [255, 255, 255]);
        assert_eq!(band(&colors, -1.0), [0, 0, 0]);
    }

    #[test]
    fn an_empty_ramp_yields_black_rather_than_panicking() {
        assert_eq!(band(&[], 0.5), [0, 0, 0]);
    }

    #[test]
    fn oklch_and_rgb_specs_resolve_the_same_way() {
        assert_eq!(ColorSpec::Rgb([1, 2, 3]).resolve(), [1, 2, 3]);
        assert_eq!(
            ColorSpec::Oklch {
                l: 1.0,
                c: 0.0,
                h: 0.0
            }
            .resolve(),
            [255, 255, 255]
        );
        assert_eq!(
            ColorSpec::Oklch {
                l: 0.0,
                c: 0.0,
                h: 0.0
            }
            .resolve(),
            [0, 0, 0]
        );
    }

    #[test]
    fn a_ramp_parses_from_both_color_forms() {
        let ramp: Ramp =
            toml::from_str::<toml::Value>("v = [[10, 20, 30], { l = 0.5, c = 0.0, h = 0.0 }]")
                .and_then(|v| v["v"].clone().try_into())
                .expect("both color forms must parse");
        assert_eq!(ramp.len(), 2);
        assert_eq!(ramp.resolve()[0], [10, 20, 30]);
    }
}
