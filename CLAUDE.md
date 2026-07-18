# graphix

Render PNG images as 24-bit ANSI art with `░▒▓█` shading blocks, sized to the terminal.

## Architecture

This is a Cargo workspace with crates under `crates/`:

- **graphix**: Library (image → cell grid → ANSI string) and the CLI binary
- **conventional-commit-check**: The commit-msg git hook binary

The rendering pipeline lives in `crates/graphix/src/lib.rs`:

1. `fit_grid` scales image dimensions into the terminal grid (a cell counts as 1x2 pixels
   of aspect, so images keep their proportions)
2. `render_cells` maps each cell's source-pixel region to a `Cell`: pixels split into a
   dark/light cluster by mean luminance; dark average = ANSI background, light average =
   foreground, and the light share picks the block (`░` 25%, `▒` 50%, `▓` 75%, `█` 100%)
3. `to_ansi` serializes the grid with 24-bit SGR sequences, deduplicating consecutive
   color runs and resetting at the end of every line

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
