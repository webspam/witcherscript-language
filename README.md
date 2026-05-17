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
- Duplicate top-level symbol names: a class, struct, enum, state, function, or event must
  not share a name with another top-level declaration anywhere in the workspace. Each
  conflicting declaration is flagged, with related-information links to the others.
  Modding-annotation member injections (`@addMethod`/`@wrapMethod`/...) are exempt.
- Duplicate local declarations (error): two parameters, two local `var`s, or a parameter
  and a local `var` with the same name inside one function. `@wrapMethod` and
  `@replaceMethod` functions are exempt — they intentionally mirror the wrapped/replaced
  signature.
- Shadowing (warning): a parameter, local `var`, or member field whose name collides with
  a `redscripts.ini` `[globals]` entry; or a local `var` whose name collides with a field
  declared in the enclosing class/struct/state. `@wrapMethod` and `@replaceMethod`
  functions are exempt.
- Unknown method on a known receiver type: a `receiver.Method()` call where `receiver`
  resolves to a workspace `class`/`struct`/`state` but `Method` is not declared on that
  type or any of its supertypes (inheritance traversed up to depth 32). Calls on
  unknown/primitive receivers, on `super`/`parent`/`virtualParent`, on casts, or through
  indexed/parenthesised expressions are skipped to avoid false positives. Private members
  count as known.
- Unknown type (`unknown_type`): a type-position identifier that doesn't resolve to a
  workspace `class`/`struct`/`enum`/`state` or a built-in primitive. Covers `extends Foo`,
  `state S in Foo`, `: Foo` annotations (including nested generics), `new Foo in owner`,
  `(Foo) value` casts, and `@addMethod(Foo)` / `@addField(Foo)` annotation arguments.
- Unknown member (`unknown_member`): `receiver.field` on a known workspace type where
  `field` is not a member of that type or any supertype. Also fires inside `default
  field = …;`, `defaults { field = …; }`, and `hint field = "…";` blocks when the
  enclosing class/struct/state has no such field. Skipped when the receiver type can't be
  inferred (cascading) or is primitive; method-call cases are owned by `unknown_method`.
- Unknown function (`unknown_function`): a bare `Foo()` call where `Foo` doesn't resolve
  to a top-level function, a method on `this` (this-shorthand, including up the
  inheritance chain), or a script-environment global.
- Unknown identifier (`unknown_identifier`): a bare identifier used as a value that
  doesn't resolve to a local, parameter, field via this-shorthand, top-level symbol, or
  script-environment global. Idents inside tree-sitter error/missing subtrees and inside
  `incomplete_member_access_expr` are suppressed to avoid noise while typing. The
  `wrappedMethod` modding macro is recognised as a valid call site when it appears
  inside the body of an `@wrapMethod`-annotated function and is therefore not flagged.
- Missing wrapped-method call (`missing_wrapped_method`): an `@wrapMethod`-annotated
  function whose body does not contain a bare `wrappedMethod(...)` call. The mod
  compiler refuses to link such a function.
- Duplicate wrapped-method call (`duplicate_wrapped_method`): every bare
  `wrappedMethod(...)` call after the first inside the same `@wrapMethod` body. Only
  the first call is expanded by the compiler.
- Ternary expression (`ternary_cond_expr`): the grammar accepts `cond ? a : b`, but the
  compiler always evaluates it to `0` / `false` / `void`. Flagged so the construct is
  rewritten as an `if`/`else` before it silently returns wrong values.

`--dump-tree` prints a concrete syntax tree with node kinds plus line/column and byte
ranges.

## LSP: witcherscript-lsp

The LSP server communicates over stdin/stdout and can be integrated with any LSP-capable
editor. Build with:

```powershell
cargo build --bin witcherscript-lsp --release
```

The resulting binary is `target/release/witcherscript-lsp.exe`.

### Debug mode (TCP)

For diagnosing issues, run the server in TCP listen mode and attach your editor as a
client:

```powershell
cargo run --bin witcherscript-lsp -- --listen 9257
```

The server binds `127.0.0.1:<port>` (loopback only — never the LAN), accepts a single
client connection, and serves it until disconnect. Server logs go to stderr in the
launching terminal; when `--listen` is set and `RUST_LOG` is unset, the default filter
is `warn,witcherscript_lsp=trace,witcherscript_parser=trace` so own-crate trace events
show up immediately and dependency crates stay quiet. Configure your editor's LSP
client extension to connect to `127.0.0.1:9257` instead of spawning the binary.

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

### LSP Configuration

The server reads the following user-configurable settings:

| Key | Type | Default | Description |
|---|---|---|---|
| `witcherscript.gameDirectory` | `string` | `""` | Absolute path to the Witcher 3 install root (e.g. `C:\GOG Games\The Witcher 3`). The server appends `content\content0\scripts` and indexes the ~1,700 base game scripts. It also loads engine globals from `bin\redscripts.ini`. |
| `witcherscript.additionalScriptDirectories` | `string[]` | `[]` | Extra root directories to walk recursively for `.ws` files. Each entry is loaded as read-only base scripts: their symbols join the global namespace, but renames are rejected. Use this when writing co-dependent mods that need to see each other's declarations. |
| `witcherscript.autoLoadModSharedImports` | `boolean` | `true` | Auto-load the **Shared Imports** mod (a specific community mod at `<gameDirectory>\Mods\modSharedImports` that most modern Witcher 3 mods depend on to avoid clashes between `import` declarations). When this flag is on and the directory exists, it is loaded automatically — see "Auto-loaded: the Shared Imports mod" below. |

#### Auto-loaded: the Shared Imports mod

Most modern Witcher 3 mods depend on a specific community mod called **Shared Imports**, installed at `<gameDirectory>\Mods\modSharedImports`. It carves out a shared set of `import function` headers so multiple mods do not redeclare clashing imports.

Because that mod is a near-universal dependency, **the LSP loads it automatically** whenever `gameDirectory` is set and the `Mods\modSharedImports` directory exists. The user does not need to list it under `additionalScriptDirectories`.

When the auto-load fires, the LSP log line carries `auto_loaded = true` and the message starts with `[auto-detected]`. Search the server log for `[auto-detected]` if you are surprised to see symbols you did not configure.

To opt out entirely, set `witcherscript.autoLoadModSharedImports` to `false`.

**How the server receives this value**

The server uses two complementary LSP mechanisms:

1. **`workspace/configuration`** (primary) — after the `initialized` notification the server
   sends a `workspace/configuration` request for `witcherscript.gameDirectory`. The
   `vscode-languageclient` `LanguageClient` fulfils this automatically from the user's VS Code
   settings; no extra client code is needed. The server also handles
   `workspace/didChangeConfiguration` notifications, so changing the path in VS Code settings
   re-indexes the base scripts without restarting.

2. **`initializationOptions`** (fallback) — the client may pass the path in the
   `initialize` request so the server has a value immediately at startup, before the
   `workspace/configuration` round-trip completes.

**VS Code plugin integration**

*`package.json` — declare the settings:*
```json
"contributes": {
  "configuration": {
    "title": "WitcherScript",
    "properties": {
      "witcherscript.gameDirectory": {
        "type": "string",
        "default": "",
        "description": "Absolute path to the Witcher 3 install root."
      },
      "witcherscript.additionalScriptDirectories": {
        "type": "array",
        "items": { "type": "string" },
        "default": [],
        "description": "Extra root directories to walk recursively for .ws files. Each is indexed as read-only base scripts."
      },
      "witcherscript.autoLoadModSharedImports": {
        "type": "boolean",
        "default": true,
        "description": "Auto-load <gameDirectory>\\Mods\\modSharedImports (the Shared Imports mod). See server README."
      }
    }
  }
}
```

*Extension activation — pass as `initializationOptions` for a fast first start:*
```typescript
const cfg = vscode.workspace.getConfiguration('witcherscript');
const clientOptions: LanguageClientOptions = {
  documentSelector: [{ scheme: 'file', language: 'witcherscript' }],
  initializationOptions: {
    gameDirectory: cfg.get<string>('gameDirectory') ?? '',
    additionalScriptDirectories: cfg.get<string[]>('additionalScriptDirectories') ?? [],
    autoLoadModSharedImports: cfg.get<boolean>('autoLoadModSharedImports') ?? true,
  },
};
```

The `LanguageClient` handles all `workspace/configuration` and `workspace/didChangeConfiguration`
traffic automatically once the setting is declared in `package.json`.

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
cargo nextest run
```

Tests run via [cargo-nextest](https://nexte.st). Install with `cargo binstall cargo-nextest` or `winget install nextest.cargo-nextest`.

Parser fixtures live under `tests/fixtures/valid` and `tests/fixtures/invalid`. Add `.ws`
files there when covering larger WitcherScript examples; the fixture tests discover those
files automatically.

Unit tests are embedded in `diagnostics/`, `symbols.rs`, `line_index.rs`, and
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
- The grammar dependency is pinned to the `webspam` fork so future grammar fixes can be
  made outside this repo and consumed by retargeting the Cargo dependency.
- Definition resolution does not yet follow inheritance chains (e.g. resolving a member
  through a base class requires the type name to match exactly).
