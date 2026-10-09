//! # graphix-attractor
//!
//! The machinery the attractor demos share: integrators, orbit trails, the
//! projection and accumulation that turn them into an image, and the command
//! line that drives it. A library with no binary, the way `graphix-noise` is.
//!
//! Each demo crate is then only the part that differs: a [`System`] giving the
//! equations, the step size and the integrator they need, plus the
//! [`cli::Defaults`] that suit the shape it draws.
//!
//! Strands are drawn as light. Every trail segment is splatted additively into
//! a floating-point buffer and the sum is tone-mapped at the end, so a crossing
//! brightens instead of hiding what lies behind it, and the glow falls out of
//! the accumulation rather than a bloom pass.
//!
//! Octants are the default rendering mode, so the terminal font has to carry
//! *Block Octants* (U+1CD00..=U+1CDE5, Unicode 16.0). Where it does not, the
//! glyphs come out as tofu and `-m braille` or `-m sextant` is the fallback.

pub mod cli;
pub mod color;
pub mod output;
pub mod render;
pub mod swarm;
pub mod system;

pub use color::Palette;
pub use render::{Fit, Renderer};
pub use swarm::{Orbit, Swarm};
pub use system::{Integrator, System, advance, trace};

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
