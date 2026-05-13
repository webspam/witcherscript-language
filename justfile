set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

alias b := build
alias r := release
alias t := test

# Show available recipes.
default:
    @just --list

# Format Rust code, run clippy & tests - optimised for agents
test:
    cargo fmt --all
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test -q -- --format terse

# Run the standard local verification.
ci:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test

# Build dev binary.
build:
    cargo build

# Build the optimised release binary.
release:
    cargo build --release
