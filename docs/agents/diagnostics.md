# Diagnostics and validation

**Module:** `src/diagnostics/`

- `src/diagnostics/mod.rs` — syntactic, single-document diagnostics (`ParseDiagnostic`,
  `collect_diagnostics`, `format_tree`), the shared workspace-diagnostic types, and the
  single-walk dispatcher `collect_cst_diagnostics_for_document`.
- `src/diagnostics/duplicate_symbols.rs` — workspace-wide index-walking rule.
- `src/diagnostics/duplicate_local.rs` — workspace-wide index-walking rule.
- `src/diagnostics/shadowing.rs` — workspace-wide index-walking rule (warning severity).
- `src/diagnostics/unknown_method.rs` — workspace-wide CST-walking rule registered via
  `CstRule`.
- `src/diagnostics/cst_walker.rs` — `CstRule` trait, `CstRuleCtx`, per-call `TypeMemo`,
  `run_rules_on_document`. Any new rule needing to walk a document's CST must register
  here rather than walking on its own.

## ParseDiagnostic

```rust
pub struct ParseDiagnostic {
    pub kind: String,              // category tag (e.g. "late_local_var_decl")
    pub message: String,           // human-readable description
    pub start: Point,              // tree-sitter Point { row, column } (0-indexed)
    pub end: Point,
    pub byte_range: Range<usize>,  // byte offsets in source
    pub snippet: Option<String>,   // the source line where the error occurred
}
```

`start.row + 1` = 1-indexed line number for display. In LSP the byte range is converted via `LineIndex::byte_range_to_range()` to UTF-16 positions.

## collect_diagnostics

```rust
pub fn collect_diagnostics(root: Node, source: &str) -> Vec<ParseDiagnostic>
```

Runs three passes over the tree and collects into a single Vec:

### Pass 1: Tree-sitter errors (`collect_tree_errors`)

Walks the entire tree recursively. For any node where `node.is_error() || node.is_missing()`:
- **Error node:** `kind = node.kind()`, `message = "syntax error"`
- **Missing node:** `kind = node.kind()`, `message = "missing {kind}"`

These cover all structural parse failures detected by tree-sitter's error recovery.

### Pass 2: Incomplete member access (`collect_incomplete_exprs`)

Walks the tree looking for `incomplete_member_access_expr` nodes. These are produced by the grammar when a `.` is typed but no identifier follows (e.g., `obj.` at end of line).

```
kind:    "incomplete_member_access_expr"
message: "incomplete member access: expected identifier after '.'"
```

### Pass 3: Late local variable declarations (`collect_late_local_vars`)

Scans inside each `func_block` node. Tracks whether any executable statement has been seen. If a `local_var_decl_stmt` appears after one, it is flagged:

```
kind:    "late_local_var_decl"
message: "local variable declarations must precede executable statements"
```

**Rules for what counts as "code statement":**
- Any named node that is NOT `comment` and NOT `nop` counts.
- `local_var_decl_stmt` itself resets nothing — it either fires or doesn't.

This rule only applies inside `func_block` nodes, not at file scope or in class bodies.

## Workspace diagnostics

Some checks need cross-file knowledge and cannot run off a single parse tree. These
produce `WorkspaceDiagnostic` (with a `relatedInformation`-style `Vec<RelatedLocation>`)
instead of `ParseDiagnostic`. Positions are already UTF-16 `SourceRange`s — taken from
`Symbol.selection_range` — so no `LineIndex` round-trip is needed at conversion time.

`collect_duplicate_symbol_diagnostics(&WorkspaceIndex) -> HashMap<uri, Vec<WorkspaceDiagnostic>>`
flags any two top-level declarations (class/struct/enum/state/function/event) sharing a
name. Symbols carrying modding annotations are skipped — `@addMethod`/`@wrapMethod`/etc.
functions are member injections, not fresh global names. It enumerates raw symbols via
`WorkspaceIndex::all_top_level()` because `top_level_by_name` dedups by name.

The LSP computes workspace diagnostics across the whole index but only *publishes* them
for open documents (`Backend::publish_open_diagnostics` in `indexing.rs`), merged with the
document's syntactic `ParseDiagnostic`s.

## LSP conversion

All diagnostics are published as:
- Severity: `ERROR`
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

**Workspace (cross-file), index-walking rule** (no CST traversal needed — operates over
`WorkspaceIndex` / `ScriptEnvironment`):

1. Add a new submodule under `src/diagnostics/` returning `HashMap<uri, Vec<WorkspaceDiagnostic>>`.
2. Re-export its entry point from `src/diagnostics/mod.rs`.
3. Call it from `Backend::publish_open_diagnostics` in `src/bin/witcherscript-lsp/indexing.rs`.
4. Add unit tests in the submodule's `#[cfg(test)]` block (fixtures cannot express cross-file rules).
5. Document the rule in `README.md`.

**Workspace (cross-file), CST-walking rule** (needs to inspect the tree of each open
document — e.g. unknown method/field access, type mismatch):

1. Add a new submodule under `src/diagnostics/` containing a unit struct (e.g. `MyRule`)
   that implements `CstRule` from `crate::diagnostics::cst_walker`.
2. In `interested_in(kind)`, return `true` only for the node kinds the rule actually
   inspects — the dispatcher uses this to short-circuit.
3. In `visit(node, ctx)`, push `WorkspaceDiagnostic` values into `ctx.diagnostics`. Use
   `infer_expr_type_memo(ctx.uri, ctx.document, ctx.db, node, byte, ctx.type_memo)`
   for receiver-type inference so chained calls share work.
4. Register the rule in `collect_cst_diagnostics_for_document` in `src/diagnostics/mod.rs`.
5. The LSP server picks it up automatically — no edit to `publish_open_diagnostics`
   needed. The per-document cache in `src/bin/witcherscript-lsp/cst_cache.rs` already
   keys on `(parse_version, workspace_generation, base_generation, env_version)`, so
   the rule re-runs only when the document is reparsed or workspace state changes.
6. Add unit tests in the submodule's `#[cfg(test)]` block.
7. Document the rule in `README.md`.

Do not walk the tree yourself in a CST rule — register interest with `CstRule` so all
rules share a single walk per document and the per-call `TypeMemo` survives across
rule invocations.

## Existing tests

Three tests in `diagnostics.rs`:
- `accepts_local_vars_before_statements` — var before code is fine
- `reports_local_vars_after_statements` — var after `a = 1` fires
- `reports_incomplete_member_access` — `super.` without ident fires

These test `collect_diagnostics()` directly with inline source strings.
