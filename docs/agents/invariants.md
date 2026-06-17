# Key invariants

These are the non-obvious constraints that will cause silent bugs if violated:

1. **Symbol IDs = vec index.** `SymbolId(n)` indexes directly into `DocumentSymbols.symbols[n]`. Never reorder, splice, or reuse IDs within a document.

2. **`SourcePosition.character` is UTF-16 code units**, not bytes. ASCII = 1 unit, non-BMP chars = 2 units. The LSP spec requires this. All position conversion goes through `LineIndex`.

3. **Base/owner class stored in typed fields.** `Symbol.base_class` holds the raw superclass name for classes/structs/states (states use it for `extends`); `Symbol.owner_class` holds the raw owner class name for states. The human-readable `"extends ClassName"` / `"in OwnerClass"` / `"in OwnerClass extends BaseState"` string is rendered on demand by `Symbol::display_detail()` for LSP display only - there is no cached detail field to parse.

4. **Loose files compile in isolation.** A file opened outside every workspace root (and outside legacy/additional dirs), or opened with no workspace folder at all, is a *loose* file (`FileScope::OutOfScope`/`SingleFile`). It is indexed into `loose_index` while open and dropped on close. Loose files resolve against `loose_index` + base + builtins only - never `workspace_index` - and project files never see loose symbols. The `file_scope` classifier is the single source of truth for routing and the `witcherscript/fileScopeStatus` notification.

5. **Private members are scoped to their defining file** during `find_references` and semantic token resolution. Do not search or highlight private members across file boundaries.

6. **Text sync is INCREMENTAL at the wire and tree-sitter layers.** `did_change` applies range-based diffs to the stored source and feeds each diff into `Tree::edit()` on the prior parse tree; the next parse passes the edited tree to `Parser::parse()` so tree-sitter reuses unchanged subtrees. A full-document replacement (no range in the change event) drops the prior tree and parses from scratch.

7. **Base scripts are read-only.** `prepare_rename()` rejects symbols *declared* in `base_scripts_index`. That guard only covers the definition - `rename()` must additionally drop any *reference* that lands in a base script (via `rename_changes`), since a workspace symbol can still be referenced from base scripts (e.g. an `@addMethod` called inside its target class).
