# Semantic tokens

Resolution-aware highlighting layered on top of the TextMate grammar. TextMate already colours keywords and the constant-language tokens (`true`/`false`/`null`/`this`/`super`); semantic tokens supply only what TextMate cannot know without resolving a name - whether an identifier is a class, a field, a local, an enum member, and so on. Whatever the grammar already handles is deliberately left unclassified here.

**Code:** `src/semantic_tokens/mod.rs` holds the walk and `classify()`; tests in `src/semantic_tokens/tests.rs`. The LSP request handlers are in `src/bin/witcherscript-lsp/queries/semantic_tokens.rs`, the delta cache in `src/bin/witcherscript-lsp/semantic_tokens_cache.rs`.

## Token types

`TOKEN_TYPES` and `TOKEN_MODIFIERS` (the LSP legend) are declared at the top of `mod.rs`; the index numbers used below (e.g. `modifier` (13)) are positions in that list. The parts not obvious from the list itself:

- `keyword` (7) and `type` (11) are registered but never emitted. `keyword` is left to the TextMate grammar; `type` exists only to keep the later indices stable - a type-annotation identifier is resolved and emitted as its real symbol kind instead.
- One token type covers several symbol kinds (`symbol_kind_to_token_type`): `class` also paints Struct, State, and native types; `function` also paints methods and events.
- Of the two modifiers, only `defaultLibrary` is emitted (on redscripts.ini globals); `declaration` is never set.

## Classification rules

The `classify()` function dispatches on `node.kind()`:

| Grammar node | Token type |
|---|---|
| `ident` | see classify_ident() below |
| `annotation_ident` | `decorator` (12) |
| `comment` | `comment` (8) |
| `literal_string` | `string` (9) |
| `literal_name` | `enumMember` (2) - CName literals like `'SomeName'` |
| `literal_int`, `literal_float`, `literal_hex` | `number` (10) |
| `specifier` | `modifier` (13) |
| `func_flavour`, `autobind_single` | `modifier` (13) |
| Anonymous node (keyword text) | `modifier` (13) if in keyword list, else skipped |

`literal_bool`, `literal_null`, `this_expr`, `super_expr`, etc. are **not classified** - TextMate grammar handles constant/language keywords.

### Keyword list (anonymous nodes → modifier)

`class`, `struct`, `enum`, `state`, `function`, `event`, `extends`, `var`, `autobind`, `defaults`, `hint`, `abstract`, `statemachine`, `latent`, `import`, `const`, `final`, `editable`, `saved`, `optional`, `out`, `inlined`, `private`, `protected`, `public`, `cleanup`, `entry`, `exec`, `quest`, `reward`, `storyscene`, `timer`, `single`

## classify_ident: declaration vs reference

`ident` nodes have two modes based on their parent:

**Declaration sites** (syntactically unambiguous, always emit):

| Parent node | Token type |
|---|---|
| `class_decl`, `struct_decl`, `state_decl` | `class` (0) |
| `enum_decl` | `enum` (1) |
| `enum_member_decl` | `enumMember` (2) |
| `func_decl`, `event_decl` | `function` (3) |
| `func_param_group` | `parameter` (4) |
| `member_var_decl` | `property` (6) |
| `local_var_decl_stmt` | `variable` (5) |
| `autobind_decl` | `property` (6) |

**Reference sites** (must resolve; no token if unresolvable):

All reference-site `ident` nodes fall through to the `_` arm in `classify_ident`, which:

1. Calls `classify_locally()` (local variables/parameters of enclosing function, then members of enclosing class/struct/state, then top-level symbols in the current document).
2. If the ident is the RHS of a `member_access_expr` (i.e. after the `.`), calls `classify_definition_at_ident()` directly, which dispatches to `resolve_member_access()` to infer the receiver type and look up the member.
3. Otherwise, calls `classify_definition_at_ident()` which searches locals, type members, document top-level, then the workspace db (`find_top_level`, `find_enum_member`, `find_script_global`). If the resolved definition is the class a script global redirects to (Go-To-Def jumps to `CR4Player` for `thePlayer`), or the synthetic INI Variable when that class is not loaded, the token is recoloured as `variable` (5) with the `defaultLibrary` modifier so `thePlayer` doesn't paint as a type. Workspace symbols that shadow the global name win normally and are not overridden.

If nothing resolves, no token is emitted for the identifier.

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

Tokens are produced in tree walk order (top-to-bottom, left-to-right), which matches LSP requirements.

## Range requests

`textDocument/semanticTokens/range` is served by `collect_semantic_tokens_in_range_cancellable`, which converts the LSP range to a byte range (clamped on conversion failure, mirroring `inlay_hints`) and prunes every CST subtree that does not intersect it. Tokens partially overlapping the range edges are included; the LSP spec permits overflow. The encoded deltas still start from the document origin, so a range payload is standalone.

## Full/delta requests

`textDocument/semanticTokens/full` and `full/delta` mint a monotonically increasing `result_id` and store the exact returned `Vec<u32>` per open document in `Backend::semantic_tokens_cache` (`src/bin/witcherscript-lsp/semantic_tokens_cache.rs`), evicted on `did_close`. A `full/delta` request recomputes tokens (the saving is wire size and client work, not server CPU), then:

- previous entry matches `previousResultId` -> `SemanticTokensDelta` with a single minimal edit from `semantic_token_edits` (whole-token prefix/suffix trim; lsp-types represents edit data as `SemanticToken` structs, so edits stay 5-u32 aligned),
- otherwise -> a full payload with a fresh `result_id` (protocol-correct fallback, never an error).

## Single-line constraint

A token is emitted only when it starts and ends on the same line. Identifiers never span lines; a multi-line string literal is the only case that can, and it is silently skipped (the per-line delta encoding can't represent a token spanning lines).

## Recursion rule

When `classify()` returns `Some(type)` for a node, the token covers the whole node span and the children are NOT visited. When `classify()` returns `None` for a named node, children are recursed. Anonymous nodes with no classification are silently skipped without recursion.

## Tests

`src/semantic_tokens/tests.rs` covers:
- Class/enum/function/field/variable declaration sites
- Resolved type annotations (only highlighted if the type is defined); primitive names like `int` get no token
- Member access with `this.field`, local variable type inference
- Inheritance: members from base classes via the db
- Unresolvable identifiers produce no token
- Script globals colour as `variable` + `defaultLibrary`; a workspace class shadowing the global name wins as `class`
- CName literals (`'SomeName'`) emit `enumMember`
- Keywords emit `modifier`
- Comments and strings emit correct types
- Cancellation returns `None`; range requests emit only the intersecting tokens
