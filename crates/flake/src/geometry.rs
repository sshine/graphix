//! The lambda-flake geometry: the NixOS logo's two strokes, extruded into
//! prisms and repeated around six wedges.

use std::f32::consts::{PI, TAU};

/// A point in flake space, before the viewing spin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A flat quadrilateral with its corners ordered around the perimeter, tagged
/// with the wedge it belongs to (the wedge picks the color).
#[derive(Debug, Clone, Copy)]
pub struct Face {
    pub corners: [Point3; 4],
    pub wedge: u32,
}

/// Lambdas arranged around the hub.
const WEDGES: u32 = 6;

/// Half the thickness of an extruded stroke.
const DEPTH: f32 = 2.0;

/// Distance from the hub to a lambda's local origin.
const R_CENTER: f32 = 14.0;

/// The radius the geometry reaches; framing and depth shading normalize
/// against it.
pub const R_OUTER: f32 = 24.0;

/// Width of a lambda stroke.
const STROKE: f32 = 2.5;

/// How the lambda is oriented inside its wedge, in radians.
const LAMBDA_ROT_Y: f32 = PI;
const LAMBDA_ROT_Z: f32 = 230.0 * PI / 180.0;

/// The lambda's outline in shape space: the spine runs from the inner tip out
/// to the left foot, and the leg branches off it toward the right foot.
const INNER_TOP: [f32; 2] = [-1.5, 7.0];
const OUTER_LEFT: [f32; 2] = [-5.0, -7.0];
const OUTER_RIGHT: [f32; 2] = [4.0, -7.0];
const CROTCH: [f32; 2] = [-3.0, 1.0];

/// A rectangle of width `w` centered on the segment from `start` to `end`.
fn stroke_quad(start: [f32; 2], end: [f32; 2], w: f32) -> [[f32; 2]; 4] {
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let len = (dx * dx + dy * dy).sqrt();
    let (nx, ny) = (-dy / len * w / 2.0, dx / len * w / 2.0);
    [
        [start[0] + nx, start[1] + ny],
        [end[0] + nx, end[1] + ny],
        [end[0] - nx, end[1] - ny],
        [start[0] - nx, start[1] - ny],
    ]
}

/// Lift a shape-space corner to flake space: orient the lambda, then swing it
/// out to the wedge at `angle`.
fn place(p: [f32; 2], z: f32, angle: f32) -> Point3 {
    let (mut px, mut py, mut pz) = (p[0], p[1], z);

    let (c, s) = (LAMBDA_ROT_Y.cos(), LAMBDA_ROT_Y.sin());
    (px, pz) = (px * c - pz * s, px * s + pz * c);

    let (c, s) = (LAMBDA_ROT_Z.cos(), LAMBDA_ROT_Z.sin());
    (px, py) = (px * c - py * s, px * s + py * c);

    let (u, v) = (R_CENTER + py, px);
    Point3 {
        x: u * angle.cos() - v * angle.sin(),
        y: u * angle.sin() + v * angle.cos(),
        z: pz,
    }
}

/// Build the whole flake: every stroke of every wedge as a prism, meaning a
/// front and back cap plus the four side strips joining them.
pub fn faces() -> Vec<Face> {
    let mut faces = Vec::with_capacity((WEDGES * 2 * 6) as usize);
    for wedge in 0..WEDGES {
        let angle = wedge as f32 * TAU / WEDGES as f32;
        let strokes = [
            stroke_quad(INNER_TOP, OUTER_LEFT, STROKE),
            stroke_quad(CROTCH, OUTER_RIGHT, STROKE),
        ];
        for stroke in strokes {
            let front = stroke.map(|p| place(p, DEPTH, angle));
            let back = stroke.map(|p| place(p, -DEPTH, angle));
            faces.push(Face {
                corners: front,
                wedge,
            });
            faces.push(Face {
                corners: back,
                wedge,
            });
            for j in 0..4 {
                let k = (j + 1) % 4;
                faces.push(Face {
                    corners: [back[j], back[k], front[k], front[j]],
                    wedge,
                });
            }
        }
    }
    faces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stroke_becomes_a_closed_prism() {
        assert_eq!(faces().len() as u32, WEDGES * 2 * 6);
    }

    #[test]
    fn geometry_stays_inside_the_outer_radius() {
        for face in faces() {
            for p in face.corners {
                let r = (p.x * p.x + p.y * p.y).sqrt();
                assert!(r <= R_OUTER, "corner at radius {r} escapes the frame");
                assert!(p.z.abs() <= DEPTH + 1e-3, "corner at depth {}", p.z);
            }
        }
    }

    #[test]
    fn wedges_are_rotations_of_each_other() {
        let faces = faces();
        let per_wedge = faces.len() / WEDGES as usize;
        let (a, b) = (faces[0].corners[0], faces[per_wedge].corners[0]);
        let ra = (a.x * a.x + a.y * a.y).sqrt();
        let rb = (b.x * b.x + b.y * b.y).sqrt();
        assert!((ra - rb).abs() < 1e-3, "wedges differ in radius");
    }
}
