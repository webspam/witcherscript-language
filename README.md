# WitcherScript Language Tools

Rust crate providing a WitcherScript (`.ws`) parser, syntax validator, and Language
Server Protocol (LSP) server built on Tree-sitter.

Two binaries are produced:

- **`witcherscript-check`** - CLI parser / parse-tree inspector.
- **`witcherscript-lsp`** - LSP server for editor integration.

## CLI: witcherscript-check

### Usage

The command accepts one or more file or directory paths. Directory inputs are searched
recursively for `.ws` files.

```
cargo run -- path/to/file.ws
cargo run -- path/to/src path/to/debug
cargo run -- path/to/file.ws --dump-tree
```

`--dump-tree` prints a concrete syntax tree with node kinds plus line/column and byte
ranges. `--max-diagnostics N` (default 20) caps the diagnostics printed per file.

Exit codes:

- `0`: all parsed files have no diagnostics.
- `1`: one or more files have tree-sitter parse errors.
- `2`: CLI, IO, setup, or parser initialisation error.

### Diagnostics

Parse diagnostics include the file path, start line/column, end line/column, byte range,
node kind, and a source-line snippet when available.

The CLI only reports tree-sitter parse errors. The cross-file validation rules below
(duplicate symbols, unknown member, shadowing, …) are produced by the library but only
surfaced through the LSP server.

## Validation rules

In addition to tree-sitter parse errors, the LSP server publishes a set of validation
diagnostics. See [docs/diagnostics/validation.md](docs/diagnostics/validation.md) for the full list.

## LSP: witcherscript-lsp

The LSP server communicates over stdin/stdout and can be integrated with any LSP-capable
editor. Build with:

```
cargo build --bin witcherscript-lsp --release
```

The resulting binary is `target/release/witcherscript-lsp.exe`.

### Debug mode (TCP)

For diagnosing issues, run the server in TCP listen mode and attach your editor as a
client:

```
cargo run --bin witcherscript-lsp -- --listen 9257
```

The server binds `127.0.0.1:<port>` (loopback only - never the LAN), accepts a single
client connection, and serves it until disconnect. Server logs go to stderr in the
launching terminal; when `--listen` is set and `RUST_LOG` is unset, the default filter
is `warn,witcherscript_lsp=trace,witcherscript_language=trace` so own-crate trace events
show up immediately and dependency crates stay quiet. Configure your editor's LSP
client extension to connect to `127.0.0.1:9257` instead of spawning the binary.

### LSP capabilities

| Capability | Detail |
|---|---|
| Text sync | Incremental document sync over the wire; the parse tree is rebuilt from the patched source each change |
| Diagnostics | Parse errors and the [validation rules](docs/diagnostics/validation.md), published after every parse |
| Go-to-definition | Locals, parameters, fields (`this.x`), and workspace-wide top-level symbols; inheritance traversed up to depth 32 |
| Find references | Locals, parameters, fields, methods, and top-level symbols |
| Document highlight | Highlights every occurrence of the symbol under the cursor within the current file; writes (assignment targets and declarations) are distinguished from reads |
| Rename | Workspace-wide rename with `prepare_rename`; symbols declared in base scripts are rejected |
| Hover | Signature or type annotation in a fenced `witcherscript` code block |
| Completion | Triggered by `.`, `:`, `@`; covers members, expressions, statements, types, annotations, and keywords |
| Signature help | Triggered by `(` and `,`; highlights the active parameter |
| Document symbols | Nested outline of classes, structs, enums, functions, methods, states, events, and fields |
| Workspace symbols | Project-wide symbol search (open with the editor's "Go to Symbol in Workspace"); fuzzy name match across your project, the base game scripts, and engine builtins, ranked best-first with project results first |
| Semantic tokens | Full-document semantic highlighting; legend exposed in `initialize` |
| Inlay hints | Parameter-name labels at call sites (`out` parameters shown as `out name:`); honours the requested range and skips a non-`out` argument that already spells the parameter name. On by default; toggle with `witcherscript.inlayHints` |
| Document formatting | Pretty-prints whole documents using `witcherscript.formatter.*` settings |
| Code actions | Quick fix for `base_script_conflict`; toggle `switch` cases and `if`/`else` chains between inline and block layout; extract a selected expression to a new local variable (declared at the top of the enclosing function, rename offered immediately) |
| Code lens | On legacy override files, a "game definition" lens above each top-level symbol that shadows a base game symbol; jumps to the vanilla definition. Gated by `witcherscript.codeLens.overriddenSymbols`. Optionally, an "N references" lens above declarations and class methods; clicking it lists the references. Gated by `witcherscript.codeLens.references` |

On startup the server indexes every `.ws` file in the workspace root(s), then keeps
open documents in sync as they are edited.

### LSP Configuration

The server reads the following user-configurable settings:

| Key | Type | Default | Description |
|---|---|---|---|
| witcherscript.gameDirectory | `string` | `""` | Witcher 3 install root. Base game scripts and engine globals are indexed from here. |
| witcherscript.additionalScriptDirectories | `string[]` | `[]` | Extra read-only script roots, walked recursively. |
| witcherscript.legacyScriptDirectories | `string[]` | `[]` | Roots holding legacy script overrides; a replaced base script is indexed as the editable override instead. |
| witcherscript.autoLoadModSharedImports | `boolean` | `true` | Auto-load the Shared Imports mod from `gameDirectory` when present (see below). |
| witcherscript.detectProjectManifests | `boolean` | `true` | Treat each `witcherscript.toml`'s `scripts_root` as a legacy script directory. |
| witcherscript.codeLens.overriddenSymbols | `boolean` | `true` | "Game definition" lens on overridden symbols in legacy override files. |
| witcherscript.codeLens.references | `boolean` | `false` | "N references" lens on top-level declarations and class methods. |
| witcherscript.inlayHints.parameterNames | `boolean` | `true` | Parameter-name inlay hints at call sites. |
| witcherscript.diagnostics.scope | `string` | `"workspace"` | Files to diagnose: `workspace`, `openFiles`, or `none`. |
| witcherscript.logLevel | `string` | `"warn"` | Server log level: `error`, `warn`, `debug`, `trace`. |
| witcherscript.formatter.lineLimit | `number` | `100` | Soft wrap width. |
| witcherscript.formatter.compactColon | `boolean` | `false` | Drop the space before `:` in type annotations. |
| witcherscript.formatter.alignMemberColons | `boolean` | `false` | Align `:` across consecutive member declarations. |
| witcherscript.formatter.annotationPlacement | `string` | `"preserve"` | `@addField` placement: `preserve`, `ownLine`, `sameLine`. |
| witcherscript.formatter.defaultPlacement | `string` | `"preserve"` | Trailing `default` placement: `preserve`, `ownLine`, `sameLine`. |
| files.exclude | `object` | `{}` | VS Code exclude globs, honored when walking script roots. |

#### Auto-loaded: the Shared Imports mod

Most modern Witcher 3 mods depend on a specific community mod called **Shared Imports**, installed at `<gameDirectory>\Mods\modSharedImports`. It carves out a shared set of `import function` headers so multiple mods do not redeclare clashing imports.

Because that mod is a near-universal dependency, **the LSP loads it automatically** whenever `gameDirectory` is set and the `Mods\modSharedImports` directory exists. The user does not need to list it under `additionalScriptDirectories` or `legacyScriptDirectories`.

It ships replacement scripts that stand in for base-game files, so the LSP indexes it as a legacy script directory: each override takes the place of the base script it replaces instead of colliding with it.

To opt out entirely, set `witcherscript.autoLoadModSharedImports` to `false`.

**How the server receives settings**

Two complementary LSP mechanisms:

1. **`workspace/configuration`** (primary) - after the `initialized` notification the
   server pulls each setting via a `workspace/configuration` request. The
   `vscode-languageclient` `LanguageClient` fulfils this automatically from the user's
   VS Code settings; no extra client code is needed. The server also handles
   `workspace/didChangeConfiguration` notifications, so changing settings live re-indexes
   when relevant without restarting.

2. **`initializationOptions`** (fallback) - the client may pass any of the above settings
   in the `initialize` request so the server has values immediately at startup, before
   the `workspace/configuration` round-trip completes.

## Symbol extraction

The library extracts a flat symbol table from each document during parsing. Symbols carry:

- `name`, `kind` (Class, Struct, Enum, EnumMember, Function, Method, Field, Variable,
  Parameter, State, Event)
- `range` / `selection_range` as UTF-16 line/character positions (LSP-compatible)
- `byte_range` / `selection_byte_range` for fast cursor queries
- `container` - parent `SymbolId` for members, `None` for top-level declarations
- `type_annotation`, `signature`, `base_class`, `owner_class`, `flavour`, `annotations`
  (`@wrapMethod`, `@addMethod`, …) - plus a `display_detail()` helper that renders
  `extends`/`in` strings for LSP hover

`WorkspaceIndex` in `src/resolve/workspace_index/` maintains a per-URI symbol list and supports
cross-file go-to-definition lookups.

## Tests

```
just test
```

Tests run via [cargo-nextest](https://nexte.st). Install with
`cargo binstall cargo-nextest` or `winget install nextest.cargo-nextest`. Nextest config
lives at `.config/nextest.toml`.

Parser fixtures live under `tests/fixtures/valid` and `tests/fixtures/invalid`. Add `.ws`
files there when covering larger WitcherScript examples; the fixture tests discover those
files automatically.

Unit tests are embedded in `src/diagnostics/`, `src/symbols/`, `src/line_index.rs`,
`src/script_env.rs`, `src/resolve/tests/`, `src/semantic_tokens/tests.rs`, and
`src/bin/witcherscript-lsp/tests.rs`. Integration tests for symbol extraction and
definition resolution live in `tests/language_features.rs`; fixture-driven parse tests
live in `tests/parser_fixtures.rs`.

## Caveats

- This tool reports Tree-sitter parse errors plus a small set of explicit validation
  rules. It does not reject every construct that the WitcherScript compiler or this
  repo's style rules may reject.
- The grammar dependency is pinned to the `webspam` fork so future grammar fixes can be
  made outside this repo and consumed by retargeting the Cargo dependency.
