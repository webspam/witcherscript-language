# Built-in types

**Files:** [src/builtins.rs](../../src/builtins.rs), [builtins/](../../builtins/)

WitcherScript has engine-magic types - `array<T>`, a fixed set of engine enums, and a handful of engine classes - that have no declaration anywhere in user code or shipped game scripts. The LSP synthesises their definitions so that completion, hover, and go-to-definition work on them. Of these, only `array<T>` needs special handling in code (generic element-type substitution); the rest flow through the normal symbol pipeline.

`builtins/enums.ws` holds most of the engine enums; two large ones get their own file (`builtins/EInputKey.ws`, `builtins/EShowFlags.ws`). Each enum is a global type and each of its values is a global symbol, used by bare name (`AD_Back`, not `EAttackDirection.AD_Back`). Both flow through the normal symbol pipeline once the files are parsed into the builtins index - no enum-specific Rust logic.

`builtins/orphan_enums.ws` collects engine enum values whose enclosing enum is unknown, under one catch-all enum. That catch-all is not a real type, so it is hidden from type completion (see Guardrails).

Engine classes get one file each, named after the class (`builtins/CR4HudModule.ws`, `builtins/CGuiObject.ws`); the synthetic URI matches the file name. They are rows in the `BUILTIN_SOURCES` table in `src/builtins.rs`.

`builtins/unknown-classes.ws`, `unknown-enums.ws`, `unknown-interfaces.ws`, and `unknown-structs.ws` are bulk catch-all files: minimal declarations for engine types that exist at runtime but have no declaration in any shipped script, so the LSP would otherwise emit "unknown type" diagnostics. They are deliberately bare (empty bodies, shallow hierarchies) - their job is to silence false diagnostics, not to model the real API. They may be filled in over time from user submissions.

The native engine value-types (`CBehTreeVal*`) are C++ primitives with no script declaration. Their single source of truth is the `NATIVE_TYPE_ACCEPTS` table in `src/types/parse.rs` (name plus the `default`-value primitives it accepts). `NATIVE_TYPES_SOURCE` generates one bare `class` stub per table entry into a single synthetic source (there is no `.ws` file); `insert_builtin` parses it under `BUILTIN_NATIVE_TYPES_URI` and re-tags those stubs to `SymbolKind::NativeType` via `DocumentSymbols::retag_top_level`. That kind keeps them usable as type annotations while excluding them from class behaviour (object-to-bool/string casts, `new`, `extends`).

## Loading

`build_builtins_index()` parses every embedded source into a `WorkspaceIndex` keyed by synthetic URIs (`witcherscript-builtin:/enums.ws`, ...). It runs once behind the `BUILTINS` `LazyLock`; `load_builtins_index()` just clones that cached index. The LSP `Backend` holds the clone as `Arc<WorkspaceIndex>` - built once at startup, never mutated.

Tests opt in via `SymbolDb::new(&ws, &base).with_builtins(&builtins)`; existing tests that don't touch built-ins are unaffected.

## Guardrails

- `prepare_rename` and `rename` reject any symbol whose URI is a builtin URI (`builtin_source(uri).is_some()`).
- `rename_changes` filters out reference sites that land inside a builtin file - same shape as the base-scripts guard.
- `SymbolDb::all_types()` includes builtin enums and classes (real, usable types) but excludes whatever `is_non_type_builtin()` flags - `array` (only valid as `array<T>`) and the orphan catch-all enum - since neither can be written as a plain type name. `all_enum_members()` still includes every builtin enum value, the orphan ones included.

## Adding a new built-in

1. Add or edit `builtins/<name>.ws`.
2. In `src/builtins.rs`, add a row to the `BUILTIN_SOURCES` table like the existing ones. A type that is not bare-writable (like `array`) must also be added to `is_non_type_builtin()`.
3. Add unit tests in `src/resolve/tests/builtin_<name>.rs` and a fixture in `tests/fixtures/valid/`.
