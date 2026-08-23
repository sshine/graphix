//! Projection and rasterization: the flake spun about its vertical axis and
//! z-buffered into an RGB image, which graphix then turns into terminal cells.

use std::f32::consts::TAU;

use graphix::image::{Rgb, RgbImage};

use crate::color::Palette;
use crate::geometry::{self, Face, Point3, R_OUTER};

/// Camera distance along +Z, which sets how strongly the near wedges flare.
const CAMERA_DIST: f32 = 50.0;

/// The share of the half-frame that the outer radius reaches head-on.
const FIT: f32 = 0.72;

/// A corner after the spin, projected to pixel coordinates. `ooz` is the
/// reciprocal depth, which doubles as the z-buffer key (larger is nearer) and
/// interpolates linearly across the projected triangle.
#[derive(Debug, Clone, Copy)]
struct Vertex {
    x: f32,
    y: f32,
    ooz: f32,
}

/// A flake spinning at a fixed rate, rendered into square frames.
#[derive(Debug, Clone)]
pub struct Renderer {
    faces: Vec<Face>,
    palette: Palette,
    size: u32,
    period: f32,
    tilt: f32,
}

impl Renderer {
    /// A renderer producing `size`x`size` frames, one revolution per `period`
    /// seconds, about a spin axis tipped `tilt` degrees toward the camera.
    ///
    /// The flake is flat and the vertical spin axis lies in its plane, so at
    /// `tilt` 0 it passes edge-on twice per revolution. Tipping the axis out of
    /// the view plane is what keeps a face turned toward the camera throughout;
    /// at 90 degrees the axis points straight at the viewer and the flake turns
    /// like a pinwheel.
    pub fn new(palette: Palette, size: u32, period: f32, tilt: f32) -> Self {
        Renderer {
            faces: geometry::faces(),
            palette,
            size: size.max(1),
            period: period.max(0.1),
            tilt: tilt.to_radians(),
        }
    }

    /// Seconds per revolution, which is also the animation's loop length.
    pub fn period_secs(&self) -> f32 {
        self.period
    }

    /// The frame at `t` seconds.
    pub fn frame(&self, t: f32) -> RgbImage {
        self.frame_at(TAU * t / self.period)
    }

    /// The frame with the flake spun `angle` radians about the vertical axis.
    pub fn frame_at(&self, angle: f32) -> RgbImage {
        let mut img = RgbImage::new(self.size, self.size);
        let mut zbuf = vec![f32::NEG_INFINITY; (self.size * self.size) as usize];
        let (cos, sin) = (angle.cos(), angle.sin());
        let (tcos, tsin) = (self.tilt.cos(), self.tilt.sin());

        for face in &self.faces {
            let spun = face.corners.map(|p| {
                // Spinning about a tipped axis, as untilt, spin, retilt: the
                // spin stays a plain rotation about the vertical.
                let (y, z) = (p.y * tcos + p.z * tsin, p.z * tcos - p.y * tsin);
                let (x, z) = (p.x * cos - z * sin, p.x * sin + z * cos);
                Point3 {
                    x,
                    y: y * tcos - z * tsin,
                    z: y * tsin + z * tcos,
                }
            });
            let color = shade(self.palette.wedge(face.wedge), &spun);
            let v = spun.map(|p| project(p, self.size));
            fill_triangle(&mut img, &mut zbuf, [v[0], v[1], v[2]], color);
            fill_triangle(&mut img, &mut zbuf, [v[0], v[2], v[3]], color);
        }
        img
    }
}

/// Perspective-project a spun point into pixel coordinates.
fn project(p: Point3, size: u32) -> Vertex {
    let half = size as f32 / 2.0;
    let focal = half * FIT * CAMERA_DIST / R_OUTER;
    let ooz = 1.0 / (p.z + CAMERA_DIST);
    Vertex {
        x: half + p.x * focal * ooz,
        y: half + p.y * focal * ooz,
        ooz,
    }
}

/// Flat-shade a face. The Lambert term against the camera direction separates
/// a prism's caps from its sides; the depth term dims the wedges that have
/// swung to the back, which is the cue the original spinner ran on alone.
fn shade(color: [u8; 3], corners: &[Point3; 4]) -> Rgb<u8> {
    let depth = corners.iter().map(|p| p.z).sum::<f32>() / 4.0;
    let depth = ((depth + R_OUTER) / (2.0 * R_OUTER)).clamp(0.0, 1.0);
    let intensity = (0.30 + 0.50 * facing(corners) + 0.30 * depth).clamp(0.0, 1.0);
    Rgb(color.map(|c| (f32::from(c) * intensity).round() as u8))
}

/// How squarely a face points at the camera, in `0..=1`: the magnitude of its
/// unit normal's Z component.
fn facing(c: &[Point3; 4]) -> f32 {
    let (ux, uy, uz) = (c[1].x - c[0].x, c[1].y - c[0].y, c[1].z - c[0].z);
    let (vx, vy, vz) = (c[3].x - c[0].x, c[3].y - c[0].y, c[3].z - c[0].z);
    let (nx, ny, nz) = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx);
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if len == 0.0 { 0.0 } else { (nz / len).abs() }
}

/// Twice the signed area of the triangle `a`, `b`, `(px, py)`.
fn edge(a: Vertex, b: Vertex, px: f32, py: f32) -> f32 {
    (b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x)
}

/// The pixel range a triangle covers, clipped to `0..limit`, or `None` when it
/// falls outside entirely.
fn span(lo: f32, hi: f32, limit: u32) -> Option<(u32, u32)> {
    if hi < 0.0 || lo >= limit as f32 {
        return None;
    }
    Some((
        lo.floor().max(0.0) as u32,
        (hi.ceil() as u32).min(limit - 1),
    ))
}

/// Fill a projected triangle, keeping the nearest fragment per pixel. Dividing
/// the barycentric weights by the signed area makes the test winding-agnostic.
fn fill_triangle(img: &mut RgbImage, zbuf: &mut [f32], v: [Vertex; 3], color: Rgb<u8>) {
    let area = edge(v[0], v[1], v[2].x, v[2].y);
    if area.abs() < f32::EPSILON {
        return;
    }
    let (w, h) = img.dimensions();
    let extent = |f: fn(&Vertex) -> f32| {
        v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(f(p)), hi.max(f(p)))
        })
    };
    let (x0, x1) = extent(|p| p.x);
    let (y0, y1) = extent(|p| p.y);
    let (Some((x0, x1)), Some((y0, y1))) = (span(x0, x1, w), span(y0, y1, h)) else {
        return;
    };

    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let b0 = edge(v[1], v[2], px, py) / area;
            let b1 = edge(v[2], v[0], px, py) / area;
            let b2 = 1.0 - b0 - b1;
            if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                continue;
            }
            let ooz = b0 * v[0].ooz + b1 * v[1].ooz + b2 * v[2].ooz;
            let idx = (y * w + x) as usize;
            if ooz > zbuf[idx] {
                zbuf[idx] = ooz;
                img.put_pixel(x, y, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Theme;

    fn renderer(size: u32) -> Renderer {
        Renderer::new(Palette::new(250.0, Theme::Dark), size, 5.0, 0.0)
    }

    fn tilted(size: u32, tilt: f32) -> Renderer {
        Renderer::new(Palette::new(250.0, Theme::Dark), size, 5.0, tilt)
    }

    /// Share of non-black pixels, i.e. how much of the frame the flake covers.
    fn coverage(img: &RgbImage) -> f32 {
        let lit = img.pixels().filter(|p| p.0 != [0, 0, 0]).count();
        lit as f32 / (img.width() * img.height()) as f32
    }

    #[test]
    fn a_frame_is_square_and_substantially_covered() {
        let img = renderer(128).frame(0.0);
        assert_eq!(img.dimensions(), (128, 128));
        let covered = coverage(&img);
        assert!(
            (0.05..0.5).contains(&covered),
            "the flake covers {covered} of the frame"
        );
    }

    #[test]
    fn the_frame_border_stays_clear() {
        let img = renderer(128).frame_at(0.0);
        let (w, h) = img.dimensions();
        for x in 0..w {
            assert_eq!(img.get_pixel(x, 0).0, [0, 0, 0]);
            assert_eq!(img.get_pixel(x, h - 1).0, [0, 0, 0]);
        }
        for y in 0..h {
            assert_eq!(img.get_pixel(0, y).0, [0, 0, 0]);
            assert_eq!(img.get_pixel(w - 1, y).0, [0, 0, 0]);
        }
    }

    #[test]
    fn the_spin_advances_and_comes_back_around() {
        let r = renderer(64);
        let differing =
            |a: &RgbImage, b: &RgbImage| a.pixels().zip(b.pixels()).filter(|(p, q)| p != q).count();
        let start = r.frame(0.0);
        assert!(differing(&start, &r.frame(r.period_secs() / 8.0)) > 100);
        // A full revolution lands back on the first frame, bar the odd pixel
        // where a rounded-off angle tips a coverage test the other way.
        assert!(differing(&start, &r.frame(r.period_secs())) < 20);
    }

    #[test]
    fn tipping_the_axis_keeps_the_quarter_turn_from_going_edge_on() {
        let quarter = TAU / 4.0;
        let flat = coverage(&renderer(128).frame_at(quarter));
        let tipped = coverage(&tilted(128, 55.0).frame_at(quarter));
        assert!(tipped > flat * 2.0, "flat {flat} vs tipped {tipped}");
    }

    #[test]
    fn a_quarter_tipped_axis_is_a_pinwheel() {
        // Spinning about the view direction only turns the flake in place, so
        // every phase covers the same area.
        let r = tilted(128, 90.0);
        let a = coverage(&r.frame_at(0.0));
        let b = coverage(&r.frame_at(TAU / 4.0));
        assert!((a - b).abs() < 0.01, "coverage {a} vs {b}");
    }

    #[test]
    fn six_fold_symmetry_leaves_coverage_unchanged() {
        let r = renderer(128);
        let a = coverage(&r.frame_at(0.0));
        let b = coverage(&r.frame_at(TAU / 2.0));
        assert!(
            (a - b).abs() < 0.02,
            "coverage {a} vs {b} after a half turn"
        );
    }

    #[test]
    fn a_degenerate_triangle_is_skipped() {
        let mut img = RgbImage::new(4, 4);
        let mut zbuf = vec![f32::NEG_INFINITY; 16];
        let v = Vertex {
            x: 2.0,
            y: 2.0,
            ooz: 1.0,
        };
        fill_triangle(&mut img, &mut zbuf, [v, v, v], Rgb([255, 255, 255]));
        assert!(img.pixels().all(|p| p.0 == [0, 0, 0]));
    }

    #[test]
    fn offscreen_triangles_touch_nothing() {
        let mut img = RgbImage::new(4, 4);
        let mut zbuf = vec![f32::NEG_INFINITY; 16];
        let off = |x: f32, y: f32| Vertex { x, y, ooz: 1.0 };
        fill_triangle(
            &mut img,
            &mut zbuf,
            [off(-20.0, -20.0), off(-10.0, -20.0), off(-20.0, -10.0)],
            Rgb([255, 255, 255]),
        );
        assert!(img.pixels().all(|p| p.0 == [0, 0, 0]));
    }
}
