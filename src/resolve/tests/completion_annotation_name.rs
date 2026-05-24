use super::super::annotation_name_completions;
use super::make_doc;
use crate::line_index::SourcePosition;

#[test]
fn annotation_name_completions_gate() {
    for (label, source, line, character, fires) in [
        ("typing partial name @w", "@w\n", 0u32, 2u32, true),
        (
            "inside annotation parens",
            "@wrapMethod(CPlayer)\n",
            0,
            12,
            false,
        ),
        (
            "inside string literal",
            "function F() { var x : string = \"hello@world\"; }",
            0,
            39,
            false,
        ),
        // `@` at byte 27 (char 27) inside a malformed function (outer ERROR contains `{`).
        // Cursor at char 28. Gate must not fire even though outer ERROR is a direct child of script.
        (
            "inside function body",
            "function a(){var b:string=\"@",
            0,
            28,
            false,
        ),
        (
            "inside function body",
            "function a(){var b:string=\"@}",
            0,
            14,
            false,
        ),
        (
            "bare @ between class decls",
            "\nclass a{\n\t\n}\n@\nclass b{function c(){}}",
            4,
            1,
            true,
        ),
        // `a@` — outer ERROR has two children (ident + inner ERROR); must not fire.
        // Cursor at char 2 (byte 2, immediately after `@` at byte 1).
        ("identifier immediately before @", "a@", 0, 2, false),
    ] {
        let doc = make_doc(source);
        let result = annotation_name_completions(&doc, SourcePosition { line, character });
        if fires {
            assert!(result.is_some(), "{label}: expected gate to fire");
        } else {
            assert!(result.is_none(), "{label}: expected gate not to fire");
        }
    }
}

#[test]
fn annotation_name_completions_fires_on_bare_at_sign() {
    // Bare `@` parses as ERROR/ERROR (no annotation_ident child).
    // Cursor is at character 1 (byte 1, immediately after `@` at bytes 0..1).
    // Gate must still fire and return the position of `@` (line 0, character 0).
    let source = "@\n";
    let doc = make_doc(source);

    let at_pos = annotation_name_completions(
        &doc,
        SourcePosition {
            line: 0,
            character: 1,
        },
    );
    assert!(at_pos.is_some(), "should fire on bare @");
    let pos = at_pos.unwrap();
    assert_eq!(pos.line, 0, "@ position line");
    assert_eq!(pos.character, 0, "@ position character");
}
