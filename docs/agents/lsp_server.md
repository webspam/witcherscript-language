# LSP server

**Module:** `src/bin/witcherscript-lsp/` - handlers split by LSP concern.

| File | Purpose |
|---|---|
| `main.rs` | Tokio entry point - wires the tracing layer + `LspLogSender`, constructs `Backend`, and serves over stdio. |
| `backend.rs` | `Backend` struct, `DbHandles` helper, `DocOp` enum, `diagnostics_document_set`; the `impl LanguageServer for Backend` block is a thin trait shim - every method body just delegates to a `_handler` sibling in `lifecycle.rs` / `text_sync.rs` / `completion.rs` / `queries.rs` / `references_rename.rs`. |
| `lifecycle.rs` | `_initialize` (advertises `ServerCapabilities`), `_initialized` (kicks off the startup index), `_did_change_configuration`. |
| `text_sync.rs` | `_did_open`, `_did_change`, `_did_close`, `_did_change_watched_files`, `_did_change_workspace_folders` - owns the editor-open document lifecycle and the loose-file index. |
| `completion.rs` | `_completion` - runs member / type / statement / class-body / annotation / wrap-method dispatch in order. |
| `queries.rs` | Read-only request handlers: `_hover`, `_goto_definition`, `_references` entry, `_document_symbol`, `_signature_help`, `_semantic_tokens_full`, `_formatting`, `_code_action`, `_document_diagnostic`, `_handle_builtin_source`. |
| `references_rename.rs` | `_references`, `_prepare_rename`, `_rename` + the `merge_documents` helper that builds the cross-doc search set (open shadows workspace shadows base; loose target sees only loose+base). |
| `convert/` | LSP↔internal conversion - `positions.rs` (ranges), `diagnostics.rs`, `completions.rs`, `symbols.rs` (document outline, hover), `file_ops.rs` (`workspace_roots`, watched-file bridging). |
| `cst_cache.rs` | Per-document parse-tree cache with invalidation hooks. |
| `indexing/` | Workspace + base-script indexing - `helpers.rs` (segments, legacy pairing), `open_documents.rs`, `legacy.rs` (manifest dirs, overrides), `scan.rs` (bulk workspace/base index). |
| `config.rs` | `fetch_config`, `DiagnosticsScope`, `ConfigChange` plumbing for `workspace/configuration`. |
| `diagnostics_publish.rs` | `compute_diagnostics_for_uri` (single-doc pull compute) and `compute_workspace_diagnostic_report` (workspace pull); both bail at version checkpoints when a newer edit superseded them. Also `publish_legacy_script_status` (`witcherscript/legacyScriptStatus`) and `publish_file_scope_status` (`witcherscript/fileScopeStatus`). |
| `file_scope.rs` | `FileScope` enum + `classify_file_scope` - routes a URI to workspace / loose / base / legacy. |
| `file_scope_status.rs` | `FileScopeStatusParams` - the `witcherscript/fileScopeStatus` notification payload. |
| `watcher.rs` | `register_file_watchers`, `apply_watched_file_events`, `classify_watched_event` - file-watcher integration. |
| `legacy_status.rs` | `LegacyScriptStatusParams` + `LegacyScriptStatusNotification` - the `witcherscript/legacyScriptStatus` protocol type. |
| `logging.rs` | `LspLogSender` tracing layer, level enum/parsing, `DEFAULT_LOG_LEVEL`. |
| `tests.rs` | Module root that declares the `tests/` submodules. |
| `tests/{code_action,completion,diagnostics,file_io,file_scope,hover,indexing,refactoring}.rs` | `#[cfg(test)]` LSP-specific tests split per feature. |
| `tests/indexing/{concurrent_doc_ops,diagnostics_scope,file_operation_events,legacy_helpers,legacy_predicates,legacy_reindex,legacy_routing,loose_files,watched_files,workspace_folder_changes}.rs` | Indexing tests further split by topic; `legacy_helpers.rs` carries the shared `LocalTempDir` / `make_backend` / `write_script` scaffolding the other `legacy_*` files reuse. |
| `tests/jsonrpc_client.rs` | Framed JSON-RPC client used by the E2E test harness. |
| `tests/e2e/` | Wire-level E2E tests that drive the real `Backend` over a `tokio::io::duplex` pair with framed JSON-RPC. See [testing.md](testing.md#testse2e-wire-level-lsp-tests). |

The binary is intentionally thin. All parse/resolve logic lives in the library (`witcherscript_language::*`). The binary only:
- Owns shared state in the `Backend` struct
- Dispatches LSP requests to library functions
- Publishes results back to the client

## Backend struct

```rust
struct Backend {
    client: ClientSocket,                                                    // async-lsp client handle
    config: Arc<ArcSwap<Config>>,                                            // user-facing settings (log level, formatter, diagnostics, …)
    documents: Arc<Mutex<HashMap<Url, ParsedDocument>>>,                     // editor-open files, keyed by the client's raw Url
    workspace_index: Arc<Mutex<WorkspaceIndex>>,                             // user project symbol index
    workspace_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,        // parsed user project files, keyed by canonical URI
    workspace_roots: Arc<Mutex<Vec<PathBuf>>>,                               // workspace root directories
    files_exclude: Arc<Mutex<Vec<String>>>,                                  // glob patterns excluded from indexing
    base_scripts_path: Arc<Mutex<Option<PathBuf>>>,                          // path to game directory
    additional_script_dirs: Arc<Mutex<Vec<PathBuf>>>,                        // extra script directories to index
    base_scripts_index: Arc<Mutex<WorkspaceIndex>>,                          // base game scripts symbol index
    base_scripts_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,     // parsed base scripts, keyed by canonical URI
    legacy_replacements: Arc<Mutex<HashMap<String, String>>>,                // canonical legacy URI → replaced base script's game-relative path
    suppressed_base_uris: Arc<Mutex<HashSet<String>>>,                     // vanilla URIs shadowed by a legacy override (still in base index)
    sent_legacy_status: Arc<Mutex<HashMap<Url, LegacyScriptStatusParams>>>,  // last legacy-script status pushed per open document
    builtins_index: Arc<WorkspaceIndex>,                                     // built-in type symbol index (read-only, no Mutex)
    script_env: Arc<Mutex<ScriptEnvironment>>,                               // INI-loaded globals
    cst_diag_cache: Arc<Mutex<HashMap<Url, cst_cache::CstCacheEntry>>>,      // cached CST diagnostics per document
    initial_index_done: Arc<AtomicBool>,                                     // set true once the startup index completes
    diagnostic_version: Arc<AtomicU64>,                                      // bumped on every notify_diagnostics_changed(); in-flight pull computes bail when this advances
    client_supports_pull_diagnostics: Arc<AtomicBool>,                       // captured from initialize; gates whether workspace/diagnostic/refresh is sent
}
```

All shared mutable state is wrapped for async-safe access: `Arc<Mutex<>>` for collections, `Arc<ArcSwap<Config>>` for the read-mostly settings bag, and `Arc<AtomicBool>` for the startup-complete flag. `builtins_index` needs no `Mutex` because it is populated once at startup and never mutated.

### DbHandles helper

```rust
struct DbHandles<'a> {
    workspace: MutexGuard<'a, WorkspaceIndex>,
    base: MutexGuard<'a, WorkspaceIndex>,
    script_env: MutexGuard<'a, ScriptEnvironment>,
    builtins: &'a WorkspaceIndex,
}
```

`Backend::db_handles()` acquires the three mutex guards simultaneously and returns a `DbHandles`. Callers then call `.db()` on it to get a fully assembled `SymbolDb` without having to lock each field individually. This prevents lock-order bugs and keeps handler code concise.

## URI handling

Every document request arrives with a `Url`. The catch is that the same file can reach the server under two different spellings. On Windows, VS Code sends `file:///c%3A/mod/script.ws` (percent-encoded drive colon, lowercase letter), while `Url::from_file_path` - used when the indexer walks the disk - produces `file:///C:/mod/script.ws`. Those two strings are unequal, so a map keyed by one spelling misses a lookup made with the other, which surfaces as duplicate-symbol diagnostics or an open file that resolution can't find.

`files::canonical_uri(uri: &Url) -> Option<String>` settles this. It round-trips the `Url` through `to_file_path()` / `from_file_path()`, letting the OS pick one spelling. It returns `None` for any URI that is not a `file://` path - builtins use a synthetic scheme - so callers handle that with `?` or `filter_map` rather than unwrapping.

Because of this, `Backend`'s document maps fall into two groups:

| Map | Key type | Spelling |
|---|---|---|
| `documents` | `Url` | raw - exactly as the client sent it |
| `workspace_documents`, `base_scripts_documents`, `legacy_replacements` | `String` | canonical |
| `WorkspaceIndex::documents` (inside `workspace_index` / `base_scripts_index`) | `String` | canonical |

`documents` is the deliberate exception. The client sends the same raw `Url` back for every follow-up request (hover, completion, …), so `documents.get(&uri)` has to match that spelling. Every other map is keyed canonically so that the background-indexed copy of a file and the editor-open copy resolve to the same entry.

When you add a map keyed by a document URI, or compare two URIs to decide whether they point at the same file, run them through `canonical_uri` first - comparing `Url`s or raw `to_string()` output directly will be wrong on Windows. `index_open_document` (`indexing/helpers.rs`) and `base_script_conflict::is_same_file` are the existing worked examples to follow.

## Implemented LSP capabilities

| Capability | Details |
|---|---|
| Text document sync | `INCREMENTAL` - only changed ranges are sent on each edit |
| Completion | Trigger chars: `.`, `:`, `@` |
| Signature help | Parameter hints at call sites; trigger chars `(` and `,`, retrigger `,` |
| Go-to-definition | Resolves symbol at cursor |
| Find references | Workspace-wide, with include_declaration option |
| Rename | Prepare + execute; blocked for base script symbols |
| Hover | Markdown with location link |
| Document symbol | Nested outline (excludes Variable/Parameter kinds) |
| Semantic tokens full | Whole-document token array |
| Workspace folders | Multi-root support |
| Pull diagnostics | `textDocument/diagnostic` and `workspace/diagnostic` (LSP 3.17) with `result_id` for "unchanged" replies; `workspace/diagnostic/refresh` is pushed when workspace state changes |

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
    notify_diagnostics_changed()               // bumps diagnostic_version + pings pull clients via workspace/diagnostic/refresh

Editor closes file
    ↓
did_close()
    documents.remove(uri)                      // drop the editor buffer
    reindex_closed_file(uri)                   // revert the index to on-disk content
    notify_diagnostics_changed()               // workspace scope keeps it; openFiles scope drops it from the next workspace pull
```

## Diagnostics scope

`witcherscript.diagnostics.scope` (`DiagnosticsScope`) decides which files `compute_workspace_diagnostic_report` returns:

- `Workspace` (default) - every workspace file appears in the workspace pull; the Problems list is complete on project open and unaffected by opening/closing tabs.
- `OpenFiles` - only editor-open files appear in the workspace pull; symbols are still indexed project-wide.
- `None` - workspace pull returns an empty `items` array; `textDocument/diagnostic` still answers per-URI requests.

`diagnostics_document_set` builds the diagnosed document set: `workspace_documents` (workspace scope only) plus open buffers, with open buffers winning. Reports are keyed by canonical URI so a file's identity stays stable across open/close.

## Diagnostic delivery

Diagnostics are pull-only (LSP 3.17). The server advertises `diagnosticProvider` with `workspaceDiagnostics: true` and never sends `textDocument/publishDiagnostics`.

- **`textDocument/diagnostic`** - `_document_diagnostic` calls `compute_diagnostics_for_uri(uri, document)` which runs the cross-file passes, runs CST for that URI, and returns `(items, result_id)`. The `result_id` is a stable hash of `(parse_version, workspace.surface_hash, base.surface_hash, env.version, legacy_db_generation)`. If the client sends back a matching `previous_result_id`, the server replies with `Unchanged`. A stale-version compute returns `ContentModified` so the client retries.
- **`workspace/diagnostic`** - `_workspace_diagnostic` calls `compute_workspace_diagnostic_report(previous, version)` which iterates `diagnostics_document_set`, doing the cross-file pass once and reusing the per-document CST cache. Each per-URI report is `Full` (with diagnostics + a `result_id`) or `Unchanged` (when `previous` matches). A stale-version compute returns `ServerCancelled` with `DiagnosticServerCancellationData { retriggerRequest: true }`, the spec-recommended form for "can't compute now."
- **Refresh** - `request_workspace_diagnostic_refresh()` sends `workspace/diagnostic/refresh` so pull clients retrigger. `notify_diagnostics_changed()` is the single state-change signal: it bumps `diagnostic_version` (in-flight stale computes self-cancel against this) and fires refresh.

## Legacy script status notification

`witcherscript/legacyScriptStatus` is a custom server→client notification (defined in `legacy_status.rs`, payload `LegacyScriptStatusParams`). It tells the editor whether an open `.ws` file actually replaces a base game script of the same game-relative path, so the VS Code "legacy script" status bar shows only for real overrides - not for brand-new scripts that merely sit in a legacy folder.

`index_base_scripts` builds `legacy_replacements` (canonical legacy URI → replaced game-relative path) and `suppressed_base_uris` (vanilla URIs shadowed by a legacy override). Overridden vanilla files **stay** in `base_scripts_index` / `base_scripts_documents` for reference search; `SymbolDb` skips them for resolution and completion. Legacy paths listed in `witcherscript.legacyScriptDirectories` (and auto `modSharedImports`) are indexed into `workspace_index` like project scripts; file-watcher changes use the same incremental path as other `.ws` files, then `refresh_legacy_override_maps()`. `publish_legacy_script_status` pushes one notification per open document, deduped against `sent_legacy_status`.

## Workspace indexing

Triggered during `initialized()` - runs after the client acknowledges `initialize()`.

```
initialized()
    ├─ fetch_config()
    │      workspace/configuration request → witcherscript.gameDirectory + logLevel + files.exclude
    │
    ├─ index_workspace()
    │      collect_witcherscript_files(&workspace_roots, &files_exclude)
    │        - walk honours .gitignore and the client's files.exclude globs
    │      for each .ws file: parse → workspace_index.update_document
    │
    └─ index_base_scripts()
           find scripts at gameDirectory/content/content0/scripts/ (+ legacyScriptDirectories)
           parse redscripts.ini → ScriptEnvironment
           rayon parallel parse (each thread: new Parser + parse_document_with_parser)
           base_scripts_index.update_document for each file (overridden vanilla URIs are not removed)
           sync legacy overrides into workspace_index; set suppressed_base_uris
```

External `.ws` changes (including under legacy dirs) go through `apply_watched_file_events` - incremental workspace upsert/remove, then `refresh_legacy_override_maps()` when a legacy path changed. There is no background full re-index coalescer.

Base scripts use `rayon` for parallel parsing. Each rayon thread gets its own `tree_sitter::Parser` because Parser is not `Send`.

## File encoding handling

`read_text_file(path)` (in `src/files.rs`) is the canonical text-file reader; route every disk read of a text file through it. It uses `encoding_rs`. This handles the mixed encodings found in shipped Witcher 3 script files and in user-authored mod files.

## Configuration

| Setting | Where | Effect |
|---|---|---|
| `witcherscript.gameDirectory` | `initializationOptions` or `workspace/configuration` | Path to game directory for base script indexing |
| `witcherscript.logLevel` | `initializationOptions` or `workspace/configuration` | Sets `config.log_level` (read via `Arc<ArcSwap<Config>>`); accepts `error`/`warn`/`info`/`debug`/`trace` |

Configuration is re-fetched on `did_change_configuration()` and base scripts are re-indexed if the game directory changed.

## Logging

`LspLogSender` is a custom `tracing::Layer` that:
- Checks `min_level` (atomic) before emitting - no allocation for filtered events
- Maps tracing levels to LSP `MessageType` (ERROR→ERROR, WARN→WARNING, INFO→INFO, DEBUG/TRACE→LOG)
- Sends messages through an `mpsc::UnboundedSender` to a background task that calls `client.log_message()`
- Default level: WARN

## Completion dispatch

The `completion()` handler tries four strategies in order, taking the first that returns results:

1. **Member completions** - if the character before the cursor is `.` or `:`, call `completion_members()` to get tiered members of the receiver type.
2. **Type completions** - if cursor is in a type annotation context, call `type_completions()`.
3. **Statement completions** - if cursor is inside a function body, call `statement_completions()`. Offers `this`, `super`, `var` keyword, locals, members, globals.
4. **Class body completions** - if cursor is in a class/struct/state body, call `class_body_keyword_completions()` and offer structural keywords.

## Completion item format

Methods, functions, and events get snippet insertions with parameter placeholders:
```
FunctionName(${1:param1}, ${2:param2})$0
```

The parameter names come from `db.parameters_of(uri, callable_id)`, which excludes optional parameters.

Documentation includes a markdown code block (from `hover_text`) and a file:line location link.

## Rename

1. `prepare_rename()` - resolve symbol at cursor; return error if symbol is declared in base scripts ("Cannot rename a symbol declared in a base script (read-only)").
2. `rename()` - call `find_references()` across all documents; produce `WorkspaceEdit` with one `TextEdit` per occurrence.

## Signature help

`signature_help()` locates the innermost call site enclosing the cursor (both closed `func_call_expr` nodes and unclosed calls recovered as `ERROR`), resolves the callee, and returns a `SignatureHelpInfo` (label + per-parameter UTF-16 label offsets + active parameter index). `convert::signature_help_response()` maps it to `lsp_types::SignatureHelp` with a single signature.

## SymbolDb construction

Built fresh for each request:

```rust
let db = SymbolDb::new(&workspace_index, &base_scripts_index)
    .with_builtins(&builtins_index)
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
| `read_text_file(path)` | BOM-sniffing text reader (UTF-8 / UTF-16 LE / UTF-16 BE) |
| `workspace_roots(params)` | Extracts root paths from `InitializeParams` |

## Adding a new LSP handler

1. Add the capability to `ServerCapabilities` in `initialize()`.
2. Implement the handler method on `Backend` (`impl LanguageServer for Backend`).
3. If the handler needs new resolve logic, add it as a `pub fn` in the appropriate `src/resolve/` submodule - not in the binary.
4. Add a `#[cfg(test)]` test in the relevant `src/bin/witcherscript-lsp/tests/<feature>.rs` file.
