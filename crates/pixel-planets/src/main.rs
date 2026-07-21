use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::Parser;
use pixel_planets::render::ZoomRamp;
use pixel_planets::{CloudMode, Error, PlanetParams, Renderer, output};

/// Procedurally generate and animate pixel-art planets in the terminal.
///
/// A planet is stateless: seed + parameters fully determine it. Terraform by
/// rerunning with different values. Use --release builds for live animation.
#[derive(Debug, Parser)]
#[command(name = "pixel-planets", version, about)]
struct Args {
    /// Seed for all procedural fields.
    #[arg(long, default_value_t = 42)]
    seed: u64,

    /// Sea level (0..=1): the elevation below which the surface floods.
    #[arg(long, default_value_t = 0.60)]
    water: f32,

    /// Global temperature bias (-1..=1): negative cools, positive warms.
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    temp: f32,

    /// Baseline atmospheric moisture (0..=1).
    #[arg(long, default_value_t = 0.5)]
    humidity: f32,

    /// Plant vigor (0..=1): palette lushness, and deserts recede as it grows.
    #[arg(long, default_value_t = 0.6)]
    vegetation: f32,

    /// Polar ice cap extent (0..=1).
    #[arg(long, default_value_t = 0.3)]
    ice: f32,

    /// Cloud coverage (0..=1).
    #[arg(long, default_value_t = 0.5)]
    cloudiness: f32,

    /// Atmospheric haze at the limb (0..=1).
    #[arg(long, default_value_t = 0.5)]
    atmosphere: f32,

    /// Algal super-bloom (0..=1): oceans tint teal in swirling patches.
    #[arg(long, default_value_t = 0.0)]
    bloom: f32,

    /// Bioluminescent shallows (0..=1): coasts glow cyan on the night side.
    #[arg(long, default_value_t = 0.0)]
    glow: f32,

    /// Axial tilt in degrees (-90..=90), in the view plane.
    #[arg(long = "tilt", default_value_t = 15.0, allow_hyphen_values = true)]
    tilt_deg: f32,

    /// Cloud model.
    #[arg(long, value_enum, default_value_t = CloudMode::Realistic)]
    clouds: CloudMode,

    /// Planet frame size in pixels (the frame is square).
    #[arg(long, default_value_t = 128)]
    size: u32,

    /// Terminal rendering granularity (see graphix).
    #[arg(short = 'm', long, value_enum, default_value_t = graphix::Mode::Octant)]
    mode: graphix::Mode,

    /// Animation frames per second.
    #[arg(long, default_value_t = 20.0)]
    fps: f32,

    /// Stop the live animation after this many seconds (0 = run until ^C).
    #[arg(long, default_value_t = 0.0)]
    duration: f32,

    /// Surface rotation period in seconds.
    #[arg(long, default_value_t = 12.0)]
    period: f32,

    /// Camera zoom: 1.0 fits the planet disc; > 1 crops into the planet
    /// ("immediate orbit"); < 1 shrinks it into the starfield.
    #[arg(long, default_value_t = 1.0)]
    zoom: f32,

    /// Animate the zoom from --zoom to this value (spaceship approach shot).
    #[arg(long)]
    zoom_to: Option<f32>,

    /// Seconds the zoom ramp takes (default: --duration if set, else one
    /// rotation period).
    #[arg(long)]
    zoom_secs: Option<f32>,

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

    /// Export one full rotation as an infinitely looping GIF.
    #[arg(long)]
    gif: Option<PathBuf>,

    /// Nearest-neighbor upscale factor for --frames and --gif output.
    #[arg(long, default_value_t = 4)]
    scale: u32,

    /// Dump the graphix terminal-cell rasterization instead of the raw
    /// planet image (--frames and --gif).
    #[arg(long)]
    terminal_preview: bool,
}

impl Args {
    fn params(&self) -> PlanetParams {
        PlanetParams {
            seed: self.seed,
            water: self.water,
            temp: self.temp,
            humidity: self.humidity,
            vegetation: self.vegetation,
            ice: self.ice,
            cloudiness: self.cloudiness,
            atmosphere: self.atmosphere,
            bloom: self.bloom,
            glow: self.glow,
            tilt_deg: self.tilt_deg,
        }
    }

    fn zoom_ramp(&self) -> ZoomRamp {
        let to = self.zoom_to.unwrap_or(self.zoom);
        let secs = self.zoom_secs.unwrap_or(if self.duration > 0.0 {
            self.duration
        } else {
            self.period
        });
        ZoomRamp {
            from: self.zoom,
            to,
            secs,
        }
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

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pixel-planets: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), Error> {
    let params = args.params();
    params.validate()?;
    let renderer = Renderer::new(
        params,
        args.clouds,
        args.size,
        args.period.max(0.1),
        args.zoom_ramp(),
    );

    if args.still {
        let (cols, rows) = graphix::terminal_grid();
        print!(
            "{}",
            ansi_frame(&renderer, args.time, cols, rows, args.mode)
        );
    } else if let (Some(n), Some(dir)) = (args.frames, &args.out) {
        if args.terminal_preview {
            dump_preview_frames(&renderer, n, args, dir)?;
        } else {
            output::dump_frames(&renderer, n, args.fps, dir, args.scale)?;
        }
    } else if let Some(path) = &args.gif {
        output::export_gif(&renderer, args.fps, path, args.scale)?;
    } else {
        animate(&renderer, args);
    }
    Ok(())
}

/// One frame rendered to ANSI, fitted to the terminal grid.
fn ansi_frame(renderer: &Renderer, t: f32, cols: u32, rows: u32, mode: graphix::Mode) -> String {
    graphix::render_image(&renderer.frame(t), cols, rows, mode)
}

/// Dump the graphix terminal-cell rasterization of each frame.
fn dump_preview_frames(
    renderer: &Renderer,
    n: u32,
    args: &Args,
    dir: &std::path::Path,
) -> Result<(), Error> {
    std::fs::create_dir_all(dir)?;
    let (cols, rows) = graphix::terminal_grid();
    let dt = 1.0 / args.fps.max(f32::MIN_POSITIVE);
    for i in 0..n {
        let img = renderer.frame(i as f32 * dt);
        let (c, r) = graphix::fit_grid(img.width(), img.height(), cols, rows);
        let grid = graphix::render_cells(&img, c, r, args.mode);
        graphix::rasterize(&grid, 8, 16).save(dir.join(format!("frame_{i:04}.png")))?;
    }
    Ok(())
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
        let args = Args::try_parse_from(["pixel-planets"]).expect("defaults must parse");
        assert_eq!(args.seed, 42);
        assert!(args.params().validate().is_ok());
    }

    #[test]
    fn output_modes_conflict() {
        assert!(Args::try_parse_from(["p", "--still", "--gif", "x.gif"]).is_err());
        assert!(
            Args::try_parse_from(["p", "--frames", "3", "--out", "d", "--gif", "x.gif"]).is_err()
        );
        assert!(Args::try_parse_from(["p", "--frames", "3"]).is_err()); // needs --out
    }

    #[test]
    fn negative_values_parse() {
        let args = Args::try_parse_from(["p", "--temp", "-0.5", "--tilt", "-30"])
            .expect("hyphen values must parse");
        assert_eq!(args.temp, -0.5);
        assert_eq!(args.tilt_deg, -30.0);
    }

    #[test]
    fn zoom_ramp_defaults_to_period() {
        let args = Args::try_parse_from(["p", "--zoom", "2", "--zoom-to", "6"]).expect("parses");
        let ramp = args.zoom_ramp();
        assert_eq!(ramp.from, 2.0);
        assert_eq!(ramp.to, 6.0);
        assert_eq!(ramp.secs, args.period);
    }

    #[test]
    fn invalid_params_are_reported() {
        let args = Args::try_parse_from(["p", "--water", "1.5", "--still"]).expect("parses");
        assert!(matches!(
            run(&args),
            Err(Error::InvalidParam { name: "water", .. })
        ));
    }
}
