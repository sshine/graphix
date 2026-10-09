//! What a block is made of, and how that becomes a sprite.
//!
//! A recipe names a palette and a pattern per face, never a pixel. Baking it
//! evaluates every texel once, applies the projection's face shading, and
//! hands the result to [`CompiledBlock`]; from then on the block draws without
//! touching a pattern again.

use graphix_noise::Noise;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::color::{Color, Ramp, shade};
use crate::face::{Face, Texel};
use crate::iso::{BlockSize, CompiledBlock};
use crate::pattern::Pattern;

/// How much light each face keeps, relative to an unshaded top.
///
/// The defaults are measured off the reference blocks: a left face is exactly
/// four fifths of the top and a right face three fifths. Shading is a property
/// of the projection rather than the material, so recipes inherit it unless
/// they say otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shading {
    /// Brightness of the top face, in `0..=1`.
    pub top: f32,
    /// Brightness of the left face, in `0..=1`.
    pub left: f32,
    /// Brightness of the right face, in `0..=1`.
    pub right: f32,
}

impl Default for Shading {
    fn default() -> Self {
        Shading {
            top: 1.0,
            left: 0.80,
            right: 0.60,
        }
    }
}

impl Shading {
    /// The brightness `face` keeps.
    pub fn factor(self, face: Face) -> f32 {
        match face {
            Face::Top => self.top,
            Face::Left => self.left,
            Face::Right => self.right,
        }
    }
}

/// One face's palette and the pattern that indexes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceRecipe {
    /// The face's palette, darkest first.
    pub ramp: Ramp,
    /// How the palette is distributed over the face's texels.
    pub pattern: Pattern,
}

impl FaceRecipe {
    /// A face made of `pattern` drawn from `ramp`.
    pub fn new(ramp: Ramp, pattern: Pattern) -> Self {
        FaceRecipe { ramp, pattern }
    }
}

/// A block: a name, a face recipe for the top and one for the sides, and the
/// shading the projection applies to them.
///
/// The projection shows a cube's top and two of its sides, and those two sides
/// differ only in brightness, so two face recipes describe every block. Build
/// one with [`BlockRecipe::new`] and the `with_` methods, or read one from
/// TOML with [`BlockRecipe::from_toml`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRecipe {
    /// The block's name, as a scene file refers to it.
    pub name: String,
    /// The upward face.
    pub top: FaceRecipe,
    /// Both side faces, before shading tells them apart.
    pub side: FaceRecipe,
    /// How much light each face keeps.
    #[serde(default)]
    pub shading: Shading,
}

impl BlockRecipe {
    /// A block named `name`, with `top` on its upward face and `side` on both
    /// visible sides.
    pub fn new(name: impl Into<String>, top: FaceRecipe, side: FaceRecipe) -> Self {
        BlockRecipe {
            name: name.into(),
            top,
            side,
            shading: Shading::default(),
        }
    }

    /// The same block with one face recipe on every face.
    pub fn uniform(name: impl Into<String>, face: FaceRecipe) -> Self {
        BlockRecipe::new(name, face.clone(), face)
    }

    /// The same block with different face shading.
    pub fn with_shading(mut self, shading: Shading) -> Self {
        self.shading = shading;
        self
    }

    /// Check that every palette the recipe reaches has at least one color.
    pub fn validate(&self) -> Result<(), Error> {
        let fail = |reason| {
            Err(Error::Recipe {
                name: self.name.clone(),
                reason,
            })
        };
        for face in [&self.top, &self.side] {
            if face.ramp.is_empty() {
                return fail("a face has an empty palette");
            }
            if empty_nested_ramp(&face.pattern) {
                return fail("a nested pattern has an empty palette");
            }
        }
        Ok(())
    }

    /// The face recipe `face` is drawn from.
    pub fn face(&self, face: Face) -> &FaceRecipe {
        match face {
            Face::Top => &self.top,
            Face::Left | Face::Right => &self.side,
        }
    }

    /// Evaluate every texel of every visible face and rasterize the result.
    ///
    /// This is the only place patterns and noise are touched. Face shading is
    /// applied here too, so the sides cost nothing extra to draw.
    pub fn bake(&self, seed: u64, size: BlockSize) -> Result<CompiledBlock, Error> {
        self.validate()?;
        let noise = Noise::new(seed);
        let n = size.texels();
        let texels = |face| {
            let recipe = self.face(face);
            let ramp = recipe.ramp.resolve();
            let factor = self.shading.factor(face);
            (0..n * n)
                .map(|i| {
                    let t = Texel {
                        face,
                        u: i % n,
                        v: i / n,
                        n,
                    };
                    shade(recipe.pattern.shade(&noise, t, &ramp), factor)
                })
                .collect::<Vec<Color>>()
        };
        let (top, left, right) = (texels(Face::Top), texels(Face::Left), texels(Face::Right));
        Ok(CompiledBlock::from_faces(size, &top, &left, &right))
    }

    /// Read a recipe from TOML.
    pub fn from_toml(text: &str) -> Result<Self, Error> {
        Ok(toml::from_str(text)?)
    }

    /// Write the recipe as TOML.
    pub fn to_toml(&self) -> Result<String, Error> {
        Ok(toml::to_string(self)?)
    }
}

/// Whether any palette nested inside `pattern` is empty.
fn empty_nested_ramp(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Fringe { ramp, pattern, .. } => ramp.is_empty() || empty_nested_ramp(pattern),
        Pattern::Layer(layers) => layers.iter().any(empty_nested_ramp),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin;
    use crate::color::ColorSpec;

    fn size() -> BlockSize {
        BlockSize::new(16, 2)
    }

    #[test]
    fn shading_matches_the_reference_blocks() {
        // Measured off misc-graphics/mineblocks.png: a stone left face is
        // [95, 95, 95] where the top is [118, 118, 118], and the right face
        // [71, 71, 71].
        let s = Shading::default();
        assert_eq!(shade([118, 118, 118], s.factor(Face::Left)), [94, 94, 94]);
        assert_eq!(shade([118, 118, 118], s.factor(Face::Right)), [71, 71, 71]);
        assert_eq!(shade([118, 118, 118], s.factor(Face::Top)), [118, 118, 118]);
    }

    #[test]
    fn the_same_seed_bakes_the_same_block() {
        let recipe = builtin::grass();
        let a = recipe.bake(42, size()).expect("grass must bake");
        let b = recipe.bake(42, size()).expect("grass must bake");
        assert_eq!(a, b);
        assert_ne!(a, recipe.bake(43, size()).expect("grass must bake"));
    }

    #[test]
    fn sides_are_darker_than_the_top() {
        // The two side faces must be distinguishable from each other and from
        // the top, or the cube reads flat.
        let block = builtin::stone().bake(1, size()).expect("stone must bake");
        let img = block.to_image();
        let q = size().quarter();
        let lum = |x, y| {
            img.get_pixel(x, y).0[..3]
                .iter()
                .map(|&v| u32::from(v))
                .sum::<u32>()
        };
        assert!(
            lum(2 * q, q) > lum(q, 3 * q),
            "the left face is not darker than the top"
        );
        assert!(
            lum(q, 3 * q) > lum(3 * q, 3 * q),
            "the right face is not darker than the left"
        );
    }

    #[test]
    fn an_empty_palette_is_rejected_rather_than_baked() {
        let empty = FaceRecipe::new(Ramp::rgb([]), Pattern::Solid { index: 0 });
        let recipe = BlockRecipe::uniform("void", empty);
        assert!(matches!(recipe.validate(), Err(Error::Recipe { .. })));
        assert!(recipe.bake(0, size()).is_err());

        // A palette nested inside a fringe counts too.
        let nested = FaceRecipe::new(
            Ramp::rgb([[1, 2, 3]]),
            Pattern::Fringe {
                depth: 2,
                jitter: 1,
                span: 2,
                ramp: Ramp::rgb([]),
                pattern: Box::new(Pattern::Solid { index: 0 }),
            },
        );
        assert!(BlockRecipe::uniform("hollow", nested).validate().is_err());
    }

    #[test]
    fn a_recipe_parses_back_from_its_own_toml() {
        for recipe in builtin::all() {
            let text = recipe.to_toml().expect("a builtin must serialize");
            let parsed = BlockRecipe::from_toml(&text).expect("a builtin must parse back");
            assert_eq!(parsed, recipe, "{} changed through TOML", recipe.name);
            assert_eq!(
                parsed.bake(9, size()).expect("bake"),
                recipe.bake(9, size()).expect("bake")
            );
        }
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let text = r#"
            name = "typo"
            [top]
            ramp = [[1, 2, 3]]
            wobble = 3
            [top.pattern.solid]
            index = 0
            [side]
            ramp = [[1, 2, 3]]
            [side.pattern.solid]
            index = 0
        "#;
        assert!(BlockRecipe::from_toml(text).is_err());
    }

    #[test]
    fn integer_scalars_parse_where_a_float_is_expected() {
        // TOML has distinct integer and float types, and a recipe written by
        // hand will say `contrast = 1` sooner or later.
        let text = r#"
            name = "flat"
            [top]
            ramp = [[1, 2, 3]]
            [top.pattern.blotch]
            scale = 3
            contrast = 1
            [side]
            ramp = [[1, 2, 3]]
            [side.pattern.speckle]
            contrast = 1
        "#;
        let recipe = BlockRecipe::from_toml(text).expect("integers must coerce to floats");
        assert_eq!(recipe.name, "flat");
    }

    #[test]
    fn a_recipe_bakes_at_every_resolution() {
        for recipe in builtin::all() {
            for texels in [1, 2, 3, 8, 16, 32] {
                let size = BlockSize::new(texels, 1);
                let block = recipe.bake(5, size).expect("bake must survive small cubes");
                assert_eq!(block.size(), size);
            }
        }
    }

    #[test]
    fn shading_is_overridable_per_recipe() {
        let flat = Shading {
            top: 1.0,
            left: 1.0,
            right: 1.0,
        };
        let face = FaceRecipe::new(Ramp::rgb([[200, 100, 50]]), Pattern::Solid { index: 0 });
        let block = BlockRecipe::uniform("flat", face)
            .with_shading(flat)
            .bake(0, size())
            .expect("bake");
        let img = block.to_image();
        let q = size().quarter();
        assert_eq!(img.get_pixel(2 * q, q).0, [200, 100, 50, 255]);
        assert_eq!(img.get_pixel(3 * q, 3 * q).0, [200, 100, 50, 255]);
    }

    #[test]
    fn a_scatter_color_survives_shading_on_the_darker_face() {
        // Inclusions are shaded with the rest of the face, so an ore nugget
        // does not glow on the side that should be in shadow.
        let face = FaceRecipe::new(
            Ramp::rgb([[100, 100, 100]]),
            Pattern::Layer(vec![
                Pattern::Solid { index: 0 },
                Pattern::Scatter {
                    cell: 1,
                    density: 1.0,
                    size: 1,
                    color: ColorSpec::Rgb([200, 200, 200]),
                },
            ]),
        );
        let block = BlockRecipe::uniform("ore", face)
            .bake(0, size())
            .expect("bake");
        let img = block.to_image();
        let q = size().quarter();
        assert_eq!(img.get_pixel(2 * q, q).0, [200, 200, 200, 255]);
        assert_eq!(img.get_pixel(3 * q, 3 * q).0, [120, 120, 120, 255]);
    }
}
