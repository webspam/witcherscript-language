# Resolution and workspace indexing

`src/resolve/` answers the requests that need knowledge spanning files: go-to-definition (`definition.rs`), references (`references.rs`), type inference (`inference.rs`), hover and signature help (`signature.rs`), and completion (`completion/`). Extract/inline refactors sit alongside (`extract_*`, `inline_var.rs`, `join_split_decl.rs`).

State lives in two types:

- **`WorkspaceIndex`** (`workspace_index/`) - the persistent store, updated per document edit. It indexes top-level symbols, type members, and superclass links, plus an identifier-occurrence map that lets reference search skip documents cheaply. One instance holds the user's workspace, a second holds the base (vanilla) scripts.
- **`SymbolDb`** (`symbol_db/`) - a per-request view layered over workspace + base + builtins; every cross-index lookup goes through it. Precedence is workspace, then base, then builtins, first hit wins - so the workspace shadows base for same-named symbols. Name resolution tries the narrowest scope first: locals and parameters, then enclosing-type members, then top-level symbols, then script globals.

## What reading the code won't tell you

The structural constraints are in [invariants.md](invariants.md). These are the behaviours that look wrong, or are invisible, from a single file:

- **Implicit base classes are synthesised, not written in source.** A class with no `extends` extends `CObject`; a state extends `CScriptableState`. `superclass_of()` supplies this - so inheritance walks must call it and never read `Symbol.base_class` directly, or they miss the implicit base.
- **Member lookup is first-name-wins up the chain, regardless of access**, to match the WitcherScript compiler: a private member in a subclass masks an accessible one above it. Do not "fix" it to reach the accessible ancestor.
- **`extends` / `in` are display strings, not data.** Superclass and state owner are the typed fields `base_class` / `owner_class`; the rendered `"extends X"` / `"in X"` is for hover only - never parse it for structural queries.
- **Generic members come back with a placeholder.** Looking up `array<int>` returns members whose element type is still `T`; `SymbolDb` substitutes it. `array` is never offered as a completable type name. See [builtins.md](builtins.md).
- **Script globals (`theGame`, ...) resolve last** and come from `redscripts.ini`, redirecting to the class definition when that class is loaded.
