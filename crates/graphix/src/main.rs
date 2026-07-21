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

    /// Instead of printing ANSI, rasterize the cell grid to a PNG at this path:
    /// a font-free, pixel-perfect preview of the terminal output.
    #[arg(long, value_name = "PATH")]
    raster: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let (term_cols, term_rows) = graphix::terminal_grid();
    let max_cols = args.width.unwrap_or(term_cols).max(1);
    let max_rows = args.height.unwrap_or(term_rows).max(1);
    match run(&args, max_cols, max_rows) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("graphix: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args, max_cols: u32, max_rows: u32) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(out) = &args.raster {
        let img = graphix::image::open(&args.image)?.to_rgb8();
        let (cols, rows) = graphix::fit_grid(img.width(), img.height(), max_cols, max_rows);
        let grid = graphix::render_cells(&img, cols, rows, args.mode);
        graphix::rasterize(&grid, 8, 16).save(out)?;
    } else {
        let art = graphix::render_file(&args.image, max_cols, max_rows, args.mode)?;
        print!("{art}");
    }
    Ok(())
}
