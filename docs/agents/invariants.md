# Key invariants

These are the non-obvious constraints that will cause silent bugs if violated:

1. **Symbol IDs = vec index.** `SymbolId(n)` indexes directly into `DocumentSymbols.symbols[n]`. Never reorder, splice, or reuse IDs within a document.

2. **`SourcePosition.character` is UTF-16 code units**, not bytes. ASCII = 1 unit, non-BMP chars = 2 units. The LSP spec requires this. All position conversion goes through `LineIndex`.

3. **Inheritance traversal hard-caps at depth 32.** The `MAX_INHERITANCE_DEPTH` const in `src/resolve/mod.rs` bounds every chain walk (`symbol_db/lookup.rs`, `completion/headers.rs`). This prevents infinite loops from circular or missing base class declarations.

4. **Base/owner class stored in typed fields.** `Symbol.base_class` holds the raw superclass name for classes/structs/states (states use it for `extends`); `Symbol.owner_class` holds the raw owner class name for states. The human-readable `"extends ClassName"` / `"in OwnerClass"` / `"in OwnerClass extends BaseState"` string is rendered on demand by `Symbol::display_detail()` for LSP display only - there is no cached detail field to parse.

5. **Optional parameters are excluded from completion snippets.** `completion_item` (in `src/bin/witcherscript-lsp/convert/completions.rs`) skips `is_optional = true` symbols when building snippet parameter lists. Do not change this - optional params should not appear as required snippet slots.

6. **Four symbol indexes, plus an override.** The LSP maintains four `WorkspaceIndex` instances: `workspace_index` (user project), `base_scripts_index` (read-only game scripts), `loose_index` (transient compilation for editor-open files belonging to no project root - see invariant 7), and `builtins_index` (embedded engine types). Requests build `SymbolDb::new(workspace, base).with_builtins(builtins)` - for same-name symbols, workspace shadows base shadows builtins. The `workspace` slot is `workspace_index` for project files and `loose_index` for loose files (`db_handles_for_with_snapshot`). The open `documents` map is not an index: it holds editor-open `ParsedDocument`s that take precedence over the indexed copy of the same file.

7. **Loose files compile in isolation.** A file opened outside every workspace root (and outside legacy/additional dirs), or opened with no workspace folder at all, is a *loose* file (`FileScope::OutOfScope`/`SingleFile`). It is indexed into `loose_index` while open and dropped on close. Loose files resolve against `loose_index` + base + builtins only - never `workspace_index` - and project files never see loose symbols. The `file_scope` classifier is the single source of truth for routing and the `witcherscript/fileScopeStatus` notification.

8. **Exec/quest functions excluded from global completions.** `all_top_level_callables()` filters on `Symbol.flavour` being `exec` or `quest`. These are special engine entry-points, not normal callables.

9. **Private members are scoped to their defining file** during `find_references` and semantic token resolution. Do not search or highlight private members across file boundaries.

10. **Text sync is INCREMENTAL at the wire and tree-sitter layers.** `did_change` applies range-based diffs to the stored source and feeds each diff into `Tree::edit()` on the prior parse tree; the next parse passes the edited tree to `Parser::parse()` so tree-sitter reuses unchanged subtrees. A full-document replacement (no range in the change event) drops the prior tree and parses from scratch.

11. **Base scripts are read-only.** `prepare_rename()` rejects symbols *declared* in `base_scripts_index`. That guard only covers the definition - `rename()` must additionally drop any *reference* that lands in a base script (via `rename_changes`), since a workspace symbol can still be referenced from base scripts (e.g. an `@addMethod` called inside its target class).
