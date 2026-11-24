# Default recipe: list available commands
default:
    @just --list

# Format all code (Rust + Nix + Markdown)
fmt:
    treefmt

# Check formatting (Rust + Nix + Markdown)
fmt-check:
    treefmt --fail-on-change --no-cache

# Run clippy lints
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run all tests
test:
    cargo test --all-features

# Run tests with verbose output
test-verbose:
    cargo test --all-features -- --nocapture

# Build release
build:
    cargo build --release --all-features

# Generate documentation
doc *args='':
    cargo doc --no-deps --all-features {{args}}

release_targets := "x86_64-unknown-linux-musl aarch64-unknown-linux-musl"

# Assert the release tag names the version cargo would publish
check-version version:
    @pkgid="$(cargo pkgid -p graphix)"; crate="v${pkgid##*#}"; \
    if [ "$crate" != "{{version}}" ]; then \
        echo "tag {{version}} does not match crate version $crate" >&2; exit 1; \
    fi

# Build the static musl release artifacts and their checksums into dist/
dist version: (check-version version)
    rm -rf dist && mkdir -p dist
    for target in {{release_targets}}; do \
        set -e; \
        cargo build --release --locked --target "$target" -p graphix; \
        name="graphix-{{version}}-$target"; \
        tar -czf "dist/$name.tar.gz" -C "target/$target/release" graphix; \
        ( cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256" ); \
    done
    @ls -1 dist

readme_args := "--project-root crates/graphix --no-title --no-license --no-badges --no-indent-headings"

# Regenerate README.md from the graphix crate docs
readme:
    cargo readme {{readme_args}} > README.md

# Check README.md is in sync with the crate docs
readme-check:
    cargo readme {{readme_args}} | diff - README.md

recipes_dir := "crates/pixel-blocks/recipes"

# Regenerate the built-in block recipes as TOML
recipes:
    cargo run -q -p pixel-blocks -- --dump-recipes {{recipes_dir}}

# Check the TOML recipes are in sync with the built-in definitions
recipes-check:
    cargo run -q -p pixel-blocks -- --dump-recipes target/recipes-check
    diff -r {{recipes_dir}} target/recipes-check

# Run CI checks locally
ci: fmt-check lint test doc readme-check recipes-check build
    @echo "All CI checks passed!"

# Seconds each animated demo runs before stopping on its own
demo_secs := "8"

# Showcase a renderer: image, blocks, planets, flake, aizawa, thomas, or all of them
demo name='all':
    @just demo-{{name}}

# Every demo in turn, stills first
demo-all: demo-image demo-blocks demo-planets demo-flake demo-aizawa demo-thomas

# A PNG as ANSI art, in every granularity graphix offers
demo-image:
    cargo run -q --release -p pixel-blocks -- --scene crates/pixel-blocks/scenes/arch.txt --out target/demo.png
    for mode in shade half-block sextant braille octant; do \
        printf '\n  %s\n\n' "$mode"; \
        cargo run -q --release -p graphix -- target/demo.png -H 20 -m "$mode"; \
    done

# Every built-in isometric block, side by side
demo-blocks:
    cargo run --release -p pixel-blocks

# A procedurally generated planet, turning under its clouds
demo-planets:
    cargo run --release -p pixel-planets -- --duration {{demo_secs}}

# The NixOS lambda flake, spinning
demo-flake:
    cargo run --release -p flake -- --duration {{demo_secs}} --tilt 25

# The Aizawa attractor, turning about its axis
demo-aizawa:
    cargo run --release -p aizawa -- --duration {{demo_secs}}

# The Thomas attractor, turning about its body diagonal
demo-thomas:
    cargo run --release -p thomas -- --duration {{demo_secs}}

# Render an example block scene (arch, stairs)
blocks scene='arch':
    cargo run --release -p pixel-blocks -- --scene crates/pixel-blocks/scenes/{{scene}}.txt

# Watch for changes and run tests
watch:
    cargo watch -x test

# Clean build artifacts
clean:
    cargo clean
