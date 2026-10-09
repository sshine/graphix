use std::process::ExitCode;

use clap::Parser;
use graphix_attractor::cli::{self, Common};
use thomas::{DEFAULTS, Thomas};

/// Trace the Thomas cyclically symmetric attractor in the terminal.
///
/// A swarm of orbits each trailing its recent path, turning about the body
/// diagonal the system's symmetry lies on, drawn with 24-bit color and Unicode
/// block octants. Use --release builds for live animation.
#[derive(Debug, Parser)]
#[command(name = "thomas", version, about)]
struct Args {
    /// The damping term. Lower is livelier; the attractor shrinks toward the
    /// origin and goes periodic as this rises.
    #[arg(long, default_value_t = Thomas::CHAOTIC)]
    b: f64,

    #[command(flatten)]
    common: Common,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match cli::run(Thomas { b: args.b }, &args.common, DEFAULTS) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("thomas: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphix_attractor::System;

    #[test]
    fn defaults_parse() {
        let args = Args::try_parse_from(["thomas"]).expect("defaults must parse");
        assert_eq!(args.common.mode, graphix::Mode::Octant);
        assert_eq!(args.b, Thomas::CHAOTIC);
    }

    #[test]
    fn the_damping_is_tunable() {
        let args = Args::try_parse_from(["t", "--b", "0.12"]).expect("b must parse");
        assert!((args.b - 0.12).abs() < 1e-12);
    }

    #[test]
    fn output_modes_conflict() {
        assert!(Args::try_parse_from(["t", "--still", "--gif", "x.gif"]).is_err());
        assert!(Args::try_parse_from(["t", "--frames", "3"]).is_err()); // needs --out
    }

    #[test]
    fn the_view_turns_about_the_body_diagonal() {
        assert_eq!(Thomas::default().symmetry_axis(), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn a_still_frame_covers_the_terminal_grid() {
        let args = Args::try_parse_from(["t", "--size", "64", "--orbits", "8", "--trail", "64"])
            .expect("args must parse");
        let renderer = args.common.renderer(Thomas { b: args.b }, DEFAULTS);
        let art = graphix::render_image(&renderer.frame_at(0.0), 80, 24, graphix::Mode::Octant);
        // A square frame is row-limited: 24 rows of 48 cells is square again.
        assert_eq!(art.lines().count(), 24);
    }

    #[test]
    fn the_terminal_output_is_drawn_from_the_octant_repertoire() {
        let args = Args::try_parse_from(["t", "--size", "128", "--orbits", "24"])
            .expect("args must parse");
        let renderer = args.common.renderer(Thomas { b: args.b }, DEFAULTS);
        let art = graphix::render_image(&renderer.frame_at(0.0), 80, 24, graphix::Mode::Octant);

        let octants = art
            .chars()
            .filter(|c| ('\u{1CD00}'..='\u{1CDE5}').contains(c))
            .count();
        assert!(octants > 20, "only {octants} octant glyphs in the output");
    }
}
