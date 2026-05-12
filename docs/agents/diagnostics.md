# Diagnostics and validation

**File:** `src/diagnostics.rs`

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

## LSP conversion

All diagnostics are published as:
- Severity: `ERROR`
- Code: the `kind` string
- Source: `"witcherscript"`
- Range: converted from `byte_range` via `line_index.byte_range_to_range(source, start, end)`

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

1. Add a new `collect_*` function in `diagnostics.rs` that walks the tree for the target pattern.
2. Call it from `collect_diagnostics()`.
3. Add a unit test in the `#[cfg(test)]` block in `diagnostics.rs`.
4. If the rule is complex, add a fixture under `tests/fixtures/invalid/` (file must produce at least one diagnostic).
5. Document the rule in the "Diagnostics" section of `README.md`.

## Existing tests

Three tests in `diagnostics.rs`:
- `accepts_local_vars_before_statements` — var before code is fine
- `reports_local_vars_after_statements` — var after `a = 1` fires
- `reports_incomplete_member_access` — `super.` without ident fires

These test `collect_diagnostics()` directly with inline source strings.
