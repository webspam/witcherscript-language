# WitcherScript Language Tools

Rust crate providing a WitcherScript (`.ws`) parser, syntax validator, and Language
Server Protocol (LSP) server built on Tree-sitter.

Two binaries are produced:

- **`witcherscript-check`** — CLI parser / parse-tree inspector.
- **`witcherscript-lsp`** — LSP server for editor integration.

## CLI: witcherscript-check

### Usage

The command accepts one or more file or directory paths. Directory inputs are searched
recursively for `.ws` files.

```powershell
cargo run -- path\to\file.ws
cargo run -- path\to\src path\to\debug
cargo run -- path\to\file.ws --dump-tree
```

If Cargo is not on `PATH` in PowerShell, use:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -- path\to\src
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

In addition to tree-sitter parse errors, the LSP server publishes the following
diagnostics:

- Local `var` declarations must precede executable statements within each function block.
  Blank lines, comments, and bare semicolons do not count as executable statements.
- Duplicate top-level symbol names: a class, struct, enum, state, function, or event must
  not share a name with another top-level declaration anywhere in the workspace. Each
  conflicting declaration is flagged, with related-information links to the others.
  Modding-annotation member injections (`@addMethod`/`@wrapMethod`/...) are exempt.
- Base-script conflict (`base_script_conflict`, error): a workspace file whose path and
  name match a base game script (e.g. a copied-and-edited `game/r4Player.ws`) and which
  redeclares the base script's top-level symbols. Each clashing declaration is flagged
  with a related-information link to the base declaration. This is a legacy full-script
  override; the message asks the user to either mark the directory under
  `witcherscript.legacyScriptDirectories` (a quick fix is offered) or switch to
  annotation-based modding. Inside such a file the generic duplicate-symbol error is
  suppressed in favour of this clearer one.
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
  enclosing class/struct/state has no such field. The `hint` case is reported at info
  level, since the compiler accepts a hint for any member name. Skipped when the receiver
  type can't be inferred (cascading) or is primitive; method-call cases are owned by
  `unknown_method`.
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
is `warn,witcherscript_lsp=trace,witcherscript_language=trace` so own-crate trace events
show up immediately and dependency crates stay quiet. Configure your editor's LSP
client extension to connect to `127.0.0.1:9257` instead of spawning the binary.

### LSP capabilities

| Capability | Detail |
|---|---|
| Text sync | Incremental document sync over the wire; the parse tree is rebuilt from the patched source each change |
| Diagnostics | Parse errors and the [validation rules](#validation-rules) above, published after every parse |
| Go-to-definition | Locals, parameters, fields (`this.x`), and workspace-wide top-level symbols; inheritance traversed up to depth 32 |
| Find references | Locals, parameters, fields, methods, and top-level symbols |
| Rename | Workspace-wide rename with `prepare_rename`; symbols declared in base scripts are rejected |
| Hover | Signature or type annotation in a fenced `witcherscript` code block |
| Completion | Triggered by `.`, `:`, `@`; covers members, expressions, statements, types, annotations, and keywords |
| Signature help | Triggered by `(` and `,`; highlights the active parameter |
| Document symbols | Nested outline of classes, structs, enums, functions, methods, states, events, and fields |
| Semantic tokens | Full-document semantic highlighting; legend exposed in `initialize` |
| Document formatting | Pretty-prints whole documents using `witcherscript.formatter.*` settings |
| Code actions | Quick fix for `base_script_conflict`: marks the conflicting directory as a legacy override directory |

On startup the server indexes every `.ws` file in the workspace root(s), then keeps
open documents in sync as they are edited.

### LSP Configuration

The server reads the following user-configurable settings:

| Key | Type | Default | Description |
|---|---|---|---|
| `witcherscript.gameDirectory` | `string` | `""` | Absolute path to the Witcher 3 install root (e.g. `C:\GOG Games\The Witcher 3`). The server appends `content\content0\scripts` and indexes the ~1,700 base game scripts. It also loads engine globals from `bin\redscripts.ini`. |
| `witcherscript.additionalScriptDirectories` | `string[]` | `[]` | Extra root directories to walk recursively for `.ws` files. Each entry is loaded as read-only base scripts: their symbols join the global namespace, but renames are rejected. Use this when writing co-dependent mods that need to see each other's declarations. |
| `witcherscript.legacyScriptDirectories` | `string[]` | `[]` | Directories holding legacy full-script overrides — copies of base game scripts edited in place. Each base script a legacy file replaces is dropped from the read-only base index, and the legacy file is indexed as a normal (editable) workspace file. Marking a directory here is what silences the `base_script_conflict` diagnostic; the diagnostic's quick fix appends to this list. The editor shows a "legacy script" status-bar indicator only for a legacy file that actually replaces a base game script — not for a brand-new script that merely sits in a legacy directory. |
| `witcherscript.autoLoadModSharedImports` | `boolean` | `true` | Auto-load the **Shared Imports** mod (a specific community mod at `<gameDirectory>\Mods\modSharedImports` that most modern Witcher 3 mods depend on to avoid clashes between `import` declarations). When this flag is on and the directory exists, it is loaded automatically — see "Auto-loaded: the Shared Imports mod" below. |
| `witcherscript.diagnostics.enable` | `boolean` | `true` | Master switch for all diagnostics (parse errors, duplicate symbols, shadowing warnings, late-local-var, etc.). Set to `false` to suppress every diagnostic the server would otherwise publish — useful when reviewing partial-port or legacy mod source where squiggles are noise. Live-toggleable: flipping it off clears any visible diagnostics; flipping it back on republishes them. |
| `witcherscript.logLevel` | `string` | `"warn"` | Server log level (`error`, `warn`, `debug`, `trace`; unknown values fall back to `warn`). Live-toggleable via `workspace/didChangeConfiguration`. |
| `witcherscript.formatter.lineLimit` | `number` | `100` | Soft wrap width for the formatter. |
| `witcherscript.formatter.compactColon` | `boolean` | `false` | Drop the space before `:` in type annotations when formatting. |
| `witcherscript.formatter.alignMemberColons` | `boolean` | `false` | Align `:` on consecutive member declarations when formatting. |
| `files.exclude` | `object` | `{}` | Standard VS Code exclude globs. The server respects these when walking workspace roots. |

#### Auto-loaded: the Shared Imports mod

Most modern Witcher 3 mods depend on a specific community mod called **Shared Imports**, installed at `<gameDirectory>\Mods\modSharedImports`. It carves out a shared set of `import function` headers so multiple mods do not redeclare clashing imports.

Because that mod is a near-universal dependency, **the LSP loads it automatically** whenever `gameDirectory` is set and the `Mods\modSharedImports` directory exists. The user does not need to list it under `additionalScriptDirectories` or `legacyScriptDirectories`.

It ships replacement scripts that stand in for base-game files, so the LSP indexes it as a legacy script directory: each override takes the place of the base script it replaces instead of colliding with it.

To opt out entirely, set `witcherscript.autoLoadModSharedImports` to `false`.

**How the server receives settings**

Two complementary LSP mechanisms:

1. **`workspace/configuration`** (primary) — after the `initialized` notification the
   server pulls each setting via a `workspace/configuration` request. The
   `vscode-languageclient` `LanguageClient` fulfils this automatically from the user's
   VS Code settings; no extra client code is needed. The server also handles
   `workspace/didChangeConfiguration` notifications, so changing settings live re-indexes
   when relevant without restarting.

2. **`initializationOptions`** (fallback) — the client may pass any of the above settings
   in the `initialize` request so the server has values immediately at startup, before
   the `workspace/configuration` round-trip completes.

**VS Code plugin integration**

Declare each setting in your extension's `package.json` under
`contributes.configuration.properties` so VS Code surfaces them in Settings UI, then
forward the current values as `initializationOptions` from your extension's activation
code:

```typescript
const cfg = vscode.workspace.getConfiguration('witcherscript');
const clientOptions: LanguageClientOptions = {
  documentSelector: [{ scheme: 'file', language: 'witcherscript' }],
  initializationOptions: {
    gameDirectory: cfg.get<string>('gameDirectory') ?? '',
    additionalScriptDirectories: cfg.get<string[]>('additionalScriptDirectories') ?? [],
    autoLoadModSharedImports: cfg.get<boolean>('autoLoadModSharedImports') ?? true,
    diagnostics: { enable: cfg.get<boolean>('diagnostics.enable') ?? true },
    logLevel: cfg.get<string>('logLevel') ?? 'warn',
    formatter: {
      lineLimit: cfg.get<number>('formatter.lineLimit') ?? 100,
      compactColon: cfg.get<boolean>('formatter.compactColon') ?? false,
      alignMemberColons: cfg.get<boolean>('formatter.alignMemberColons') ?? false,
    },
  },
};
```

The `LanguageClient` handles all `workspace/configuration` and
`workspace/didChangeConfiguration` traffic automatically once the settings are declared
in `package.json`.

## Symbol extraction

The library extracts a flat symbol table from each document during parsing. Symbols carry:

- `name`, `kind` (Class, Struct, Enum, EnumVariant, Function, Method, Field, Variable,
  Parameter, State, Event)
- `range` / `selection_range` as UTF-16 line/character positions (LSP-compatible)
- `byte_range` / `selection_byte_range` for fast cursor queries
- `container` — parent `SymbolId` for members, `None` for top-level declarations
- `type_annotation`, `signature`, `base_class`, `owner_class`, `flavour`, `annotations`
  (`@wrapMethod`, `@addMethod`, …) — plus a `display_detail()` helper that renders
  `extends`/`in` strings for LSP hover

`WorkspaceIndex` in `src/resolve/db.rs` maintains a per-URI symbol list and supports
cross-file go-to-definition lookups.

## Tests

```powershell
just test
```

Tests run via [cargo-nextest](https://nexte.st). Install with
`cargo binstall cargo-nextest` or `winget install nextest.cargo-nextest`. Nextest config
lives at `.config/nextest.toml`.

Parser fixtures live under `tests/fixtures/valid` and `tests/fixtures/invalid`. Add `.ws`
files there when covering larger WitcherScript examples; the fixture tests discover those
files automatically.

Unit tests are embedded in `src/diagnostics/`, `src/symbols.rs`, `src/line_index.rs`,
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
