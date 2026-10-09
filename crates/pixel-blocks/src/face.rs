//! The three faces a cube shows in this projection, and the coordinates
//! patterns address their texels by.

/// A visible face of a cube. The projection is fixed, so a cube shows its top
/// and exactly two of its four sides; the remaining three faces are never
/// drawn and are not baked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    /// The upward face, drawn as the diamond.
    Top,
    /// The face at the far end of the `y` axis, drawn as the lower-left
    /// parallelogram.
    Left,
    /// The face at the far end of the `x` axis, drawn as the lower-right
    /// parallelogram.
    Right,
}

/// One texel of one face: `u` runs left to right as the face is drawn, `v`
/// runs top to bottom, and `n` is the cube's texel resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texel {
    /// Which face the texel belongs to.
    pub face: Face,
    /// Column within the face, in `0..n`.
    pub u: u32,
    /// Row within the face, in `0..n`.
    pub v: u32,
    /// Texels per cube edge.
    pub n: u32,
}

impl Texel {
    /// The texel's centre on the cube, in half-texel units so it is exact.
    ///
    /// Hashing this rather than `(u, v)` is what makes a pattern continuous
    /// across the cube's edges: two texels that touch in 3D get neighbouring
    /// keys whichever faces they sit on. The mapping is injective because each
    /// face pins exactly one coordinate to the even value `2n` while the other
    /// two stay odd.
    pub fn key(self) -> [i64; 3] {
        let (n, u, v) = (self.n as i64, self.u as i64, self.v as i64);
        match self.face {
            Face::Top => [2 * u + 1, 2 * v + 1, 2 * n],
            Face::Left => [2 * u + 1, 2 * n, 2 * n - 2 * v - 1],
            Face::Right => [2 * n, 2 * n - 2 * u - 1, 2 * n - 2 * v - 1],
        }
    }

    /// The texel's column in the band running around the cube's sides, in
    /// `0..2n`, or `None` for the top face.
    ///
    /// The two visible sides meet at the cube's front vertical edge, where
    /// `Left`'s last column abuts `Right`'s first. Patterns that vary along
    /// the band (a grass fringe, a course of bricks) index by this so they do
    /// not step at that edge. The band's two ends fall on the silhouette and
    /// never meet on screen, so it does not wrap.
    pub fn ring(self) -> Option<u32> {
        match self.face {
            Face::Top => None,
            Face::Left => Some(self.u),
            Face::Right => Some(self.n + self.u),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(n: u32) -> Vec<Texel> {
        let mut out = Vec::new();
        for face in [Face::Top, Face::Left, Face::Right] {
            for v in 0..n {
                for u in 0..n {
                    out.push(Texel { face, u, v, n });
                }
            }
        }
        out
    }

    #[test]
    fn keys_are_unique_across_faces() {
        for n in [1, 2, 16] {
            let keys: std::collections::HashSet<[i64; 3]> =
                all(n).iter().map(|t| t.key()).collect();
            assert_eq!(keys.len(), 3 * (n * n) as usize, "collision at n = {n}");
        }
    }

    #[test]
    fn keys_stay_inside_the_cube() {
        let n = 16;
        for t in all(n) {
            for c in t.key() {
                assert!((0..=2 * i64::from(n)).contains(&c), "{c} escapes the cube");
            }
        }
    }

    #[test]
    fn touching_texels_on_different_faces_have_neighbouring_keys() {
        // The cube's front vertical edge: Left's last column and Right's first
        // are the same strip of the cube seen from two sides, so their centres
        // sit one half-texel apart in each of two axes.
        let n = 16;
        for v in 0..n {
            let l = Texel {
                face: Face::Left,
                u: n - 1,
                v,
                n,
            }
            .key();
            let r = Texel {
                face: Face::Right,
                u: 0,
                v,
                n,
            }
            .key();
            let d: i64 = (0..3).map(|i| (l[i] - r[i]).abs()).sum();
            assert_eq!(d, 2, "keys {l:?} and {r:?} are not adjacent");
        }
    }

    #[test]
    fn the_side_band_is_contiguous_across_the_front_edge() {
        let n = 16;
        let left = Texel {
            face: Face::Left,
            u: n - 1,
            v: 0,
            n,
        }
        .ring();
        let right = Texel {
            face: Face::Right,
            u: 0,
            v: 0,
            n,
        }
        .ring();
        assert_eq!(left.map(|r| r + 1), right);
        assert_eq!(
            Texel {
                face: Face::Top,
                u: 0,
                v: 0,
                n
            }
            .ring(),
            None
        );
    }
}
