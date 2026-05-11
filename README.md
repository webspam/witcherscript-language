# WitcherScript Language Tools

Rust workspace providing a WitcherScript (`.ws`) parser, syntax validator, and Language
Server Protocol (LSP) server built on Tree-sitter.

Two binaries are produced:

- **`witcherscript-parser`** — CLI syntax validator and parse-tree inspector.
- **`witcherscript-lsp`** — LSP server for editor integration (go-to-definition, hover,
  document symbols, inline diagnostics).

## CLI: witcherscript-parser

### Usage

From this directory:

```powershell
cargo run -- ..\src\LightRewrite.ws
cargo run -- ..\src ..\debug
cargo run -- ..\debug\editor\LRDebug_ToastOneLiner.ws --dump-tree
```

If Cargo is not on `PATH` in PowerShell, use:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -- ..\src ..\debug
```

The command accepts one or more file or directory paths. Directory inputs are searched
recursively for `.ws` files.

Exit codes:

- `0`: all parsed files have no diagnostics.
- `1`: one or more files parsed with syntax or validation diagnostics.
- `2`: CLI, IO, setup, or parser initialisation error.

### Diagnostics

Diagnostics include the file path, start line/column, end line/column, byte range, node
kind, and a source-line snippet when available.

Current validation rules:

- Local `var` declarations must precede executable statements within each function block.
  Blank lines, comments, and bare semicolons do not count as executable statements.

`--dump-tree` prints a concrete syntax tree with node kinds plus line/column and byte
ranges.

## LSP: witcherscript-lsp

The LSP server communicates over stdin/stdout and can be integrated with any LSP-capable
editor. Build with:

```powershell
cargo build --bin witcherscript-lsp --release
```

The resulting binary is `target/release/witcherscript-lsp.exe`.

### LSP capabilities

| Capability | Detail |
|---|---|
| Text sync | Full document sync on open and change |
| Diagnostics | Syntax errors and validation rules published on every parse |
| Go-to-definition | Locals, parameters, fields (`this.x`), and workspace-wide top-level symbols |
| Hover | Signature or type annotation shown in a fenced `witcherscript` code block |
| Document symbols | Nested outline of classes, structs, enums, functions, methods, states, events, and fields |

On startup the server indexes every `.ws` file in the workspace root(s), then keeps
open documents in sync as they are edited.

## Symbol extraction

The library extracts a flat symbol table from each document during parsing. Symbols carry:

- `name`, `kind` (Class, Struct, Enum, EnumVariant, Function, Method, Field, Variable,
  Parameter, State, Event)
- `range` / `selection_range` as UTF-16 line/character positions (LSP-compatible)
- `byte_range` / `selection_byte_range` for fast cursor queries
- `container` — parent `SymbolId` for members, `None` for top-level declarations
- `type_annotation`, `signature`, `detail`, `annotations` (`@wrapMethod`, `@addMethod`, …)

The `WorkspaceIndex` in `resolve.rs` maintains a per-URI symbol list and supports
cross-file go-to-definition lookups.

## Tests

```powershell
cargo test
```

Parser fixtures live under `tests/fixtures/valid` and `tests/fixtures/invalid`. Add `.ws`
files there when covering larger WitcherScript examples; the fixture tests discover those
files automatically.

Unit tests are embedded in `diagnostics.rs`, `symbols.rs`, `line_index.rs`, and
`resolve.rs`. Integration tests for language features (symbol extraction, definition
resolution) live in `tests/language_features.rs`.

## Current Validation Result

Validated against the local Light Rewrite corpus:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -- ..\src ..\debug --max-diagnostics 5
```

Result: all 32 `.ws` files under `src/` and `debug/` parsed cleanly with
`tree-sitter-witcherscript` tag `v0.13.0` from
`https://github.com/webspam/tree-sitter-witcherscript`.

No syntax or local variable ordering failures were found in the local corpus during this
pass.

## Caveats

- This tool reports Tree-sitter parse errors plus a small set of explicit validation rules.
  It does not reject every construct that the WitcherScript compiler or this repo's style
  rules may reject.
- The current grammar accepts ternary expressions, even though this project treats
  ternaries as invalid WitcherScript. That is deliberately documented rather than patched
  in this prototype.
- The grammar dependency is pinned to the `webspam` fork so future grammar fixes can be
  made outside this repo and consumed by retargeting the Cargo dependency.
- Definition resolution does not yet follow inheritance chains (e.g. resolving a member
  through a base class requires the type name to match exactly).
