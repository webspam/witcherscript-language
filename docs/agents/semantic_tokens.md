# Semantic tokens

**Files:** `src/semantic_tokens/mod.rs`, `src/semantic_tokens/tests.rs`

## Token types

```rust
pub const TOKEN_TYPES: &[&str] = &[
    "class",      // 0  — Class, Struct, State declarations and references
    "enum",       // 1  — Enum declarations
    "enumMember", // 2  — EnumVariant; also CName literals ('SomeName')
    "function",   // 3  — Function, Method, Event
    "parameter",  // 4  — Parameter
    "variable",   // 5  — Variable (local)
    "property",   // 6  — Field, autobind_decl
    "keyword",    // 7  — registered but NEVER emitted (TextMate handles keywords)
    "comment",    // 8  — comment nodes
    "string",     // 9  — literal_string
    "number",     // 10 — literal_int, literal_float, literal_hex
    "type",       // 11 — registered to preserve indices but NEVER emitted
    "decorator",  // 12 — annotation_ident (@addField etc.)
    "modifier",   // 13 — specifier, func_flavour, autobind_single, declaration keywords
];

pub const TOKEN_MODIFIERS: &[&str] = &["declaration"];
```

Index 11 (`"type"`) is intentionally never emitted. Type-annotation identifiers are resolved to their actual symbol kind and emitted with that kind's token type instead.

## Classification rules

The `classify()` function dispatches on `node.kind()`:

| Grammar node | Token type |
|---|---|
| `ident` | see classify_ident() below |
| `annotation_ident` | `decorator` (12) |
| `comment` | `comment` (8) |
| `literal_string` | `string` (9) |
| `literal_name` | `enumMember` (2) — CName literals like `'SomeName'` |
| `literal_int`, `literal_float`, `literal_hex` | `number` (10) |
| `specifier` | `modifier` (13) |
| `func_flavour`, `autobind_single` | `modifier` (13) |
| Anonymous node (keyword text) | `modifier` (13) if in keyword list, else skipped |

`literal_bool`, `literal_null`, `this_expr`, `super_expr`, etc. are **not classified** — TextMate grammar handles constant/language keywords.

### Keyword list (anonymous nodes → modifier)

`class`, `struct`, `enum`, `state`, `function`, `event`, `extends`, `var`, `autobind`, `defaults`, `hint`, `abstract`, `statemachine`, `latent`, `import`, `const`, `final`, `editable`, `saved`, `optional`, `out`, `inlined`, `private`, `protected`, `public`, `cleanup`, `entry`, `exec`, `quest`, `reward`, `storyscene`, `timer`, `single`

## classify_ident: declaration vs reference

`ident` nodes have two modes based on their parent:

**Declaration sites** (syntactically unambiguous, always emit):

| Parent node | Token type |
|---|---|
| `class_decl`, `struct_decl`, `state_decl` | `class` (0) |
| `enum_decl` | `enum` (1) |
| `enum_decl_variant` | `enumMember` (2) |
| `func_decl`, `event_decl` | `function` (3) |
| `func_param_group` | `parameter` (4) |
| `member_var_decl` | `property` (6) |
| `local_var_decl_stmt` | `variable` (5) |
| `autobind_decl` | `property` (6) |

**Reference sites** (must resolve; no token if unresolvable):

| Parent node | Behavior |
|---|---|
| `type_annot`, `new_expr` | `resolve_ident()` — classify by resolved symbol kind |
| `member_access_expr` (after `.`) | `resolve_member_ident()` — infer receiver type, look up member |
| `member_access_expr` (before `.`) | `resolve_ident()` — classify receiver |
| all other parents | `resolve_ident()` |

## resolve_ident resolution order

1. Local variables/parameters of the enclosing function (search children of enclosing function symbol, checking `selection_byte_range.start <= byte_offset` to avoid seeing names declared later)
2. Members of the enclosing class/struct/state (non-callable members first, then methods/events)
3. Top-level symbols in the current document
4. `db.find_top_level(name)` (workspace + base)

If nothing resolves, no token is emitted for the identifier.

## resolve_member_ident

For `receiver.member` expressions:

| Receiver kind | Type inference |
|---|---|
| `this_expr` | name of enclosing class |
| `super_expr`, `parent_expr`, `virtual_parent_expr` | superclass from `detail` (`"extends X"`) |
| `ident` | `type_annotation` of the resolved local/param in the enclosing function |
| anything else | returns None (no token) |

After getting the type name, checks:
1. Local document symbols for a class/struct/state with that name, then its children.
2. `db.find_member(type, member, Public)` across workspace + base.

## Encoding

LSP semantic tokens use delta encoding. The output is a flat `Vec<u32>` with groups of 5:

```
[delta_line, delta_start, length, token_type, token_modifiers_bitset]
```

`delta_line` and `delta_start` are relative to the previous token (not absolute). On a new line `delta_start` resets to the absolute column. Token modifiers bitset is always 0 (the `declaration` modifier is registered but not currently emitted).

Tokens are produced in tree walk order (top-to-bottom, left-to-right), which matches LSP requirements.

## Single-line constraint

Multi-line tokens are silently skipped:
```rust
if range.start.line == range.end.line && range.end.character > range.start.character {
    // emit token
}
```

This is a defensive check; in practice WitcherScript identifiers don't span lines, but string literals can. String literals that happen to span lines are skipped rather than causing encoding errors.

## Recursion rule

When `classify()` returns `Some(type)` for a node, the token covers the whole node span and the children are NOT visited. When `classify()` returns `None` for a named node, children are recursed. Anonymous nodes with no classification are silently skipped without recursion.

## Tests

`src/semantic_tokens/tests.rs` — 21 tests covering:
- Class/enum/function/field/variable declaration sites
- Resolved type annotations (only highlighted if the type is defined)
- Member access with `this.field`, local variable type inference
- Inheritance: members from base classes via the db
- Unresolvable identifiers produce no token
- CName literals (`'SomeName'`) emit `enumMember`
- Keywords emit `modifier`
- Comments and strings emit correct types
