# Test infrastructure

For *how* to write tests (style, patterns, helpers), see [writing-tests.md](writing-tests.md). This file is orientation: where tests live and how to run them.

## Where tests live

- Library unit tests: `#[cfg(test)] mod tests` at the bottom of the file they cover (e.g. `line_index.rs`, `script_env.rs`).
- Per-area test modules, one subsystem each: `src/diagnostics/`, `src/symbols/`, `src/resolve/tests/`, `src/semantic_tokens/`, `src/formatter/tests/`.
- LSP tests: `src/bin/witcherscript-lsp/tests/` (handler logic) and `.../tests/e2e/` (framed JSON-RPC against a real `Backend`).
- Whole-workspace E2E: `.../tests/e2e/session/` drives a real server against on-disk workspaces under `tests/workspaces/`, with `insta` snapshots (see below).
- Shared toolkit: `src/test_support/` - `TestDb`, the `Fixture` marker parser, and name-assertion helpers, exposed via the on-by-default `test-support` Cargo feature.
- Crate-root integration tests: `tests/parser_fixtures.rs` and `tests/language_features.rs`.

## Fixture markers

`TestDb::new(fixture_str)` and the e2e `Fixture::parse(fixture_str)` share one string format:

- `$0` - exactly one cursor (stripped before parsing).
- `//^^^ label` - annotates a span on the **previous content line**; retrievable via `t.span("label")`.
- `//- /path.ws` - starts a new virtual file; without any `//-`, content lands under `file:///main.ws`.

Annotation lines are stripped before parsing, so positions reference the *stripped* line numbering. Positions are UTF-16 code units (LSP-compatible).

## Parse-fixture directories

`tests/fixtures/valid/*.ws` must parse with zero diagnostics; `tests/fixtures/invalid/*.ws` must produce at least one. `tests/parser_fixtures.rs` auto-discovers and runs both, so adding a grammar feature only needs a new fixture file. (`tests/fixtures/formatter/` is not auto-discovered; the formatter tests `include_str!` it directly.)

## Whole-workspace E2E suite

`EditorSession` tests in `tests/e2e/session/` drive a real server over on-disk workspaces - derive from an existing scenario.

- Workspaces: `tests/workspaces/<name>/` - a `workspace.toml` (roots and `witcherscript.*` settings) plus the `.ws` files; positional probes use the file's `$0` cursor.
- Snapshots: `tests/e2e_snapshots/`. A new snapshotting test needs `let _guard = e2e_snapshots().bind_to_scope();`.

## Running tests

```
just test      # cargo fmt + cargo clippy + cargo nextest run
just ci        # cargo fmt --check + cargo clippy -D warnings + cargo nextest run
```

- After changing an output formatter (hover markdown, snippet, diagnostic message): `UPDATE_EXPECT=1 cargo test` rewrites stale `expect![[]]` literals.
- There are no doctests.
