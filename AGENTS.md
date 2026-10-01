# Agent guidelines for witcherscript-language

## Repository overview

This is a Rust crate (`witcherscript-language`). Binaries:

- `witcherscript-check` - CLI syntax validator (`src/main.rs`)
- `witcherscript-lsp` - LSP server (`src/bin/witcherscript-lsp/`)
- `wsformat` - CLI formatter (`src/bin/wsformat.rs`)

## Detail docs

Start with [architecture.md](docs/agents/architecture.md) for the source file tree, module graph, data-flow pipeline, and index model. Then the area docs:

| Doc | Covers |
| --- | --- |
| [resolution.md](docs/agents/resolution.md) | Resolution, inference, references, signatures, completion; `SymbolDb` / `WorkspaceIndex` |
| [mod_resolve.md](docs/agents/mod_resolve.md) | Rules to follow when editing resolve / parsing / syntax code (read first) |
| [symbols.md](docs/agents/symbols.md) | `DocumentSymbols`, `Symbol`, `SymbolKind`, `extract_symbols` |
| [diagnostics.md](docs/agents/diagnostics.md) | Syntactic and cross-file validation rules |
| [semantic_tokens.md](docs/agents/semantic_tokens.md) | `TOKEN_TYPES`, classification, highlighting |
| [lsp_server.md](docs/agents/lsp_server.md) | LSP backend: handlers, capabilities, URI handling, indexing, text sync |
| [builtins.md](docs/agents/builtins.md) | Embedded engine types (`array<T>`, classes, enums) |
| [class_body_specifiers.md](docs/agents/class_body_specifiers.md) | Which specifiers and flavours are valid in a class body |
| [testing.md](docs/agents/testing.md) | Test inventory, fixtures |
| [writing-tests.md](docs/agents/writing-tests.md) | How to write tests: style, helpers, fixture markers |
| [language.md](docs/agents/language.md) | WitcherScript language cheat sheet |
| [invariants.md](docs/agents/invariants.md) | Non-obvious constraints that cause silent bugs - read before touching resolution, indexing, or text sync |

## Build and test

Use justfile recipes, not hand-rolled cargo commands: `just build`, and `just precommit` (fmt + clippy + nextest in one). The test inventory and fixtures are in [docs/agents/testing.md](docs/agents/testing.md).

## Code style

See `CODESTYLE.md` for the normative Rust code standard.
