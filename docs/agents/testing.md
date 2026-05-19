# Test infrastructure

## Where tests live

| Location | What it tests |
|---|---|
| `src/diagnostics.rs` `#[cfg(test)]` | `collect_diagnostics()` — late vars, incomplete exprs |
| `src/symbols.rs` `#[cfg(test)]` | `extract_symbols()` — params, locals, functions |
| `src/line_index.rs` `#[cfg(test)]` | `LineIndex` — byte↔position conversions, UTF-16 |
| `src/script_env.rs` `#[cfg(test)]` | INI parsing, globals section, symbol positions |
| `src/resolve/tests/` | Everything in the `resolve/` submodules (~3400 lines across 11 files, most comprehensive) |
| `src/semantic_tokens/tests.rs` | `collect_semantic_tokens()` — classify, resolve, encode |
| `src/bin/witcherscript-lsp/tests.rs` | LSP-specific: encoding, hover markdown, completion items, rename |
| `src/bin/witcherscript-lsp/tests/e2e/` | Wire-level E2E tests that drive the real `Backend` over a tokio duplex pair with framed JSON-RPC |
| `tests/parser_fixtures.rs` | Parametrized parse tests over all fixture files |
| `tests/language_features.rs` | Integration tests for symbol extraction + resolution |

## Fixture-based parse tests

`tests/parser_fixtures.rs` discovers and runs tests on all `.ws` files in two directories:

**`tests/fixtures/valid/`** — all must parse with zero diagnostics

| File | Constructs covered |
|---|---|
| `basic_function.ws` | top-level function, local vars, if, return |
| `mod_annotations_and_defaults.ws` | enum, struct, class with inheritance, @addField/@wrapMethod/@addMethod, defaults blocks, timer function, array<T>, for loop, new expr |
| `state_machine.ws` | statemachine class, state X in Y, entry function, event OnEnterState/OnLeaveState, while, SleepOneFrame, super.X, parent.X |

**`tests/fixtures/invalid/`** — all must produce at least one diagnostic

| File | Error |
|---|---|
| `bad_parameter_list.ws` | parameter without `:` type separator → tree-sitter error |
| `missing_semicolon.ws` | var decl without `;` → tree-sitter "missing" |
| `unclosed_block.ws` | unclosed class body brace → tree-sitter error |

When adding a new grammar feature or parse rule, add or update a fixture rather than relying solely on unit tests for complex syntax.

## resolve/tests/ — authoritative test patterns

This directory (~3400 lines across 11 files) is the canonical reference for how to write resolution and completion tests. Use it as examples before adding new tests under `src/resolve/tests/`.

| File | What it covers |
|---|---|
| `definition.rs` | `resolve_definition` — top-level functions, methods, enum variants, receiver vars |
| `references.rs` | `find_references` — scoping, include_declaration flag, private member scoping |
| `inheritance.rs` | `this`/`super`/`parent`, access levels, inherited method resolution |
| `chaining.rs` | Method-on-return-value, multi-level chained calls |
| `script_globals.rs` | INI globals, redirect to class, local shadows global |
| `parameters.rs` | `parameters_of`, `wrap_method_snippet` |
| `completion_members.rs` | `completion_members` — dot-access, tier ordering |
| `completion_statement.rs` | `statement_completions` — locals, members, globals, `this`/`super`, loop/switch flags, context guards |
| `completion_type.rs` | `type_completions`, `extends_completions` |
| `completion_keywords.rs` | `class_body_keyword_completions` — specifier state machine |
| `completion_annotation.rs` | `annotation_arg_completions`, `after_wrap_method_completions` |

**Test categories covered:**
- Definition resolution for top-level functions, class methods, enum variants, fields, locals, parameters
- Word-boundary and cursor-position edge cases
- Protected/private visibility scoping (private = file-only; protected = accessible from subclass)
- Method resolution through inheritance chains
- `this.member`, `super.method`, `parent.X` (state→owner class, public only)
- Variable receiver type inference (`obj.Method()` → resolve obj → get type → find Method)
- Chained calls: `func().method().chain()`
- `this`/`super`/`parent` keyword resolution
- Script globals from INI redirecting to class definitions
- `completion_members()` with tier ordering (own < inherited)
- `type_completions()` returning class/struct/enum/builtin types
- `statement_completions()` with locals, members, globals, has_this, has_super
- Exec/quest functions excluded from statement completions
- `find_references()` with include_declaration flag
- Private member scoping to file
- Local variable scoping to function

**Test fixture helper pattern (from language_features.rs):**
```rust
let source = include_str!("fixtures/valid/mod_annotations_and_defaults.ws");
let doc = parse_document(source).unwrap();
let mut index = WorkspaceIndex::default();
index.update_document("file:///test.ws", &doc);
let base = WorkspaceIndex::default();
let db = SymbolDb::new(&index, &base);

// resolve a symbol at a position
let result = resolve_definition("file:///test.ws", &doc, &db, SourcePosition { line: 5, character: 10 });
assert!(result.is_some());
```

**Inline source pattern (from resolve/tests.rs):**
```rust
fn make_doc(source: &str) -> ParsedDocument { parse_document(source).unwrap() }
fn make_index(uri: &str, doc: &ParsedDocument) -> WorkspaceIndex {
    let mut idx = WorkspaceIndex::default();
    idx.update_document(uri, doc);
    idx
}
```

## tests/e2e/ wire-level LSP tests

`src/bin/witcherscript-lsp/tests/e2e/` exercises the full LSP stack: tests speak framed JSON-RPC to a real `Backend` running inside the same process. The server is wired with the same `Router` + `LifecycleLayer` + `CatchUnwindLayer` + `ConcurrencyLayer` + `TracingLayer` stack as `main.rs`, so a regression in dispatch, encoding, or framing fails a test rather than slipping past the unit suite.

| File | What it covers |
|---|---|
| `harness.rs` | `LspClient::spawn()` builds the server on a `tokio::io::duplex` pair and returns a client that speaks `Content-Length`-framed JSON. Exposes `request<R>`, `notify<N>`, `open`, `change_full`, `wait_diagnostics`. |
| `fixture.rs` | Parses inline source with rust-analyzer-style markers: `//- /path.ws` for multi-file, `$0` for the cursor, `//^^^ label` for an expected span on the line above. |
| `definition.rs` | `textDocument/definition` golden paths |
| `completion.rs` | `textDocument/completion` golden paths |
| `hover.rs` | `textDocument/hover` golden paths |
| `rename.rs` | `textDocument/rename` golden paths |
| `diagnostics.rs` | `textDocument/publishDiagnostics` after open and after change |

**Fixture format:**

```rust
let f = Fixture::parse(concat!(
    "function Foo() {}\n",
    "//       ^^^ name\n",
    "function Bar() { Fo$0o(); }\n",
));
```

- `$0` is the cursor (exactly one per fixture).
- `//^^^ label` annotates a span on the previous content line; `Fixture::span("label")` returns `(Url, Range)`.
- `//- /lib.ws` starts a new virtual file in a multi-file fixture; without any `//- ` marker the source lands under `file:///main.ws`.
- Annotation lines are stripped before the source is sent to the server, so positions reference the *stripped* line numbering.

**Writing an E2E test:**

```rust
#[tokio::test]
async fn definition_resolves_function_callsite_to_declaration() {
    let f = Fixture::parse(concat!(
        "function Foo() {}\n",
        "//       ^^^ name\n",
        "function Bar() { Fo$0o(); }\n",
    ));
    let mut client = LspClient::spawn().await;
    for file in &f.files {
        client.open(&file.uri, &file.text).await;
    }
    let (cursor_uri, pos) = f.cursor();
    let resp = client.request::<GotoDefinition>(/* params at pos */).await;
    let (expected_uri, expected_range) = f.span("name");
    // assert resp points at expected_uri/expected_range
}
```

**When to add one:**

- Wiring up a new LSP capability: one golden-path E2E case per request type.
- A bug that only manifested through the JSON-RPC layer (lsp-types schema mismatch, layer ordering, dispatch arm).

For pure resolve / completion / inference logic, prefer `src/resolve/tests/`; E2E tests should not duplicate that coverage.

## Running tests

```
just test      # cargo fmt + cargo clippy + cargo nextest run
just ci        # cargo fmt --check + cargo clippy -D warnings + cargo nextest run
```

Both recipes run tests via [cargo-nextest](https://nexte.st), which produces a compact per-test status table instead of the verbose `cargo test` output. Install locally with `cargo binstall cargo-nextest` or `winget install nextest.cargo-nextest`. Config lives at `.config/nextest.toml`.

There are no doctests in this repo, so a separate `cargo test --doc` step is not needed.

## When to add what kind of test

| Scenario | Where to add |
|---|---|
| New grammar construct | Fixture in `tests/fixtures/valid/` + `parser_fixtures.rs` picks it up automatically |
| New validation rule | Unit test in `diagnostics.rs` + fixture in `tests/fixtures/invalid/` if complex |
| New symbol kind | Test in `symbols.rs` `#[cfg(test)]` + cases in `resolve/tests.rs` |
| New resolution case | Test in `resolve/tests.rs` (inline source) |
| New completion case | Test in `resolve/tests.rs` or a new `language_features.rs` test |
| New LSP handler | Test in `src/bin/witcherscript-lsp/tests.rs` (handler logic) + a golden-path case in `tests/e2e/<feature>.rs` (wire-level) |
| Regression that only shows up over JSON-RPC (serialization, dispatch, framing) | Test in `src/bin/witcherscript-lsp/tests/e2e/` |
| New semantic token | Test in `semantic_tokens/tests.rs` |

## Benchmarks

Performance is tracked in two layers under `benches/`. Layout and intent:

| File | Tool | Where it runs | Purpose |
|---|---|---|---|
| `benches/lib_*.rs` | criterion | Local | Wall-clock timing of parse / symbols / index / resolve / completion. Use during refactor iteration with `just bench-baseline` and `just bench-compare`. |
| `benches/iai_lib.rs` | iai-callgrind | CI (Linux) + local on WSL | Instruction counts under cachegrind. Deterministic; immune to CI runner noise. This is the regression-gating layer. |
| `benches/lsp_smoke.rs` | criterion | Local only | Spawns the release `witcherscript-lsp` binary and measures cold start + warm request latency over stdio. Local sanity check, not gated. |
| `benches/common/synth.rs` | helper | n/a | Deterministic `synth_file` / `synth_workspace` generators. Each bench includes it via `#[path = "common/synth.rs"]`. |

The wire-level smoke bench shares `src/bin/witcherscript-lsp/tests/jsonrpc_client.rs` with the E2E harness: same framing code, two transports.

### How the iai-callgrind gate works in CI

iai-callgrind writes per-bench instruction counts under `target/iai/` and, on each run, compares against whatever previous results it finds in that directory. CI persists those results across runs via `actions/cache`:

- Cache key is `iai-<os>-<commit-sha>`. Restore key is the prefix `iai-<os>-`.
- Only `push` events on `master` save the cache. PR runs restore but never save. That keeps the restore prefix resolving to a master baseline only: a PR can't poison its own follow-up commit by saving a regressed cache that the next commit restores from.
- If a PR finds the cache empty (e.g. GitHub expired it after a week of no activity), the job checks out `origin/master`, runs iai once to seed `target/iai/`, then checks back out to the PR HEAD before the gating run. That keeps the gate live across quiet periods at the cost of one extra bench run on cold-cache PRs.
- The seed step skips itself when `origin/master` does not yet have `benches/iai_lib.rs` (e.g. on the PR that first introduces the gate, before master has caught up). It emits a GitHub Actions warning annotation so the inactive-gate condition is obvious from the workflow summary, but the job stays green.
- `IAI_CALLGRIND_REGRESSION=ir=5.0` makes the job fail if instruction reads (`ir`) climb more than 5% vs the restored baseline. The first ever run on master prints absolute counts and exits zero, since there's nothing to compare against yet.

Fixture work inside each `#[library_benchmark]` runs in a `#[bench(setup = ...)]` callback so cachegrind only measures the target call, not the surrounding parse / index build. See `benches/iai_lib.rs` for the pattern.

Recipes:

```
just bench                   # criterion library benches (wall-clock)
just bench-baseline pre      # save a labelled criterion baseline
just bench-compare pre       # compare current run against saved baseline
just bench-iai               # iai-callgrind (requires valgrind, Linux/WSL)
just bench-lsp               # criterion LSP smoke benches against release binary
```

When adding a new hot path that needs perf coverage:

1. Add a criterion bench under `benches/lib_<area>.rs` for local iteration.
2. Add an iai-callgrind bench in `benches/iai_lib.rs` for CI gating.
3. If the path is reached only through the LSP wire protocol, also add a scenario to `benches/lsp_smoke.rs`.

CI runs only `iai_lib` (on `ubuntu-latest` with valgrind installed). Criterion benches are local; CI wall-clock measurement is too noisy to gate on.

## assert_symbol helper

`tests/language_features.rs` defines a small helper used in integration tests:

```rust
fn assert_symbol(symbols: &DocumentSymbols, kind: SymbolKind, name: &str) {
    assert!(symbols.all().iter().any(|s| s.kind == kind && s.name == name),
        "expected symbol {name:?} of kind {kind:?}");
}
```

Use this pattern when verifying symbol extraction in integration tests.
