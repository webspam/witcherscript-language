# Built-in types

**Files:** [src/builtins.rs](../../src/builtins.rs), [builtins/](../../builtins/)

WitcherScript has engine-magic types - `array<T>`, a fixed set of engine enums, and a handful of engine classes - that have no declaration anywhere in user code or shipped game scripts. The LSP synthesises their definitions so that completion, hover, and go-to-definition work on them.

`builtins/enums.ws` holds the engine enums. Each enum is a global type and each of its values is a global symbol, used by bare name (`AD_Back`, not `EAttackDirection.AD_Back`). Both flow through the normal symbol pipeline once the file is parsed into the builtins index - no enum-specific Rust logic.

`builtins/orphan_enums.ws` collects engine enum values whose enclosing enum is unknown, under one catch-all enum. That catch-all is not a real type, so it is hidden from type completion (see Guardrails).

Engine classes get one file each, named after the class (`builtins/CR4HudModule.ws`); the synthetic URI is derived from the file name. They are listed in the `CLASS_BUILTINS` table in `src/builtins.rs`, so adding the next class is one `include_str!` row.

`builtins/unknown-classes.ws`, `unknown-enums.ws`, `unknown-interfaces.ws`, and `unknown-structs.ws` are bulk catch-all files: minimal declarations for engine types that exist at runtime but have no declaration in any shipped script, so the LSP would otherwise emit "unknown type" diagnostics. They are deliberately bare (empty bodies, shallow hierarchies) - their job is to silence false diagnostics, not to model the real API. They may be filled in over time from user submissions. Each is one `include_str!` row in `BUILTIN_SOURCES`.

The native engine value-types (`CBehTreeVal*`) are C++ primitives with no script declaration. Their single source of truth is the `NATIVE_TYPE_ACCEPTS` table in `src/types/parse.rs` (name plus accepted `default` primitive). `build_builtins_index` emits a bare `class` stub per table entry on demand - there is no `.ws` file - and `insert_builtin` re-tags those stubs to `SymbolKind::NativeType` via `DocumentSymbols::retag_top_level`. That kind keeps them usable as type annotations while excluding them from class behaviour (object-to-bool/string casts, `new`, `extends`).

## Source of truth

`builtins/array.ws` is a real `.ws` file embedded at build time via `include_str!`. To change the array API, edit that file and rebuild - no Rust changes required.

The grammar cannot parse `class array<T>` (generic params on a class decl produce tree-sitter `ERROR` nodes), so the file is written without the `<T>` header. The bare ident `T` inside method signatures stands in for the element type. A one-line comment at the top of `builtins/array.ws` documents this convention.

## Loading

`load_builtins_index()` parses every embedded source into a fresh `WorkspaceIndex` keyed by synthetic URIs (`witcherscript-builtin:/array.ws`, ...). The LSP `Backend` holds it as `Arc<WorkspaceIndex>` - built once at startup, never mutated.

Tests opt in via `SymbolDb::new(&ws, &base).with_builtins(&builtins)`; existing tests that don't touch built-ins are unaffected.

## Generic substitution

When a query asks for members of `array<int>`:

1. `parse_generic_type("array<int>") → ("array", "int")`.
2. The chain looks up `array` (no generics) in workspace → base → builtins.
3. Each returned `Definition` is passed through `substitute_in_definition(def, "array<int>", "int")`, which:
   - Structurally replaces `Type::Named("T")` with the element type inside `type_annotation` (`Type::substitute_named`, recursing through `Type::Array`).
   - Rewrites `container_name` from `array` to `array<int>` so hover shows `(method) array<int>.PushBack(value: int): void`.

Parameter types are substituted at display time: `SymbolDb::display_parameters_of(&definition)` fetches the callable's `Parameter` symbols and applies the same substitution when `container_name` parses as a generic instance. Hover, signature help, and completion detail all render from it.

Nested generics (`array<array<int>>`) substitute one level: `Last()` resolves with `type_annotation` `array<int>`.

## Guardrails

- `prepare_rename` and `rename` reject any symbol whose URI is a builtin URI (`builtin_source(uri).is_some()`).
- `rename_changes` filters out reference sites that land inside a builtin file - same shape as the base-scripts guard.
- `SymbolDb::all_types()` includes builtin enums and classes (real, usable types) but excludes whatever `is_non_type_builtin()` flags - `array` (only valid as `array<T>`) and the orphan catch-all enum - since neither can be written as a plain type name. `all_enum_members()` still includes every builtin enum value, the orphan ones included.

## Adding a new built-in

1. Add or edit `builtins/<name>.ws`. Use `T` as the generic placeholder (if needed) and the same conventions as `array.ws`. For an engine class, name the file after the class (`builtins/<ClassName>.ws`).
2. In `src/builtins.rs`: for an engine class, add a `("witcherscript-builtin:/<ClassName>.ws", include_str!(...))` row to `CLASS_BUILTINS` - nothing else. For other built-ins, add a `const FOO_WS: &str = include_str!("../builtins/<name>.ws")`, a `BUILTIN_<NAME>_URI` constant, an `insert_builtin(...)` call in `load_builtins_index()`, and the URI to `builtin_source()`.
3. If the type is generic, the substitution layer in `src/resolve/symbol_db/` will work automatically - it keys off `parse_generic_type()` (in `src/types/`) and is not array-specific.
4. Add unit tests in `src/resolve/tests/builtin_<name>.rs` and a fixture in `tests/fixtures/valid/`.
