use std::process::ExitCode;

use aizawa::{DEFAULTS, Preset};
use clap::Parser;
use graphix_attractor::cli::{self, Common};

/// Trace the Aizawa attractor in the terminal.
///
/// A swarm of orbits through the system behind the zensical.org background,
/// each trailing its recent path, turning about the attractor's axis and drawn
/// with 24-bit color and Unicode block octants. Use --release builds for live
/// animation.
#[derive(Debug, Parser)]
#[command(name = "aizawa", version, about)]
struct Args {
    /// Which parameter set to integrate. The default is the one the
    /// zensical.org hero runs on, which is quasiperiodic rather than chaotic;
    /// "published" is the strange attractor proper.
    #[arg(long, value_enum, default_value_t = Preset::Zensical)]
    preset: Preset,

    #[command(flatten)]
    common: Common,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match cli::run(args.preset.params(), &args.common, DEFAULTS) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("aizawa: {err}");
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
        let args = Args::try_parse_from(["aizawa"]).expect("defaults must parse");
        assert_eq!(args.common.mode, graphix::Mode::Octant);
        assert_eq!(args.preset, Preset::Zensical);
    }

    #[test]
    fn output_modes_conflict() {
        assert!(Args::try_parse_from(["a", "--still", "--gif", "x.gif"]).is_err());
        assert!(
            Args::try_parse_from(["a", "--frames", "3", "--out", "d", "--gif", "x.gif"]).is_err()
        );
        assert!(Args::try_parse_from(["a", "--frames", "3"]).is_err()); // needs --out
    }

    #[test]
    fn the_published_preset_is_selectable() {
        let args = Args::try_parse_from(["a", "--preset", "published"]).expect("preset must parse");
        assert_eq!(args.preset, Preset::Published);
    }

    #[test]
    fn the_system_defaults_apply_when_the_command_line_is_silent() {
        let args = Args::try_parse_from(["a"]).expect("defaults must parse");
        assert_eq!(args.common.tilt.unwrap_or(DEFAULTS.tilt), DEFAULTS.tilt);
        let given = Args::try_parse_from(["a", "--tilt", "70"]).expect("tilt must parse");
        assert_eq!(given.common.tilt, Some(70.0));
    }

    #[test]
    fn the_view_turns_about_the_third_coordinate() {
        // Aizawa is symmetric about its z axis, unlike the cyclic systems.
        assert_eq!(Preset::Zensical.params().symmetry_axis(), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_still_frame_covers_the_terminal_grid() {
        let args = Args::try_parse_from(["a", "--size", "64", "--orbits", "8", "--trail", "64"])
            .expect("args must parse");
        let renderer = args.common.renderer(args.preset.params(), DEFAULTS);
        let art = graphix::render_image(&renderer.frame_at(0.0), 80, 24, graphix::Mode::Octant);
        // A square frame is row-limited: 24 rows of 48 cells is square again.
        assert_eq!(art.lines().count(), 24);
    }

    #[test]
    fn the_terminal_output_is_drawn_from_the_octant_repertoire() {
        // graphix covers the pattern-to-glyph mapping itself. What this checks
        // is that the frames this crate produces carry enough detail per cell
        // to reach the octant block at all, rather than collapsing to whole
        // blocks the way a flat or empty image would.
        let args = Args::try_parse_from(["a", "--size", "128", "--orbits", "24"])
            .expect("args must parse");
        let renderer = args.common.renderer(args.preset.params(), DEFAULTS);
        let art = graphix::render_image(&renderer.frame_at(0.0), 80, 24, graphix::Mode::Octant);

        let octants = art
            .chars()
            .filter(|c| ('\u{1CD00}'..='\u{1CDE5}').contains(c))
            .count();
        assert!(octants > 20, "only {octants} octant glyphs in the output");

        // Nothing outside what octant mode is allowed to draw from: the octant
        // range, the Block Elements and quarter blocks it folds onto for the
        // patterns Unicode encodes elsewhere, and the SGR escapes around them.
        let stray: Vec<char> = art
            .chars()
            .filter(|&c| {
                !matches!(c,
                    '\u{1CD00}'..='\u{1CDE5}'
                    | '\u{2580}'..='\u{259F}'
                    | '\u{1CEA0}'..='\u{1CEAB}'
                    | '\u{1FB82}'..='\u{1FB85}'
                    | '\u{1FBE6}'..='\u{1FBE7}'
                    | ' ' | '\n' | '\u{1b}' | '[' | ';' | 'm' | '0'..='9'
                )
            })
            .collect();
        assert!(stray.is_empty(), "unexpected glyphs: {stray:?}");
    }
}
