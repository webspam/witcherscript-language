# Symbol system

**Module:** `src/symbols/`

`extract_symbols` walks a parsed CST and produces a `DocumentSymbols`: a flat, per-file list of `Symbol`s - one per declaration (class, function, field, local, ...). This is the per-file declaration model every higher layer (resolution, hover, outline, semantic tokens) queries. Types live in `types.rs`, the CST walk in `extract.rs`.

## SymbolKind

The kind of declaration a `Symbol` is: `Class`, `NativeType`, `Struct`, `Enum`, `EnumMember`, `Function`, `Method`, `Field`, `Variable`, `Parameter`, `State`, `Event`.

`Function` vs `Method` is decided at extraction time, not by syntax: a `func_decl` with a container becomes `Method`, one without becomes `Function`.

`NativeType` is the one kind the extractor never emits: native engine types are declared as `class` in the builtin sources, then `retag_top_level` rewrites them from `Class` to `NativeType` during builtins ingestion.

## Symbol

Each `Symbol` carries its name, kind, source ranges (both LSP and byte), container, and the typed pieces resolution needs - declared type, base/owner class, func flavour, annotations, access level, specifiers. Read `symbols/types.rs` for the fields.

Two non-obvious facts:

- **No rendered text is stored.** Signatures and field declarations are rendered on demand from these fields by `resolve/signature.rs`; do not add cached display strings.
- **`base_class` / `owner_class` are the typed source of truth** for `extends` and a state's owner. Structural code (inheritance walks, `superclass_by_name`) reads them directly; `display_detail()` renders them to `"extends X"` / `"in Y"` for display only - never parse those strings back (see [invariants.md](invariants.md)).

## DocumentSymbols

The per-file result: a flat `Vec<Symbol>` indexed by `SymbolId(n)`, plus prebuilt name/container/byte lookup indexes that back the query helpers. IDs are assigned sequentially at extraction and never change.

Most query helpers are obvious from their names (`by_id`, `children_of`, `member_of`, `*_by_name`). The two that are not:

- `enclosing_symbol_at(byte, kinds)` - smallest symbol of those kinds covering a byte ("which function/class am I in?").
- `local_at_byte(function, name, before_byte)` - a local or parameter visible at a point, respecting declaration order.

## Adding a new symbol kind

1. Add the variant to `SymbolKind` (`symbols/types.rs`).
2. Handle its grammar node in `enter_in_body` (or the relevant `enter_in_*` dispatcher) in `SymbolExtractor` (`symbols/extract.rs`).
3. Map it in `symbol_kind_to_token_type()` (`semantic_tokens/mod.rs`).
4. Map it in `lsp_symbol_kind()` (`src/bin/witcherscript-lsp/convert/symbols.rs`).
5. Map it in `hover_text()` (`resolve/signature.rs`) if its label differs.
6. Add tests.
