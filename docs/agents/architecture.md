# Architecture overview

## Module map

Top-level modules and their responsibilities. Per-file detail lives in the area docs (indexed in [AGENTS.md](../../AGENTS.md)); this section changes only when a subsystem is added or repurposed.

```
src/
├── lib.rs            module declarations (bare `pub mod`s)
├── main.rs           CLI validator entry point (witcherscript-check)
├── bin/witcherscript-lsp/  LSP server binary  (see lsp_server.md)
├── cst/              shared tree-sitter CST traversal primitives
├── document.rs       parse orchestration; ParsedDocument
├── diagnostics/      ParseDiagnostic + cross-file CST rule passes  (see diagnostics.md)
├── symbols/          Symbol, DocumentSymbols, extract_symbols  (see symbols.md)
├── resolve/          resolution, inference, completion, refactors; SymbolDb / WorkspaceIndex  (see resolution.md, mod_resolve.md)
├── semantic_tokens/  semantic-token classification  (see semantic_tokens.md)
├── formatter/        document formatter (entry point formatter.rs)
├── builtins.rs       embedded engine types parsed into a WorkspaceIndex  (see builtins.md)
├── types/            structured Type enum + type-annotation parsing
├── script_env.rs     INI script-globals parser (ScriptEnvironment)
├── line_index.rs     byte ↔ UTF-16 position mapping (LSP-compatible)
├── files.rs          .ws file discovery + canonical_uri
├── strings.rs        string utilities
└── test_support/     TestDb + fixture parser, test-only  (see writing-tests.md)

tests/                crate-root integration tests + .ws fixtures (valid/, invalid/, formatter/)
```

## Module dependency graph

```
Leaves (no intra-crate dependencies):
  cst         shared tree-sitter CST traversal primitives
  line_index  byte <-> UTF-16 position mapping
  strings     string utilities
  types       structured Type enum + type-annotation parsing
  files       .ws discovery + canonical_uri

symbols     ──► cst, line_index
script_env  ──► files, line_index, symbols, types
formatter   ──► cst

document    ──► cst, line_index, symbols, diagnostics
diagnostics ──► cst, line_index, files, script_env, symbols, types, document, resolve
resolve     ──► cst, line_index, strings, symbols, types, script_env, formatter, document

builtins        ──► document, resolve, symbols, types
semantic_tokens ──► cst, line_index, document, resolve, symbols
test_support    ──► document, resolve, symbols  (test-only, behind `test-support`)

lib  ──► declares all of the above (bare `pub mod`s, no curated re-exports)

bin/witcherscript-lsp/ ──► witcherscript_language::* (all library modules)
main                   ──► witcherscript_language::{document, files, diagnostics}

NOTE: document, diagnostics, and resolve form an intra-crate dependency cycle
(permitted within a single crate). parse_document runs the syntactic pass +
symbol extraction; the cross-file diagnostic passes call resolve; resolve reads
the parsed documents.
```

## Data flow pipeline

```
.ws file on disk
    │
    ▼
parse_document(source)          [document.rs]
    │  tree-sitter parse → Tree
    │  LineIndex::new(source)
    │  walk(root, Fused::new(SyntaxDiagnostics, SymbolExtractor))
    │      one CST walk feeds the syntactic-diagnostic and symbol passes together
    ▼
ParsedDocument { source, tree, line_index, diagnostics, symbols, parse_version }
    │
    ├─► WorkspaceIndex::update_document(uri, doc)    [resolve/workspace_index/]
    │       folds the doc's symbols into top_level_by_name, enum_member_by_name,
    │       superclass_by_name, states_by_owner, member_by_type, doc_idents, …
    │       and the cached completion_catalog
    │
    └─► LSP response handlers                        [bin/witcherscript-lsp/{completion,queries/*,references_rename,…}.rs]
            SymbolDb::new(workspace, base).with_builtins(builtins).with_script_env(env)
            resolve_definition / completion_members / statement_completions / …

Cross-file diagnostics (unknown symbol, type mismatch, …) are NOT produced here;
they run later from the LSP via collect_cst_diagnostics_for_document(uri, doc, db),
which needs a SymbolDb and is cached per file (cst_cache.rs).
```

## Index model

Every reader handler reads from a single immutable snapshot, `Compilation`, published through `Arc<ArcSwap<Compilation>>` on the `Backend`. A handler does one `compilation.load_full()` and then reads lock-free for its whole duration. Writers serialise on `writer_lock`, build a copy-on-write shadow `Compilation` via `CompilationBuilder` (only the changed fields are cloned), and atomically swap it in.

The snapshot holds three `WorkspaceIndex` symbol indexes plus the parsed-document caches; a fourth index (`builtins_index`) lives on the `Backend` itself because it is never mutated:

| Name | Type | Source |
|------|------|--------|
| `workspace_index` | `Arc<WorkspaceIndex>` | user project .ws files (under a manifest / workspace root) |
| `loose_index` | `Arc<WorkspaceIndex>` | open .ws files not part of any project |
| `base_scripts_index` | `Arc<WorkspaceIndex>` | Witcher 3 game scripts (read-only) |
| `builtins_index` | `Arc<WorkspaceIndex>` | embedded builtin types (on `Backend`, never mutated) |
| `documents` | `Arc<HashMap<Url, Arc<ParsedDocument>>>` | editor-open files; override the indexed copy |
| `workspace_documents` / `base_scripts_documents` | `Arc<HashMap<String, Arc<ParsedDocument>>>` | parsed-tree cache for background-indexed files, so semantic tokens / references read trees without re-parsing |

The snapshot also carries `script_env`, the `suppressed_base_uris` set (vanilla scripts shadowed by the workspace), and a `filtered_base_catalogs` cache.

When constructing a `SymbolDb` for a request:
- workspace shadows base, builtins are always visible: `SymbolDb::new(&workspace_index, &base_scripts_index).with_builtins(&builtins_index).with_script_env(&script_env)`
- `.with_suppressed_base_uris(...)` / `.with_prefiltered_base(...)` apply the base-shadowing filter
- open `documents` take precedence over `workspace_documents` for the file being edited

## Key types and who produces/consumes them

| Type | Produced by | Consumed by |
|------|------------|------------|
| `ParsedDocument` | `document::parse_document()` | LSP handlers, WorkspaceIndex, resolve, semantic_tokens |
| `DocumentSymbols` | `symbols::extract_symbols()` (via `SymbolExtractor`) | resolve functions, semantic_tokens, LSP document_symbol |
| `WorkspaceIndex` | `WorkspaceIndex::update_document()` | `SymbolDb` (workspace + base + optional builtins), `Compilation` |
| `SymbolDb<'_>` | constructed per-request in LSP handlers | resolve_definition, completion_*, find_references, semantic_tokens |
| `Compilation` | `CompilationBuilder` (LSP writers) | every reader handler, via one lock-free `ArcSwap` load |
| `Definition` | resolve functions | LSP handlers (hover, goto, completion items) |
| `Type` | `types::parse` (type-annotation parsing) | inference, assignability, type_mismatch |
| `ParseDiagnostic` | `SyntaxDiagnostics` (parse-time) + the CST diagnostic passes | LSP `publish_diagnostics` |
| `LineIndex` | `LineIndex::new(source)` | position_to_byte / byte_to_position in all modules |
| `ScriptEnvironment` | `script_env::parse_script_environment()` | `SymbolDb::with_script_env()` |

## Binary entry points

**`src/main.rs`** - CLI validator
- Accepts file/directory paths, recursively finds .ws files
- Parses each with `parse_document_with_parser()` (reusing one Parser instance)
- Optionally dumps tree via `format_tree()`
- Exit code: 0 (ok), 1 (diagnostics found), 2 (runtime error)
- Flags: `--dump-tree`, `--max-diagnostics N`

**`src/bin/witcherscript-lsp/`** - LSP server. Handlers are split by concern across `lifecycle.rs`, `text_sync.rs`, `completion.rs`, `references_rename.rs`, and the `queries/` directory (one file per read-only request), with `convert/` for LSP-to-internal conversion and the indexing / caching / status modules alongside.
- Async Tokio runtime; async-lsp router over stdin/stdout (or TCP in listen mode)
- `Backend` publishes shared state as an immutable `Compilation` snapshot behind `Arc<ArcSwap<>>`; readers load it lock-free, writers serialise on a `writer_lock` and atomically swap a copy-on-write replacement
- All parse/resolve logic lives in the library; the binary only orchestrates
- See [lsp_server.md](lsp_server.md) for full details
