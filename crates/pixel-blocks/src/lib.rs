//! # pixel-blocks
//!
//! Procedural isometric pixel-art blocks, drawn in the terminal (via
//! [`graphix`]) or written to a PNG: Minecraft-style cubes in a 2:1 dimetric
//! projection, arranged on an integer grid.
//!
//! A block is **stateless**: a [`BlockRecipe`] plus a seed fully determine it.
//! A recipe names a palette ramp and a pattern per face, never individual
//! pixels, so the same recipe yields a coherent block at any texel resolution.
//!
//! Recipes are authored twice over: as Rust values through a builder, and as
//! TOML for the blocks that ship with the crate. Baking a recipe evaluates its
//! patterns once into a [`CompiledBlock`], a finished sprite whose rows are
//! contiguous, so drawing a scene of a thousand blocks copies rows and
//! evaluates no noise at all.
//!
//! ```sh
//! pixel-blocks                          # the built-in blocks, side by side
//! pixel-blocks --block grass            # one block
//! pixel-blocks --scene scenes/arch.txt  # a scene from an ASCII layer file
//! pixel-blocks --recipe ore.toml --out ore.png
//! ```
//!
//! `scenes/` holds worked examples of the layer format: `arch.txt` spans open
//! air and leaves a block hanging with nothing under it, `stairs.txt` terraces
//! a hillside. `recipes/` holds every built-in recipe as TOML, generated from
//! [`builtin`] by `just recipes`.

pub mod ascii;
pub mod builtin;
pub mod color;
pub mod face;
pub mod iso;
pub mod pattern;
pub mod recipe;
pub mod scene;

pub use color::{Color, ColorSpec, Ramp};
pub use iso::{BlockSize, CompiledBlock};
pub use pattern::Pattern;
pub use recipe::{BlockRecipe, FaceRecipe, Shading};
pub use scene::{Placement, Scene};

/// Errors returned by the library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A recipe cannot be baked as written.
    #[error("block {name}: {reason}")]
    Recipe {
        /// The offending block's name.
        name: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A scene file could not be read as blocks.
    #[error("scene line {line}: {reason}")]
    Scene {
        /// One-based line number in the scene file.
        line: usize,
        /// What went wrong there.
        reason: String,
    },
    /// A recipe file is not valid TOML, or not a valid recipe.
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
    /// A recipe could not be written as TOML.
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),
    /// An output file or directory could not be written.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// An image could not be encoded.
    #[error(transparent)]
    Image(#[from] graphix::image::ImageError),
}
