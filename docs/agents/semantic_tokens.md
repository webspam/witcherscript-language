# Semantic tokens

Resolution-aware highlighting layered on top of the TextMate grammar. TextMate already colours keywords and the constant-language tokens (`true`/`false`/`null`/`this`/`super`); semantic tokens supply only what TextMate cannot know without resolving a name - whether an identifier is a class, a field, a local, an enum member, and so on. Whatever the grammar already handles is deliberately left unclassified here.

**Code:** `src/semantic_tokens/mod.rs` holds the walk and `classify()`; tests in `src/semantic_tokens/tests.rs`. The LSP request handlers are in `src/bin/witcherscript-lsp/queries/semantic_tokens.rs`, the delta cache in `src/bin/witcherscript-lsp/semantic_tokens_cache.rs`.

## Data flow

All three LSP requests - `full`, `full/delta`, `range` - funnel into `collect_in_byte_range`. It walks the CST in tree order (top-to-bottom, left-to-right, the order LSP requires) and calls `classify()` on each node. Four properties of that walk are load-bearing:

- A node that classifies is emitted as one token spanning the whole node, and **its children are not visited**. If you teach `classify()` to colour a parent node, you suppress every token inside it.
- A named node that does not classify is recursed into; an anonymous (keyword/punctuation) node that does not classify is skipped without recursing.
- A token is emitted only when it starts and ends on one line. Identifiers never span lines; a multi-line string literal is the one thing that can, and it is dropped, because the delta encoding cannot represent a token across lines.
- An identifier that resolves to nothing emits no token. Highlighting is therefore best-effort: an undefined type simply stays uncoloured rather than being guessed.

The collected tokens are then delta-encoded (see [Encoding](#encoding)).

## Token types and the TextMate split

`TOKEN_TYPES` and `TOKEN_MODIFIERS` (the LSP legend) are declared at the top of `mod.rs`; index numbers used in this doc (e.g. `modifier` (13)) are positions in that list. The parts that are not obvious from the list itself:

- `keyword` (7) and `type` (11) are registered but never emitted. `keyword` is left to TextMate; `type` only reserves its index so the later ones stay stable - a type annotation is resolved and emitted as its target's real kind instead.
- One token type covers several symbol kinds (`symbol_kind_to_token_type`): `class` also paints Struct, State, and native types; `function` also paints methods and events.
- Only `defaultLibrary` is ever emitted, and only on redscripts.ini globals; `declaration` is registered but never set.

## What classify() colours

`classify()` dispatches on `node.kind()`. The non-trivial cases:

- **Literals** take their obvious type, with one surprise: a CName literal (`'SomeName'`) is emitted as `enumMember`, not `string`.
- **`specifier`, `func_flavour`, `autobind_single`, and the declaration/modifier keywords** (the `class`/`var`/`private`/`latent`/... set in `classify_anonymous_keyword`) are `modifier`. Control-flow and constant keywords are intentionally absent from that set - TextMate owns them.
- **`annotation_ident`** (`@addField`, ...) is `decorator`.
- **`ident`** is the hard case, below.

## Identifiers: declaration vs reference

A declaration-site ident takes a fixed token type from its parent decl node (class/struct/state -> `class`, enum -> `enum`, enum member -> `enumMember`, func/event -> `function`, member-var and autobind -> `property`, local var -> `variable`, param -> `parameter`). No resolution is involved; these always emit.

Every other ident is a reference and must be resolved, in `classify_ident`'s fallback arm:

1. `classify_locally()` - locals/params of the enclosing function, then members of the enclosing class/struct/state, then document top-level.
2. If the ident is the right-hand side of a `member_access_expr` (after the `.`), it resolves through `resolve_member_access()`, which infers the receiver's type (below) and looks the member up on it.
3. Otherwise `classify_definition_at_ident()` walks the same local scope out to the workspace db.

A reference that resolves is coloured by its definition's kind. One special case: redscripts.ini script globals (e.g. `thePlayer`) resolve to the class they alias, but are recoloured `variable` + `defaultLibrary` so they do not paint as a type. A workspace symbol that shadows the global name wins normally and keeps its real kind.

### Receiver-type inference (`resolve_member_access`, `src/resolve/inference.rs`)

| Receiver | Inferred type |
|---|---|
| `this_expr` | enclosing class/struct/state |
| `super_expr` | its `base_class` |
| `parent_expr`, `virtual_parent_expr` | the state's `owner_class` |
| `ident` | the local/param/field's declared type, else a script-global's type |
| `func_call_expr`, `member_access_expr` | the receiver's return type, inferred recursively |

The member is then looked up on that type in the document's own symbols first, then the workspace db.

## Encoding

LSP delta-encodes tokens as a flat `Vec<u32>` in groups of five: `[delta_line, delta_start, length, token_type, token_modifiers_bitset]`. `delta_line` and `delta_start` are relative to the previous token; on a new line `delta_start` is the absolute column. The modifier bitset is 0 for every token except redscripts.ini globals (`defaultLibrary`, bit 1).

## Request handlers

- **`range`** prunes the walk to CST subtrees intersecting the requested byte range (clamped on conversion failure, as `inlay_hints` does). Edge-overlapping tokens are kept - the spec permits overflow - and the deltas still start from the document origin, so the payload stands alone.
- **`full` / `full/delta`** mint a monotonic `result_id` and cache the emitted `Vec<u32>` per open document in `Backend::semantic_tokens_cache`, dropped on `did_close`. `full/delta` always recomputes (the win is wire size, not server CPU): if the cached `result_id` matches the client's `previousResultId` it returns a single minimal edit (`semantic_token_edits`, a whole-token prefix/suffix trim); otherwise it returns a full payload with a fresh id, which the protocol allows.

## Tests

`tests.rs` exercises each path: declaration sites, reference resolution and inheritance via the db, the script-global recolour and its workspace-shadow override, range and cancellation, and the negative cases that must emit nothing (unresolvable idents, primitive type names).
