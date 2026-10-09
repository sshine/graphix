//! The command line every attractor demo shares, and the loop behind it.
//!
//! The flags that mean the same thing everywhere carry their defaults here.
//! The ones a system has an opinion about are optional, and fall back to the
//! [`Defaults`] its binary supplies: how many orbits read well, how long a
//! trail should be, how far above the equator to sit, and how long the
//! warm-up has to run before the first draw.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use graphix::image::RgbImage;

use crate::color::Palette;
use crate::render::Renderer;
use crate::swarm::Swarm;
use crate::system::System;
use crate::{Error, output};

/// Flags shared by every attractor binary.
#[derive(Debug, Clone, clap::Args)]
pub struct Common {
    /// Number of orbits traced at once. [default: set by the system]
    #[arg(long)]
    pub orbits: Option<usize>,

    /// Points of trail kept behind each orbit. [default: set by the system]
    #[arg(long)]
    pub trail: Option<usize>,

    /// Seed for the starting points and the per-orbit tint.
    #[arg(long, default_value_t = 1)]
    pub seed: u64,

    /// Frame size in pixels (the frame is square).
    #[arg(long, default_value_t = 256)]
    pub size: u32,

    /// Terminal rendering granularity (see graphix).
    #[arg(short = 'm', long, value_enum, default_value_t = graphix::Mode::Octant)]
    pub mode: graphix::Mode,

    /// Animation frames per second.
    #[arg(long, default_value_t = 20.0)]
    pub fps: f32,

    /// Seconds per revolution. [default: set by the system]
    #[arg(long)]
    pub period: Option<f32>,

    /// Degrees above the attractor's equator to view it from (-90..=90), where
    /// the equator is square on to the system's symmetry axis and 90 looks
    /// straight down it. [default: set by the system]
    #[arg(long, allow_hyphen_values = true)]
    pub tilt: Option<f32>,

    /// How hard the tone curve climbs, i.e. overall brightness. [default: set
    /// by the system]
    #[arg(long)]
    pub exposure: Option<f32>,

    /// Integration steps per frame, which is how fast the strands move.
    /// [default: set by the system]
    #[arg(long)]
    pub steps: Option<u32>,

    /// Stop the live animation after this many seconds (0 = run until ^C).
    #[arg(long, default_value_t = 0.0)]
    pub duration: f32,

    /// Print a single ANSI frame and exit.
    #[arg(long, conflicts_with_all = ["frames", "gif"])]
    pub still: bool,

    /// Timestamp (seconds) setting the turn of the frame drawn by --still.
    #[arg(long, default_value_t = 0.0, requires = "still")]
    pub time: f32,

    /// Dump this many PNG frames instead of animating (requires --out).
    #[arg(long, requires = "out", conflicts_with = "gif")]
    pub frames: Option<u32>,

    /// Directory for --frames output (created if missing).
    #[arg(long, requires = "frames")]
    pub out: Option<PathBuf>,

    /// Export one full revolution as an infinitely looping GIF. The turn comes
    /// back around but the orbits do not, so the seam shows in the strands.
    #[arg(long)]
    pub gif: Option<PathBuf>,

    /// Write the graphix terminal-cell rasterization instead of the raw frame
    /// (--frames and --gif), i.e. what the animation looks like in the terminal.
    #[arg(long)]
    pub terminal_preview: bool,
}

/// What a binary wants when the command line does not say.
#[derive(Debug, Clone, Copy)]
pub struct Defaults {
    pub orbits: usize,
    pub trail: usize,
    /// Steps run before the first draw, on top of enough to fill the trail.
    ///
    /// Where orbits separate this only wants to be generous, since a longer
    /// warm-up spreads the strands further apart. Where they converge it has
    /// to stay short, or every strand ends up tracing the same curve.
    pub warmup_extra: usize,
    pub period: f32,
    pub tilt: f32,
    pub exposure: f32,
    pub steps: u32,
}

impl Common {
    /// The renderer these flags and defaults describe.
    pub fn renderer<S: System>(&self, system: S, defaults: Defaults) -> Renderer<S> {
        let trail = self.trail.unwrap_or(defaults.trail);
        let swarm = Swarm::new(
            system,
            self.orbits.unwrap_or(defaults.orbits),
            trail,
            self.seed,
            trail + defaults.warmup_extra,
        );
        Renderer::new(
            swarm,
            Palette::default(),
            self.size,
            self.period.unwrap_or(defaults.period),
            self.tilt.unwrap_or(defaults.tilt),
        )
        .with_exposure(self.exposure.unwrap_or(defaults.exposure))
        .with_steps_per_frame(self.steps.unwrap_or(defaults.steps))
    }
}

/// Restores the cursor and SGR state when the live animation ends, including
/// on panic. (^C bypasses this: recover with `printf '\e[?25h'`, or use
/// --duration for a clean stop.)
struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        print!("\x1b[?25h\x1b[0m");
        let _ = std::io::stdout().flush();
    }
}

/// Build the renderer and do whatever the flags ask of it.
pub fn run<S: System>(system: S, common: &Common, defaults: Defaults) -> Result<(), Error> {
    let mut renderer = common.renderer(system, defaults);

    if common.still {
        let (cols, rows) = graphix::terminal_grid();
        let angle = std::f32::consts::TAU * common.time / renderer.period_secs();
        let img = renderer.frame_at(angle);
        print!("{}", graphix::render_image(&img, cols, rows, common.mode));
    } else if let (Some(n), Some(dir)) = (common.frames, &common.out) {
        output::dump_frames(frames(&mut renderer, n, common), dir)?;
    } else if let Some(path) = &common.gif {
        // One full revolution, so the turn at least lines up.
        let n = (common.fps * renderer.period_secs()).round().max(1.0) as u32;
        output::export_gif(frames(&mut renderer, n, common), common.fps, path)?;
    } else {
        animate(&mut renderer, common);
    }
    Ok(())
}

/// The frame sequence for file output: `n` frames spaced by `--fps`, each one
/// optionally put through the cell rasterization the terminal would show.
fn frames<'a, S: System>(
    renderer: &'a mut Renderer<S>,
    n: u32,
    common: &'a Common,
) -> impl Iterator<Item = RgbImage> + 'a {
    let dt = 1.0 / common.fps.max(f32::MIN_POSITIVE);
    let (cols, rows) = graphix::terminal_grid();
    let mut i = 0;
    std::iter::from_fn(move || {
        if i >= n {
            return None;
        }
        i += 1;
        let img = renderer.frame(dt);
        if !common.terminal_preview {
            return Some(img);
        }
        let (c, r) = graphix::fit_grid(img.width(), img.height(), cols, rows);
        Some(graphix::rasterize(
            &graphix::render_cells(&img, c, r, common.mode),
            8,
            16,
        ))
    })
}

/// The live terminal animation loop: cursor-home redraws inside synchronized
/// output brackets, paced by wall-clock time.
fn animate<S: System>(renderer: &mut Renderer<S>, common: &Common) {
    let (cols, rows) = graphix::terminal_grid();
    let fps = common.fps.clamp(0.5, 120.0);
    let frame_dt = Duration::from_secs_f32(1.0 / fps);
    let start = Instant::now();
    let mut next = start;

    let _guard = TermGuard;
    // Clear once, hide the cursor for the duration of the animation.
    print!("\x1b[2J\x1b[?25l");
    if common.duration <= 0.0 {
        // Parked on the last line; visible until the first frame overwrites it.
        print!("\x1b[{rows};1Hpress ^C to stop (then `printf '\\e[?25h'` if the cursor is lost)");
    }
    loop {
        if common.duration > 0.0 && start.elapsed().as_secs_f32() >= common.duration {
            break;
        }
        let img = renderer.frame(1.0 / fps);
        let art = graphix::render_image(&img, cols, rows, common.mode);
        print!("\x1b[?2026h\x1b[H{art}\x1b[?2026l");
        let _ = std::io::stdout().flush();
        next += frame_dt;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}
