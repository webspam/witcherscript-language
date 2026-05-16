# Built-in types

**Files:** [src/builtins.rs](../../src/builtins.rs), [builtins/](../../builtins/)

WitcherScript has engine-magic types — most importantly `array<T>` — that have no declaration anywhere in user code or shipped game scripts. The LSP synthesises their definitions so that completion, hover, and go-to-definition work on them.

## Source of truth

`builtins/array.ws` is a real `.ws` file embedded at build time via `include_str!`. To change the array API, edit that file and rebuild — no Rust changes required.

The grammar cannot parse `class array<T>` (generic params on a class decl produce tree-sitter `ERROR` nodes), so the file is written without the `<T>` header. The bare ident `T` inside method signatures stands in for the element type. A one-line comment at the top of `builtins/array.ws` documents this convention.

## Loading

`load_builtins_index()` parses every embedded source into a fresh `WorkspaceIndex` keyed by synthetic URIs (`witcherscript-builtin:///array.ws`, ...). The LSP `Backend` holds it as `Arc<WorkspaceIndex>` — built once at startup, never mutated.

Tests opt in via `SymbolDb::new(&ws, &base).with_builtins(&builtins)`; existing tests that don't touch built-ins are unaffected.

## Generic substitution

When a query asks for members of `array<int>`:

1. `parse_generic_type("array<int>") → ("array", "int")`.
2. The chain looks up `array` (no generics) in workspace → base → builtins.
3. Each returned `Definition` is passed through `substitute_in_definition(def, "array<int>", "int")`, which:
   - Replaces whole-ident occurrences of `T` with `int` in `type_annotation`, `signature`, `detail`.
   - Rewrites `container_name` from `array` to `array<int>` so hover shows `(method) array<int>.PushBack(param1: int) : void`.

Whole-ident substitution: `T`, `<T>`, `: T)`, `: T;` all match; `TArray`, `MyT`, `T_x` do not. See `substitute_placeholder()` in `src/resolve/mod.rs`.

Nested generics (`array<array<int>>`) substitute one level: `Last() : T` becomes `Last() : array<int>`.

## Guardrails

- `prepare_rename` and `rename` reject any symbol whose URI is a builtin URI (`builtin_source(uri).is_some()`).
- `rename_changes` filters out reference sites that land inside a builtin file — same shape as the base-scripts guard.
- `SymbolDb::all_types()` excludes builtins so `array` doesn't pollute the type-completion list.

## Adding a new built-in

1. Add or edit `builtins/<name>.ws`. Use `T` as the generic placeholder (if needed) and the same conventions as `array.ws`.
2. In `src/builtins.rs`: add a `const FOO_WS: &str = include_str!("../builtins/<name>.ws")`, a `BUILTIN_<NAME>_URI` constant, and an `insert_builtin(&mut index, ..., FOO_WS)` call in `load_builtins_index()`. Add the URI to `builtin_source()`.
3. If the type is generic, the substitution layer in `src/resolve/mod.rs` will work automatically — it keys off `parse_generic_type()` and is not array-specific.
4. Add unit tests in `src/resolve/tests/builtin_<name>.rs` and a fixture in `tests/fixtures/valid/`.
