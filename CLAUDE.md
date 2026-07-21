# graphix

Render PNG images as 24-bit ANSI art with Unicode blocks or braille dots, sized to the terminal.

## Architecture

This is a Cargo workspace with crates under `crates/`:

- **graphix**: Library (image → cell grid → ANSI string) and the CLI binary
- **pixel-planets**: Procedural pixel-art planet simulator/renderer (stateless: seed + params)
- **conventional-commit-check**: The commit-msg git hook binary

Crates follow one shape: a library holding the logic and public API (doc comments on all
public items), plus a thin clap binary returning ExitCode. Workspace membership is automatic
(`crates/*`); crates inherit `workspace.package` fields, take dependencies from
`[workspace.dependencies]` via `.workspace = true`, and opt into workspace lints with
`[lints] workspace = true`.

The rendering pipeline lives in `crates/graphix/src/lib.rs`:

1. `fit_grid` scales image dimensions into the terminal grid (a cell counts as 1x2 pixels
   of aspect, so images keep their proportions)
1. `render_cells` maps each cell's source-pixel region to a `Cell` per the chosen `Mode`:
   - `Shade` (Block Elements `░▒▓█`): pixels split into a dark/light cluster by mean
     luminance; dark average = ANSI background, light average = foreground, and the light
     share picks the block (`░` 25%, `▒` 50%, `▓` 75%, `█` 100%)
   - `HalfBlock` (Block Elements `▀`): two square pixels per cell, top half = foreground,
     bottom half = background, each averaged exactly
   - `Sextant` (Block Sextants U+1FB00..=U+1FB3B): a 2x3 solid-fill matrix per cell
   - `Braille` (Braille Patterns U+2800..=U+28FF): a 2x4 dot matrix per cell
   - `Octant` (Block Octants U+1CD00..=U+1CDE5, Unicode 16): a 2x4 solid-fill matrix per cell
   - `Sextant`/`Braille`/`Octant` share `lit_subcells`: subdivide the cell into an NxM grid
     and light each subcell nearer the light cluster than the dark; sextant/octant then fold
     patterns Unicode omits (halves, quadrants) back onto the Block Elements glyphs
1. `to_ansi` serializes the grid with 24-bit SGR sequences, deduplicating consecutive
   color runs and resetting at the end of every line

## Visual feedback (for Claude)

The sandbox cannot take terminal screenshots, but PNG files can be viewed with the Read tool:

```sh
cargo run -p graphix -- <img.png> -m octant --raster out.png   # font-free PNG preview of the ANSI output
cargo run --release -p pixel-planets -- --frames 4 --out dir/  # planet animation frames as PNGs
```

## Development

### Nix (recommended)

This project uses a Nix flake for reproducible development:

```sh
direnv allow    # auto-activates the dev shell
# or manually:
nix develop     # enter the dev shell
```

Git hooks are managed by hk-nix (pre-commit: treefmt; pre-push: deadnix, clippy,
readme-check, lock-check; commit-msg: conventional commits).

The flake is dendritic: flake-parts + import-tree over `nix/`, so every `.nix` file there is
a flake-parts module. Adding a crate needs one `buildCrate` line in `nix/packages.nix`.

### Common commands

```sh
just            # list all available commands
just fmt        # format code
just lint       # run clippy
just test       # run all tests
just demo       # render the sample image
just ci         # run all CI checks locally
```

## Conventions

- Use `thiserror` for error types; avoid `unwrap()` (denied by clippy lint)
- All public items should have doc comments
- README.md is generated from the graphix crate's doc comment (`just readme`)
- Run `just ci` before pushing
