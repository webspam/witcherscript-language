# LSP server

**Module:** `src/bin/witcherscript-lsp/` - the binary. All parse/resolve logic lives in the library (`witcherscript_language::*`); the binary only owns shared state (`Backend`), dispatches LSP requests to library functions, and publishes results back.

## Layout

Handlers are split by LSP concern. The `impl LanguageServer for Backend` block in `backend.rs` is a thin shim - each method delegates to a `_handler` sibling:

- `lifecycle.rs` - initialize / initialized / config change.
- `text_sync.rs` - did_open/change/close, watched files; owns the open-document lifecycle and loose-file index.
- `completion.rs` - the completion dispatcher.
- `queries.rs` - read-only requests (hover, goto, references entry, symbols, signature help, semantic tokens, inlay hints, formatting, code actions, diagnostic).
- `references_rename.rs` - references, rename, and the cross-doc search set.
- `indexing/`, `convert/`, `cst_cache.rs`, `watcher.rs`, `config.rs`, `diagnostics_publish.rs`, `file_scope*.rs`, `legacy_status.rs`, `logging.rs` - supporting modules named for their job.

Tests live under `tests/` (split per feature) and `tests/e2e/` (wire-level; see [testing.md](testing.md#testse2e-wire-level-lsp-tests)).

## State and SymbolDb

`Backend` holds all shared state behind `Arc<Mutex<…>>` (collections), `Arc<ArcSwap<Config>>` (read-mostly settings), and atomics (flags / version counters); `builtins_index` is a plain `Arc` (populated once, never mutated). Read the struct in `backend.rs` for the field list.

A `SymbolDb` is built fresh per request from the indexes. `Backend::db_handles()` locks the workspace / base / script-env guards together and returns a `DbHandles`; call `.db()` on it to assemble the `SymbolDb` (plus builtins) without locking each field by hand - this fixes lock order. For the file being edited, the editor-open `ParsedDocument` (from `documents`) takes precedence over the background-indexed copy.

## URI handling (Windows trap)

The same file reaches the server under two spellings: VS Code sends `file:///c%3A/…` (percent-encoded, lowercase drive), while `Url::from_file_path` (used by the indexer) produces `file:///C:/…`. These are unequal, so a map keyed by one misses a lookup by the other - surfacing as duplicate-symbol diagnostics or "file not found" in resolution.

`files::canonical_uri(uri) -> String` round-trips through `to_file_path()` / `from_file_path()` so the OS picks one spelling (non-`file://` URIs are returned unchanged). **Every map keyed by a document URI, and every URI equality check, must go through `canonical_uri` first.** The sole exception is `documents` (editor-open buffers), keyed by the raw `Url` because the client sends that same spelling back on every follow-up request. Worked examples: `index_open_document` and `base_script_conflict::is_same_file`.

## Diagnostics

Pull-only (LSP 3.17): the server advertises `diagnosticProvider` with `workspaceDiagnostics: true` and never sends `publishDiagnostics`.

- `textDocument/diagnostic` and `workspace/diagnostic` reply with a `result_id` (a hash of the relevant versions / surface-hashes) so unchanged docs return `Unchanged`; a stale-version compute returns `ContentModified` / `ServerCancelled` so the client retries.
- `witcherscript.diagnostics.scope` (`DiagnosticsScope`: `Workspace` default / `OpenFiles` / `None`) decides which files the workspace pull returns. Symbols are indexed project-wide regardless.
- **Refresh signal:** `publish_compilation` bumps `state_version` and signals `views_dirty` whenever a view-relevant field changes. `run_view_refresher` wakes on it and pushes the semantic-tokens / code-lens / diagnostics refreshes. Coalescing is structural (`Notify` holds one permit, so a burst folds into one wake). In-flight stale computes self-cancel against `state_version`.

## Indexing

Kicked off in `initialized()` (after the client acks `initialize`): `index_workspace()` walks the roots (honouring `.gitignore` + `files.exclude`) and parses each `.ws` into `workspace_index`; `index_base_scripts()` finds scripts under `gameDirectory/.../scripts/` (+ `legacyScriptDirectories`), parses `redscripts.ini` into `ScriptEnvironment`, and parses in parallel via `rayon` (each thread owns its own `tree_sitter::Parser`, which is not `Send`).

External `.ws` changes go through `apply_watched_file_events` - incremental upsert/remove, then `refresh_legacy_override_maps()` if a legacy path changed. There is no background full re-index.

Route every disk read of a text file through `files::read_text_file` - it sniffs the mixed encodings found in shipped Witcher 3 scripts and user mods.

**Legacy overrides:** a `.ws` file that replaces a base script of the same game-relative path. Overridden vanilla files stay in the base index/documents for reference search but are skipped by `SymbolDb` for resolution/completion. `witcherscript/legacyScriptStatus` notifies the editor which open files are real overrides.

## Capabilities

Text sync (incremental), completion (`.`/`:`/`@`), signature help, goto-definition, find references, document highlight, rename (blocked for base-script symbols), hover, document/workspace symbols, semantic tokens, inlay hints, multi-root workspace folders, pull diagnostics. See `initialize()` for the advertised `ServerCapabilities`.

## Adding a new LSP handler

1. Add the capability to `ServerCapabilities` in `initialize()`.
2. Implement the handler on `Backend`, delegating to a `_handler` sibling in the concern-appropriate file.
3. New resolve logic goes in `src/resolve/`, not the binary.
4. Add a `#[cfg(test)]` test in `tests/<feature>.rs`.
