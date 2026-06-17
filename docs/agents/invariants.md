# Key invariants

These are the non-obvious constraints that will cause silent bugs if violated:

1. **Symbol IDs = vec index.** `SymbolId(n)` indexes directly into `DocumentSymbols.symbols[n]`. Never reorder, splice, or reuse IDs within a document.

2. **`SourcePosition.character` is UTF-16 code units**, not bytes. ASCII = 1 unit, non-BMP chars = 2 units. The LSP spec requires this. All position conversion goes through `LineIndex`.

3. **Read base/owner from the typed fields, never the display string.** `Symbol.base_class` (raw superclass; states use it for `extends`) and `Symbol.owner_class` (raw state owner) are the source of truth. `Symbol::display_detail()` renders the `"extends X"` / `"in Y"` text on demand for display only - never parse it for structural queries.

4. **Loose files compile in isolation.** A file opened outside every workspace root, or with no workspace folder, is *loose*: indexed into `loose_index` while open, dropped on close. It resolves against `loose_index` + base + builtins only, never `workspace_index`; project files never see loose symbols. `file_scope` is the single source of truth for routing.

5. **Private members are scoped to their defining file** during `find_references` and semantic token resolution. Do not search or highlight private members across file boundaries.

6. **Text sync is INCREMENTAL at the wire and tree-sitter layers.** `did_change` applies range-based diffs to the stored source and feeds each diff into `Tree::edit()` on the prior parse tree; the next parse passes the edited tree to `Parser::parse()` so tree-sitter reuses unchanged subtrees. A full-document replacement (no range in the change event) drops the prior tree and parses from scratch.

7. **Base scripts are read-only.** `prepare_rename()` rejects symbols *declared* in `base_scripts_index`. That guard only covers the definition - `rename()` must additionally drop any *reference* that lands in a base script (via `rename_changes`), since a workspace symbol can still be referenced from base scripts (e.g. an `@addMethod` called inside its target class).
