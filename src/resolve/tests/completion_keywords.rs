use super::super::class_body_keyword_completions;
use super::make_doc;
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

fn kw(doc: &ParsedDocument, line: u32, character: u32) -> Vec<&'static str> {
    class_body_keyword_completions(doc, SourcePosition { line, character })
}

#[test]
fn class_body_kw_blank_position_offers_all_keywords() {
    // Cursor on an otherwise empty line inside a class body.
    let source = "class CExample {\n  \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 2);
    for expected in &[
        "var", "function", "event", "autobind", "private", "public", "import", "editable", "saved",
        "default", "defaults", "hint",
    ] {
        assert!(
            result.contains(expected),
            "blank class body should offer '{expected}'"
        );
    }
}

#[test]
fn class_body_kw_blank_after_function_block_offers_all_keywords() {
    // Blank line after a complete function body — must offer all class-body keywords.
    let source = "class CExample {\n  function Foo() {\n  }\n  \n}\n";
    let doc = make_doc(source);
    // Line 3, col 2 is on the blank line after the closing `}` of the function.
    let result = kw(&doc, 3, 2);
    for expected in &[
        "var", "function", "event", "private", "editable", "defaults",
    ] {
        assert!(
            result.contains(expected),
            "blank line after function body should offer '{expected}'"
        );
    }
}

#[test]
fn class_body_kw_partial_word_still_offers_keywords() {
    // Typing `p` — partial prefix of `private/protected/public`. The ident is not a
    // recognised specifier but also not a decl keyword; the server should still offer
    // all class-body keywords so the client can do prefix filtering.
    let source = "class C {\n  p\n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 3);
    for expected in &["var", "function", "private", "protected", "public"] {
        assert!(
            result.contains(expected),
            "partial identifier should not suppress class-body keywords: missing '{expected}'"
        );
    }
}

#[test]
fn class_body_kw_after_complete_specifier_plus_space_offers_filtered_keywords() {
    // `public ` — `public` is fully typed and parsed as a specifier inside an ERROR
    // node, cursor is in trailing whitespace past the ERROR boundary.
    let source = "class C {\n  public \n}\n";
    let doc = make_doc(source);
    // col 9 = one past the trailing space after `public`
    let result = kw(&doc, 1, 9);
    assert!(
        result.contains(&"var"),
        "should still offer var after public"
    );
    assert!(!result.contains(&"public"), "should not re-offer public");
    assert!(
        !result.contains(&"private"),
        "should not offer private after an access modifier"
    );
}

#[test]
fn class_body_kw_specifier_followed_by_valid_statement_still_offers_filtered_keywords() {
    // When tree-sitter error-recovers `public ` into a single member_var_decl with the
    // following line, the cursor inside that node must still see the specifier prefix.
    let source = "class C {\n  public \n  public var valid : bool;\n}\n";
    let doc = make_doc(source);
    // col 9 = after `  public ` on line 1
    let result = kw(&doc, 1, 9);
    assert!(result.contains(&"var"), "should offer var after public");
    assert!(!result.contains(&"public"), "should not re-offer public");
    assert!(
        !result.contains(&"private"),
        "should not offer private after access modifier"
    );
}

#[test]
fn class_body_kw_not_offered_inside_func_block() {
    let source = "class CExample {\n  function Foo() {\n    \n  }\n}\n";
    let doc = make_doc(source);
    // Line 2, col 4 is inside the function body — no class-body keywords.
    let result = kw(&doc, 2, 4);
    assert!(
        result.is_empty(),
        "must not offer class-body keywords inside a func_block"
    );
}

#[test]
fn class_body_kw_not_offered_outside_class() {
    let source = "function Foo() {}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 0, 0);
    assert!(
        result.is_empty(),
        "must not offer class-body keywords at top level"
    );
}

#[test]
fn class_body_kw_after_access_modifier_offers_decl_and_remaining_specifiers() {
    // "  private " — access seen, cursor after the space.
    let source = "class CExample {\n  private \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 10);
    assert!(
        result.contains(&"var"),
        "should offer var after access modifier"
    );
    assert!(
        result.contains(&"function"),
        "should offer function after access modifier"
    );
    assert!(
        result.contains(&"autobind"),
        "should offer autobind after access modifier"
    );
    assert!(
        result.contains(&"final"),
        "should offer final after access modifier"
    );
    assert!(
        result.contains(&"latent"),
        "should offer latent after access modifier"
    );
    assert!(
        result.contains(&"editable"),
        "should offer editable after access modifier"
    );
    assert!(!result.contains(&"private"), "should not re-offer private");
    assert!(
        !result.contains(&"import"),
        "should not offer import after access modifier"
    );
}

#[test]
fn class_body_kw_after_editable_suppresses_func_keywords_and_const() {
    let source = "class CExample {\n  editable \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 10);
    assert!(result.contains(&"var"), "should offer var after editable");
    assert!(
        result.contains(&"saved"),
        "should offer saved after editable"
    );
    assert!(
        result.contains(&"inlined"),
        "should offer inlined after editable"
    );
    assert!(!result.contains(&"const"), "const cannot follow editable");
    assert!(
        !result.contains(&"function"),
        "function invalid after editable"
    );
    assert!(!result.contains(&"final"), "final invalid after editable");
    assert!(!result.contains(&"latent"), "latent invalid after editable");
    assert!(
        !result.contains(&"autobind"),
        "autobind invalid after editable"
    );
    assert!(
        !result.contains(&"private"),
        "access cannot follow editable"
    );
    assert!(!result.contains(&"public"), "access cannot follow editable");
}

#[test]
fn class_body_kw_saved_is_terminal_no_more_var_specifiers() {
    let source = "class CExample {\n  saved \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 8);
    assert!(result.contains(&"var"), "should offer var after saved");
    assert!(
        !result.contains(&"editable"),
        "editable cannot follow saved"
    );
    assert!(!result.contains(&"const"), "const cannot follow saved");
    assert!(!result.contains(&"inlined"), "inlined cannot follow saved");
    assert!(!result.contains(&"private"), "access cannot follow saved");
    assert!(!result.contains(&"public"), "access cannot follow saved");
}

#[test]
fn class_body_kw_after_access_and_saved_no_further_var_specifiers() {
    let source = "class CExample {\n  public saved \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 14);
    assert!(result.contains(&"var"), "should offer var");
    assert!(!result.contains(&"inlined"), "inlined cannot follow saved");
    assert!(!result.contains(&"const"), "const cannot follow saved");
    assert!(
        !result.contains(&"editable"),
        "editable cannot follow saved"
    );
}

#[test]
fn class_body_kw_after_const_only_offers_inlined() {
    let source = "class CExample {\n  const \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 8);
    assert!(result.contains(&"var"), "should offer var after const");
    assert!(
        result.contains(&"inlined"),
        "should offer inlined after const"
    );
    assert!(
        !result.contains(&"editable"),
        "editable cannot follow const"
    );
    assert!(!result.contains(&"saved"), "saved cannot follow const");
}

#[test]
fn class_body_kw_after_final_suppresses_var_and_autobind() {
    let source = "class CExample {\n  final \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 8);
    assert!(
        result.contains(&"function"),
        "should offer function after final"
    );
    assert!(
        result.contains(&"latent"),
        "should offer latent after final"
    );
    assert!(!result.contains(&"var"), "var invalid after final");
    assert!(
        !result.contains(&"autobind"),
        "autobind invalid after final"
    );
    assert!(
        !result.contains(&"editable"),
        "editable invalid after final"
    );
    assert!(!result.contains(&"private"), "access cannot follow final");
    assert!(!result.contains(&"public"), "access cannot follow final");
}

#[test]
fn class_body_kw_after_optional_no_access_only_autobind() {
    let source = "class CExample {\n  optional \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 11);
    assert!(
        result.contains(&"autobind"),
        "should offer autobind after optional"
    );
    assert!(
        !result.contains(&"private"),
        "access cannot follow optional"
    );
    assert!(
        !result.contains(&"protected"),
        "access cannot follow optional"
    );
    assert!(!result.contains(&"public"), "access cannot follow optional");
    assert!(!result.contains(&"var"), "var invalid after optional");
    assert!(
        !result.contains(&"function"),
        "function invalid after optional"
    );
}

#[test]
fn class_body_kw_after_import_suppresses_var_group_and_autobind() {
    let source = "class CExample {\n  import \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 9);
    assert!(result.contains(&"var"), "should offer var after import");
    assert!(
        result.contains(&"function"),
        "should offer function after import"
    );
    assert!(
        result.contains(&"private"),
        "should offer access after import"
    );
    assert!(result.contains(&"final"), "should offer final after import");
    assert!(!result.contains(&"import"), "should not re-offer import");
    assert!(
        !result.contains(&"autobind"),
        "autobind invalid after import"
    );
    assert!(
        !result.contains(&"editable"),
        "editable invalid after import"
    );
}

#[test]
fn class_body_kw_after_decl_keyword_returns_empty() {
    // Once "var" has been typed the specifier phase is over.
    let source = "class CExample {\n  private var \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 14);
    assert!(
        result.is_empty(),
        "no keyword completions after a declaration keyword"
    );
}

#[test]
fn class_body_kw_struct_does_not_offer_function_or_autobind() {
    let source = "struct SData {\n  \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 2);
    assert!(result.contains(&"var"), "struct should offer var");
    assert!(
        !result.contains(&"function"),
        "struct must not offer function"
    );
    assert!(
        !result.contains(&"autobind"),
        "struct must not offer autobind"
    );
    assert!(!result.contains(&"event"), "struct must not offer event");
    assert!(!result.contains(&"final"), "struct must not offer final");
}

#[test]
fn class_body_kw_state_offers_same_as_class() {
    let source = "state SIdle in CPlayer {\n  \n}\n";
    let doc = make_doc(source);
    let result = kw(&doc, 1, 2);
    for expected in &["var", "function", "event", "autobind", "private", "final"] {
        assert!(
            result.contains(expected),
            "state body should offer '{expected}'"
        );
    }
}
