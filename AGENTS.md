# Agent guidelines for witcherscript-language

## Repository overview

This is a Rust crate (`witcherscript-parser`) that produces two binaries:

- `witcherscript-parser` — CLI syntax validator (`src/main.rs`)
- `witcherscript-lsp` — LSP server (`src/bin/witcherscript-lsp.rs`)

The library surface is in `src/`:

| File             | Purpose                                                                                   |
| ---------------- | ----------------------------------------------------------------------------------------- |
| `lib.rs`         | Module declarations                                                                       |
| `diagnostics.rs` | `ParseDiagnostic`, tree-error and late-local-var collection, `format_tree`                |
| `document.rs`    | `ParsedDocument` (source + tree + line_index + diagnostics + symbols), parse entry points |
| `files.rs`       | Recursive `.ws` file collection via `walkdir`                                             |
| `line_index.rs`  | Byte-offset ↔ UTF-16 line/character position mapping (LSP-compatible)                     |
| `resolve.rs`     | `WorkspaceIndex`, `resolve_definition`, `hover_text`                                      |
| `symbols.rs`     | `DocumentSymbols`, `Symbol`, `SymbolKind`, `extract_symbols`                              |

Grammar: `tree-sitter-witcherscript` pinned to `v0.13.0` from the `webspam` GitHub fork.

## Build

```
cargo build
```

## Test

```
cargo test
```

The test suite includes:

- Embedded `#[cfg(test)]` modules in `diagnostics.rs`, `symbols.rs`, `line_index.rs`,
  `resolve.rs`, and `src/bin/witcherscript-lsp.rs`.
- `tests/parser_fixtures.rs` — fixture-driven parse tests; discovers every `.ws` file
  under `tests/fixtures/valid/` (must parse cleanly) and `tests/fixtures/invalid/`
  (must produce at least one tree-sitter diagnostic).
- `tests/language_features.rs` — integration tests for symbol extraction and definition
  resolution.

IMPORTANT: When adding a new grammar construct or validation rule, add or update a fixture file and
a targeted unit test.

## Committing changes

Commit each logical change as a separate commit as soon as it is complete — do not
accumulate unrelated edits into a single commit. This keeps `git bisect` useful and
makes the history easy to read.

Before committing:

1. Run `cargo fmt --all` to format code.
2. Run `cargo clippy --all-targets` and fix any warnings.
3. Run `cargo test` and confirm all tests pass.
4. Stage only the files relevant to the change — avoid `git add .` when unrelated files
   are dirty.

Commit message format: one imperative-mood subject line (≤50 chars), blank line, then
optional body. Be CONCISE. Examples:

```
Add hover text for enum variant symbols

Extend hover_text() to emit "enum variant <Name>" for EnumVariant kind.
```

```
Fix late-local-var rule skipping nop statements
```

## Code style

- No comments unless the reason is non-obvious (hidden constraint, workaround, subtle
  invariant). Never describe _what_ the code does.
- No `unwrap()` in library code; use `?` or `Option`/`Result` combinators. `unwrap()` is
  acceptable in tests.
- No `pub` on symbols that do not need to be visible outside the module.
- Keep the LSP binary (`witcherscript-lsp.rs`) thin: parse/resolve logic belongs in the
  library, not in the binary.
- `LineIndex` positions are always UTF-16 code-unit offsets to stay compatible with the
  LSP specification.

## Adding a validation rule

1. Add the detection logic in `diagnostics.rs` (extend `collect_diagnostics` or add a
   new `collect_*` helper).
2. Add a unit test directly in `diagnostics.rs`.
3. Add or extend a fixture file under `tests/fixtures/` if the rule is complex enough to
   warrant one.
4. Document the new rule in the "Diagnostics" section of `README.md`.

## Adding an LSP capability

1. Enable the capability in `initialize` in `src/bin/witcherscript-lsp.rs`.
2. Implement the handler method on `Backend`.
3. If the handler needs new resolve logic, add it to `resolve.rs` (not the binary).
4. Add a unit test in the `#[cfg(test)]` block at the bottom of `witcherscript-lsp.rs`.

## Dependencies

- Do not add new dependencies without a clear reason. Prefer the standard library.
- `tree-sitter` is pinned to `=0.22.6`; do not bump it without also checking the
  `tree-sitter-witcherscript` grammar compatibility.
- The grammar tag is pinned in `Cargo.toml`; retarget it by changing the `tag` field and
  running `cargo update`.
