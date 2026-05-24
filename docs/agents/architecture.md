# Architecture overview

## Source file tree

```
src/
├── lib.rs                          module declarations
├── main.rs                         CLI binary (witcherscript-check)
├── bin/
│   ├── dump_tree.rs               developer helper: prints a tree-sitter parse tree
│   └── witcherscript-lsp/         LSP server binary (handlers split by LSP concern)
│       ├── main.rs                tokio entry point, tracing setup, Backend wiring
│       ├── backend.rs             Backend struct + thin LanguageServer trait impl (delegates to siblings)
│       ├── lifecycle.rs           _initialize / _initialized / configuration change handlers
│       ├── text_sync.rs           _did_open / _did_change / _did_close + workspace folder + watched files
│       ├── completion.rs          _completion handler + dispatch through resolve/completion/
│       ├── queries.rs             hover, definition, document symbols, signature help, semantic tokens, formatting, code action
│       ├── references_rename.rs   _references, _prepare_rename, _rename + cross-doc merge_documents helper
│       ├── config.rs              Config struct, settings parsing
│       ├── convert.rs             LSP↔internal conversion (ranges, completion items, hover, file reader)
│       ├── cst_cache.rs           per-file CST diagnostic cache
│       ├── diagnostics_publish.rs publish_diagnostics helper
│       ├── file_scope.rs          FileScope classifier (workspace / loose / base / legacy)
│       ├── file_scope_status.rs   witcherscript/fileScopeStatus notification type
│       ├── indexing.rs            workspace + base-script indexing (game dir, modSharedImports, settings refresh)
│       ├── legacy_status.rs       witcherscript/legacyScriptStatus notification type
│       ├── logging.rs             LspLogSender tracing layer + level parsing
│       ├── watcher.rs             file-system watcher integration
│       ├── tests.rs               #[cfg(test)] LSP-specific unit tests
│       └── tests/                 E2E and integration tests (per-feature files + e2e/ subdir)
├── cst/                            shared tree-sitter CST traversal primitives
│   ├── ancestors.rs               ancestor-of-kind lookup
│   ├── grammar.rs                 grammar node-kind helpers
│   ├── nav.rs                     child / sibling navigation
│   └── offsets.rs                 byte-offset → node queries
├── document.rs                     parse orchestration, ParsedDocument
├── diagnostics/                    ParseDiagnostic, collect_diagnostics, per-pass modules
│   ├── mod.rs                      public API: collect_diagnostics, ParseDiagnostic
│   ├── cst_walker.rs               CstRule trait, run_rules_on_document
│   ├── base_script_conflict.rs     workspace-vs-base script conflict check
│   ├── duplicate_local.rs          duplicate local variable check
│   ├── duplicate_symbols.rs        duplicate top-level symbol check
│   ├── shadowing.rs                variable shadowing check
│   ├── unknown_method.rs           unknown method call check
│   ├── unknown_symbol.rs           unknown symbol reference check
│   └── wrapped_method.rs           wrapped-method signature check
├── files.rs                        recursive .ws file discovery, canonical_uri
├── formatter.rs                    document formatter entry point (textDocument/formatting)
├── formatter/
│   ├── core.rs                     traversal + line-fitting core
│   ├── declarations.rs             class/struct/enum/state declaration formatting
│   ├── signatures.rs               function / event signature formatting
│   └── statements.rs               statement + expression formatting
├── line_index.rs                   byte ↔ UTF-16 position mapping (LSP-compatible)
├── script_env.rs                   INI script globals parser
├── symbols.rs                      SymbolKind, Symbol, DocumentSymbols, extract_symbols
├── resolve/
│   ├── mod.rs                      public API: WorkspaceIndex, SymbolDb, resolve_definition
│   ├── ast.rs                      re-exports cst/ navigation helpers; BUILTIN_TYPES
│   ├── workspace_index.rs          WorkspaceIndex internals (per-doc symbol store, top-level / member / superclass maps)
│   ├── symbol_db.rs                SymbolDb construction; cross-index member chain; generic substitution (array<T>)
│   ├── definition.rs               goto-definition logic
│   ├── inference.rs                type inference
│   ├── references.rs               find-references logic
│   ├── signature.rs                signature-help logic
│   ├── completion/                 completion submodule
│   │   ├── mod.rs
│   │   ├── body_function.rs        statement / expression / default+hint member completions inside function bodies
│   │   ├── body_class.rs           class-body keyword completions (specifier state machine)
│   │   ├── body_script.rs          script-level body completions
│   │   ├── headers.rs              completions in declarations/headers (annotations, extends, state-owner)
│   │   ├── members.rs              member-access completions
│   │   └── types.rs                type-name completions
│   └── tests/                      resolution + completion tests
└── semantic_tokens/
    ├── mod.rs                      TOKEN_TYPES, collect_semantic_tokens, classify
    └── tests.rs                    semantic token tests

tests/
├── parser_fixtures.rs              fixture-driven parse tests (valid/ and invalid/)
├── language_features.rs            integration tests for symbol extraction + resolution
└── fixtures/
    ├── valid/                      .ws files that must parse with zero diagnostics
    └── invalid/                    .ws files that must produce at least one diagnostic
```

## Module dependency graph

```
cst        leaf — shared tree-sitter CST traversal primitives
line_index leaf — byte ↔ UTF-16 position mapping

document ──► diagnostics ──► cst
         ──► line_index
         ──► symbols ──► line_index, cst

resolve  ──► document
         ──► line_index
         ──► script_env ──► symbols, line_index
         ──► symbols
         ──► cst

formatter ──► cst

semantic_tokens ──► line_index
                ──► resolve
                ──► symbols

lib      ──► declares all of the above (no curated re-exports — bare `pub mod`s)

bin/witcherscript-lsp/ ──► witcherscript_language::* (all library modules)
main                  ──► witcherscript_language::* (document, files, diagnostics)
```

## Data flow pipeline

```
.ws file on disk
    │
    ▼
parse_document(source)          [document.rs]
    │  tree-sitter parse → Tree
    │  LineIndex::new(source)
    │  collect_diagnostics(root, source)
    │  extract_symbols(root, source, line_index)
    ▼
ParsedDocument { source, tree, line_index, diagnostics, symbols }
    │
    ├─► WorkspaceIndex::update_document(uri, doc)    [resolve/workspace_index.rs]
    │       inserts into top_level_by_name, member_by_type,
    │       superclass_by_name, doc_idents
    │
    └─► LSP response handlers                        [bin/witcherscript-lsp/{completion,queries,references_rename,…}.rs]
            SymbolDb::new(workspace, base).with_script_env(env).with_builtins(builtins)
            resolve_definition / completion_members / statement_completions / …
```

## Index model

The LSP server maintains three separate `WorkspaceIndex` symbol indexes, plus the open-documents map that overrides them for files being edited:

| Name | Type | Source |
|------|------|--------|
| `workspace_index` | `WorkspaceIndex` | user project .ws files |
| `base_scripts_index` | `WorkspaceIndex` | Witcher 3 game scripts (read-only) |
| `builtins_index` | `Arc<WorkspaceIndex>` | embedded builtin types (never mutated) |
| open `documents` | `HashMap<Url, ParsedDocument>` | editor-open files (not an index — overrides the indexed copy) |

`workspace_documents` and `base_scripts_documents` hold the `ParsedDocument` cache for background-indexed files so semantic tokens / references can read their trees without re-parsing.

When constructing a `SymbolDb` for a request:
- workspace shadows base, builtins are always visible: `SymbolDb::new(&workspace_index, &base_scripts_index).with_script_env(&env).with_builtins(&builtins_index)`
- open documents take precedence over `workspace_documents` for the file being edited

## Key types and who produces/consumes them

| Type | Produced by | Consumed by |
|------|------------|------------|
| `ParsedDocument` | `document::parse_document()` | LSP handlers, WorkspaceIndex, resolve, semantic_tokens |
| `DocumentSymbols` | `symbols::extract_symbols()` | resolve functions, semantic_tokens, LSP document_symbol |
| `WorkspaceIndex` | `WorkspaceIndex::update_document()` | `SymbolDb` (wraps two of them) |
| `SymbolDb<'_>` | constructed per-request in LSP handlers | resolve_definition, completion_*, find_references, semantic_tokens |
| `Definition` | resolve functions | LSP handlers (hover, goto, completion items) |
| `ParseDiagnostic` | `diagnostics::collect_diagnostics()` | LSP `publish_diagnostics` |
| `LineIndex` | `LineIndex::new(source)` | position_to_byte / byte_to_position in all modules |
| `ScriptEnvironment` | `script_env::parse_script_environment()` | `SymbolDb::with_script_env()` |

## Binary entry points

**`src/main.rs`** — CLI validator
- Accepts file/directory paths, recursively finds .ws files
- Parses each with `parse_document_with_parser()` (reusing one Parser instance)
- Optionally dumps tree via `format_tree()`
- Exit code: 0 (ok), 1 (diagnostics found), 2 (runtime error)
- Flags: `--dump-tree`, `--max-diagnostics N`

**`src/bin/witcherscript-lsp/`** — LSP server (module split across `main.rs`, `backend.rs` + the per-concern handler files `lifecycle.rs` / `text_sync.rs` / `completion.rs` / `queries.rs` / `references_rename.rs`, plus `convert.rs`, `cst_cache.rs`, `indexing.rs`, `config.rs`, `diagnostics_publish.rs`, `file_scope.rs`, `file_scope_status.rs`, `legacy_status.rs`, `watcher.rs`, `logging.rs`, and `tests.rs` + per-feature files under `tests/`)
- Async Tokio runtime; async-lsp framework over stdin/stdout
- `Backend` struct holds all shared state behind `Arc<Mutex<>>`
- All parse/resolve logic lives in the library; the binary only orchestrates
- See [lsp_server.md](lsp_server.md) for full details
