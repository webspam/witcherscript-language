# Semantic tokens

Resolution-aware highlighting layered on top of the TextMate grammar. TextMate already colours keywords and the constant-language tokens (`true`/`false`/`null`/`this`/`super`); semantic tokens supply only what TextMate cannot know without resolving a name - whether an identifier is a class, a field, a local, an enum member, and so on. Whatever the grammar already handles is deliberately left unclassified here.

**Code:** `src/semantic_tokens/mod.rs` holds the walk and `classify()`; tests in `src/semantic_tokens/tests.rs`. The LSP request handlers are in `src/bin/witcherscript-lsp/queries/semantic_tokens.rs`, the delta cache in `src/bin/witcherscript-lsp/semantic_tokens_cache.rs`.

## Data flow

All three LSP requests - `full`, `full/delta`, `range` - funnel into `collect_in_byte_range`. It walks the CST in tree order (top-to-bottom, left-to-right, the order LSP requires) and calls `classify()` on each node. Four properties of that walk are load-bearing:

- A node that classifies is emitted as one token spanning the whole node, and **its children are not visited**. If you teach `classify()` to colour a parent node, you suppress every token inside it.
- A named node that does not classify is recursed into; an anonymous (keyword/punctuation) node that does not classify is skipped without recursing.
- A token is emitted only when it starts and ends on one line. Identifiers never span lines; a multi-line string literal is the one thing that can, and it is dropped, because the delta encoding cannot represent a token across lines.
- An identifier that resolves to nothing emits no token. Highlighting is best-effort: an undefined type stays uncoloured rather than being guessed.

## Token types

`TOKEN_TYPES` and `TOKEN_MODIFIERS` (the LSP legend) are declared at the top of `mod.rs`; the index numbers used below (e.g. `modifier` (13)) are positions in that list. The parts not obvious from the list itself:

- `keyword` (7) and `type` (11) are registered but never emitted. `keyword` is left to the TextMate grammar; `type` exists only to keep the later indices stable - a type-annotation identifier is resolved and emitted as its real symbol kind instead.
- One token type covers several symbol kinds (`symbol_kind_to_token_type`): `class` also paints Struct, State, and native types; `function` also paints methods and events.
- Of the two modifiers, only `defaultLibrary` is emitted (on redscripts.ini globals); `declaration` is never set.

## What classify() colours

`classify()` dispatches on `node.kind()`. The non-trivial cases:

- **Literals** take their obvious type, with one surprise: a CName literal (`'SomeName'`) is emitted as `enumMember`, not `string`.
- **`specifier`, `func_flavour`, `autobind_single`, and the declaration/modifier keywords** (the `class`/`var`/`private`/`latent`/... set in `classify_anonymous_keyword`) are `modifier`. Control-flow and constant keywords are intentionally absent - TextMate owns them.
- **`annotation_ident`** (`@addField`, ...) is `decorator`.
- **`ident`** is the hard case, below.

`literal_bool`, `literal_null`, `this_expr`, and `super_expr` are deliberately left unclassified - the grammar already colours them.

## Identifiers: declaration vs reference

A declaration-site ident takes a fixed token type from its parent decl node (class/struct/state -> `class`, enum -> `enum`, enum member -> `enumMember`, func/event -> `function`, member-var and autobind -> `property`, local var -> `variable`, param -> `parameter`). No resolution is involved; these always emit.

Every other ident is a reference and must be resolved. It falls through to the `_` arm in `classify_ident`, which:

1. `classify_locally()` - locals/params of the enclosing function, then members of the enclosing class/struct/state, then document top-level.
2. If the ident is the right-hand side of a `member_access_expr` (after the `.`), it resolves through `resolve_member_access()`, which infers the receiver's type (below) and looks the member up on it.
3. Otherwise `classify_definition_at_ident()` walks the same local scope out to the workspace db.

A reference that resolves is coloured by its definition's kind. One special case: redscripts.ini script globals (e.g. `thePlayer`) resolve to the class they alias - or to a synthetic variable when that class isn't loaded - and are recoloured `variable` + `defaultLibrary` so they do not paint as a type. A workspace symbol that shadows the global name wins normally and keeps its real kind.

## resolve_member_access (for `receiver.member` expressions)

`resolve_member_access()` in `src/resolve/inference.rs` infers the receiver type:

| Receiver kind | Type inference |
|---|---|
| `this_expr` | name of enclosing class/struct/state |
| `super_expr` | `base_class` of enclosing type |
| `parent_expr`, `virtual_parent_expr` | `owner_class` of enclosing state |
| `ident` | `type_annotation` of the resolved local/param/field, or `db.script_global_type()` |
| `func_call_expr`, `member_access_expr` | return type inferred recursively |
| anything else | returns None (no token) |

After getting the type name, looks up the member in the current document's symbols then `db.find_member()`.

## Encoding

LSP semantic tokens use delta encoding. The output is a flat `Vec<u32>` with groups of 5:

```
[delta_line, delta_start, length, token_type, token_modifiers_bitset]
```

`delta_line` and `delta_start` are relative to the previous token (not absolute). On a new line `delta_start` resets to the absolute column. The token modifiers bitset is 0 for almost every token; redscripts.ini globals are the only emitted tokens that set a modifier (`defaultLibrary`, bit 1).

## Request handlers

- **`range`** prunes the walk to CST subtrees intersecting the requested byte range (clamped on conversion failure, as `inlay_hints` does). Edge-overlapping tokens are kept - the spec permits overflow - and the deltas still start from the document origin, so the payload stands alone.
- **`full` / `full/delta`** mint a monotonic `result_id` and cache the emitted `Vec<u32>` per open document in `Backend::semantic_tokens_cache`, dropped on `did_close`. `full/delta` always recomputes (the win is wire size, not server CPU): if the cached `result_id` matches the client's `previousResultId` it returns a single minimal edit (`semantic_token_edits`, a whole-token prefix/suffix trim); otherwise it returns a full payload with a fresh id, which the protocol allows.

## Tests

`tests.rs` exercises each path: declaration sites, reference resolution and inheritance via the db, the script-global recolour and its workspace-shadow override, range and cancellation, and the negative cases that must emit nothing (unresolvable idents, primitive type names).
