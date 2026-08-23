//! The accent palette: two OKLCH colors of one hue, alternating between the
//! wedges, plus the OKLCH-to-sRGB conversion they need.

/// Which terminal background the palette is tuned for. High OKLCH lightness
/// pops against a dark background but washes out on white, so each theme gets
/// its own lightness pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// The two colors the wedges alternate between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub dark: [u8; 3],
    pub light: [u8; 3],
}

impl Palette {
    /// The palette for `hue` (in degrees), tuned for `theme`.
    pub fn new(hue: f32, theme: Theme) -> Self {
        let (dark_l, light_l) = match theme {
            Theme::Dark => (0.62, 0.90),
            Theme::Light => (0.45, 0.70),
        };
        Palette {
            dark: oklch(dark_l, 0.18, hue),
            light: oklch(light_l, 0.16, hue),
        }
    }

    /// The color of the `wedge`-th lambda around the hub.
    pub fn wedge(&self, wedge: u32) -> [u8; 3] {
        if wedge.is_multiple_of(2) {
            self.dark
        } else {
            self.light
        }
    }
}

/// Convert OKLCH (lightness in `0..=1`, chroma, hue in degrees) to 8-bit sRGB,
/// clipping per channel when the color falls outside the sRGB gamut.
pub fn oklch(l: f32, c: f32, hue_deg: f32) -> [u8; 3] {
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
    fn achromatic_lightness_is_gray() {
        let [r, g, b] = oklch(1.0, 0.0, 0.0);
        assert_eq!([r, g, b], [255, 255, 255]);
        assert_eq!(oklch(0.0, 0.0, 0.0), [0, 0, 0]);
    }

    #[test]
    fn nixos_blue_reads_as_blue() {
        let [r, g, b] = oklch(0.62, 0.18, 250.0);
        assert!(b > g && g > r, "expected a blue ramp, got {r},{g},{b}");
    }

    #[test]
    fn wedges_alternate_between_the_two_colors() {
        let palette = Palette::new(250.0, Theme::Dark);
        assert_eq!(palette.wedge(0), palette.dark);
        assert_eq!(palette.wedge(1), palette.light);
        assert_eq!(palette.wedge(2), palette.dark);
        assert_ne!(palette.dark, palette.light);
    }

    #[test]
    fn the_light_theme_darkens_both_colors() {
        let dark = Palette::new(250.0, Theme::Dark);
        let light = Palette::new(250.0, Theme::Light);
        let sum = |c: [u8; 3]| c.iter().map(|&v| u32::from(v)).sum::<u32>();
        assert!(sum(light.dark) < sum(dark.dark));
        assert!(sum(light.light) < sum(dark.light));
    }
}
