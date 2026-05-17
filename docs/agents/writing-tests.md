# Writing tests

Style and structure guidelines for test code in this repo. For *where* each kind of test lives and what it covers, see [testing.md](testing.md).

## Prefer table-driven tests over copy/paste

When several tests share setup and only the input/expected output varies, write a single test with a structured input table and a loop — not N near-identical tests.

```rust
#[test]
fn classifies_identifier_kinds() {
    struct Case {
        name: &'static str,
        source: &'static str,
        expected: SymbolKind,
    }
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

**Each case must carry a unique label**, and every assertion inside the loop must include that label in its message. When the suite fails, the panic output must name which case failed — otherwise you are left guessing which of 12 inputs broke.

Use table-driven form when:

- Setup is identical across cases and only the input/expected output varies.
- You have three or more analogous cases (two is borderline — judge by clarity).
- The cases form a logical group ("operator precedence", "edge cases for empty input", "all the visibility modifiers").

Use separate tests when:

- Setup genuinely differs (different fixtures, different harness state).
- Assertion logic differs in a way that does not compress cleanly into one loop body.

## Do not copy/paste tests

Duplicated tests drift. Someone updates one case's assertion, forgets the others, and the suite quietly disagrees with itself. If you catch yourself duplicating a test to tweak one constant, parametrize it via the table-driven pattern above, or pull the shared setup into a helper. Copy/paste is acceptable only when the duplication is genuinely temporary and you delete it in the same change.
