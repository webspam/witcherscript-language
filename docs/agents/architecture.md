# Architecture overview

## Source file tree

Most modules carry a colocated `tests.rs` (or `tests/` submodule), omitted here; only dedicated test directories are shown.

```
src/
├── lib.rs                          module declarations
├── main.rs                         CLI binary (witcherscript-check)
├── bin/
│   ├── dump_tree.rs               developer helper: prints a tree-sitter parse tree
│   └── witcherscript-lsp/         LSP server binary (handlers split by LSP concern)
│       ├── main.rs                router + middleware wiring; stdio / TCP serve
│       ├── backend.rs             Backend state struct + shared handler helpers
│       ├── compilation.rs         atomically-swapped index/document snapshot + COW builder
│       ├── lifecycle.rs           initialize / initialized; registers LSP capabilities
│       ├── text_sync.rs           didOpen / didChange / didClose + watched-file events
│       ├── edit_queue.rs          in-flight edit state pending reindex
│       ├── completion.rs          completion handler; dispatch to resolve/completion/
│       ├── completion_cache.rs    merged global/type completion list, cached by hash
│       ├── references_rename.rs   references, prepareRename, rename + cross-doc merge
│       ├── config.rs              Config struct, DiagnosticsScope, settings parsing
│       ├── project_manifest.rs    parse witcherscript.toml; resolve scripts_root
│       ├── convert/               LSP <-> internal conversion
│       │   ├── mod.rs             re-exports the convert sub-modules
│       │   ├── positions.rs       LSP <-> internal position / range
│       │   ├── diagnostics.rs     internal diagnostics + base-conflict actions to LSP
│       │   ├── completions.rs     definitions to CompletionItem / SignatureHelp
│       │   ├── symbols.rs         symbols to DocumentSymbol / WorkspaceSymbol / hover
│       │   ├── highlights.rs      HighlightKind to LSP DocumentHighlight
│       │   ├── inlay_hints.rs     InlayHintInfo to LSP InlayHint
│       │   ├── file_ops.rs        create / delete / rename params to file events
│       │   └── refactor/          code-action refactorings (extract, inline, if/switch, join/split)
│       ├── queries/               read-only request handlers
│       │   ├── mod.rs             shared query helpers; FormatOptions
│       │   ├── hover.rs           hover
│       │   ├── definition.rs      goto-definition + goto-type-definition
│       │   ├── document_symbol.rs document symbols
│       │   ├── workspace_symbol.rs workspace symbol search
│       │   ├── signature_help.rs  signature help
│       │   ├── semantic_tokens.rs semantic tokens full / delta / range
│       │   ├── document_highlight.rs document highlight
│       │   ├── inlay_hint.rs      inlay hints (config-gated)
│       │   ├── code_action.rs     base-conflict + refactor code actions
│       │   ├── code_lens.rs       base-definition + reference-count lenses
│       │   ├── diagnostics.rs     document + workspace pull-diagnostic handlers
│       │   └── formatting.rs      full-document formatting
│       ├── semantic_tokens_cache.rs  per-document semantic token cache + delta
│       ├── cst_cache.rs           per-file CST diagnostic cache
│       ├── diagnostics_publish.rs bundles + publishes all diagnostic categories
│       ├── view_refresh.rs        client refresh requests on state-version change
│       ├── file_scope.rs          classify a URI (project / legacy / base / loose)
│       ├── file_scope_status.rs   witcherscript/fileScopeStatus notification type
│       ├── legacy_status.rs       witcherscript/legacyScriptStatus notification type
│       ├── indexing/              workspace + base-script indexing (helpers, open_documents, legacy, scan)
│       ├── logging.rs             tracing layer forwarding events to the client
│       ├── watcher.rs             file watchers to canonical upsert / delete
│       └── tests/                 unit + E2E / integration tests (per-feature + e2e/ subdir)
├── builtins.rs                     embed + parse engine .ws sources into a WorkspaceIndex
├── cst/                            shared tree-sitter CST traversal primitives
│   ├── mod.rs                      re-exports the cst submodules
│   ├── ancestors.rs                ancestor-of-kind lookup
│   ├── descendants.rs              collect / detect descendant nodes by kind
│   ├── fields.rs                   generated grammar field-name constants
│   ├── grammar.rs                  error-recovery-aware call / member / arg accessors
│   ├── if_stmt.rs                  if-branch exclusivity + else-chain analysis
│   ├── kinds.rs                    generated grammar node-kind constants
│   ├── literals.rs                 classify whether a node is a constant literal
│   ├── nav.rs                      child / sibling navigation (nth, named, field)
│   ├── offsets.rs                  byte-offset to node queries; cursor classification
│   ├── sourcegen.rs                test-only: regenerate kinds.rs / fields.rs
│   └── walk.rs                     iterative pre/post-order visitor; Fused two-visitor
├── document.rs                     parse orchestration, ParsedDocument
├── diagnostics/                    ParseDiagnostic + the cross-file CST rule passes
│   ├── mod.rs                      registers passes; collect_cst_diagnostics_for_document
│   ├── cst_walker.rs               CstRule trait, parallel pass runner
│   ├── abstract_instantiation.rs   flag new on abstract classes
│   ├── annotation_state_target.rs  validate state-targeting annotation names
│   ├── base_script_conflict.rs     workspace files shadowing base-game scripts
│   ├── duplicate_local.rs          duplicate parameter / local variable names
│   ├── duplicate_symbols.rs        cross-file top-level name collisions
│   ├── inherited_field.rs          member field duplicating an inherited field
│   ├── override_consistency.rs     access level + param count on overrides
│   ├── shadowing.rs                locals / fields shadowing globals or fields
│   ├── state_owner.rs              a state's owner class must be a statemachine
│   ├── super_field_access.rs       illegal field access through super
│   ├── type_mismatch.rs            assignment / return / call-arg type checks
│   ├── unknown_method.rs           method calls resolving to no known member
│   ├── unknown_symbol.rs           references resolving to no known symbol
│   ├── unused_symbol.rs            parameters / locals / fields never referenced
│   └── wrapped_method.rs           validate @wrapMethod usage and call patterns
├── files.rs                        recursive .ws file discovery, canonical_uri
├── formatter.rs                    document formatter entry point (textDocument/formatting)
├── formatter/
│   ├── core.rs                     formatter state: emit, indent, comment flushing
│   ├── action.rs                   indent + substitution helpers
│   ├── declarations.rs             class / struct / enum / state declarations
│   ├── signatures.rs               function parameter lists + return types
│   ├── if_action.rs                if-chain collapse/expand layout rewrites
│   ├── switch_action.rs            switch collapse/expand layout rewrites
│   ├── statements/                 statement + expression formatting
│   │   ├── mod.rs                  statement formatting dispatch
│   │   ├── if_stmt.rs              per-if block layout, forced-block coercion
│   │   └── switch.rs               switch arm collection + aligned rendering
│   └── tests/                      formatter fixture + unit tests
├── line_index.rs                   byte ↔ UTF-16 position mapping (LSP-compatible)
├── script_env.rs                   INI script globals parser
├── strings.rs                      string utilities: suffixing, casing, identifiers
├── types/                          structured Type enum + type-annotation parsing
│   ├── mod.rs                      Type enum (Primitive, Named, Array, Null, ...)
│   └── parse.rs                    parse type annotations; alias + native-type rules
├── test_support/                   test-only helpers (gated by the test-support feature)
│   ├── mod.rs                      TestDb: build a WorkspaceIndex from a fixture string
│   └── fixture.rs                  marker fixture parser ($0 cursor, ^^^ spans, //- headers)
├── symbols/                        SymbolKind, Symbol, DocumentSymbols, extract_symbols
│   ├── mod.rs                      public re-export surface for symbols
│   ├── types.rs                    Symbol, SymbolId, SymbolKind, Specifiers, AccessLevel
│   ├── extract.rs                  SymbolExtractor: CST walk to DocumentSymbols
│   └── util.rs                     node_text, CST helper text extraction
├── resolve/
│   ├── mod.rs                      public re-export facade for the resolve subsystem
│   ├── ast.rs                      shared cst/ navigation helpers; BUILTIN_TYPE_COMPLETIONS
│   ├── assignability.rs            pure type-compatibility engine + implicit cast table
│   ├── definition.rs               goto-definition: resolve identifier at a position
│   ├── type_definition.rs          goto-type-definition: resolve a declared type
│   ├── inference.rs                expression type inference; name / member resolution
│   ├── references.rs               find-all-references for a resolved definition
│   ├── document_highlight.rs       read/write occurrences of the symbol under cursor
│   ├── signature.rs                signature help, hover text, parameter rendering
│   ├── inlay_hints.rs              parameter-name inlay hints for call sites
│   ├── name_context.rs             NameContext: position restricting valid symbol kinds
│   ├── overrides.rs                pair workspace symbols with the base defs they shadow
│   ├── shadowed_base.rs            base index view with overridden URIs filtered out
│   ├── state_classes.rs            engine-synthesised backing class for state decls
│   ├── reaching_defs.rs            reaching-definitions analysis for one local
│   ├── writes.rs                   classify write sites (assign targets, out-args)
│   ├── selection.rs                classify / trim a byte-range selection for refactors
│   ├── edit_plan.rs                EditPlan / Splice / Extraction byte-range edits
│   ├── extract_var.rs              extract-variable refactor
│   ├── inline_var.rs               inline-variable refactor
│   ├── join_split_decl.rs          join / split variable-declaration refactors
│   ├── completion_catalog.rs       CompletionCatalog: global callable / type / enum lists
│   ├── workspace_symbols.rs        ranked workspace-wide symbol search (workspace/symbol)
│   ├── subscription_registry.rs    tracks which docs observe which symbol names
│   ├── body_model/                 request-scoped semantic model of one callable body
│   ├── extract_callable/           extract function / method (captures, render, statements)
│   ├── workspace_index/            WorkspaceIndex (mod, indices, lookup, subscribers)
│   ├── symbol_db/                  SymbolDb (mod, lookup, generics)
│   ├── completion/                 completion submodule
│   │   ├── mod.rs                  re-export facade for the completion sub-modules
│   │   ├── body_function.rs        expression / statement completions in bodies
│   │   ├── body_class.rs           keyword completions in class / state / struct bodies
│   │   ├── body_script.rs          keyword + annotation completions at script top level
│   │   ├── comment.rs              predicate: is the cursor inside a comment?
│   │   ├── globals.rs              merge global callables, script globals, enum members
│   │   ├── headers.rs              keyword completions in declaration headers (extends, in)
│   │   ├── members.rs              member-access completions by inferred receiver type
│   │   ├── new_expr.rs             type + lifetime completions for new expressions
│   │   └── types.rs                type-name completions at annotation / cast positions
│   └── tests/                      resolution + completion + refactor tests
└── semantic_tokens/
    └── mod.rs                      TOKEN_TYPES, collect_semantic_tokens, classify

tests/
├── parser_fixtures.rs              fixture-driven parse tests (valid/ and invalid/)
├── language_features.rs            integration tests for symbol extraction + resolution
└── fixtures/
    ├── valid/                      .ws files that must parse with zero diagnostics
    ├── invalid/                    .ws files that must produce at least one diagnostic
    └── formatter/                  before/after inputs for formatter fixture tests
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

The LSP server maintains three separate `WorkspaceIndex` symbol indexes, plus the open-documents map that overrides them for files being edited:

| Name | Type | Source |
|------|------|--------|
| `workspace_index` | `WorkspaceIndex` | user project .ws files |
| `base_scripts_index` | `WorkspaceIndex` | Witcher 3 game scripts (read-only) |
| `builtins_index` | `Arc<WorkspaceIndex>` | embedded builtin types (never mutated) |
| open `documents` | `HashMap<Url, ParsedDocument>` | editor-open files (not an index - overrides the indexed copy) |

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

**`src/main.rs`** - CLI validator
- Accepts file/directory paths, recursively finds .ws files
- Parses each with `parse_document_with_parser()` (reusing one Parser instance)
- Optionally dumps tree via `format_tree()`
- Exit code: 0 (ok), 1 (diagnostics found), 2 (runtime error)
- Flags: `--dump-tree`, `--max-diagnostics N`

**`src/bin/witcherscript-lsp/`** - LSP server (module split across `main.rs`, `backend.rs` + the per-concern handler files `lifecycle.rs` / `text_sync.rs` / `completion.rs` / `queries.rs` / `references_rename.rs`, plus `convert/`, `cst_cache.rs`, `indexing/`, `config.rs`, `diagnostics_publish.rs`, `file_scope.rs`, `file_scope_status.rs`, `legacy_status.rs`, `watcher.rs`, `logging.rs`, and `tests.rs` + per-feature files under `tests/`)
- Async Tokio runtime; async-lsp framework over stdin/stdout
- `Backend` struct holds all shared state behind `Arc<Mutex<>>`
- All parse/resolve logic lives in the library; the binary only orchestrates
- See [lsp_server.md](lsp_server.md) for full details
