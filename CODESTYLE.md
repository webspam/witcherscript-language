# Code Style

This document defines the standard new and modified Rust code in this workspace is held to. It is normative, not descriptive: where existing code disagrees, the existing code is what changes. Reviewers cite sections from this document when judging whether a PR or feature meets the bar.

Authoritative external references:

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) for naming, trait impls, and library conventions. Item codes like `C-CASE`, `C-NEWTYPE`, `C-CONV` refer to entries in that checklist.
- [Clippy lints](https://rust-lang.github.io/rust-clippy/master/). The `correctness`, `suspicious`, `style`, `complexity`, and `perf` categories are must-fix. `pedantic`, `restriction`, and `nursery` are advisory; suppress with a comment naming the reason.

`CLAUDE.md` and `AGENTS.md` still govern process (relative paths, commit message style, agent workflow). This document governs the code itself.

## Functions

Extract named functions over long closures or deeply nested bodies. One responsibility per function; if it needs a comment to explain what it does, it needs a better name or a split.
Prefer self-describing names (`resolve_symbol_in_scope`, `apply_text_edit`) over generic ones (`process`, `handler`, `helper`).

## Naming

Casing follows Rust API Guidelines `C-CASE`: `snake_case` for functions, methods, modules, variables; `CamelCase` for types and traits; `SCREAMING_SNAKE_CASE` for constants and statics.
Conversion methods use the `as_`/`to_`/`into_` verbs per `C-CONV`. Collection iterators use `iter`/`iter_mut`/`into_iter` per `C-ITER`. Iterator type names match their producer per `C-ITER-TY`.
Names use consistent word order across the crate (`C-WORD-ORDER`): if one method is `from_path`, sibling methods are `from_str`, not `parse_str`.

## Comments

**Any comments MUST be terse AND concise. Less is more.**

**ONLY** write a comment when the *why* is non-obvious: a hidden constraint, a subtle invariant, a workaround for a specific behaviour. Never describe what the code does; well-named identifiers already do that. `///` doc comments are held to the same bar as ordinary comments: write one when the *why* or the *contract* is non-obvious, not because a function is public.

## Iterators vs loops

Iterator chains are the default for transforms: `map`, `filter`, `collect`, `fold`, `find`, `any`, `all`.
Switch to an explicit `for` loop when the body accumulates non-trivial state, branches widely, mutates external context, or grows past a few lines. The choice is about clarity, not performance: Rust reliably compiles idiomatic iterator chains down to optimal code with zero overhead, so pick whichever form reads best for the specific transform.

## Control flow

Guard clauses and early returns over nested `if`/`else`. `match` over an `if let` / `else if let` chain once there are more than two arms. For closed enums, write exhaustive `match` arms; do not use `_ => ...` to silence the compiler, since that defeats the warning when a variant is added.
The exhaustiveness rule applies to closed Rust enums. For open-domain dispatches (matching on string tags such as `node.kind()`, externally-defined identifiers, or any value whose universe is not enumerated in the type system) a catch-all `_ => ...` is idiomatic.

## Early returns must not hide bugs

A guard that returns must distinguish "correctly did nothing" from "exited because something was broken". Before a guard returns, decide which you have: if the condition should never happen, make it observable by logging it or returning an `Err`, *then* return. Silently discarding error state with `let _ = result;`, `.ok()`, or an unconsidered `if let Ok(...)` erases the difference between the two and is a rule violation.

## Error handling

`Result` is the default; propagate with `?`. `.unwrap()` and `.expect("...")` are only acceptable when the call site is a documented invariant the type system cannot express, and a brief comment names that invariant on the same or preceding line.

Use `thiserror` to define typed error enums for library-style modules where callers may want to match on the variant. Use `anyhow` only at binary boundaries (`main`, request handlers, top-level commands) where the error is destined for a log line or a user message. One error-handling approach per crate; do not mix `thiserror` and `anyhow` definitions in the same module.

Errors fail loud. No silent `catch`-equivalent: an `Err` that is intentionally dropped is explicit, named, and justified in a comment.

## Type safety

- Newtypes (`C-NEWTYPE`) for distinct domains: a `SymbolId` is not a `u32`, a `FilePath` is not a `String`. The wrapper costs nothing at runtime and prevents whole categories of mix-ups.
- No `bool` parameters where an enum conveys intent (`C-CUSTOM-TYPE`). `fn open(path, ReadOnly)` reads at the call site; `fn open(path, true)` does not.
- No bare `Option<bool>` for tri-state; define a three-variant enum.
- `#[non_exhaustive]` on public enums you may extend, so downstream `match`es do not break on a new variant.
- `Box<dyn Any>` is a code smell. Use it only with a written justification.

## Ownership and borrowing

Borrow by default. Parameters take `&str` over `String`, `&[T]` over `Vec<T>`, `&Path` over `PathBuf`. Return owned values when ownership transfer is the point; otherwise return a borrow tied to an input lifetime.
`.clone()` is a deliberate choice with a reason behind it, not a way to silence the borrow checker. Repeated `.clone()` calls in one function indicate a wrong data model; reconsider, and reshape it.
`Cow<'_, T>` is reserved for cases where both the borrowed and the owned path are actually taken; do not introduce it speculatively.

## Mutability

Immutable by default. `mut` is a deliberate signal that the binding will change.
Interior mutability (`Cell`, `RefCell`, `Mutex`, `RwLock`, `OnceCell`) is for cases where shared mutation is genuinely required, not for convenience. Each use needs a one-line justification of what is shared and what guarantees safety (single-threaded module, lock discipline, init-once semantics).

## Module organisation

Small files, one responsibility each. When a file accumulates two unrelated jobs, split it.
`pub(crate)` is the default visibility for anything wider than a single module; `pub` is reserved for deliberate crate-boundary surface. No `pub use` re-export shims kept around "just in case"; if a path moves, callers move with it.

## Logging

Logging goes through the `tracing` crate; do not use `println!`/`eprintln!` outside of binaries' top-level error paths or test scaffolding.

- `trace` for fine-grained diagnostic detail (per-token, per-symbol, per-event).
- `debug` for developer-relevant events during normal dev work.
- `info` and above are visible in production. Use sparingly: an `info!` per request is fine, an `info!` per loop iteration is not.

Each log site uses structured fields (`tracing::debug!(path = %p, "loaded")`), not interpolated strings.

## Design: read, compute, cache

Separate the three phases. Read functions perform I/O and are fallible. Compute functions are pure, take borrowed inputs, and return owned outputs; they are the testable core. Cache holds the memoised result of compute against a known input identity.
Do not interleave them: a function that reads a file, parses it, and stores the result in a global map at once is three responsibilities pretending to be one, and is much harder to test, retry, or invalidate correctly.

## Caches update, do not rebuild

When an input changes, prefer applying a delta to the cached value over discarding it and recomputing from scratch. The delta must be well-defined and demonstrably cheaper than the rebuild for the rule to apply.
Document the invariant the update preserves: a one-line comment naming what the cache holds and what the update keeps true. A cache whose invariant is not written down will, eventually, drift.

Existing caches in this workspace use fingerprint-driven invalidate-and-rebuild; treat that as the legacy default. New caches default to delta updates, and existing rebuild-style caches may be converted where appropriate. Some (the CST diagnostics cache is a likely example) may stay invalidate-and-rebuild because tracking incremental deltas across a parsed-tree change is not cheaper than recomputing. The per-cache assessment is deliberately out of scope for this guide; the standard sets the direction, not the schedule.

## Performance

Always take a small, clean change that wins on data-structure choice or algorithmic complexity. The canonical example: a `Vec<T>` followed by `.iter().find(|x| x.id == k)` is an O(n) scan; if the access pattern is by key, a `HashMap<K, T>` is one line different and is O(1). Take the lookup.
Do not optimise speculatively, and do not let perf concerns drive the *shape* of the code (see [Iterators vs loops](#iterators-vs-loops)); but a clean structural win at the same readability cost is not optional.

## Testing

Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they cover. Integration tests live under `tests/` at the crate root.
Test names describe the behaviour being asserted: `returns_none_when_input_empty`, `errors_on_missing_field`, not `test_foo_1` or `it_works`. Use `assert_eq!`/`assert!` with a message naming the failed expectation. Snapshot tests are appropriate only when the output is both large *and* stable; do not snapshot a value you could write a direct assertion for.

## Async and concurrency

Keep `async` at the edges, sync at the core. Network and FS boundaries are `async`; the analysis and resolution code they call is plain functions. Do not mark the whole call tree `async` without justification.
One runtime per binary, constructed at the top of `main`. Library crates do not pick a runtime.
`Send + Sync` is the default bound for shared state; narrow it only with justification.
Prefer message passing (`tokio::sync::mpsc`, `oneshot`) over shared `Mutex` when ownership can move; reach for a lock only when shared mutable state genuinely cannot be avoided. Request-handler architectures with workspace-wide state read and mutated by many concurrent handlers (e.g. the LSP backend) are the canonical exception: handlers do not own slices of state, they query and mutate by identity, and locks are the appropriate primitive there.
When a lock is warranted, prefer `parking_lot::Mutex`/`parking_lot::RwLock` over the `std::sync` equivalents: no poisoning, no `LockResult` to unwrap, and faster in practice.

## Public-API conventions

`pub` struct fields are rare; types expose constructors and methods, not raw state, so internal invariants are enforceable (`C-STRUCT-PRIVATE`).
Mark extension-prone public enums and structs `#[non_exhaustive]`.
Use the sealed-trait pattern (`C-SEALED`) for traits not meant for downstream implementation.
Use the builder pattern only when a type has many optional fields *and* construction order or validation matters; for two or three fields, a direct constructor reads better.

## Principles

- Guard clauses over nested `if`.
- Explicit over implicit; no truthiness coercion (`if value` is not Rust, but the principle holds: prefer `value.is_some()` over `let Some(_) = value`).
- `const` and immutable bindings by default; mutate deliberately.
- Narrow types: discriminated `enum`s over structs of optional fields.
- `Result` and `Option` at boundaries, narrowed inward. No `unwrap`-into-`unwrap` chains.
- Validate external input at the boundary; trust internal callers.
- Name magic values as `const`.
- Errors fail loud; no silent discard.
- Small files, one responsibility each.
- No dense one-liners; clarity beats brevity.

## General

No premature abstractions. Three similar lines beats an abstraction that is not yet justified by its use.
No backwards-compatibility shims, `_unused` renames, or `// removed` placeholder comments for deleted code; if it is gone, it is gone.
Do not design for hypothetical future requirements unless explicitly instructed to.
