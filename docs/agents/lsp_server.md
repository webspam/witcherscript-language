# LSP server

**Module:** `src/bin/witcherscript-lsp/` (~3300 lines across 10 files)

| File | Purpose |
|---|---|
| `main.rs` | Tokio entry point — wires the tracing layer + `LspLogSender`, constructs `Backend`, and serves over stdio. |
| `backend.rs` | `Backend` struct, all `LanguageServer` handler impls (`initialize`, `did_open`, `completion`, `hover`, `rename`, …). |
| `convert.rs` | LSP↔internal type conversion: `lsp_range`, `source_position`, `lsp_symbol_kind`, `completion_item`, `document_symbols`, `hover_markdown`, `read_script_file`, `workspace_roots`. |
| `cst_cache.rs` | Per-document parse-tree cache with invalidation hooks. |
| `indexing.rs` | Workspace + base-script discovery and indexing helpers (game-directory scan, `modSharedImports` auto-load, settings refresh). |
| `config.rs` | `fetch_config`, `apply_diagnostics_toggle`, `ConfigChange` plumbing for `workspace/configuration`. |
| `diagnostics_publish.rs` | `publish_open_diagnostics`, `publish_syntactic_only` — diagnostic emission for open documents. |
| `watcher.rs` | `register_file_watchers`, `apply_watched_file_events`, `classify_watched_event` — file-watcher integration. |
| `logging.rs` | `LspLogSender` tracing layer, level enum/parsing, `DEFAULT_LOG_LEVEL`. |
| `tests.rs` + `tests/{completion,diagnostics,file_io,hover,indexing,refactoring}.rs` | `#[cfg(test)]` LSP-specific tests split per feature. |

The binary is intentionally thin. All parse/resolve logic lives in the library (`witcherscript_language::*`). The binary only:
- Owns shared state in the `Backend` struct
- Dispatches LSP requests to library functions
- Publishes results back to the client

## Backend struct

```rust
struct Backend {
    client: ClientSocket,                                       // async-lsp client handle
    config: Arc<ArcSwap<Config>>,                               // user-facing settings (log level, formatter, diagnostics, …)
    documents: Arc<Mutex<HashMap<Url, ParsedDocument>>>,        // editor-open files
    workspace_index: Arc<Mutex<WorkspaceIndex>>,                // user project symbol index
    workspace_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,  // parsed user project files
    workspace_roots: Arc<Mutex<Vec<PathBuf>>>,                  // workspace root directories
    base_scripts_path: Arc<Mutex<Option<PathBuf>>>,             // path to game directory
    base_scripts_index: Arc<Mutex<WorkspaceIndex>>,             // base game scripts symbol index
    base_scripts_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>, // parsed base scripts
    script_env: Arc<Mutex<ScriptEnvironment>>,                  // INI-loaded globals
}
```

All shared mutable state is wrapped for async-safe access: `Arc<Mutex<>>` for collections, `Arc<ArcSwap<Config>>` for the read-mostly settings bag.

## Implemented LSP capabilities

| Capability | Details |
|---|---|
| Text document sync | `FULL` — complete document text on every change |
| Completion | Trigger chars: `.`, `:`, `@` |
| Signature help | Parameter hints at call sites; trigger chars `(` and `,`, retrigger `,` |
| Go-to-definition | Resolves symbol at cursor |
| Find references | Workspace-wide, with include_declaration option |
| Rename | Prepare + execute; blocked for base script symbols |
| Hover | Markdown with location link |
| Document symbol | Nested outline (excludes Variable/Parameter kinds) |
| Semantic tokens full | Whole-document token array |
| Workspace folders | Multi-root support |

## Document lifecycle

```
Editor opens/changes file
    ↓
did_open() / did_change()
    ↓
update_open_document(uri, text)
    parse_document(text) → ParsedDocument
    workspace_index.update_document(uri, &doc)
    documents.insert(uri, doc)
    client.publish_diagnostics(lsp_diagnostics(&doc))

Editor closes file
    ↓
did_close()
    client.publish_diagnostics(uri, vec![])    // clear diagnostics
    (document stays in workspace_index)
```

## Workspace indexing

Triggered during `initialized()` — runs after the client acknowledges `initialize()`.

```
initialized()
    ├─ fetch_config()
    │      workspace/configuration request → witcherscript.gameDirectory + logLevel + files.exclude
    │
    ├─ index_workspace()
    │      collect_witcherscript_files(&workspace_roots, &files_exclude)
    │        — walk honours .gitignore and the client's files.exclude globs
    │      for each .ws file: parse → workspace_index.update_document
    │
    └─ index_base_scripts()
           find scripts at gameDirectory/content/content0/scripts/
           parse redscripts.ini → ScriptEnvironment
           rayon parallel parse (each thread: new Parser + parse_document_with_parser)
           base_scripts_index.update_document for each file
```

Base scripts use `rayon` for parallel parsing. Each rayon thread gets its own `tree_sitter::Parser` because Parser is not `Send`.

## File encoding handling

`read_script_file(path)` detects BOM and decodes accordingly:
- UTF-8 (default, no BOM)
- UTF-16 LE (BOM: `FF FE`)
- UTF-16 BE (BOM: `FE FF`)

This handles the mixed encodings found in shipped Witcher 3 script files.

## Configuration

| Setting | Where | Effect |
|---|---|---|
| `witcherscript.gameDirectory` | `initializationOptions` or `workspace/configuration` | Path to game directory for base script indexing |
| `witcherscript.logLevel` | `initializationOptions` or `workspace/configuration` | Sets `config.log_level` (read via `Arc<ArcSwap<Config>>`); accepts `error`/`warn`/`info`/`debug`/`trace` |

Configuration is re-fetched on `did_change_configuration()` and base scripts are re-indexed if the game directory changed.

## Logging

`LspLogSender` is a custom `tracing::Layer` that:
- Checks `min_level` (atomic) before emitting — no allocation for filtered events
- Maps tracing levels to LSP `MessageType` (ERROR→ERROR, WARN→WARNING, INFO→INFO, DEBUG/TRACE→LOG)
- Sends messages through an `mpsc::UnboundedSender` to a background task that calls `client.log_message()`
- Default level: WARN

## Completion dispatch

The `completion()` handler tries four strategies in order, taking the first that returns results:

1. **Member completions** — if the character before the cursor is `.` or `:`, call `completion_members()` to get tiered members of the receiver type.
2. **Type completions** — if cursor is in a type annotation context, call `type_completions()`.
3. **Statement completions** — if cursor is inside a function body, call `statement_completions()`. Offers `this`, `super`, `var` keyword, locals, members, globals.
4. **Class body completions** — if cursor is in a class/struct/state body, call `class_body_completions()` and offer structural keywords.

## Completion item format

Methods, functions, and events get snippet insertions with parameter placeholders:
```
FunctionName(${1:param1}, ${2:param2})$0
```

The parameter names come from `db.parameters_of(uri, callable_id)`, which excludes optional parameters.

Documentation includes a markdown code block (from `hover_text`) and a file:line location link.

## Rename

1. `prepare_rename()` — resolve symbol at cursor; return error if symbol is declared in base scripts ("Cannot rename a symbol declared in a base script (read-only)").
2. `rename()` — call `find_references()` across all documents; produce `WorkspaceEdit` with one `TextEdit` per occurrence.

## Signature help

`signature_help()` locates the innermost call site enclosing the cursor (both closed `func_call_expr` nodes and unclosed calls recovered as `ERROR`), resolves the callee, and returns a `SignatureHelpInfo` (label + per-parameter UTF-16 label offsets + active parameter index). `convert::signature_help_response()` maps it to `lsp_types::SignatureHelp` with a single signature.

## SymbolDb construction

Built fresh for each request:

```rust
let db = SymbolDb::new(&workspace_index, &base_scripts_index)
    .with_script_env(&script_env);
```

Open `documents` (editor-open) take precedence over `workspace_documents` (background-indexed) for the file being edited: the LSP passes the editor's `ParsedDocument` directly to resolve functions, not the indexed copy.

## Utility functions

| Function | Description |
|---|---|
| `lsp_range(range: SourceRange)` | `SourceRange` → LSP `Range` |
| `source_position(pos: Position)` | LSP `Position` → `SourcePosition` |
| `hover_markdown(def, doc)` | Formats hover content: code block + location link |
| `hover_location_markdown(uri, range)` | `file:///path#L{line}` markdown link |
| `completion_item(def, doc, db)` | Builds `CompletionItem` with snippet + docs |
| `document_symbols(syms, parent_id)` | Recursively builds LSP `DocumentSymbol` tree; skips Variable/Parameter |
| `lsp_symbol_kind(kind)` | `SymbolKind` → LSP `SymbolKind` enum |
| `read_script_file(path)` | UTF-8/16 aware file reader |
| `workspace_roots(params)` | Extracts root paths from `InitializeParams` |

## Adding a new LSP handler

1. Add the capability to `ServerCapabilities` in `initialize()`.
2. Implement the handler method on `Backend` (`impl LanguageServer for Backend`).
3. If the handler needs new resolve logic, add it as a `pub fn` in the appropriate `src/resolve/` submodule — not in the binary.
4. Add a `#[cfg(test)]` test in the relevant `src/bin/witcherscript-lsp/tests/<feature>.rs` file.
