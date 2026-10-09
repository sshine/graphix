//! The ember palette the strands are tinted from, and the tone curve that
//! turns accumulated light into pixel values.

/// The two colors orbits are tinted between.
///
/// The defaults are the pair the zensical.org hero uses, which is also the
/// gradient on that page's call-to-action button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The warm end, reached at tint 0.
    pub amber: [u8; 3],
    /// The cool end, reached at tint 1.
    pub rose: [u8; 3],
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            amber: [0xFF, 0xC1, 0x20],
            rose: [0xFF, 0x8A, 0x8C],
        }
    }
}

impl Palette {
    /// The color at `tint`, clamped to `0..=1`.
    pub fn tint(&self, tint: f32) -> [f32; 3] {
        let t = tint.clamp(0.0, 1.0);
        [0, 1, 2].map(|i| {
            let (a, b) = (f32::from(self.amber[i]), f32::from(self.rose[i]));
            (a + (b - a) * t) / 255.0
        })
    }
}

/// Map accumulated light to an 8-bit channel value.
///
/// Strands are drawn additively and cross one another thousands of times, so
/// the buffer runs far past 1.0 wherever the attractor folds over itself.
/// Clipping there would flatten every crossing into the same white; this curve
/// is asymptotic instead, so a busy region keeps getting brighter without ever
/// running out of headroom, and `exposure` sets how fast it climbs.
pub fn tone_map(value: f32, exposure: f32) -> u8 {
    let v = 1.0 - (-value * exposure.max(0.0)).exp();
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tint_runs_from_one_end_to_the_other() {
        let p = Palette::default();
        let to_bytes = |c: [f32; 3]| c.map(|v| (v * 255.0).round() as u8);
        assert_eq!(to_bytes(p.tint(0.0)), p.amber);
        assert_eq!(to_bytes(p.tint(1.0)), p.rose);
    }

    #[test]
    fn the_tint_clamps_outside_the_unit_range() {
        let p = Palette::default();
        assert_eq!(p.tint(-5.0), p.tint(0.0));
        assert_eq!(p.tint(5.0), p.tint(1.0));
    }

    #[test]
    fn the_midpoint_sits_between_the_two_ends() {
        let p = Palette::default();
        let mid = p.tint(0.5);
        // Green is what separates the two colors; red is equal in both.
        assert!(mid[1] > p.rose[1] as f32 / 255.0);
        assert!(mid[1] < p.amber[1] as f32 / 255.0);
    }

    #[test]
    fn tone_mapping_is_monotone_and_never_clips_early() {
        assert_eq!(tone_map(0.0, 1.0), 0);
        let mut last = 0;
        for i in 1..200 {
            let v = tone_map(i as f32 * 0.05, 1.0);
            assert!(v >= last, "tone curve went backwards at {i}");
            last = v;
        }
        assert_eq!(last, 255);
    }

    #[test]
    fn more_exposure_is_brighter() {
        assert!(tone_map(0.2, 4.0) > tone_map(0.2, 1.0));
    }
}
