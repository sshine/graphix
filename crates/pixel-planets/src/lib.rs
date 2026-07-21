//! # pixel-planets
//!
//! Procedurally generate already-habitable pixel-art planets and render them
//! animated in the terminal (via [`graphix`]): the planet rotates while its
//! cloud layer drifts independently.
//!
//! A planet is **stateless**: it is fully determined by a seed plus a handful
//! of macro-ecosystem parameters ([`PlanetParams`]). "Terraforming" is simply
//! rerunning with different parameters — raise `--water` and the coasts flood,
//! lower `--temp` and the ice caps creep toward the equator. Every parameter
//! exists because it visibly changes the rendering.
//!
//! ```sh
//! pixel-planets --seed 42                          # live rotating planet
//! pixel-planets --water 0.75 --temp -0.3           # terraform: flood + cool
//! pixel-planets --clouds cartoon --still           # one frame, puffy clouds
//! pixel-planets --zoom 1 --zoom-to 5 --gif fly.gif # spaceship approach shot
//! ```
//!
//! Future weird-but-renderable knobs: desertification bands, volcanic
//! night-glow at mountain peaks, ring systems.

pub mod biome;
pub mod clouds;
pub mod noise;
pub mod output;
pub mod planet;
pub mod render;

pub use clouds::CloudMode;
pub use planet::PlanetParams;
pub use render::Renderer;

/// Errors returned by the library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A planet parameter is outside its documented range.
    #[error("parameter {name} = {value} is out of range {range}")]
    InvalidParam {
        /// The CLI-facing parameter name.
        name: &'static str,
        /// The offending value.
        value: f32,
        /// Human-readable description of the accepted range.
        range: &'static str,
    },
    /// An output file or directory could not be written.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A frame could not be encoded.
    #[error(transparent)]
    Image(#[from] image::ImageError),
}
