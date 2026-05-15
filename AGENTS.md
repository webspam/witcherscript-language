# Agent guidelines for witcherscript-language

## Repository overview

This is a Rust crate (`witcherscript-parser`) that produces two binaries:

- `witcherscript-parser` — CLI syntax validator (`src/main.rs`)
- `witcherscript-lsp` — LSP server (`src/bin/witcherscript-lsp/`)

## Module quick reference

| File                           | Purpose                                                                        | Detail doc                                           |
| ------------------------------ | ------------------------------------------------------------------------------ | ---------------------------------------------------- |
| `src/lib.rs`                   | Module declarations, public API surface                                        |                                                      |
| `src/document.rs`              | `ParsedDocument`, parse entry points                                           |                                                      |
| `src/diagnostics/`             | `ParseDiagnostic`/`collect_diagnostics` (syntactic), `WorkspaceDiagnostic` (cross-file) | [diagnostics.md](docs/agents/diagnostics.md)         |
| `src/files.rs`                 | Recursive `.ws` file collection via `walkdir`                                  |                                                      |
| `src/line_index.rs`            | Byte ↔ UTF-16 position mapping (LSP-compatible)                                |                                                      |
| `src/script_env.rs`            | Script globals from `redscripts.ini`                                           |                                                      |
| `src/symbols.rs`               | `DocumentSymbols`, `Symbol`, `SymbolKind`, `extract_symbols`                   | [symbols.md](docs/agents/symbols.md)                 |
| `src/resolve/mod.rs`           | `WorkspaceIndex`, `SymbolDb`, `resolve_definition`, completions                | [resolution.md](docs/agents/resolution.md)           |
| `src/resolve/tests/`           | ~3400-line test suite split across 11 focused files — use as pattern reference | [testing.md](docs/agents/testing.md)                 |
| `src/semantic_tokens/mod.rs`   | `TOKEN_TYPES`, `collect_semantic_tokens`, classify                             | [semantic_tokens.md](docs/agents/semantic_tokens.md) |
| `src/semantic_tokens/tests.rs` | Semantic token unit tests                                                      |                                                      |
| `src/main.rs`                  | CLI binary entry point                                                         | [architecture.md](docs/agents/architecture.md)       |
| `src/bin/witcherscript-lsp/`   | LSP server — `Backend` + handlers (`backend.rs`), `main` (`main.rs`), LSP↔internal conversion (`convert.rs`), workspace/base-script indexing (`indexing.rs`), tracing layer (`logging.rs`), tests (`tests.rs`) | [lsp_server.md](docs/agents/lsp_server.md)           |

Full architecture diagram and data flow: [docs/agents/architecture.md](docs/agents/architecture.md)

## Task guide — what to touch for a given task

| Task                                        | Files to modify                                                                                                                                                                                    |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Add a validation rule                       | `src/diagnostics.rs` + test + fixture under `tests/fixtures/invalid/` + README                                                                                                                     |
| Add a new LSP capability                    | `src/bin/witcherscript-lsp/backend.rs` + `src/resolve/mod.rs` if it needs new resolve logic                                                                                                        |
| Add a new symbol kind                       | `src/symbols.rs` (SymbolKind enum), `src/resolve/mod.rs` (hover_text), `src/semantic_tokens/mod.rs` (symbol_kind_to_token_type + classify_ident), `src/bin/witcherscript-lsp/convert.rs` (lsp_symbol_kind) |
| Add a new completion context                | `src/resolve/mod.rs` (new pub fn) + `src/bin/witcherscript-lsp/backend.rs` (completion() dispatch)                                                                                                 |
| Fix a resolution bug                        | `src/resolve/mod.rs` + the relevant file under `src/resolve/tests/`                                                                                                                                |
| Change highlighting                         | `src/semantic_tokens/mod.rs` + `src/semantic_tokens/tests.rs`                                                                                                                                      |
| Fix position/encoding bug                   | `src/line_index.rs` + its `#[cfg(test)]` block                                                                                                                                                     |
| Add WitcherScript syntax support            | Grammar repo (`tree-sitter-witcherscript`) is external; pin new tag in `Cargo.toml`                                                                                                                |
| Inspect grammar node kinds / rule structure | Read `../tree-sitter-witcherscript/grammar.js` (relative to repo root). Online: https://raw.githubusercontent.com/webspam/tree-sitter-witcherscript/refs/heads/master/grammar.js                   |

## WitcherScript language cheat sheet

**Primitive types:** `bool`, `byte`, `float`, `int`, `name`, `string`, `void`

**Declaration keywords:** `class`, `struct`, `enum`, `state`, `function`, `event`, `var`, `autobind`, `defaults`, `hint`

**Class modifiers:** `abstract`, `statemachine`

**Function flavours:** `entry`, `exec`, `quest`, `reward`, `storyscene`, `timer`, `latent`, `import`

**Access modifiers:** `private`, `protected`, `public` (default when absent)

**Variable modifiers:** `editable`, `saved`, `const`, `final`, `optional`, `out`, `inlined`

**Special receivers:** `this` (enclosing class), `super` (base class), `parent` (state → owner class)

**Common modding annotations:**

- `@addField(ClassName)` — inject field into existing class
- `@addMethod(ClassName)` — inject method
- `@wrapMethod(ClassName)` — wrap existing method
- `@replaceMethod(ClassName)` — replace existing method

**State machines:** `statemachine class X extends Y { }` / `state S in X { entry function Run() { } }`

**No static members.** Top-level functions are the global namespace. `exec` and `quest` functions are excluded from completion globals.

**`autobind` declarations** bind game-engine objects into class fields at runtime.

**`CName` literals** use single quotes: `'SomeName'` — classified as `enumMember` in semantic tokens.

## Key invariants

These are the non-obvious constraints that will cause silent bugs if violated:

1. **Symbol IDs = vec index.** `SymbolId(n)` indexes directly into `DocumentSymbols.symbols[n]`. Never reorder, splice, or reuse IDs within a document.

2. **`SourcePosition.character` is UTF-16 code units**, not bytes. ASCII = 1 unit, non-BMP chars = 2 units. The LSP spec requires this. All position conversion goes through `LineIndex`.

3. **Inheritance traversal hard-caps at depth 32.** Both `WorkspaceIndex::find_member_in_chain` and `SymbolDb::find_member_chain_cross` return `None`/empty at depth > 32. This prevents infinite loops from circular or missing base class declarations.

4. **Base/owner class stored in typed fields.** `Symbol.base_class` holds the raw superclass name for classes/structs; `Symbol.owner_class` holds the raw owner class name for states. `Symbol.detail` still contains the human-readable `"extends ClassName"` / `"in OwnerClass"` string for LSP display only — never parse it for structural queries.

5. **Optional parameters are excluded from `parameters_of()`.** `is_optional = true` symbols are skipped when building completion snippet parameter lists. Do not change this — optional params should not appear as required snippet slots.

6. **Three separate indexes.** The LSP maintains `workspace_index` (user project), `base_scripts_index` (read-only game scripts), and the open `documents` map (editor-open files). Requests use `SymbolDb::new(workspace, base)` — workspace shadows base for same-name symbols. Open documents take precedence over the indexed copy of the same file.

7. **Exec/quest functions excluded from global completions.** `all_top_level_callables()` filters signatures starting with `"exec "` or `"quest "`. These are special engine entry-points, not normal callables.

8. **Private members are scoped to their defining file** during `find_references` and semantic token resolution. Do not search or highlight private members across file boundaries.

9. **Text sync is FULL.** Every file change sends the complete document text. There is no incremental tree reuse between edits.

10. **Base scripts are read-only.** `prepare_rename()` rejects symbols _declared_ in `base_scripts_index`. That guard only covers the definition — `rename()` must additionally drop any _reference_ that lands in a base script (via `rename_changes`), since a workspace symbol can still be referenced from base scripts (e.g. an `@addMethod` called inside its target class).

## Build

Use justfile recipes instead of hand-crafting your own build / test commands:

```
just build
```

## Test

```
just test
```

Tests run via [cargo-nextest](https://nexte.st), which produces a compact per-test
status table instead of the verbose `cargo test` output. Install locally with
`cargo binstall cargo-nextest` or `winget install nextest.cargo-nextest`. Config
lives at `.config/nextest.toml`.

The test suite includes:

- Embedded `#[cfg(test)]` modules in `diagnostics/`, `symbols.rs`, `line_index.rs`,
  `script_env.rs`, `resolve/tests/`, `semantic_tokens/tests.rs`, and `src/bin/witcherscript-lsp/tests.rs`.
- `tests/parser_fixtures.rs` — fixture-driven parse tests; discovers every `.ws` file
  under `tests/fixtures/valid/` (must parse cleanly) and `tests/fixtures/invalid/`
  (must produce at least one tree-sitter diagnostic).
- `tests/language_features.rs` — integration tests for symbol extraction and definition
  resolution.

See [docs/agents/testing.md](docs/agents/testing.md) for the full breakdown of what lives where and when to add each kind of test.

IMPORTANT: When adding a new grammar construct or validation rule, add or update a
fixture file and a targeted unit test.

## Committing changes

Commit each logical change as a separate commit as soon as it is complete — do not
accumulate unrelated edits into a single commit. This keeps `git bisect` useful and
makes the history easy to read.

Before committing:

1. Run `cargo fmt --all` to format code.
2. Run `cargo clippy --all-targets` and fix any warnings.
3. Run `just test` and confirm all tests pass.
4. Stage only the files relevant to the change — avoid `git add .` when unrelated files
   are dirty.

### Commit messages

IMPORTANT: The first part of the commit message should be HUMAN RELATABLE. DO NOT just
write which part of the code you changed; instead, what actual problem is it fixing /
what goal is it achieving?

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
- Keep the LSP binary (`src/bin/witcherscript-lsp/`) thin: parse/resolve logic belongs in the
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

1. Enable the capability in `initialize` in `src/bin/witcherscript-lsp/backend.rs`.
2. Implement the handler method on `Backend`.
3. If the handler needs new resolve logic, add it to `resolve/mod.rs` (not the binary).
4. Add a unit test in `src/bin/witcherscript-lsp/tests.rs`.

## Releasing

Version bumps follow the process in [RELEASING.md](RELEASING.md).

## Dependencies

- Do not add new dependencies without a clear reason. Prefer the standard library.
- `tree-sitter` is pinned to `=0.26.8`; do not bump it without also checking the
  `tree-sitter-witcherscript` grammar compatibility.
- The grammar tag is pinned in `Cargo.toml`; retarget it by changing the `tag` field and
  running `cargo update`.
