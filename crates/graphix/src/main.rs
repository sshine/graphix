use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// Render a PNG image as 24-bit ANSI art sized to the terminal.
#[derive(Debug, Parser)]
#[command(name = "graphix", version, about)]
struct Args {
    /// Path to the input PNG image.
    image: PathBuf,

    /// Maximum output width in terminal columns (defaults to the terminal width).
    #[arg(short = 'w', long)]
    width: Option<u32>,

    /// Maximum output height in terminal rows (defaults to the terminal height,
    /// minus one row for the shell prompt).
    #[arg(short = 'H', long)]
    height: Option<u32>,

    /// Rendering granularity, coarsest to finest: shade blocks (shade), half
    /// blocks (half-block), 2x3 sextants (sextant), 2x4 braille dots (braille),
    /// or 2x4 octants (octant, needs a Unicode 16 font).
    #[arg(short = 'm', long, value_enum, default_value_t = graphix::Mode::Shade)]
    mode: graphix::Mode,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let (term_cols, term_rows) = graphix::terminal_grid();
    let max_cols = args.width.unwrap_or(term_cols).max(1);
    let max_rows = args.height.unwrap_or(term_rows).max(1);
    match graphix::render_file(&args.image, max_cols, max_rows, args.mode) {
        Ok(art) => {
            print!("{art}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("graphix: {err}");
            ExitCode::FAILURE
        }
    }
}
