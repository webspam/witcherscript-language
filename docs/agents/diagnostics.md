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

- `unknown_method.rs`, `type_mismatch.rs`, `abstract_instantiation.rs`, `super_field_access.rs`, `state_owner.rs`, `annotation_state_target.rs`, `inherited_field.rs`, `override_consistency.rs`, `unused_symbol.rs`, `wrapped_method.rs`.
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

## Workspace diagnostics

Some checks need cross-file knowledge and cannot run off a single parse tree. These
produce `WorkspaceDiagnostic` (with a `relatedInformation`-style `Vec<RelatedLocation>`)
instead of `ParseDiagnostic`. Positions are already UTF-16 `SourceRange`s - taken from
`Symbol.selection_range` - so no `LineIndex` round-trip is needed at conversion time.

`collect_duplicate_symbol_diagnostics(&WorkspaceIndex) -> HashMap<uri, Vec<WorkspaceDiagnostic>>`
flags any two top-level declarations (class/struct/enum/state/function/event) sharing a
name. Symbols carrying modding annotations are skipped - `@addMethod`/`@wrapMethod`/etc.
functions are member injections, not fresh global names. It enumerates raw symbols via
`WorkspaceIndex::all_top_level()` because `top_level_by_name` dedups by name.

`collect_duplicate_local_diagnostics(&WorkspaceIndex) -> HashMap<uri, Vec<WorkspaceDiagnostic>>`
flags parameters or local variables that share a name within the same function scope
(kind `"duplicate_local"`). Functions annotated with `@wrapMethod` or `@replaceMethod`
are exempt because those annotations intentionally redeclare parameters from the wrapped
signature.

The LSP computes workspace diagnostics across the whole index and serves them via pull
(`textDocument/diagnostic` for a single URI, `workspace/diagnostic` for the whole set).
`Backend::compute_diagnostics_for_uri` and `Backend::compute_workspace_diagnostic_report` in
`diagnostics_publish.rs` are the two entry points; both merge the cross-file workspace
diagnostics with the document's syntactic `ParseDiagnostic`s.

## LSP conversion

All diagnostics are returned in pull reports as:
- Severity: `ERROR` for most rules; `WARNING` for shadowing, the `ternary_cond_expr`
  `ParseDiagnostic`, and any rule that sets `Severity::Warning` on its `WorkspaceDiagnostic`
- Code: the `kind` string
- Source: `"witcherscript"`
- Range: `ParseDiagnostic` is converted from `byte_range` via
  `line_index.byte_range_to_range(source, start, end)`; `WorkspaceDiagnostic` already
  carries a UTF-16 `SourceRange` and converts directly via `lsp_range`.

## format_tree

```rust
pub fn format_tree(root: Node) -> String
```

Dumps the full concrete syntax tree for debugging. Each node is formatted as:
```
{indent}{kind}[ERROR|MISSING] [{row}:{col}-{row}:{col}] bytes {start}..{end}
```

Used by the CLI's `--dump-tree` flag.

## Adding a new validation rule

**Syntactic (single-document) rule:**

1. Add a new `collect_*` function in `src/diagnostics/mod.rs` that walks the tree for the target pattern.
2. Call it from `collect_diagnostics()`.
3. Add a unit test in the `#[cfg(test)]` block in `src/diagnostics/mod.rs`.
4. If the rule is complex, add a fixture under `tests/fixtures/invalid/` (file must produce at least one diagnostic).
5. Document the rule in the "Diagnostics" section of `README.md`.

**Workspace (cross-file), index-walking rule** (no CST traversal needed - operates over
`WorkspaceIndex` / `ScriptEnvironment`):

1. Add a new submodule under `src/diagnostics/` returning `HashMap<uri, Vec<WorkspaceDiagnostic>>`.
2. Re-export its entry point from `src/diagnostics/mod.rs`.
3. Call it from `collect_workspace_diagnostics` in `src/bin/witcherscript-lsp/diagnostics_publish.rs` (the helper feeds both pull entry points).
4. Add unit tests in the submodule's `#[cfg(test)]` block (fixtures cannot express cross-file rules).
5. Document the rule in `README.md`.

**Workspace (cross-file), CST-walking rule** (needs to inspect the tree of each open
document - e.g. unknown method/field access, type mismatch):

1. Add a new submodule under `src/diagnostics/` containing a unit struct (e.g. `MyRule`)
   that implements `CstRule` from `crate::diagnostics::cst_walker`.
2. In `interested_in(kind)`, return `true` only for the node kinds the rule actually
   inspects - the dispatcher uses this to short-circuit.
3. In `visit(node, ctx)`, push `WorkspaceDiagnostic` values into `ctx.diagnostics`. Use
   `infer_expr_type_memo(ctx.uri, ctx.document, ctx.db, node, byte, ctx.type_memo)`
   for receiver-type inference so chained calls share work.
4. Register the rule struct in `collect_cst_diagnostics_for_document` in
   `src/diagnostics/mod.rs`. The LSP picks it up automatically - no edit to the
   pull handlers needed.
5. The per-document cache in `src/bin/witcherscript-lsp/cst_cache.rs` already keys on
   `(parse_version, workspace_generation, base_generation, env_version)`, so rules
   registered in `collect_cst_diagnostics_for_document` re-run only when the document is
   reparsed or workspace state changes.
6. Add unit tests in the submodule's `#[cfg(test)]` block.
7. Document the rule in `README.md`.

Do not walk the tree yourself in a CST rule - register interest with `CstRule` so all
rules share a single walk per document and the per-call `TypeMemo` survives across
rule invocations.

## Existing tests

Five tests in `src/diagnostics/mod.rs`:
- `accepts_local_vars_before_statements` - var before code is fine
- `reports_local_vars_after_statements` - var after `a = 1` fires
- `reports_ternary_expression` - `cond ? a : b` fires `ternary_cond_expr`
- `accepts_non_ternary_expression` - plain assignment produces no diagnostic
- `reports_incomplete_member_access` - `super.` without ident fires

These test `collect_diagnostics()` directly with inline source strings.
