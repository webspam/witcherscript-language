# Agent guidelines for witcherscript-language

## Repository overview

This is a Rust crate (`witcherscript-language`) that produces two binaries:

- `witcherscript-check` - CLI syntax validator (`src/main.rs`)
- `witcherscript-lsp` - LSP server (`src/bin/witcherscript-lsp/`)

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
| [testing.md](docs/agents/testing.md) | Test inventory, fixtures, benchmarks |
| [writing-tests.md](docs/agents/writing-tests.md) | How to write tests: style, helpers, fixture markers |
| [language.md](docs/agents/language.md) | WitcherScript language cheat sheet |
| [invariants.md](docs/agents/invariants.md) | Non-obvious constraints that cause silent bugs - read before touching resolution, indexing, or text sync |

## Build and test

Use justfile recipes, not hand-rolled cargo commands: `just build`, and `just test` (fmt + clippy + nextest in one). The test inventory, fixtures, and benchmarks are in [docs/agents/testing.md](docs/agents/testing.md).

IMPORTANT: When adding a new grammar construct or validation rule, add or update a fixture file and a targeted unit test.

## Committing changes

Commit each logical change as a separate commit as soon as it is complete - do not accumulate unrelated edits into a single commit. This keeps `git bisect` useful and makes the history easy to read.

Before committing:

1. Run `just test` and confirm all tests pass (runs fmt and clippy automatically).

### Commit messages

IMPORTANT: The first part of the commit message should be HUMAN RELATABLE. DO NOT just write which part of the code you changed; instead, what actual problem is it fixing / what goal is it achieving?

## Code style

See `CODESTYLE.md` for the normative Rust code standard.
