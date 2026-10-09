use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use pixel_blocks::{BlockRecipe, BlockSize, Error, Scene, ascii, builtin, scene};

/// Draw isometric pixel-art blocks.
///
/// With no arguments, every built-in block is drawn side by side. Give
/// --scene to lay blocks out on a grid, or --out to write a PNG with a
/// transparent background instead of printing to the terminal.
#[derive(Debug, Parser)]
#[command(name = "pixel-blocks", version, about)]
struct Args {
    /// Draw one block by name, rather than all of them.
    #[arg(short, long, conflicts_with = "scene")]
    block: Option<String>,

    /// Draw the blocks laid out by a scene file.
    #[arg(short, long, value_name = "PATH")]
    scene: Option<PathBuf>,

    /// Load extra block recipes from these TOML files; they take precedence
    /// over the built-ins of the same name.
    #[arg(short, long = "recipe", value_name = "PATH")]
    recipes: Vec<PathBuf>,

    /// Texels along a cube edge, as a texture's resolution.
    #[arg(short, long, default_value_t = 16)]
    texels: u32,

    /// Pixels per texel.
    #[arg(long, default_value_t = 4)]
    scale: u32,

    /// Seed for every procedural pattern.
    #[arg(long, default_value_t = 0)]
    seed: u64,

    /// Write a PNG here instead of printing ANSI to the terminal.
    #[arg(short, long, value_name = "PATH")]
    out: Option<PathBuf>,

    /// Write every built-in recipe to this directory as TOML and exit.
    #[arg(long, value_name = "DIR", conflicts_with_all = ["block", "scene", "out"])]
    dump_recipes: Option<PathBuf>,

    /// Terminal rendering granularity (see graphix).
    #[arg(short = 'm', long, value_enum, default_value_t = graphix::Mode::Octant)]
    mode: graphix::Mode,

    /// Background color behind the blocks in the terminal, as `r,g,b`.
    #[arg(long, default_value = "0,0,0", value_parser = parse_color)]
    background: [u8; 3],
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pixel-blocks: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), Error> {
    let catalog = catalog(&args.recipes)?;
    if let Some(dir) = &args.dump_recipes {
        return dump_recipes(dir);
    }

    let size = BlockSize::new(args.texels, args.scale);
    let scene = match (&args.scene, &args.block) {
        (Some(path), _) => {
            let text = std::fs::read_to_string(path)?;
            ascii::parse(&text)?.build(size, args.seed, |name| find(&catalog, name))?
        }
        (None, Some(name)) => {
            let recipe = find(&catalog, name).ok_or_else(|| unknown_block(name, &catalog))?;
            let mut scene = Scene::new();
            let handle = scene.add(recipe.bake(args.seed, size)?);
            scene.place(0, 0, 0, handle);
            scene
        }
        (None, None) => sampler(&catalog, size, args.seed)?,
    };

    let img = scene.render();
    match &args.out {
        Some(path) => img.save(path)?,
        None => {
            let rgb = scene::flatten(&img, args.background);
            let (cols, rows) = graphix::terminal_grid();
            print!("{}", graphix::render_image(&rgb, cols, rows, args.mode));
        }
    }
    Ok(())
}

/// Every built-in recipe, with the ones read from `paths` overriding or
/// extending them.
fn catalog(paths: &[PathBuf]) -> Result<Vec<BlockRecipe>, Error> {
    let mut catalog = builtin::all();
    for path in paths {
        let recipe = BlockRecipe::from_toml(&std::fs::read_to_string(path)?)?;
        match catalog.iter_mut().find(|r| r.name == recipe.name) {
            Some(existing) => *existing = recipe,
            None => catalog.push(recipe),
        }
    }
    Ok(catalog)
}

/// The recipe named `name`, if the catalog has one.
fn find(catalog: &[BlockRecipe], name: &str) -> Option<BlockRecipe> {
    catalog.iter().find(|r| r.name == name).cloned()
}

/// The error for a block nobody has a recipe for, listing what is available.
fn unknown_block(name: &str, catalog: &[BlockRecipe]) -> Error {
    let known: Vec<&str> = catalog.iter().map(|r| r.name.as_str()).collect();
    Error::Recipe {
        name: format!("{name} (known: {})", known.join(", ")),
        reason: "no recipe with that name",
    }
}

/// Every block in the catalog in a row, as the reference image lays them out.
fn sampler(catalog: &[BlockRecipe], size: BlockSize, seed: u64) -> Result<Scene, Error> {
    let mut scene = Scene::new();
    for (i, recipe) in catalog.iter().enumerate() {
        let handle = scene.add(recipe.bake(seed, size)?);
        // Stepping along x and back along y moves one block to the right
        // without moving it up or down.
        let step = i as i32;
        scene.place(step, -step, 0, handle);
    }
    Ok(scene)
}

/// Write every built-in recipe to `dir` as `<name>.toml`.
fn dump_recipes(dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir)?;
    for recipe in builtin::all() {
        std::fs::write(dir.join(format!("{}.toml", recipe.name)), recipe.to_toml()?)?;
    }
    Ok(())
}

/// Parse an `r,g,b` color for clap.
fn parse_color(text: &str) -> Result<[u8; 3], String> {
    let parts: Vec<&str> = text.split(',').collect();
    let [r, g, b] = parts.as_slice() else {
        return Err(format!(
            "expected three comma-separated values, got {text:?}"
        ));
    };
    let channel = |s: &str| s.trim().parse::<u8>().map_err(|e| format!("{s:?}: {e}"));
    Ok([channel(r)?, channel(g)?, channel(b)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let args = Args::try_parse_from(["pixel-blocks"]).expect("defaults must parse");
        assert_eq!(args.mode, graphix::Mode::Octant);
        assert_eq!(args.texels, 16);
        assert_eq!(args.background, [0, 0, 0]);
        assert!(args.block.is_none() && args.scene.is_none());
    }

    #[test]
    fn a_block_and_a_scene_conflict() {
        assert!(Args::try_parse_from(["p", "-b", "stone", "-s", "x.txt"]).is_err());
        assert!(Args::try_parse_from(["p", "--dump-recipes", "d", "-o", "x.png"]).is_err());
    }

    #[test]
    fn colors_parse_and_bad_ones_are_rejected() {
        let args = Args::try_parse_from(["p", "--background", "10, 20,30"]).expect("parse");
        assert_eq!(args.background, [10, 20, 30]);
        for bad in ["1,2", "1,2,3,4", "1,2,300", "red"] {
            assert!(
                Args::try_parse_from(["p", "--background", bad]).is_err(),
                "{bad} parsed"
            );
        }
    }

    #[test]
    fn an_unknown_block_lists_the_ones_that_exist() {
        let args = Args::try_parse_from(["p", "-b", "quartz"]).expect("parse");
        let err = run(&args).expect_err("quartz is not a block");
        let message = err.to_string();
        assert!(
            message.contains("quartz") && message.contains("stone"),
            "{message}"
        );
    }

    #[test]
    fn the_sampler_draws_every_block_in_one_row() {
        // Each block steps right without stepping down, so the row is one
        // sprite tall however many blocks it holds.
        let size = BlockSize::new(4, 1);
        let scene = sampler(&builtin::all(), size, 0).expect("bake");
        let img = scene.render();
        assert_eq!(img.height(), size.sprite());
        assert_eq!(img.width(), size.sprite() * builtin::all().len() as u32);
    }
}
