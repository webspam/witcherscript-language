# Agent guidelines for witcherscript-language

## Repository overview

This is a Rust crate (`witcherscript-language`) that produces two binaries:

- `witcherscript-check` - CLI syntax validator (`src/main.rs`)
- `witcherscript-lsp` - LSP server (`src/bin/witcherscript-lsp/`)

## Detail docs

Start with [architecture.md](docs/agents/architecture.md) for the source file tree, module graph, data-flow pipeline, and index model. Then the area docs:

| Doc | Covers |
| --- | --- |
| [resolution.md](docs/agents/resolution.md) | Resolution, inference, references, signatures, completion; `SymbolDb` / `WorkspaceIndex` |
| [mod_resolve.md](docs/agents/mod_resolve.md) | Rules to follow when editing resolve / parsing / syntax code (read first) |
| [symbols.md](docs/agents/symbols.md) | `DocumentSymbols`, `Symbol`, `SymbolKind`, `extract_symbols` |
| [diagnostics.md](docs/agents/diagnostics.md) | Syntactic and cross-file validation rules |
| [semantic_tokens.md](docs/agents/semantic_tokens.md) | `TOKEN_TYPES`, classification, highlighting |
| [lsp_server.md](docs/agents/lsp_server.md) | LSP backend: handlers, capabilities, URI handling, indexing, text sync |
| [builtins.md](docs/agents/builtins.md) | Embedded engine types (`array<T>`, classes, enums) |
| [class_body_specifiers.md](docs/agents/class_body_specifiers.md) | Which specifiers and flavours are valid in a class body |
| [testing.md](docs/agents/testing.md) | Test inventory, fixtures, benchmarks |
| [writing-tests.md](docs/agents/writing-tests.md) | How to write tests: style, helpers, fixture markers |
| [language.md](docs/agents/language.md) | WitcherScript language cheat sheet |
| [invariants.md](docs/agents/invariants.md) | Non-obvious constraints that cause silent bugs |

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

## Build and test

Use justfile recipes, not hand-rolled cargo commands: `just build`, and `just test` (fmt + clippy + nextest in one). The test inventory, fixtures, and benchmarks are in [docs/agents/testing.md](docs/agents/testing.md).

IMPORTANT: When adding a new grammar construct or validation rule, add or update a fixture file and a targeted unit test.

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
