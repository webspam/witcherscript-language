set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

alias r := release
alias t := test

# Show available recipes.
default:
    @just --list

# Format Rust code and run tests.
test:
    cargo fmt --all
    cargo test

# Run the standard local verification.
ci:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test

# Build the optimised release binary.
release:
    cargo build --release
