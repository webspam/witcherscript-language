# Resolution and workspace indexing

**Module:** `src/resolve/` - split across `mod.rs` (helpers, `Definition`, `ObservationSet`), `workspace_index/` (`WorkspaceIndex`: `mod`, `indices`, `subscribers`, `lookup`), `symbol_db/` (`SymbolDb`: `mod`, `lookup`, `generics`), `definition.rs` (`resolve_definition` and self/super/this resolution), `references.rs` (`find_references`), `inference.rs` (`infer_type` and type-context helpers), `signature.rs` (`signature_help`, `hover_text`, `render_signature`), `ast.rs` (re-exports the shared `cst/` navigation helpers; defines `BUILTIN_TYPE_COMPLETIONS`), `extract_var.rs` / `extract_callable/{mod,statements,captures,render}.rs` (extract-to-variable / extract-to-function / extract-to-method refactor cores) over shared `edit_plan.rs` (`Extraction`/`EditPlan`/`Splice` output and splice application), with selection classification in `selection.rs` and the write-site walker in `writes.rs`, and `completion/{members,types,body_function,body_class,body_script,headers}.rs`. Tests live under `src/resolve/tests/`.

## WorkspaceIndex

The persistent cross-document symbol store. One instance exists for the user workspace, one for base scripts.

```rust
pub struct WorkspaceIndex {
    documents: HashMap<String, Vec<Symbol>>,                          // uri → all symbols in that document
    top_level_by_name: HashMap<String, Vec<Definition>>,             // name → top-level defs
    superclass_by_name: HashMap<String, Vec<(String, String)>>,      // class name → base class
    member_by_type: HashMap<String, HashMap<String, Vec<Definition>>>, // container → members
    doc_idents: HashMap<String, HashMap<String, Vec<Range<usize>>>>, // ident occurrence index
    // plus enum/state lookups, a prebuilt completion catalog, and fingerprint bookkeeping
}
```

`doc_idents` is a pre-built occurrence index used by `find_references` to quickly check which documents contain a given identifier by name before doing the more expensive semantic resolution pass.

### Mutations

```rust
WorkspaceIndex::update_document(uri, document)  // remove old entries, re-insert from document.symbols
WorkspaceIndex::remove_document(uri)             // clean removal from all maps
```

### Queries

```rust
find_top_level(name)                              // O(1) HashMap lookup
direct_member_of(container, name, min_access)     // O(1) lookup with access check
direct_members_of(container, min_access)          // all direct members of a type
superclass_of(class_name)                         // one hop up the chain
full_parameters_of(uri, callable_id)              // Parameter symbols of a callable, in order
find_symbol_at_selection(uri, selection)          // O(n) doc scan by selection byte range
find_symbol_by_name(uri, name)                    // O(n) doc scan by name
all_types()                                       // all Class/Struct/State/Enum symbols
all_top_level_callables()                         // all Function/Event, excluding exec/quest
```

`SymbolDb::definition_at_selection(uri, selection, name, container)` re-derives a symbol after edits (completionItem/resolve): exact selection-range match first, then by name. `container` routes the lookup through `find_member` for members.

## SymbolDb

A per-request view combining workspace + base + builtins indexes.

```rust
pub struct SymbolDb<'a> {
    workspace: &'a WorkspaceIndex,
    base: &'a WorkspaceIndex,
    builtins: Option<&'a WorkspaceIndex>,
    script_env: Option<&'a ScriptEnvironment>,
}

// Construction
SymbolDb::new(&workspace_index, &base_scripts_index)
    .with_script_env(&script_env)        // optional; adds INI globals
    .with_builtins(&builtins_index)      // optional; engine-magic types like `array`

// Precedence: workspace → base → builtins (for same-name symbols, first hit wins)
```

`SymbolDb` mirrors most `WorkspaceIndex` queries but searches workspace first, then base, then builtins. For member resolution it uses `find_member_chain_cross()` which can traverse inheritance across all three indexes simultaneously.

### Implicit base classes

A class with no `extends` implicitly extends `CObject`; a state with no `extends` implicitly extends `CScriptableState`. The engine enforces this; the workspace doesn't write it in source. `SymbolDb::superclass_of()` synthesises the fallback so every inheritance walk sees the implicit base - callers must go through `superclass_of`, not read `Symbol.base_class` directly, or the fallback is missed (see invariant 3 in [invariants.md](invariants.md)). Cycle protection: `CObject`/`IScriptable`/`ISerializable` and `CScriptableState` itself get no synthesised base.

### Generic type lookup (array<T>)

When `find_member` / `members_of_tiered` / `direct_members_of` are called with a container like `array<int>`, `SymbolDb` extracts the constructor (`array`) and element (`int`) via `parse_generic_type()`, looks up members of `array` in the chain, and then substitutes the placeholder ident `T` with `int` in each returned `Definition`'s `type_annotation`, `signature`, `detail`, and `container_name`. Substitution is whole-ident only (`T` and `<T>` match, `TArray` does not). See [docs/agents/builtins.md](builtins.md) for the full story.

`all_types()` deliberately omits the builtins index - we never want `array` to appear as a user-completable type name.

## resolve_definition priority chain

```
resolve_definition(uri, document, db, position)
    │
    ├─ 1. Self keyword? (this/super/parent)
    │      this   → enclosing class definition
    │      super  → superclass of enclosing class
    │      parent → owner class of enclosing state (public members only)
    │
    ├─ 2. After dot in member_access_expr?
    │      → infer_expr_type(receiver) → find_member(type, name, Protected)
    │
    ├─ 3. Local variable or parameter in enclosing function
    │
    ├─ 4. Member of enclosing class/struct/state
    │
    ├─ 5. Top-level symbol in this document
    │
    ├─ 6. Top-level symbol in workspace (db.find_top_level)
    │      └─ workspace shadows base for same-name symbols
    │
    ├─ 7. Script global from INI (db.find_script_global)
    │      redirects to the class definition if the class is loaded
    │
    └─ 8. Definition site itself (fallback: cursor is on the name being defined)
```

## Member chain traversal

`SymbolDb::find_member(container, name, min_access)`:
1. Walk the inheritance chain starting at `container`. At each level, check direct members in workspace, base, then builtins.
2. The walk is **first-name-wins**: the first member matching `name` at any depth terminates the walk, regardless of its access level. A `private` declaration in a subclass therefore masks any same-name member further up the chain - you cannot skip past it to reach an accessible ancestor. This matches the WitcherScript compiler.
3. After the walk, the found member's access is compared to `min_access`: if it is too low, `find_member` returns `None`.
4. Hard stop at depth 32 (prevents infinite loops from circular inheritance in malformed code).

Callers that want every visible member regardless of access pass `AccessLevel::Private`. `default x = ...;` / `defaults { x = ...; }` / `hint x = ...;` lookups do exactly that, because a subclass may legitimately override the default or hint of a private inherited field; the diagnostic that catches outside-class access to a private member (`private_member_access`) does the same and then performs its own enclosing-class check.

`SymbolDb::members_of` / `members_of_tiered` follow the same first-name-wins rule for enumeration - the closest declaration wins per name - and then filter the resulting set by `min_access`.

## infer_expr_type

Used to determine the receiver's type for member access and chained calls:

| Receiver node | Inferred type |
|---|---|
| `this_expr` | name of enclosing class/struct/state |
| `super_expr` / `parent_expr` / `virtual_parent_expr` | superclass (via `detail`) |
| `ident` | `type_annotation` of the resolved local/param/member |
| `func_call_expr` | return type of the resolved function (recursive) |
| `member_access_expr` | return type of the resolved member (recursive) |
| `new_expr` | type name from the new expression |

## Completion functions

### `completion_members(uri, document, db, position)`
Called when the trigger character is `.` or `:`. Returns `Vec<(u8, Definition)>` where `u8` is the tier:
- `0` = own member of the receiver's type
- `1` = inherited member

Access level: `Public` for global use; `Protected` from within the same class.

### `default_or_hint_member_completions(document, db, position)`
Called when the cursor sits in the `member` position of `default x = ...`, `defaults { x = ...; }`, or `hint x = ...`. Returns members of the enclosing class with private inherited fields included (a subclass may override the default or hint of a private inherited field).

### `type_completions(document, db, position)`
Called in type annotation context. Returns:
- All `Class`, `Struct`, `Enum`, `State` symbols from workspace + base
- `BUILTIN_TYPE_COMPLETIONS`: `["bool", "byte", "float", "int", "name", "string", "void"]`

### `new_type_completions(uri, document, db, position)`
Called when the cursor is in the class slot of a `new` expression (after the `new` keyword, before or inside the class ident). Returns class symbols narrowed to the expected type (LHS of the surrounding `var` decl or assignment) plus its descendants; falls back to every class when no expected type can be inferred or the expected type is unknown.

### `new_lifetime_completions(uri, document, db, position)`
Called when the cursor is in the lifetime slot of a `new` expression (after `new C in`). Returns class-typed locals, parameters, and class fields of the enclosing type visible at the cursor. Tree-sitter parses `new C in ;` with `in` in an ERROR sibling of `new_expr`; the helper accepts both shapes.

### `statement_completions(uri, document, db, position)`
Called in function body context. Returns `StatementCompletions`:
```rust
pub struct StatementCompletions {
    pub locals: Vec<Definition>,    // local vars + params in scope
    pub members: Vec<Definition>,   // members of enclosing class
    pub globals: Vec<Definition>,   // all top-level callables (excluding exec/quest)
    pub has_this: bool,
    pub has_super: bool,
    pub has_parent: bool,           // state body: offers parent/virtual_parent
    pub in_switch: bool,            // cursor is inside a switch block
    pub in_loop: bool,              // cursor is inside a for/while/do-while loop
}
```

### `class_body_keyword_completions(document, position)`
Called in class/struct/state body. Returns `Vec<&'static str>` - the keyword candidates valid at the cursor position given which specifiers have already been written. Returns an empty vec when the cursor is not in a type body or follows a completed declaration keyword.

## signature_help

`signature_help(uri, document, db, position)` powers `textDocument/signatureHelp`. It finds the innermost call site around the cursor - a closed `func_call_expr`, or an unclosed call that tree-sitter recovers as an `ERROR` node containing a callee, `(`, and optional `func_call_args` - resolves the callee via `resolve_definition_at_byte`, and builds a `SignatureHelpInfo`:

- `label` - `Name(p1 : T1, optional p2 : T2) : Ret`, built from `db.display_parameters_of()` so **all** parameters (including optional/out, in order) appear, with generic element types substituted.
- `parameters` - `[start, end)` UTF-16 offsets of each parameter substring within `label`.
- `active_parameter` - index derived by counting `,` tokens before the cursor, clamped to the last parameter; `None` when the callee takes no parameters.

## find_references

```rust
find_references(definition, definition_document, search_documents, db, include_declaration)
    → Vec<(String uri, SourceRange)>
```

Scoping rules:
- **Local variables / parameters** → only within the `func_block` byte range
- **Private members** → only within the defining file URI
- **Public / protected members** → all documents in `search_documents`

For each candidate document, the `doc_idents` index is consulted first to skip documents that don't contain the identifier by name at all, then each occurrence is semantically verified by calling `resolve_definition` and checking that it resolves to the same symbol.

## hover_text

Formats a symbol as a multi-line string for LSP hover:

Callable parameter lists come from `render_signature(db.display_parameters_of(..), return_type, colon)`.

| Kind | Format |
|------|--------|
| `Method` | `(method) ClassName.name(params) : ReturnType` |
| `Field` | `(field) <declaration as written>` (falls back to `(field) name : Type`) |
| `Function` | `function name(params) : ReturnType` |
| `Class`/`Struct`/`State` | `class Name` + `extends Base` on next line |
| `Enum` | `enum Name` |
| `EnumMember` | `enum member Name` |
| `Variable` | `var name : Type` |
| `Parameter` | `(parameter) name : Type` |
| `Event` | `event Name(params)` |

Annotations are prepended as `@name(arg), @name2(arg2)`.

## Built-in type names

Two `&[&str]` constants live in `ast.rs`, both re-exported from `resolve`:

```rust
// Full set of names treated as known types - engine primitives plus their
// CamelCase aliases. `unknown_symbol` consults this so it never flags them.
pub const BUILTIN_TYPES: &[&str] = &[
    "bool", "byte", "float", "int", "name", "string", "void",
    "Bool", "Float", "String", "CName",
    "Int32", "Int16", "Int8", "Uint8", "Uint16", "Uint32", "Uint64", "StringAnsi",
];

// Primitive subset offered as completions in a type-annotation context.
pub const BUILTIN_TYPE_COMPLETIONS: &[&str] =
    &["bool", "byte", "float", "int", "name", "string", "void"];
```

Neither set lives in any `WorkspaceIndex`. `type_completions()` offers `BUILTIN_TYPE_COMPLETIONS`; resolution and the `unknown_symbol` diagnostic treat every name in `BUILTIN_TYPES` as a valid primitive.

## Script environment (INI globals)

`ScriptEnvironment` is populated from `gameDirectory/bin/redscripts.ini`, which contains `[globals]` entries like:
```ini
[globals]
theGame=CR4Game
thePlayer=CR4Player
```

When resolving a name like `theGame`:
1. `find_script_global("theGame")` finds the global.
2. If `CR4Game` is a known class in the loaded index, returns the class definition.
3. Otherwise returns a synthetic symbol pointing to the INI file.

Script globals are the last resort in the priority chain (after workspace and base).

### Engine-injected overrides

The game engine injects a small, fixed set of globals at runtime independently of `redscripts.ini`. `apply_engine_overrides` adds them after the INI parse, but only when the INI does not already mention them - any existing entry is treated as deliberate customisation and left alone.

| Global | Injected type |
| --- | --- |
| `theCamera` | `CCameraDirector` |
| `theTelemetry` | `CR4TelemetryScriptProxy` |

This list is closed - do not add more entries without confirming the engine actually injects the global.

## Key constraints

- Exec/quest functions are **excluded** from `all_top_level_callables()` and therefore from statement completions, matched on `Symbol.flavour`.
- Optional parameters are excluded from completion snippet slots (`completion_item` in the LSP binary filters `is_optional`).
- The inheritance depth cap is **32** in both `WorkspaceIndex` (single-index chain) and `SymbolDb` (cross-index chain).
- Superclass is stored in `Symbol.base_class` (used for classes, structs, and states' `extends` clause). The display string `"extends ClassName"` is rendered on demand by `Symbol::display_detail()` - never parse it for structural queries, use the typed field.
- State owner is stored in `Symbol.owner_class`; rendered by `display_detail()` as `"in OwnerClass"` (or `"in OwnerClass extends BaseState"` when the state also extends another state). For `parent` keyword resolution only `Public` members of the owner are accessible.
