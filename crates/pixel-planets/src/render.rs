//! The frame renderer: orthographic projection with a zoom camera, axial
//! tilt, surface rotation, cloud compositing, and posterized dithered
//! lighting for the pixel-art look.

use std::f32::consts::TAU;

use graphix::image::RgbImage;

use crate::biome::{Color, mix};
use crate::clouds::{CloudField, CloudMode, CloudSample, rot_y};
use crate::noise::{hash_to_unit, splitmix64};
use crate::planet::{PlanetParams, SurfaceMap};

/// Flat cloud body color.
const CLOUD_LIT: Color = [240, 244, 248];
/// Cloud edge/underside color.
const CLOUD_SHADED: Color = [190, 198, 210];
/// The dark blue night sides sink toward.
const NIGHT_BLUE: Color = [6, 8, 24];
/// Bioluminescent glow color.
const GLOW_CYAN: Color = [40, 220, 200];
/// Atmospheric haze color at the limb.
const HAZE: Color = [90, 140, 220];

/// The standard 4x4 Bayer matrix, normalized to `0..1` (multiples of 1/16).
const BAYER4: [[f32; 4]; 4] = [
    [0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0],
    [12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0],
    [3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0],
    [15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0],
];

/// How the camera zoom changes over a run.
#[derive(Debug, Clone, Copy)]
pub struct ZoomRamp {
    /// Zoom at `t = 0` (1.0 = the planet disc exactly fits the frame).
    pub from: f32,
    /// Zoom the ramp eases toward (equal to `from` for a static camera).
    pub to: f32,
    /// Seconds the ramp takes; the zoom holds at `to` afterwards.
    pub secs: f32,
}

impl ZoomRamp {
    /// A static camera at `zoom`.
    pub fn fixed(zoom: f32) -> Self {
        ZoomRamp {
            from: zoom,
            to: zoom,
            secs: 1.0,
        }
    }

    /// The zoom at time `t`: smoothstep-eased from `from` to `to`, then held.
    pub fn at(&self, t: f32) -> f32 {
        let x = (t / self.secs.max(f32::MIN_POSITIVE)).clamp(0.0, 1.0);
        let s = x * x * (3.0 - 2.0 * x);
        self.from + (self.to - self.from) * s
    }

    /// The largest zoom the ramp reaches, for sizing the surface bake.
    pub fn max_zoom(&self) -> f32 {
        self.from.max(self.to)
    }
}

/// Renders frames of one planet.
pub struct Renderer {
    /// The planet being rendered.
    pub params: PlanetParams,
    surface: SurfaceMap,
    clouds: CloudField,
    size: u32,
    period_secs: f32,
    zoom: ZoomRamp,
}

impl Renderer {
    /// Bake a planet's surface and cloud layer for rendering at `size`x`size`
    /// pixels, rotating once per `period_secs`.
    pub fn new(
        params: PlanetParams,
        mode: CloudMode,
        size: u32,
        period_secs: f32,
        zoom: ZoomRamp,
    ) -> Self {
        let surface = SurfaceMap::bake(&params, zoom.max_zoom());
        let clouds = CloudField::new(params.seed, mode, params.cloudiness, period_secs);
        Renderer {
            params,
            surface,
            clouds,
            size,
            period_secs,
            zoom,
        }
    }

    /// The rotation period in seconds.
    pub fn period_secs(&self) -> f32 {
        self.period_secs
    }

    /// Render the deterministic frame at absolute time `t` seconds.
    pub fn frame(&self, t: f32) -> RgbImage {
        let size = self.size;
        let center = size as f32 / 2.0;
        let radius = center - 1.0;
        let zoom = self.zoom.at(t);
        let tilt = self.params.tilt_deg.to_radians();
        let phase = TAU * t / self.period_secs.max(f32::MIN_POSITIVE);
        // Sun from the upper left, fixed in view space.
        let sun = normalize([-0.55, 0.35, 0.75]);

        let mut img = RgbImage::new(size, size);
        for py in 0..size {
            for px in 0..size {
                let x = (px as f32 + 0.5 - center) / radius / zoom;
                let y = -((py as f32 + 0.5 - center) / radius) / zoom;
                let r2 = x * x + y * y;
                let color = if r2 > 1.0 {
                    self.space_pixel(px, py, r2, zoom)
                } else {
                    let n = [x, y, (1.0 - r2).sqrt()];
                    self.sphere_pixel(n, r2, t, tilt, phase, sun, px, py)
                };
                img.put_pixel(px, py, graphix::image::Rgb(color));
            }
        }
        img
    }

    /// Space background: black with sparse hashed stars, and a thin haze rim
    /// just outside the planet's limb.
    fn space_pixel(&self, px: u32, py: u32, r2: f32, zoom: f32) -> Color {
        let rim = 1.0 + 2.0 / (self.size as f32 / 2.0 - 1.0) / zoom;
        if r2 < rim * rim && self.params.atmosphere > 0.0 {
            return mix([0, 0, 0], HAZE, 0.8 * self.params.atmosphere);
        }
        let h = splitmix64(self.params.seed ^ 0x57A2 ^ (u64::from(px) << 20) ^ u64::from(py));
        if hash_to_unit(h) < 0.004 {
            let b = 120 + (hash_to_unit(splitmix64(h)) * 120.0) as u8;
            return [b, b, b];
        }
        [0, 0, 0]
    }

    /// A pixel on the planet disc: surface, clouds, then lighting.
    #[allow(clippy::too_many_arguments)]
    fn sphere_pixel(
        &self,
        n: [f32; 3],
        r2: f32,
        t: f32,
        tilt: f32,
        phase: f32,
        sun: [f32; 3],
        px: u32,
        py: u32,
    ) -> Color {
        // Tilt the axis into the view plane, then unwind the surface spin.
        let v = rot_z(n, -tilt);
        let p = rot_y(v, -phase);
        let (surface, glow_mask) = self.surface.sample(p);
        let cloudiness = self.params.cloudiness;

        // Clouds live in the tilted frame: independent of surface rotation.
        let cloud = self.clouds.sample(v, t, cloudiness, self.period_secs);
        let (mut color, is_cloud) = match cloud {
            CloudSample::Lit => (CLOUD_LIT, true),
            CloudSample::Shaded => (CLOUD_SHADED, true),
            CloudSample::Clear => (surface, false),
        };

        // Cheap cast shadow: a cloud between this surface point and the sun
        // darkens it one step.
        if !is_cloud {
            let toward_sun = normalize([
                v[0] + 0.04 * sun[0],
                v[1] + 0.04 * sun[1],
                v[2] + 0.04 * sun[2],
            ]);
            if self
                .clouds
                .sample(toward_sun, t, cloudiness, self.period_secs)
                != CloudSample::Clear
            {
                color = mix(color, [0, 0, 0], 0.25);
            }
        }

        // Posterized lighting with ordered dithering.
        let mut l = dot(n, sun);
        l += (BAYER4[(py % 4) as usize][(px % 4) as usize] - 0.5) * 0.12;
        // Limb darkening: drop one band near the edge.
        if r2 > 0.90 {
            l -= 0.38;
        }
        if l > 0.40 {
            color
        } else if l > 0.02 {
            mix(color, [0, 0, 0], 0.32)
        } else if glow_mask > 0 && !is_cloud {
            // Bioluminescent shallows: the night side glows instead of dims,
            // dither-thresholded into speckles.
            let intensity = f32::from(glow_mask) / 255.0;
            if BAYER4[(py % 4) as usize][(px % 4) as usize] < intensity {
                GLOW_CYAN
            } else {
                night(color)
            }
        } else {
            night(color)
        }
    }
}

/// Night-side shading: heavily dimmed and sunk toward dark blue.
fn night(color: Color) -> Color {
    mix(mix(color, [0, 0, 0], 0.78), NIGHT_BLUE, 0.3)
}

/// Rotate `v` about the Z axis by `angle` radians.
fn rot_z(v: [f32; 3], angle: f32) -> [f32; 3] {
    let (sin, cos) = angle.sin_cos();
    [v[0] * cos - v[1] * sin, v[0] * sin + v[1] * cos, v[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt().max(f32::MIN_POSITIVE);
    v.map(|c| c / len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer(zoom: ZoomRamp) -> Renderer {
        Renderer::new(
            PlanetParams::default(),
            CloudMode::Realistic,
            64,
            12.0,
            zoom,
        )
    }

    #[test]
    fn frames_are_deterministic() {
        let r = renderer(ZoomRamp::fixed(1.0));
        assert_eq!(r.frame(1.234).into_raw(), r.frame(1.234).into_raw());
    }

    #[test]
    fn frames_change_over_time() {
        let r = renderer(ZoomRamp::fixed(1.0));
        assert_ne!(r.frame(0.0).into_raw(), r.frame(3.0).into_raw());
    }

    #[test]
    fn rotations_preserve_norm() {
        let v = normalize([0.3, -0.5, 0.8]);
        for angle in [0.0, 0.7, -2.1] {
            for r in [rot_y(v, angle), rot_z(v, angle)] {
                assert!((dot(r, r) - 1.0).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn zoomed_in_frame_has_no_space_pixels() {
        // At zoom 3 the disc overflows the frame: every pixel is on-sphere.
        // Space background is pure black (or a grey star); night-side sphere
        // pixels always carry some of the dark-blue tint, so no pixel may be
        // exactly black.
        let r = renderer(ZoomRamp::fixed(3.0));
        let img = r.frame(0.25);
        assert!(img.pixels().all(|p| p.0 != [0, 0, 0]));
    }

    #[test]
    fn zoom_ramp_eases_and_holds() {
        let ramp = ZoomRamp {
            from: 1.0,
            to: 5.0,
            secs: 4.0,
        };
        assert_eq!(ramp.at(0.0), 1.0);
        assert_eq!(ramp.at(4.0), 5.0);
        assert_eq!(ramp.at(100.0), 5.0); // holds
        let (a, b, c) = (ramp.at(1.0), ramp.at(2.0), ramp.at(3.0));
        assert!(a < b && b < c, "ramp must be monotone");
        assert!(ramp.max_zoom() == 5.0);
    }

    #[test]
    fn zoomed_out_frame_is_mostly_space() {
        let r = renderer(ZoomRamp::fixed(0.25));
        let img = r.frame(0.0);
        let dark = img
            .pixels()
            .filter(|p| p.0[0] < 32 && p.0[1] < 32 && p.0[2] < 40)
            .count();
        assert!(dark as f32 / (64.0 * 64.0) > 0.7);
    }
}
