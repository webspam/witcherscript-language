# Test infrastructure

For *how* to write tests (style, patterns, helpers), see [writing-tests.md](writing-tests.md). This file is the inventory + non-obvious detail.

## Where tests live

| Location | What it tests |
|---|---|
| `src/diagnostics/tests.rs` | `collect_diagnostics()` - syntactic CST rules: late locals, struct access modifiers, int-literal overflow, ternary |
| `src/diagnostics/<rule>/tests.rs` | One file per diagnostic rule (see `unknown_symbol/tests.rs` as the cleanest pattern) |
| `src/symbols/tests.rs` | `extract_symbols()` |
| `src/line_index.rs` `#[cfg(test)]` | `LineIndex` - byte↔position conversions, UTF-16 |
| `src/script_env.rs` `#[cfg(test)]` | INI parsing, globals section, symbol positions |
| `src/resolve/tests/` | `resolve/` submodules - definition, references, completion, inheritance, signature help, builtins |
| `src/semantic_tokens/tests.rs` | `collect_semantic_tokens()` - classify, resolve, encode |
| `src/formatter/tests/` | `format_document()` output; some cases `include_str!` from `tests/fixtures/formatter/` |
| `src/bin/witcherscript-lsp/tests/` | LSP handler unit tests (`completion.rs`, `diagnostics.rs`, `hover.rs`, `indexing/*.rs`, `refactoring.rs`) |
| `src/bin/witcherscript-lsp/tests/e2e/` | Wire-level E2E - framed JSON-RPC against a real `Backend` over `tokio::io::duplex` |
| `src/test_support/` | Shared toolkit: `TestDb`, `Fixture` marker parser, name assertion helpers |
| `tests/parser_fixtures.rs` | Auto-discovers `tests/fixtures/{valid,invalid}/*.ws` and asserts parse-clean / diagnostic-emitted |
| `tests/language_features.rs` | Cross-cutting integration: symbol extraction + resolution over a known fixture file |

## resolve/tests/ inventory

| File | Covers |
|---|---|
| `definition.rs` | `resolve_definition` + `resolve_all_definitions` |
| `references.rs` | `find_references` - scoping, include_declaration flag, private member scoping |
| `inheritance.rs` | `this`/`super`/`parent`, access levels, inherited method resolution |
| `chaining.rs` | Method-on-return-value, multi-level chained calls |
| `inference.rs` | `infer_type` - expression type inference |
| `base_shadowing.rs` | Mod-over-base shadowing: suppressed base stays indexed but out of top-level lookup |
| `overrides.rs` | `overridden_top_level` - which base symbol a mod definition overrides |
| `state_classes.rs` | Synthetic `CState*` class name resolution |
| `script_globals.rs` | INI globals, redirect to class, local shadows global |
| `parameters.rs` | `display_parameters_of` - ordering, optional flags, multi-name groups |
| `completion_members.rs` | `completion_members` - dot-access, tier ordering |
| `completion_statement.rs` | `statement_completions` - locals, members, globals, has_this/has_super, in_loop/in_switch |
| `completion_type.rs` | `type_completions`, `extends_completions`, `state_owner_completions`, `class_header_keyword_completions` |
| `completion_keywords.rs` | `class_body_keyword_completions` - specifier state machine |
| `completion_script_keywords.rs` | Script-level keyword completions |
| `completion_new.rs` | Class slot of a `new` expression (`new_type_completions`, `new_lifetime_completions`) |
| `completion_comment.rs` | `position_in_comment` - completion suppressed inside comments |
| `completion_annotation_name.rs` | `annotation_name_completions` |
| `completion_annotation_arg.rs` | `annotation_arg_completions` |
| `completion_annotation_wrap.rs` | `override_completions` (overridable-method list after `@wrapMethod`/`@replaceMethod`) |
| `completion_annotation_replace_global.rs` | `override_completions` global-function case (`@replaceMethod` without `()`) |
| `completion_annotation_body.rs` | Inside `@addMethod` / `@wrapMethod` bodies: statement, member, definition resolution |
| `completion_default_hint.rs` | `default_or_hint_member_completions` |
| `builtin_array.rs` | Built-in `array<T>` resolution, `parse_generic_type`, members/hover via `load_builtins_index` |
| `builtin_classes.rs` / `builtin_enums.rs` / `builtin_native_types.rs` | Embedded engine classes, enums, native (`CBehTreeVal*`) types |
| `index.rs` | `WorkspaceIndex::all_top_level` - multi-document iteration |
| `signature_help.rs` | `signature_help` - parameter hints + active param tracking |
| `document_highlight.rs`, `inlay_hints.rs`, `workspace_symbols.rs` | One file per like-named LSP feature |
| `extract_func.rs`, `extract_method.rs`, `extract_var.rs`, `inline_var.rs`, `join_split_decl.rs`, `extract_access.rs` | Refactoring code actions; `extract_access` pins the access-level matrix |
| `mod.rs` | `make_doc` helper + submodule declarations |

## Canonical examples

When writing a new test, copy the closest existing pattern instead of re-deriving:

- **Resolve / completion at a cursor** → `src/resolve/tests/inheritance.rs` (`TestDb::new` + `$0` marker + `#[rstest] #[case]`).
- **Multi-document fixture** → `src/resolve/tests/definition.rs` cases using `//- /path.ws` headers.
- **Inline-snapshot golden output** (hover markdown, formatter output) → `src/bin/witcherscript-lsp/tests/hover.rs` (`expect-test`).
- **Wire-level LSP request** → `src/bin/witcherscript-lsp/tests/e2e/definition.rs` (`Fixture::parse` + `LspClient::spawn`).
- **Per-diagnostic test module** → `src/diagnostics/unknown_symbol/tests.rs` (`TestDb` + `collect_unknown_symbol_diagnostics` + a `kinds` helper over the result).
- **Decoded semantic-token assertions** → `src/semantic_tokens/tests.rs` (`decode_tokens` → `Vec<SemanticTokenView>`).

## Fixture markers

`TestDb::new(fixture_str)` and the e2e `Fixture::parse(fixture_str)` accept the same string format:

- `$0` - exactly one cursor (stripped before parsing).
- `//^^^ label` - annotates a span on the **previous content line**; retrievable via `t.span("label")`.
- `//- /path.ws` - starts a new virtual file; without any `//-`, content lands under `file:///main.ws`.
- Annotation lines are stripped before the source reaches the parser, so positions reference the *stripped* line numbering.

Positions are UTF-16 code units (LSP-compatible).

## Parse-fixture directory

`tests/fixtures/valid/` - all `.ws` files must parse with zero diagnostics. `tests/fixtures/invalid/` - all must produce at least one diagnostic. `tests/parser_fixtures.rs` discovers and runs both. When adding a grammar feature, add a fixture rather than relying solely on unit tests. (`tests/fixtures/formatter/` is not auto-discovered; the formatter tests `include_str!` it directly.)

## Running tests

```
just test      # cargo fmt + cargo clippy + cargo nextest run
just ci        # cargo fmt --check + cargo clippy -D warnings + cargo nextest run
```

The `test-support` Cargo feature (on by default) exposes `witcherscript_language::test_support::*` so the LSP binary's test crate and integration tests can use the same `TestDb` / `Fixture` helpers as the library's own tests. Release builders that want to drop the helpers entirely can pass `--no-default-features`.

After changing an output formatter (hover markdown, snippet, diagnostic message): `UPDATE_EXPECT=1 cargo test` rewrites stale `expect![[]]` literals in place. For `insta` snapshots: `cargo insta review`.

There are no doctests.

## When to add what

| Scenario | Where |
|---|---|
| New grammar construct | Fixture in `tests/fixtures/valid/` + `parser_fixtures.rs` picks it up automatically |
| New validation rule | Unit test in `src/diagnostics/<rule>/tests.rs` (use `unknown_symbol/` as the template) + fixture in `tests/fixtures/invalid/` if complex |
| New symbol kind | `symbols/tests.rs` + cases in `src/resolve/tests/` |
| New resolution case | `src/resolve/tests/` (use `TestDb::new` with a `$0` marker) |
| New completion case | `src/resolve/tests/` |
| New LSP handler | `src/bin/witcherscript-lsp/tests/<feature>.rs` (handler logic) + a golden-path case in `tests/e2e/<feature>.rs` (wire-level) |
| Regression that only shows up over JSON-RPC | `src/bin/witcherscript-lsp/tests/e2e/` |
| New semantic token | `semantic_tokens/tests.rs` (use `decode_tokens` + `SemanticTokenView`) |
