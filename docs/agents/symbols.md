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

No symbol carries rendered signature or declaration text. Callable signatures are rendered on demand by `render_signature()` in `resolve/signature.rs` from the callable's `Parameter` symbols plus its `type_annotation` (the return type); field declarations are rendered the same way by `hover_text()` from `access`, `specifiers`, `name`, and `type_annotation`.

**`Symbol::display_detail()`** renders the human-readable detail string used in hover popups, the document outline, and completion items. It reads from `base_class` / `owner_class`:
- Class/Struct: `"extends BaseClass"` (or `None` if no base)
- State: `"in OwnerClass"`, `"in OwnerClass extends BaseState"`, `"extends BaseState"`, or `None`
- All others: `None`

Structural queries (e.g. building `superclass_by_name`, walking inheritance chains) read `base_class` / `owner_class` directly. The rendered detail string is display-only.

## DocumentSymbols

```rust
pub struct DocumentSymbols {
    symbols: Vec<Symbol>,                                                  // source of truth
    by_start_byte: Vec<SymbolId>,                                          // sorted by start byte
    top_level_by_name: HashMap<String, Vec<SymbolId>>,
    type_by_name: HashMap<String, Vec<SymbolId>>,
    members_by_container: HashMap<SymbolId, HashMap<String, Vec<SymbolId>>>,
    locals_in_function: HashMap<SymbolId, HashMap<String, Vec<SymbolId>>>,
}
```

`symbols` is the source of truth; `SymbolId(n)` directly indexes `symbols[n]`. IDs are assigned sequentially and never change after extraction. The remaining fields are secondary lookup indexes built once by `build_indexes()` (called from `finish`) so name, container, and byte queries are O(1) rather than full scans.

### API

| Method | Description |
|--------|-------------|
| `all()` | All symbols in the document |
| `by_id(id)` | O(1) lookup by ID |
| `children_of(parent)` | Iterate symbols whose `.container == parent`; pass `None` for top-level |
| `enclosing_symbol_at(byte, kinds)` | Smallest symbol of given kinds that contains `byte`; used to determine "which function/class am I in?" |
| `top_level_by_name(name)` | Top-level symbol with that name, preferring a non-State declaration |
| `top_level_by_name_filtered(name, accept)` | First top-level symbol with that name whose kind satisfies `accept` |
| `type_by_name(name)` | Object-typed symbol (class, native type, struct, or state) with that name, preferring non-State |
| `type_by_name_filtered(name, accept)` | First object-typed symbol with that name whose kind satisfies `accept` |
| `member_of(container, name)` | Iterate members of `container` with that name |
| `local_at_byte(function, name, before_byte)` | Local or parameter named `name` in scope at `before_byte` |

## Adding a new symbol kind

1. Add variant to `SymbolKind` in `symbols/types.rs`.
2. Handle the new grammar node in `enter_in_body` (or the relevant `enter_in_*` dispatcher) in `SymbolExtractor` (`symbols/extract.rs`).
3. Add mapping in `symbol_kind_to_token_type()` in `semantic_tokens/mod.rs`.
4. Add mapping in `lsp_symbol_kind()` in `src/bin/witcherscript-lsp/convert/symbols.rs`.
5. Add mapping in `hover_text()` in `resolve/signature.rs` if the label text is different.
6. Add tests.
