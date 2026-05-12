# Test infrastructure

## Where tests live

| Location | What it tests |
|---|---|
| `src/diagnostics.rs` `#[cfg(test)]` | `collect_diagnostics()` — late vars, incomplete exprs |
| `src/symbols.rs` `#[cfg(test)]` | `extract_symbols()` — params, locals, functions |
| `src/line_index.rs` `#[cfg(test)]` | `LineIndex` — byte↔position conversions, UTF-16 |
| `src/script_env.rs` `#[cfg(test)]` | INI parsing, globals section, symbol positions |
| `src/resolve/tests.rs` | Everything in `resolve/mod.rs` (~1800 lines, most comprehensive) |
| `src/semantic_tokens/tests.rs` | `collect_semantic_tokens()` — classify, resolve, encode |
| `src/bin/witcherscript-lsp.rs` `#[cfg(test)]` | LSP-specific: encoding, hover markdown, completion items, rename |
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

## resolve/tests.rs — authoritative test patterns

This file (~1800 lines) is the canonical reference for how to write resolution and completion tests. Use it as examples before writing new tests in `resolve/mod.rs`.

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

## Running tests

```
just test      # cargo fmt + cargo test (minimal output)
just ci        # cargo fmt --check + cargo clippy -D warnings + cargo test
```

## When to add what kind of test

| Scenario | Where to add |
|---|---|
| New grammar construct | Fixture in `tests/fixtures/valid/` + `parser_fixtures.rs` picks it up automatically |
| New validation rule | Unit test in `diagnostics.rs` + fixture in `tests/fixtures/invalid/` if complex |
| New symbol kind | Test in `symbols.rs` `#[cfg(test)]` + cases in `resolve/tests.rs` |
| New resolution case | Test in `resolve/tests.rs` (inline source) |
| New completion case | Test in `resolve/tests.rs` or a new `language_features.rs` test |
| New LSP handler | Test in `witcherscript-lsp.rs` `#[cfg(test)]` |
| New semantic token | Test in `semantic_tokens/tests.rs` |

## assert_symbol helper

`tests/language_features.rs` defines a small helper used in integration tests:

```rust
fn assert_symbol(symbols: &DocumentSymbols, kind: SymbolKind, name: &str) {
    assert!(symbols.all().iter().any(|s| s.kind == kind && s.name == name),
        "expected symbol {name:?} of kind {kind:?}");
}
```

Use this pattern when verifying symbol extraction in integration tests.
