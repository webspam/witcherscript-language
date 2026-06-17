# Symbol system

**Module:** `src/symbols/`

| File | Purpose |
|------|---------|
| `types.rs` | `SymbolId`, `SymbolKind`, `Symbol`, `DocumentSymbols` and index queries |
| `extract.rs` | `extract_symbols`, `SymbolExtractor` CST walk |
| `util.rs` | `node_text`, child-text/base-type helpers |
| `tests.rs` | Unit tests |

## SymbolKind

```rust
pub enum SymbolKind {
    Class,        // class_decl
    NativeType,   // builtin class retagged during builtins ingestion (no grammar node)
    Struct,       // struct_decl
    Enum,         // enum_decl
    EnumMember,   // enum_member_decl
    Function,     // func_decl at top level (no container)
    Method,       // func_decl inside a class/struct/state (has container)
    Field,        // member_var_decl or autobind_decl inside a class/struct/state
    Variable,     // local_var_decl_stmt inside a function body
    Parameter,    // ident inside func_param_group
    State,        // state_decl (associated with an owner class)
    Event,        // event_decl (top-level or inside a class)
}
```

`Function` vs `Method` is determined at extraction time: if a `func_decl` node has a non-None container it becomes `Method`.

`NativeType` is the only variant not produced by extraction: native engine types are stubbed as `class` in the builtin source (no native-type syntax exists), then `DocumentSymbols::retag_top_level` rewrites their kind from `Class` to `NativeType` during builtins ingestion.

## Symbol struct

```rust
pub struct Symbol {
    pub id: SymbolId,                         // Opaque index; equals position in DocumentSymbols.symbols vec
    pub name: String,                         // Identifier text
    pub kind: SymbolKind,
    pub range: SourceRange,                   // Full node span (LSP positions, UTF-16)
    pub selection_range: SourceRange,         // Identifier token span only
    pub byte_range: Range<usize>,             // Full node byte offsets
    pub selection_byte_range: Range<usize>,   // Identifier token byte offsets
    pub container: Option<SymbolId>,          // Parent symbol ID; None = top-level
    pub container_name: Option<String>,       // Cached parent name for fast index inserts
    pub type_annotation: Option<Type>,        // Parsed declared type (var/field/param type, callable return)
    pub base_class: Option<String>,           // Raw superclass name for Class/Struct/State
    pub owner_class: Option<String>,          // Raw owner class name for State (second ident in state_decl)
    pub flavour: Option<FuncFlavour>,         // func_flavour keyword for callables (e.g. quest, timer)
    pub annotations: Vec<Annotation>,        // @addField, @wrapMethod, etc.
    pub access: AccessLevel,                  // default: Public
    pub specifiers: Specifiers,              // non-access modifier bitset (editable, optional, out, ...)
}
```

No symbol stores rendered text; `resolve/signature.rs` renders signatures and declarations on demand from the fields above.

`base_class` / `owner_class` are the typed source of truth for `extends` and a state's owner. `display_detail()` renders them as `"extends X"` / `"in Y"` for display only - structural code reads the fields and never parses those strings back (see [invariants.md](invariants.md)).

## DocumentSymbols

The per-file result: a flat `Vec<Symbol>` plus prebuilt name/container/byte lookup indexes. `SymbolId(n)` indexes the vec; IDs are stable for the document's lifetime, and within a callable they run func -> params -> locals.

Most query helpers are obvious by name (`by_id`, `children_of`, `member_of`, `*_by_name`). The two that are not:
- `enclosing_symbol_at(byte, kinds)` - smallest symbol of those kinds covering a byte ("which function/class am I in?").
- `local_at_byte(function, name, before_byte)` - a local/param visible at a point, respecting declaration order.

## Adding a new symbol kind

1. Add variant to `SymbolKind` in `symbols/types.rs`.
2. Handle the new grammar node in `enter_in_body` (or the relevant `enter_in_*` dispatcher) in `SymbolExtractor` (`symbols/extract.rs`).
3. Add mapping in `symbol_kind_to_token_type()` in `semantic_tokens/mod.rs`.
4. Add mapping in `lsp_symbol_kind()` in `src/bin/witcherscript-lsp/convert/symbols.rs`.
5. Add mapping in `hover_text()` in `resolve/signature.rs` if the label text is different.
6. Add tests.
