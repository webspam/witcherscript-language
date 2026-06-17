# Writing tests

Style and structure guidelines for test code in this repo. For *where* each kind of test lives and what it covers, see [testing.md](testing.md).

## Prefer table-driven tests over copy/paste

When several tests share setup and only the input/expected output varies, write a single parametrized test - not N near-identical tests.

Two forms exist in this repo; both are accepted.

### Preferred: `#[rstest] #[case]`

```rust
use rstest::rstest;

#[rstest]
#[case::top_level_function("top-level function", "function f() {}", SymbolKind::Function)]
#[case::class("class", "class C {}", SymbolKind::Class)]
#[case::enumeration("enum", "enum E { A, B }", SymbolKind::Enum)]
fn classifies_identifier_kinds(
    #[case] name: &str,
    #[case] source: &str,
    #[case] expected: SymbolKind,
) {
    let got = classify(source);
    assert_eq!(got, expected, "case {}: classify mismatch", name);
}
```

Each `#[case::label(...)]` becomes its own entry in nextest output (`classifies_identifier_kinds::case_1_top_level_function`), so a single failing case isolates cleanly and the others keep running. Thread the human-readable label through as a `#[case]` parameter so assertion messages can still name the case explicitly.

### Acceptable: `struct Case` + for-loop

```rust
#[test]
fn classifies_identifier_kinds() {
    struct Case { name: &'static str, source: &'static str, expected: SymbolKind }
    let cases = [
        Case { name: "top-level function", source: "function f() {}", expected: SymbolKind::Function },
        Case { name: "class",              source: "class C {}",      expected: SymbolKind::Class },
        Case { name: "enum",               source: "enum E { A, B }", expected: SymbolKind::Enum },
    ];
    for c in cases {
        let got = classify(c.source);
        assert_eq!(got, c.expected, "case {}: classify mismatch", c.name);
    }
}
```

The first failing case short-circuits the rest. Still better than copy/paste; use when the file is already on this pattern or when rstest's macro surface is awkward.

### Rule

**Each case must carry a unique label**, and every assertion must include that label in its message. When the suite fails, the panic output must name which case failed - otherwise you are left guessing which of 12 inputs broke. (rstest's case names cover this for free; the for-loop pattern does it via `c.name` in the message.)

## Use the shared test toolkit

`src/test_support/` holds the canonical helpers. Inside the library, `use crate::test_support::TestDb`; inside the LSP binary or integration tests, `use witcherscript_language::test_support::TestDb` (the `test-support` Cargo feature is on by default).

```rust
let t = TestDb::new("class CExample {\n  function $0Bar() {}\n}\n");
let (uri, pos) = t.cursor();
let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).unwrap();
```

- `$0` is the cursor marker (exactly one per fixture).
- `//- /path.ws` headers split a fixture into multiple virtual files. Without any `//-` header, the source lands under `file:///main.ws`.
- `//^^^ label` annotates a span on the previous content line; retrieve it with `t.span("label")`.
- Positions are UTF-16 code units (LSP-compatible).

Helpers in `test_support`:

- `TestDb::new(fixture_str)` - parses, indexes, exposes `db()`, `cursor()`, `span(label)`, `doc_for(uri)`, `primary_uri()`, `primary_doc()`, `search_docs()`.
- `def_names(&[Definition])` / `def_names_tiered(&[(u8, Definition)])` - extract `Vec<&str>` of symbol names.
- `assert_names_contain(actual, expected)` / `assert_names_exclude(actual, excluded)` - canonical membership assertions for completion-result name lists.

Prefer these over hand-rolling `parse_document` + `WorkspaceIndex` + `SymbolDb` scaffolds. The `make_doc` helper in `src/resolve/tests/mod.rs` remains available for low-level resolve tests, but `TestDb` is the default entry point.

## Inline snapshots: `expect-test`

For golden output (hover markdown, formatter output, decoded diagnostics), use `expect_test::expect!` so the expected value lives next to the test logic:

```rust
let actual = hover_markdown(&def);
expect![[r#"
    ```witcherscript
    var x : int
    ```

    Defined in [main.ws:0](file:///main.ws#L0)"#]]
.assert_eq(&actual);
```

When the formatter changes, regenerate every stale expectation in one go: `UPDATE_EXPECT=1 cargo test`.

For larger or structured snapshots (multi-symbol completion result vectors, full LSP responses) use `insta` instead - see its docs. We have both crates as dev-deps; pick the one that matches the output size.

## Markers, not magic numbers

When a test needs a cursor, use a `$0` marker in the source - never hand-counted `SourcePosition { line: N, character: M }` literals. A reader must not have to count characters to verify a test, and a 1-character edit to the source must not silently move the cursor onto the wrong token.

Exception: tests that read a fixture file from `tests/fixtures/` cannot embed `$0` (that would break the parser-fixture suite). For those, keep the hand-counted positions but pull them into a `for` loop or `#[rstest] #[case]` so the positions live alongside their human-readable labels.
