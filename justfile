set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

alias b := build
alias r := release
alias t := test
alias f := fmt
alias c := clippy
alias s := lsp-listen
alias serve := lsp-listen

# Show available recipes.
default:
    @just --list

# Format Rust code, run clippy fix & tests - optimised for agents
test: fmt clippy-fix
    cargo nextest run

# Format all Rust code
fmt:
    cargo fmt --all

# Clippy check
clippy:
    cargo clippy --all-targets --all-features

# Clippy fix
clippy-fix:
    cargo clippy --fix --all-targets --all-features

# Run CI checks - skips clippy pedantic
ci:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings -A clippy::pedantic
    cargo nextest run

# Build dev binary
build:
    cargo build

# Build the optimised release binary
release:
    cargo build --release

release-lsp:
    cargo build --release --bin witcherscript-lsp

release-format:
    cargo build --release --bin wsformat

# Run criterion library benches (wall-clock, local)
bench:
    cargo bench --bench lib_parse --bench lib_symbols --bench lib_index --bench lib_resolve --bench lib_completion

# Save a criterion baseline (e.g. before a refactor): just bench-baseline pre
bench-baseline name:
    cargo bench --bench lib_parse --bench lib_symbols --bench lib_index --bench lib_resolve --bench lib_completion -- --save-baseline {{name}}

# Compare current run against a saved baseline
bench-compare name:
    cargo bench --bench lib_parse --bench lib_symbols --bench lib_index --bench lib_resolve --bench lib_completion -- --baseline {{name}}

# Run iai-callgrind benches (instruction counts; requires valgrind, Linux or WSL)
bench-iai:
    cargo bench --bench iai_lib

# Run the local LSP smoke benches against the release-built binary
bench-lsp:
    cargo bench --bench lsp_smoke

# Run the LSP server in TCP listen mode (default port 9257). Stderr -> target/lsp-tcp.log.
# Uses `cmd /c` for the redirect because PowerShell's `2>` mangles native stderr
# (UTF-16 + NativeCommandError wrapping); cmd does true fd-2 redirection.
lsp-listen port='9257':
    cargo build --bin witcherscript-lsp
    if (Test-Path target/lsp-tcp.log) { Remove-Item target/lsp-tcp.log }
    Write-Host "witcherscript-lsp listening on 127.0.0.1:{{port}} (logs -> target/lsp-tcp.log)"
    cmd /c "target\debug\witcherscript-lsp.exe --listen {{port}} 2> target\lsp-tcp.log"
