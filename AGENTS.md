# Agent guidelines for witcherscript-language

## Repository overview

This is a Rust crate (`witcherscript-language`) that produces two binaries:

- `witcherscript-check` - CLI syntax validator (`src/main.rs`)
- `witcherscript-lsp` - LSP server (`src/bin/witcherscript-lsp/`)

## Module quick reference

| File                           | Purpose                                                                        | Detail doc                                           |
| ------------------------------ | ------------------------------------------------------------------------------ | ---------------------------------------------------- |
| `src/lib.rs`                   | Module declarations                                                             |                                                      |
| `src/document.rs`              | `ParsedDocument`, parse entry points                                           |                                                      |
| `src/cst/`                     | Shared tree-sitter CST traversal primitives - use these, never hand-roll a walk | _no detail doc yet_                                  |
| `src/diagnostics/`             | `ParseDiagnostic`/`collect_diagnostics` (syntactic), `WorkspaceDiagnostic` (cross-file) | [diagnostics.md](docs/agents/diagnostics.md)         |
| `src/files.rs`                 | Recursive `.ws` file collection via the `ignore` crate; `canonical_uri` URI normalisation | [lsp_server.md](docs/agents/lsp_server.md#uri-handling) |
| `src/line_index.rs`            | Byte ↔ UTF-16 position mapping (LSP-compatible)                                |                                                      |
| `src/script_env.rs`            | Script globals from `redscripts.ini`                                           |                                                      |
| `src/symbols/`                 | `DocumentSymbols`, `Symbol`, `SymbolKind`, `extract_symbols`                   | [symbols.md](docs/agents/symbols.md)                 |
| `src/builtins.rs` + `builtins/` | Synthetic engine types (`array<T>`) embedded from `.ws` files                  | [builtins.md](docs/agents/builtins.md)               |
| `src/formatter.rs` + `formatter/` | Document formatter - powers `textDocument/formatting`                        | _no detail doc yet_                                  |
| `src/resolve/`                 | Resolution, inference, references, signatures, completion, and the extract refactors. `symbol_db/` + `workspace_index/` back lookups; `completion/` and `extract_*` are submodule trees. See detail doc for the full layout. | [resolution.md](docs/agents/resolution.md)           |
| `src/resolve/tests/`           | Test suite split across many focused files - use as pattern reference | [testing.md](docs/agents/testing.md)                 |
| `src/semantic_tokens/mod.rs`   | `TOKEN_TYPES`, `collect_semantic_tokens`, classify                             | [semantic_tokens.md](docs/agents/semantic_tokens.md) |
| `src/semantic_tokens/tests.rs` | Semantic token unit tests                                                      |                                                      |
| `src/main.rs`                  | CLI binary entry point                                                         | [architecture.md](docs/agents/architecture.md)       |
| `src/bin/witcherscript-lsp/`   | LSP server: `Backend` + thin `LanguageServer` impl (`backend.rs`), handlers grouped by concern (`completion.rs`, `queries.rs`, `references_rename.rs`, `text_sync.rs`, `lifecycle.rs`), plus `convert/`, `indexing/`, `cst_cache.rs`, `watcher.rs`, and tests under `tests/`. See detail doc. | [lsp_server.md](docs/agents/lsp_server.md)           |
| `benches/`                     | Perf benches: criterion `lib_*.rs` (local wall-clock), `iai_lib.rs` (iai-callgrind, CI regression gate), `lsp_smoke.rs` (local LSP-binary smoke); shared synth in `common/synth.rs` | [testing.md](docs/agents/testing.md#benchmarks)      |

Full architecture diagram and data flow: [docs/agents/architecture.md](docs/agents/architecture.md)

## Task guide - what to touch for a given task

| Task                                        | Files to modify                                                                                                                                                                                    |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Add a validation rule                       | `src/diagnostics/` (rule module + `mod.rs`) + test + fixture under `tests/fixtures/invalid/` + README                                                                                                                     |
| Add a new LSP capability                    | `src/bin/witcherscript-lsp/lifecycle.rs` (advertise it in `_initialize`) + a new handler in the file matching its LSP concern (`completion.rs`, `queries.rs`, `references_rename.rs`, `text_sync.rs`) + a trait-impl shim in `backend.rs` + the appropriate `src/resolve/` submodule if it needs new resolve logic + a wire-level golden-path case in `src/bin/witcherscript-lsp/tests/e2e/<feature>.rs` |
| Add a new symbol kind                       | `src/symbols/types.rs` (SymbolKind enum), `src/resolve/signature.rs` (hover_text), `src/semantic_tokens/mod.rs` (symbol_kind_to_token_type + classify_ident), `src/bin/witcherscript-lsp/convert/symbols.rs` (lsp_symbol_kind) |
| Add a new completion context                | `src/resolve/completion/` (new pub fn in the relevant submodule) + `src/bin/witcherscript-lsp/completion.rs` (`_completion` dispatch)                                                              |
| Fix a resolution bug                        | `src/resolve/definition.rs` or `src/resolve/inference.rs` + the relevant file under `src/resolve/tests/`                                                                                           |
| Change highlighting                         | `src/semantic_tokens/mod.rs` + `src/semantic_tokens/tests.rs`                                                                                                                                      |
| Fix position/encoding bug                   | `src/line_index.rs` + its `#[cfg(test)]` block                                                                                                                                                     |
| Add WitcherScript syntax support            | Grammar repo (`tree-sitter-witcherscript`) is external; pin new tag in `Cargo.toml`                                                                                                                |
| Inspect grammar node kinds / rule structure | Read `../tree-sitter-witcherscript/grammar.js` (relative to repo root). Online: https://raw.githubusercontent.com/webspam/tree-sitter-witcherscript/refs/heads/master/grammar.js                   |
| Add or edit a built-in method (e.g. `array.NewMethod`) | Edit `builtins/<name>.ws` + add a test under `src/resolve/tests/builtin_<name>.rs`                                                                                |

## WitcherScript language cheat sheet

Primitives, keywords, modifiers, receivers, annotations, and state-machine syntax: [docs/agents/language.md](docs/agents/language.md).

## Key invariants

The non-obvious constraints that cause silent bugs if violated (symbol IDs, UTF-16 positions, inheritance depth cap, the index model, loose files, read-only base scripts, incremental text sync, ...): [docs/agents/invariants.md](docs/agents/invariants.md). Read these before touching resolution, indexing, or text sync.

## Build

Use justfile recipes instead of hand-crafting your own build / test commands:

```
just build
```

## Test

```
just test
```

Tests run via cargo-nextest, which produces a compact per-test status table
instead of the verbose `cargo test` output.

The test suite includes:

- Embedded `#[cfg(test)]` modules in `diagnostics/`, `symbols/`, `line_index.rs`,
  `script_env.rs`, `resolve/tests/`, `semantic_tokens/tests.rs`, and `src/bin/witcherscript-lsp/tests.rs`.
- `tests/parser_fixtures.rs` - fixture-driven parse tests; discovers every `.ws` file
  under `tests/fixtures/valid/` (must parse cleanly) and `tests/fixtures/invalid/`
  (must produce at least one tree-sitter diagnostic).
- `tests/language_features.rs` - integration tests for symbol extraction and definition
  resolution.

See [docs/agents/testing.md](docs/agents/testing.md) for the full breakdown of what lives where and when to add each kind of test.

IMPORTANT: When adding a new grammar construct or validation rule, add or update a
fixture file and a targeted unit test.

## Committing changes

Commit each logical change as a separate commit as soon as it is complete - do not
accumulate unrelated edits into a single commit. This keeps `git bisect` useful and
makes the history easy to read.

Before committing:

1. Run `just test` and confirm all tests pass (runs fmt and clippy automatically).
2. Stage only the files relevant to the change - avoid `git add .` when unrelated files
   are dirty.

### Commit messages

IMPORTANT: The first part of the commit message should be HUMAN RELATABLE. DO NOT just
write which part of the code you changed; instead, what actual problem is it fixing /
what goal is it achieving?

Commit message format: one imperative-mood subject line (≤50 chars). Be CONCISE. Examples:

```txt
Add hover text for enum member symbols
```

```txt
Fix late-local-var rule skipping nop statements
```

## Code style

See `CODESTYLE.md` for the normative Rust code standard.

## Adding a validation rule

1. Add the detection logic in `src/diagnostics/` (extend `collect_diagnostics` in `mod.rs` or add a new rule module).
2. Add a unit test in `src/diagnostics/tests.rs` or the rule module.
3. Add or extend a fixture file under `tests/fixtures/` if the rule is complex enough to warrant one.
4. Document the new rule in the "Diagnostics" section of `README.md`.

## Adding an LSP capability

1. Enable the capability in `_initialize` in `src/bin/witcherscript-lsp/lifecycle.rs`.
2. Add the `_handler` method on `Backend` in the file matching its LSP concern: completions in `completion.rs`; hover/definition/symbols/signature-help/semantic-tokens/formatting/code-action in `queries.rs`; references/rename in `references_rename.rs`; text sync + workspace folder events in `text_sync.rs`.
3. Wire the trait method in `backend.rs` to call `self._handler(params).await`.
4. If the handler needs new resolve logic, add it to the appropriate `resolve/` submodule (not the binary).
5. Add a unit test in the relevant `src/bin/witcherscript-lsp/tests/<feature>.rs`.

## Releasing

Version bumps follow the process in [RELEASING.md](RELEASING.md).

## Dependencies

- Do not add new dependencies without a clear reason. Prefer the standard library.
- `tree-sitter` is pinned to `=0.26.8`; do not bump it without also checking the
  `tree-sitter-witcherscript` grammar compatibility.
- The grammar tag is pinned in `Cargo.toml`; retarget it by changing the `tag` field and
  running `cargo update`.
