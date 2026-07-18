use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use graphix::{fit_grid, render_cells, to_ansi};

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
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("failed to read {path}: {source}")]
    Image {
        path: PathBuf,
        source: image::ImageError,
    },
}

fn terminal_cells() -> (u32, u32) {
    match terminal_size::terminal_size() {
        Some((terminal_size::Width(w), terminal_size::Height(h))) => {
            (u32::from(w.max(1)), u32::from(h.saturating_sub(1).max(1)))
        }
        None => (80, 24),
    }
}

fn run(args: &Args) -> Result<(), Error> {
    let img = image::open(&args.image)
        .map_err(|source| Error::Image {
            path: args.image.clone(),
            source,
        })?
        .to_rgb8();

    let (term_cols, term_rows) = terminal_cells();
    let max_cols = args.width.unwrap_or(term_cols).max(1);
    let max_rows = args.height.unwrap_or(term_rows).max(1);

    let (cols, rows) = fit_grid(img.width(), img.height(), max_cols, max_rows);
    print!("{}", to_ansi(&render_cells(&img, cols, rows)));
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("graphix: {err}");
            ExitCode::FAILURE
        }
    }
}
