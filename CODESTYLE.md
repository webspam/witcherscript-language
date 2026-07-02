# Code Style

Normative standard for new and modified Rust code; where existing code disagrees, the existing code is what changes. `CLAUDE.md`/`AGENTS.md` govern process; this document governs the code.

Item codes like `C-NEWTYPE` refer to the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) checklist.

## Functions and naming

- Named functions over long closures or deeply nested bodies
- One responsibility each; a function that needs a comment needs a better name or a split
- Self-describing names (`resolve_symbol_in_scope`), not generic ones (`process`, `helper`)
- Conversions use `as_`/`to_`/`into_` (`C-CONV`)
- Iterators are `iter`/`iter_mut`/`into_iter` (`C-ITER`); iterator types named after their producer (`C-ITER-TY`)
- Consistent word order across sibling names (`C-WORD-ORDER`): `from_path`, `from_str`, not `parse_str`

## Comments

Short and terse, and only when the *why* is non-obvious: a hidden constraint, invariant, or workaround. Never describe what code does. `///` doc comments meet the same bar; being public is not a reason.

## Control flow

- Guard clauses and early returns over nested `if`/`else`
- `match` over an `if let` chain past two arms
- Closed enums: exhaustive arms, no `_ =>`
  - Open-domain dispatch (string tags like `node.kind()`, or any value the type system does not enumerate): `_ =>` is idiomatic
- Iterator chains for transforms; switch to a `for` loop when the body accumulates non-trivial state, branches widely, mutates external context, or grows past a few lines
  - Clarity decides, not performance

## Error handling

- `Result` default; propagate with `?`
- `.unwrap()`/`.expect()` only for a documented invariant the type system cannot express, named in an adjacent comment
- `thiserror` for library-style modules; `anyhow` only at binary boundaries
  - One approach per crate; do not mix the two in one module
- A guard return must distinguish "correctly did nothing" from "something broke"; make the broken case observable (log or `Err`) before returning
  - `let _ = result;`, `.ok()`, or an unconsidered `if let Ok(...)` are violations
  - An intentionally dropped `Err` is explicit and justified in a comment
- Exception: `let _ = write!(buf, ...)` on `String`/`Vec<u8>`; `fmt::Write` on a buffer cannot fail
  - Does not extend to `io::Write`

## Types

- Newtypes for distinct domains (`C-NEWTYPE`): a `SymbolId` is not a `u32`
- Enums over `bool` parameters (`C-CUSTOM-TYPE`); a three-variant enum over `Option<bool>`
- Discriminated enums over structs of optional fields
- Builders only for many optional fields where construction order or validation matters; otherwise a direct constructor
- `Box<dyn Any>` needs written justification
- Validate external input at the boundary; trust internal callers
- Name magic values as `const`

## Ownership and mutability

- Borrow by default: `&str`, `&[T]`, `&Path` parameters
- Return owned values only when transferring ownership
- `.clone()` is deliberate, not a borrow-checker silencer; repeated clones in one function mean the data model is wrong
- `Cow<'_, T>` only when both borrowed and owned paths are actually taken
- Interior mutability needs a one-line justification of what is shared and what guarantees safety

## Modules and visibility

- Small files, one responsibility; split at two unrelated jobs
- `pub(crate)` default; `pub` only for deliberate crate-boundary surface
- No `pub use` re-export shims
- `pub` struct fields are rare; enforce invariants via constructors and methods (`C-STRUCT-PRIVATE`)

## Logging

`tracing` only; no `println!`/`eprintln!` outside binaries' top-level error paths and test scaffolding.
`trace` for per-token/symbol/event detail; `debug` for dev events; `info` and above are production-visible, use sparingly.
Structured fields (`tracing::debug!(path = %p, "loaded")`), not interpolated strings.

## Read, compute, cache

Separate the phases: read does fallible I/O; compute is pure over borrowed inputs (the testable core); cache memoises compute against an input identity. Do not interleave them.

Caches apply deltas rather than rebuild when the delta is well-defined and demonstrably cheaper. Document the invariant the update preserves in one line. Existing fingerprint invalidate-and-rebuild caches are legacy; new caches default to deltas, and conversions happen where they pay (some, like CST diagnostics, may reasonably stay rebuild).

## Performance

Take structural wins at equal readability: keyed access wants a `HashMap`, not a `Vec` scan. Do not optimise speculatively or let perf drive code shape.

## Testing

Unit tests in `#[cfg(test)] mod tests` at the bottom of the file they cover; integration tests under `tests/`. Names state the behaviour (`returns_none_when_input_empty`); assert messages name the failed expectation. Snapshots only for output that is large *and* stable.

## Async and concurrency

- `async` at network/FS edges; the analysis code they call is plain functions
- One runtime per binary, built in `main`; libraries pick none
- `Send + Sync` is the default bound for shared state
- Message passing over shared `Mutex` when ownership can move
  - Exception: workspace-wide state mutated by many concurrent handlers (the LSP backend) is properly lock-based
- `parking_lot` locks over `std::sync`: no poisoning, faster

## General

- Suppress advisory Clippy lints with a comment naming the reason
- No dense one-liners; clarity over brevity
- No premature abstractions; but moving a small shared helper to a common file is common sense, not abstraction
- No backwards-compatibility shims, `_unused` renames, or `// removed` placeholders; if it is gone, it is gone
- Do not design for hypothetical future requirements unless instructed
