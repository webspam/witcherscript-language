# Diagnostics and validation

**Module:** `src/diagnostics/`

For the catalogue of every diagnostic code, its severity, and what it means, see [../diagnostics/validation.md](../diagnostics/validation.md). This doc covers the machinery: where rules live, how they run, and how to add one.

Two diagnostic types, two phases:

- `ParseDiagnostic` - syntactic, single-document. One CST walk over one parse tree; no cross-file knowledge. Computed at parse time, stored on `ParsedDocument.diagnostics`.
- `WorkspaceDiagnostic` - cross-file. Needs the index or sibling documents. Two flavours: *index-walking* rules read a `WorkspaceIndex`; *CST-walking* rules walk each document's tree but consult a `SymbolDb`.

## Module map

Infrastructure (`mod.rs`, `cst_walker.rs`):

- `mod.rs` - the data types (`ParseDiagnostic`, `WorkspaceDiagnostic`, `Severity`, `RelatedLocation`), the syntactic walk (`collect_diagnostics` / `SyntaxDiagnostics`), the CST-rule dispatcher `collect_cst_diagnostics_for_document`, and `format_tree`.
- `cst_walker.rs` - the `CstRule` trait, `CstRuleCtx`, the shared single-walk driver `run_rules_on_document`, the per-call `TypeMemo`, error-subtree tracking, and `run_parallel_pass`.
- `tests.rs` - tests for the syntactic walk.

Index-walking rules, each `collect_*_diagnostics(&WorkspaceIndex, ...) -> HashMap<uri, Vec<WorkspaceDiagnostic>>`:

- `duplicate_symbols.rs`, `duplicate_local.rs`, `base_script_conflict.rs`, `shadowing.rs`.

CST-walking rules, each a unit struct implementing `CstRule`, registered in `collect_cst_diagnostics_for_document`:

- `unknown_method.rs`, `type_mismatch.rs`, `arg_count.rs`, `abstract_instantiation.rs`, `super_field_access.rs`, `state_owner.rs`, `annotation_state_target.rs`, `inherited_field.rs`, `override_consistency.rs`, `parent_outside_state.rs`, `unused_symbol.rs`, `wrapped_method.rs`.
- `unknown_symbol.rs` is the exception: it runs as a separate parallel pass (`run_unknown_symbol_parallel`), not in the shared walk, because it records resolver observations that merge back into the `SymbolDb`.

Each rule module owns its `#[cfg(test)] mod tests`.

## Data types

`ParseDiagnostic` carries a `kind` tag, message, tree-sitter `Point`s (`start`/`end`, 0-indexed), a `byte_range`, and a `snippet` of the source line. It has **no** severity field; severity is decided at LSP-conversion time. `start.row + 1` is the 1-indexed display line.

`WorkspaceDiagnostic` carries its own `Severity`, an already-UTF-16 `SourceRange` (from `Symbol.selection_range`, or converted by the rule via `LineIndex::byte_range_to_range`), a `Vec<RelatedLocation>` (the `relatedInformation` list), and optional `data` (`base_script_conflict` uses it to drive its code action).

## Syntactic pass

`collect_diagnostics(root, source)` runs `SyntaxDiagnostics` (a `CstVisitor`) in a single walk that always descends - missing tokens are often anonymous and must still be visited. `SyntaxDiagnostics::enter` appends a `ParseDiagnostic` for each match on the current node:

- tree-sitter `is_error()` / `is_missing()` nodes (`Syntax error`, or `Missing {kind}`);
- `incomplete_member_access_expr`, `ternary_cond_expr`, `string_linefeed`, `int_overflow` (int/hex literal past 32 bits), `event_return_not_void`, `event_bare_return`, `non_constant_default`, and `struct_property_access_modifier`;
- inside a `func_block`, a `local_var_decl_stmt` that appears after any executable statement (`late_local_var_decl`). `comment` and `nop` are not executable; the rule fires only inside `func_block`.

Each emitted code, message, and severity is listed in [../diagnostics/validation.md](../diagnostics/validation.md).

## Workspace pass: entry points and caching

The LSP serves diagnostics by pull (`textDocument/diagnostic` per URI, `workspace/diagnostic` for the set). Both go through `Backend::compute_diagnostics_for_uri` and `Backend::compute_workspace_diagnostic_report` in `src/bin/witcherscript-lsp/diagnostics_publish.rs`, which assemble each file's report from three sources: the document's `ParseDiagnostic`s, the index-walking bundle, and the CST-walking results.

- Index-walking rules run in `collect_workspace_diagnostics` and are memoised as a `DiagnosticsBundle`, keyed by a `BundleFingerprint` (workspace + loose generations, base surface, env, legacy-dirs hash).
- CST-walking rules run through `cst_diagnostics_with_cache` (`cst_cache.rs`), a per-document cache keyed by `(parse_version, DbFingerprint)`; misses compute in parallel across files.
- A legacy-override file shows only `base_script_conflict`, not the overlapping `duplicate_symbol` (`duplicates_not_explained_by_conflict`).

## LSP conversion

In `src/bin/witcherscript-lsp/convert/diagnostics.rs`. Every diagnostic carries `code = kind` and `source = "witcherscript"`.

- `lsp_diagnostics` converts a document's `ParseDiagnostic`s, mapping `byte_range` through `LineIndex`; severity is `ERROR` for all except `ternary_cond_expr` (`WARNING`).
- `lsp_workspace_diagnostic` uses the `WorkspaceDiagnostic`'s own `Severity`, attaches the `Unnecessary` tag for `unused_symbol` (so editors fade it), and passes through `related` and `data`.
- `base_script_conflict_code_actions` turns a `base_script_conflict`'s `data` into an "add to legacyScriptDirectories" quick fix.

## format_tree

`format_tree(root)` dumps the full CST for debugging - one line per node, `{kind}` plus an `ERROR`/`MISSING` marker, its `{row}:{col}` span, and byte range. Used by the CLI `--dump-tree` flag.

## Adding a new validation rule

Every new rule needs a row in [../diagnostics/validation.md](../diagnostics/validation.md).

**Syntactic (single-document):** add a check in `mod.rs` and fire it from `SyntaxDiagnostics::enter` on the target node kind; add a case to `tests.rs`. For a rule best exercised by a whole file, add a fixture under `tests/fixtures/invalid/` (it must produce at least one diagnostic).

**Workspace, index-walking** (reads `WorkspaceIndex` / `ScriptEnvironment`, no CST walk): add a submodule returning `HashMap<uri, Vec<WorkspaceDiagnostic>>`, re-export its entry point from `mod.rs`, and call it from `collect_workspace_diagnostics` in `diagnostics_publish.rs` - extending `BundleFingerprint` if it reads an input not already covered. Unit-test in the submodule (fixtures cannot express cross-file rules).

**Workspace, CST-walking** (inspects each tree, consults the `SymbolDb`):

1. Add a submodule with a unit struct implementing `CstRule`. `interested_in(kind)` returns `true` only for the kinds it inspects (the dispatcher short-circuits on it); `visit` bails when `ctx.in_error_subtree`, then pushes into `ctx.diagnostics`. Use `infer_type_memo(..., ctx.type_memo)` for receiver-type inference so chained calls share work.
2. Register the struct in `collect_cst_diagnostics_for_document` in `mod.rs`. The pull handlers and the `cst_cache` (keyed on `(parse_version, DbFingerprint)`) pick it up automatically.
3. Unit-test in the submodule.

Never walk the tree yourself in a CST rule: register interest with `CstRule` so all rules share one walk per document and the per-call `TypeMemo` survives across rule invocations.
