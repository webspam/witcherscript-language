# Architecture overview

## Source file tree

```
src/
├── lib.rs                          module declarations, public API surface
├── main.rs                         CLI binary (witcherscript-check)
├── bin/
│   ├── dump_tree.rs               developer helper: prints a tree-sitter parse tree
│   └── witcherscript-lsp/         LSP server binary (~3150 lines across 10 files + tests/)
│       ├── main.rs                tokio entry point, tracing setup, Backend wiring
│       ├── backend.rs             Backend struct + all LanguageServer handler impls
│       ├── config.rs              Config struct, settings parsing
│       ├── convert.rs             LSP↔internal conversion (ranges, completion items, hover, file reader)
│       ├── cst_cache.rs           per-file CST diagnostic cache
│       ├── diagnostics_publish.rs publish_diagnostics helper
│       ├── indexing.rs            workspace + base-script indexing (game dir, modSharedImports, settings refresh)
│       ├── logging.rs             LspLogSender tracing layer + level parsing
│       ├── watcher.rs             file-system watcher integration
│       ├── tests.rs               #[cfg(test)] LSP-specific unit tests
│       └── tests/                 E2E and integration tests (8 files + e2e/ subdir)
├── document.rs                     parse orchestration, ParsedDocument
├── diagnostics/                    ParseDiagnostic, collect_diagnostics, per-pass modules
│   ├── mod.rs                      public API: collect_diagnostics, ParseDiagnostic
│   ├── cst_walker.rs               tree-sitter CST traversal
│   ├── duplicate_local.rs          duplicate local variable check
│   ├── duplicate_symbols.rs        duplicate top-level symbol check
│   ├── shadowing.rs                variable shadowing check
│   ├── unknown_method.rs           unknown method call check
│   ├── unknown_symbol.rs           unknown symbol reference check
│   └── wrapped_method.rs           wrapped-method signature check
├── files.rs                        recursive .ws file discovery
├── line_index.rs                   byte ↔ UTF-16 position mapping (LSP-compatible)
├── script_env.rs                   INI script globals parser
├── symbols.rs                      SymbolKind, Symbol, DocumentSymbols, extract_symbols
├── resolve/
│   ├── mod.rs                      public API: WorkspaceIndex, SymbolDb, resolve_definition
│   ├── ast.rs                      AST helpers
│   ├── db.rs                       WorkspaceIndex internals, SymbolDb construction
│   ├── definition.rs               goto-definition logic
│   ├── inference.rs                type inference
│   ├── references.rs               find-references logic
│   ├── signature.rs                signature-help logic
│   ├── completion/                 completion submodule (5 files)
│   │   ├── mod.rs
│   │   ├── bodies.rs               completions inside function bodies
│   │   ├── headers.rs              completions in declarations/headers
│   │   ├── members.rs              member-access completions
│   │   └── types.rs                type-name completions
│   └── tests/                      ~16 files of resolution + completion tests
└── semantic_tokens/
    ├── mod.rs                      TOKEN_TYPES, collect_semantic_tokens, classify
    └── tests.rs                    ~230 lines of semantic token tests

tests/
├── parser_fixtures.rs              fixture-driven parse tests (valid/ and invalid/)
├── language_features.rs            integration tests for symbol extraction + resolution
└── fixtures/
    ├── valid/                      .ws files that must parse with zero diagnostics
    └── invalid/                    .ws files that must produce at least one diagnostic
```

## Module dependency graph

```
document ──► diagnostics
         ──► line_index
         ──► symbols ──► line_index

resolve  ──► document
         ──► line_index
         ──► script_env ──► symbols, line_index
         ──► symbols

semantic_tokens ──► line_index
                ──► resolve
                ──► symbols

lib      ──► all of the above (re-exports)

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
    ├─► WorkspaceIndex::update_document(uri, doc)    [resolve/db.rs]
    │       inserts into top_level_by_name, member_by_type,
    │       superclass_by_name, doc_idents
    │
    └─► LSP response handlers                        [bin/witcherscript-lsp/backend.rs]
            SymbolDb::new(workspace, base).with_script_env(env).with_builtins(builtins)
            resolve_definition / completion_members / statement_completions / …
```

## Three-index model

The LSP server maintains four separate, parallel indexes:

| Name | Type | Source |
|------|------|--------|
| `workspace_index` | `WorkspaceIndex` | user project .ws files |
| `base_scripts_index` | `WorkspaceIndex` | Witcher 3 game scripts (read-only) |
| `builtins_index` | `Arc<WorkspaceIndex>` | embedded builtin types (never mutated) |
| open `documents` | `HashMap<Url, ParsedDocument>` | editor-open files (not yet re-indexed) |

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

**`src/bin/witcherscript-lsp/`** — LSP server (module split across `main.rs`, `backend.rs`, `convert.rs`, `cst_cache.rs`, `indexing.rs`, `config.rs`, `diagnostics_publish.rs`, `watcher.rs`, `logging.rs`, and `tests.rs` + per-feature files under `tests/`)
- Async Tokio runtime; tower-lsp framework over stdin/stdout
- `Backend` struct holds all shared state behind `Arc<Mutex<>>`
- All parse/resolve logic lives in the library; the binary only orchestrates
- See [lsp_server.md](lsp_server.md) for full details
