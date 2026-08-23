use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::Parser;
use flake::{Error, Palette, Renderer, Theme, output};
use graphix::image::RgbImage;

/// Spin the NixOS lambda flake in the terminal.
///
/// Six extruded lambdas around a hub, turning about the vertical axis, drawn
/// with 24-bit color and Unicode block octants. Use --release builds for live
/// animation.
#[derive(Debug, Parser)]
#[command(name = "flake", version, about)]
struct Args {
    /// Accent hue in degrees (OKLCH); the default is NixOS blue.
    #[arg(long, default_value_t = 250.0)]
    hue: f32,

    /// Lightness tuning for the terminal background in use.
    #[arg(long, value_enum, default_value_t = Theme::Dark)]
    theme: Theme,

    /// Frame size in pixels (the frame is square).
    #[arg(long, default_value_t = 256)]
    size: u32,

    /// Terminal rendering granularity (see graphix).
    #[arg(short = 'm', long, value_enum, default_value_t = graphix::Mode::Octant)]
    mode: graphix::Mode,

    /// Animation frames per second.
    #[arg(long, default_value_t = 20.0)]
    fps: f32,

    /// Seconds per revolution.
    #[arg(long, default_value_t = 5.0)]
    period: f32,

    /// Degrees to tip the spin axis toward the camera (-90..=90). At 0 the
    /// flake spins flat and passes edge-on twice per revolution; at 90 the axis
    /// points at the viewer and it turns like a pinwheel.
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    tilt: f32,

    /// Stop the live animation after this many seconds (0 = run until ^C).
    #[arg(long, default_value_t = 0.0)]
    duration: f32,

    /// Print a single ANSI frame and exit.
    #[arg(long, conflicts_with_all = ["frames", "gif"])]
    still: bool,

    /// Timestamp (seconds) of the frame rendered by --still.
    #[arg(long, default_value_t = 0.0, requires = "still")]
    time: f32,

    /// Dump this many PNG frames instead of animating (requires --out).
    #[arg(long, requires = "out", conflicts_with = "gif")]
    frames: Option<u32>,

    /// Directory for --frames output (created if missing).
    #[arg(long, requires = "frames")]
    out: Option<PathBuf>,

    /// Export one full revolution as an infinitely looping GIF.
    #[arg(long)]
    gif: Option<PathBuf>,

    /// Write the graphix terminal-cell rasterization instead of the raw frame
    /// (--frames and --gif), i.e. what the animation looks like in the terminal.
    #[arg(long)]
    terminal_preview: bool,
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

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("flake: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), Error> {
    let renderer = Renderer::new(
        Palette::new(args.hue, args.theme),
        args.size,
        args.period,
        args.tilt,
    );

    if args.still {
        let (cols, rows) = graphix::terminal_grid();
        print!(
            "{}",
            ansi_frame(&renderer, args.time, cols, rows, args.mode)
        );
    } else if let (Some(n), Some(dir)) = (args.frames, &args.out) {
        output::dump_frames(frames(&renderer, n, args), dir)?;
    } else if let Some(path) = &args.gif {
        // One full revolution, so the loop is seamless.
        let n = (args.fps * renderer.period_secs()).round().max(1.0) as u32;
        output::export_gif(frames(&renderer, n, args), args.fps, path)?;
    } else {
        animate(&renderer, args);
    }
    Ok(())
}

/// One frame rendered to ANSI, fitted to the terminal grid.
fn ansi_frame(renderer: &Renderer, t: f32, cols: u32, rows: u32, mode: graphix::Mode) -> String {
    graphix::render_image(&renderer.frame(t), cols, rows, mode)
}

/// The frame sequence for file output: `n` frames spaced by `--fps`, each one
/// optionally put through the cell rasterization the terminal would show.
fn frames<'a>(
    renderer: &'a Renderer,
    n: u32,
    args: &'a Args,
) -> impl Iterator<Item = RgbImage> + 'a {
    let dt = 1.0 / args.fps.max(f32::MIN_POSITIVE);
    let (cols, rows) = graphix::terminal_grid();
    (0..n).map(move |i| {
        let img = renderer.frame(i as f32 * dt);
        if !args.terminal_preview {
            return img;
        }
        let (c, r) = graphix::fit_grid(img.width(), img.height(), cols, rows);
        graphix::rasterize(&graphix::render_cells(&img, c, r, args.mode), 8, 16)
    })
}

/// The live terminal animation loop: cursor-home redraws inside synchronized
/// output brackets, paced by wall-clock time.
fn animate(renderer: &Renderer, args: &Args) {
    let (cols, rows) = graphix::terminal_grid();
    let frame_dt = Duration::from_secs_f32(1.0 / args.fps.clamp(0.5, 120.0));
    let start = Instant::now();
    let mut next = start;

    let _guard = TermGuard;
    // Clear once, hide the cursor for the duration of the animation.
    print!("\x1b[2J\x1b[?25l");
    if args.duration <= 0.0 {
        // Parked on the last line; visible until the first frame overwrites it.
        print!("\x1b[{rows};1Hpress ^C to stop (then `printf '\\e[?25h'` if the cursor is lost)");
    }
    loop {
        let t = start.elapsed().as_secs_f32();
        if args.duration > 0.0 && t >= args.duration {
            break;
        }
        let art = ansi_frame(renderer, t, cols, rows, args.mode);
        print!("\x1b[?2026h\x1b[H{art}\x1b[?2026l");
        let _ = std::io::stdout().flush();
        next += frame_dt;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let args = Args::try_parse_from(["flake"]).expect("defaults must parse");
        assert_eq!(args.mode, graphix::Mode::Octant);
        assert_eq!(args.theme, Theme::Dark);
    }

    #[test]
    fn output_modes_conflict() {
        assert!(Args::try_parse_from(["f", "--still", "--gif", "x.gif"]).is_err());
        assert!(
            Args::try_parse_from(["f", "--frames", "3", "--out", "d", "--gif", "x.gif"]).is_err()
        );
        assert!(Args::try_parse_from(["f", "--frames", "3"]).is_err()); // needs --out
    }

    #[test]
    fn a_still_frame_covers_the_terminal_grid() {
        let renderer = Renderer::new(Palette::new(250.0, Theme::Dark), 64, 5.0, 0.0);
        let art = ansi_frame(&renderer, 0.0, 80, 24, graphix::Mode::Octant);
        // A square frame is row-limited: 24 rows of 48 cells is square again.
        assert_eq!(art.lines().count(), 24);
    }
}
