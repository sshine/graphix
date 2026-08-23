//! # flake
//!
//! Spin the NixOS lambda flake in the terminal (via [`graphix`]): six extruded
//! lambdas arranged around a hub, turning about the vertical axis.
//!
//! The flake is **stateless**: the spin angle is a function of elapsed time,
//! and everything else is geometry fixed at build time. Only the palette and
//! the frame resolution are parameters.
//!
//! ```sh
//! flake                         # live spinning flake
//! flake --hue 130 --still       # one frame, green instead of NixOS blue
//! flake --theme light           # tuned for a light terminal background
//! flake --gif flake.gif         # one revolution, looping
//! ```
//!
//! This is a port of the ASCII spinner on jokamachi.systems, which plotted a
//! point cloud straight to characters. Here the flake is rasterized into an RGB
//! image first, so graphix picks the glyphs and the 24-bit colors, and every
//! rendering mode (octants by default) comes for free.

pub mod color;
pub mod geometry;
pub mod output;
pub mod render;

pub use color::{Palette, Theme};
pub use render::Renderer;

/// Errors returned by the library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An output file or directory could not be written.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A frame could not be encoded.
    #[error(transparent)]
    Image(#[from] graphix::image::ImageError),
}
