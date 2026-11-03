# Loads POLARIS_GRAFANA_TOKEN (and anything else) from a local .env file.
set dotenv-load

default_config := "examples/homelab.toml"

# List available recipes
default:
    @just --list

# Run the dashboard in the current terminal (needs POLARIS_GRAFANA_TOKEN)
run config=default_config:
    cargo run --package polaris-tui -- {{config}}

# Run in a fixed-size window mimicking the rack screen (font size in pixels, as kmscon font.size)
run-sized size="1280x400" font_pixels="13" config=default_config:
    scripts/run-sized.sh {{size}} {{font_pixels}} {{config}}

# Validate a configuration file without starting the dashboard
check config=default_config:
    cargo run --quiet --package polaris-tui -- --check {{config}}

# Run every test
test:
    cargo test --workspace

# Lint with warnings denied
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Format Rust and Nix sources
fmt:
    cargo fmt --all
    nix fmt -- flake.nix nix/*.nix

# Run everything CI would: format check, lint, tests and Nix flake checks
ci:
    cargo fmt --all --check
    just lint
    just test
    nix flake check

# Build the native Nix package into ./result
build:
    nix build .#polaris-tui

# Cross-compile the Raspberry Pi Zero 2 W (aarch64) package into ./result-aarch64
build-pi:
    nix build .#polaris-tui-aarch64 --out-link result-aarch64
