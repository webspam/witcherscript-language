use super::super::script_body_keyword_completions;
use super::make_doc;
use crate::document::ParsedDocument;
use crate::line_index::SourcePosition;

fn kw(doc: &ParsedDocument, line: u32, character: u32) -> Vec<&'static str> {
    script_body_keyword_completions(doc, SourcePosition { line, character })
}

#[test]
fn script_kw_blank_offers_all_starters() {
    let doc = make_doc("\n");
    let result = kw(&doc, 0, 0);
    for expected in &[
        "class",
        "state",
        "struct",
        "enum",
        "function",
        "var",
        "import",
        "abstract",
        "statemachine",
        "final",
        "latent",
        "cleanup",
        "entry",
        "exec",
        "quest",
        "reward",
        "storyscene",
        "timer",
    ] {
        assert!(
            result.contains(expected),
            "blank script scope should offer '{expected}', got {result:?}"
        );
    }
    for forbidden in &["private", "protected", "public"] {
        assert!(
            !result.contains(forbidden),
            "access modifiers are class-body only, must not appear at script scope"
        );
    }
}

#[test]
fn script_kw_blank_between_decls_offers_all() {
    let doc = make_doc("class A {}\n\nclass B {}\n");
    let result = kw(&doc, 1, 0);
    for expected in &["class", "function", "import", "final"] {
        assert!(
            result.contains(expected),
            "blank line between decls should offer '{expected}', got {result:?}"
        );
    }
    assert!(
        !result.contains(&"private"),
        "access modifiers are class-body only"
    );
}

#[test]
fn script_kw_not_offered_inside_class_body() {
    let doc = make_doc("class C {\n  \n}\n");
    let result = kw(&doc, 1, 2);
    assert!(
        result.is_empty(),
        "must not offer script-scope keywords inside a class body, got {result:?}"
    );
}

#[test]
fn script_kw_not_offered_inside_func_block() {
    let doc = make_doc("function F() {\n  \n}\n");
    let result = kw(&doc, 1, 2);
    assert!(
        result.is_empty(),
        "must not offer script-scope keywords inside a function body, got {result:?}"
    );
}

#[test]
fn script_kw_blank_offers_modding_annotations() {
    let doc = make_doc("\n");
    let result = kw(&doc, 0, 0);
    for expected in &["addField", "addMethod", "wrapMethod", "replaceMethod"] {
        assert!(
            result.contains(expected),
            "blank script scope should offer annotation '{expected}', got {result:?}"
        );
    }
}

#[test]
fn script_kw_annotations_not_offered_after_specifier() {
    for source in &["import \n", "final \n", "abstract \n", "statemachine \n"] {
        let doc = make_doc(source);
        let result = kw(&doc, 0, source.len() as u32 - 1);
        for forbidden in &["addField", "addMethod", "wrapMethod", "replaceMethod"] {
            assert!(
                !result.contains(forbidden),
                "annotations must not follow a specifier in `{source:?}`, got {result:?}"
            );
        }
    }
}

#[test]
fn script_kw_after_add_field_offers_access_modifiers() {
    let doc = make_doc("@addField(CName)\n\n");
    let result = kw(&doc, 1, 0);
    for expected in &["private", "protected", "public"] {
        assert!(
            result.contains(expected),
            "after @addField, access modifier '{expected}' must be offered, got {result:?}"
        );
    }
}

#[test]
fn script_kw_after_add_method_offers_access_modifiers() {
    let doc = make_doc("@addMethod(CName)\n\n");
    let result = kw(&doc, 1, 0);
    for expected in &["private", "protected", "public"] {
        assert!(
            result.contains(expected),
            "after @addMethod, access modifier '{expected}' must be offered, got {result:?}"
        );
    }
}

#[test]
fn script_kw_after_wrap_method_no_access_modifiers() {
    for source in &["@wrapMethod(CName)\n\n", "@replaceMethod(CName)\n\n"] {
        let doc = make_doc(source);
        let result = kw(&doc, 1, 0);
        for forbidden in &["private", "protected", "public"] {
            assert!(
                !result.contains(forbidden),
                "{source:?} does not inject a member declaration; access modifiers must not appear, got {result:?}"
            );
        }
    }
}

#[test]
fn script_kw_access_modifiers_gated_off_after_a_specifier() {
    let doc = make_doc("@addMethod(CName)\nfinal \n");
    let result = kw(&doc, 1, 6);
    for forbidden in &["private", "protected", "public"] {
        assert!(
            !result.contains(forbidden),
            "access modifiers must precede other specifiers, got {result:?}"
        );
    }
}

#[test]
fn script_kw_annotations_not_offered_inside_class_body() {
    let doc = make_doc("class C {\n  \n}\n");
    let result = kw(&doc, 1, 2);
    assert!(
        result.is_empty(),
        "annotations are script-scope only, must not appear in a class body, got {result:?}"
    );
}

#[test]
fn script_kw_after_import_filters_to_compatible_decls() {
    let doc = make_doc("import \n");
    let result = kw(&doc, 0, 7);
    for expected in &[
        "class",
        "state",
        "struct",
        "function",
        "abstract",
        "statemachine",
        "final",
        "latent",
    ] {
        assert!(
            result.contains(expected),
            "after import should offer '{expected}', got {result:?}"
        );
    }
    assert!(!result.contains(&"import"), "should not re-offer import");
    assert!(
        !result.contains(&"enum"),
        "enum takes no specifiers at script scope"
    );
    assert!(!result.contains(&"var"), "var takes no specifiers");
    for forbidden in &["private", "protected", "public"] {
        assert!(
            !result.contains(forbidden),
            "access modifiers are class-body only"
        );
    }
}

#[test]
fn script_kw_after_statemachine_only_class_path() {
    let doc = make_doc("statemachine \n");
    let result = kw(&doc, 0, 13);
    assert!(result.contains(&"class"), "statemachine → class");
    assert!(result.contains(&"abstract"), "statemachine abstract class");
    assert!(result.contains(&"import"), "statemachine import class");
    assert!(!result.contains(&"state"), "statemachine excludes state");
    assert!(!result.contains(&"struct"), "statemachine excludes struct");
    assert!(!result.contains(&"enum"), "statemachine excludes enum");
    assert!(
        !result.contains(&"function"),
        "statemachine excludes function"
    );
    assert!(!result.contains(&"var"), "statemachine excludes var");
    assert!(
        !result.contains(&"private"),
        "statemachine is not on the function path"
    );
}

#[test]
fn script_kw_after_abstract_offers_class_or_state() {
    let doc = make_doc("abstract \n");
    let result = kw(&doc, 0, 9);
    assert!(result.contains(&"class"));
    assert!(result.contains(&"state"));
    assert!(result.contains(&"import"));
    assert!(
        !result.contains(&"statemachine"),
        "corpus only has 'statemachine abstract', never 'abstract statemachine'"
    );
    assert!(!result.contains(&"struct"));
    assert!(!result.contains(&"function"));
    assert!(!result.contains(&"abstract"));
    assert!(!result.contains(&"private"));
}

#[test]
fn script_kw_after_final_drops_latent_ordering() {
    let doc = make_doc("final \n");
    let result = kw(&doc, 0, 6);
    assert!(result.contains(&"function"));
    assert!(result.contains(&"latent"));
    assert!(result.contains(&"timer"));
    assert!(!result.contains(&"final"), "should not re-offer final");
    assert!(
        !result.contains(&"private"),
        "access cannot follow final on this path"
    );
    assert!(!result.contains(&"class"));
}

#[test]
fn script_kw_after_latent_offers_flavours_and_function() {
    let doc = make_doc("latent \n");
    let result = kw(&doc, 0, 7);
    assert!(result.contains(&"function"));
    assert!(result.contains(&"quest"));
    assert!(result.contains(&"storyscene"));
    assert!(!result.contains(&"latent"), "should not re-offer latent");
    assert!(
        !result.contains(&"final"),
        "final must precede latent, not follow"
    );
    assert!(!result.contains(&"private"));
    assert!(!result.contains(&"class"));
}

#[test]
fn script_kw_after_flavour_only_function() {
    let doc = make_doc("timer \n");
    let result = kw(&doc, 0, 6);
    assert!(result.contains(&"function"));
    assert!(!result.contains(&"timer"), "no second flavour");
    assert!(!result.contains(&"quest"), "no second flavour");
    assert!(!result.contains(&"final"));
    assert!(!result.contains(&"latent"));
    assert!(!result.contains(&"private"));
}

#[test]
fn script_kw_after_decl_keyword_returns_empty() {
    let doc = make_doc("class \n");
    let result = kw(&doc, 0, 6);
    assert!(
        result.is_empty(),
        "no keyword completions after a decl keyword, got {result:?}"
    );
}

#[test]
fn script_kw_after_function_decl_keyword_returns_empty() {
    let doc = make_doc("function \n");
    let result = kw(&doc, 0, 9);
    assert!(
        result.is_empty(),
        "no keyword completions after `function`, got {result:?}"
    );
}

#[test]
fn script_kw_partial_word_still_offers_keywords() {
    let doc = make_doc("cl\n");
    let result = kw(&doc, 0, 2);
    for expected in &["class", "function", "import", "final"] {
        assert!(
            result.contains(expected),
            "partial identifier should not suppress script-scope keywords: missing '{expected}'"
        );
    }
    assert!(
        !result.contains(&"private"),
        "access modifiers are class-body only"
    );
}

#[test]
fn script_kw_offered_after_annotation_on_next_line() {
    let doc = make_doc("@addMethod(CPlayer)\n\n");
    let result = kw(&doc, 1, 0);
    for expected in &["function", "final", "latent"] {
        assert!(
            result.contains(expected),
            "should offer '{expected}' on the line following an annotation"
        );
    }
}

#[test]
fn script_kw_chain_statemachine_import_abstract_only_class() {
    let doc = make_doc("statemachine import abstract \n");
    let result = kw(&doc, 0, 29);
    assert!(result.contains(&"class"));
    assert!(!result.contains(&"state"));
    assert!(!result.contains(&"struct"));
    assert!(!result.contains(&"function"));
    assert!(!result.contains(&"statemachine"));
    assert!(!result.contains(&"abstract"));
    assert!(!result.contains(&"import"));
}

#[test]
fn script_kw_chain_import_final_only_function() {
    let doc = make_doc("import final \n");
    let result = kw(&doc, 0, 13);
    assert!(result.contains(&"function"));
    assert!(result.contains(&"latent"));
    assert!(result.contains(&"timer"));
    assert!(!result.contains(&"class"));
    assert!(!result.contains(&"final"));
    assert!(!result.contains(&"import"));
    assert!(
        !result.contains(&"private"),
        "access modifiers are class-body only"
    );
}
