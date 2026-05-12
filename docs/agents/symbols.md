# Symbol system

**File:** `src/symbols.rs`

## SymbolKind

```rust
pub enum SymbolKind {
    Class,        // class_decl
    Struct,       // struct_decl
    Enum,         // enum_decl
    EnumVariant,  // enum_decl_variant (child of Enum)
    Function,     // func_decl at top level (no container)
    Method,       // func_decl inside a class/struct/state (has container)
    Field,        // member_var_decl inside a class/struct/state
    Variable,     // local_var_decl_stmt inside a function body
    Parameter,    // ident inside func_param_group
    State,        // state_decl (associated with an owner class)
    Event,        // event_decl (top-level or inside a class)
}
```

`Function` vs `Method` is determined at extraction time: if a `func_decl` node has a non-None container it becomes `Method`.

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
    pub type_annotation: Option<String>,      // "int", "CR4Player", etc.
    pub signature: Option<String>,            // Full text from flavour/event keyword to "{" for callables;
                                              // full node text for Fields
    pub detail: Option<String>,               // "extends ClassName" for Class/Struct/State;
                                              // "in OwnerClass" for State
    pub annotations: Vec<Annotation>,        // @addField, @wrapMethod, etc.
    pub access: AccessLevel,                  // default: Public
    pub is_optional: bool,                    // true if specifier "optional" present (Parameters only)
}
```

**`detail` field format:**
- Class/Struct: `"extends BaseClass"` (or `None` if no base)
- State: `"in OwnerClass"` (second `ident` in the state_decl node)
- All others: `None`

This string is parsed by `WorkspaceIndex` to build `superclass_by_name` and by resolve to determine inheritance chains.

## AccessLevel

```rust
pub enum AccessLevel { Private, Protected, Public }  // Ord: Private < Protected < Public
```

Default is `Public` (WitcherScript default when no specifier is present).

When traversing an inheritance chain, access is tightened: a child class can never see inherited `Private` members, and the minimum rises to `Protected` when going deeper (`min_access.max(AccessLevel::Protected)`).

## Annotation

```rust
pub struct Annotation {
    pub name: String,           // without @, e.g. "addField"
    pub argument: Option<String>,  // optional argument, e.g. "CR4Player"
}
```

Common annotations in WitcherScript modding:
- `@addField(ClassName)` — inject a field into an existing class
- `@addMethod(ClassName)` — inject a method
- `@wrapMethod(ClassName)` — wrap an existing method
- `@replaceMethod(ClassName)` — replace an existing method

Annotations on a declaration node appear as siblings immediately before it in the AST. The extractor accumulates them in `pending_annotations` and attaches them to the next non-annotation symbol.

## DocumentSymbols

```rust
pub struct DocumentSymbols { symbols: Vec<Symbol> }
```

The vec is the only storage; `SymbolId(n)` directly indexes `symbols[n]`. IDs are assigned sequentially and never change after extraction.

### API

| Method | Description |
|--------|-------------|
| `all()` | All symbols in the document |
| `by_id(id)` | O(1) lookup by ID |
| `children_of(parent_id)` | Iterate symbols whose `.container == parent_id` |
| `enclosing_symbol_at(byte, kinds)` | Smallest symbol of given kinds that contains `byte`; used to determine "which function/class am I in?" |
| `mark_optional(id)` | Sets `is_optional = true` (called during parameter extraction) |

## Grammar nodes handled during extraction

| Grammar node | Produces |
|---|---|
| `class_decl` | `SymbolKind::Class` |
| `struct_decl` | `SymbolKind::Struct` |
| `enum_decl` | `SymbolKind::Enum` |
| `enum_decl_variant` | `SymbolKind::EnumVariant` |
| `state_decl` | `SymbolKind::State` |
| `func_decl` | `Function` or `Method` (depending on container) |
| `event_decl` | `SymbolKind::Event` |
| `member_var_decl` | `SymbolKind::Field` |
| `local_var_decl_stmt` | `SymbolKind::Variable` |
| `func_param_group` | `SymbolKind::Parameter` (one per `ident` in the group) |
| `annotation` | Parsed into `Annotation`, attached to next symbol |
| `type_annot` | Text extracted as `type_annotation` |
| `specifier` | Sets `access` (`private`/`protected`) or `is_optional` (`optional`) |
| `func_flavour` | Included in `signature` text |
| `func_block` | Scope for locals and parameters |

## extract_symbols walk

1. `SymbolExtractor::visit_children(root, None, vec![])` starts the walk.
2. Named children are visited in order. If a child is `annotation`, it is parsed and pushed to `pending_annotations`; the loop continues without visiting it as a symbol.
3. The next non-annotation named child consumes `pending_annotations` and calls `visit()`.
4. `visit()` dispatches on `node.kind()`. Unknown node kinds fall through to `visit_children()`, which recurses with the current container.
5. For callables, params are extracted from `func_params → func_param_group`, then locals from `func_block`.
6. Container ID is threaded through every recursive call. Top-level symbols have `container = None`.

## Adding a new symbol kind

1. Add variant to `SymbolKind` in `symbols.rs`.
2. Handle the new grammar node in `visit()` in `SymbolExtractor`.
3. Add mapping in `symbol_kind_to_token_type()` in `semantic_tokens/mod.rs`.
4. Add mapping in `lsp_symbol_kind()` in `witcherscript-lsp.rs`.
5. Add mapping in `hover_text()` in `resolve/mod.rs` if the label text is different.
6. Add tests.
