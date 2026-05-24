use super::super::annotation_arg_completions;
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;

#[test]
fn annotation_arg_completions_offers_classes_for_all_modding_annotations() {
    for (annotation, should_fire) in [
        ("@addField", true),
        ("@addMethod", true),
        ("@wrapMethod", true),
        ("@replaceMethod", true),
        ("@someUnknownAnnotation", false),
    ] {
        // Closed parens so tree-sitter emits a well-formed annotation node.
        let source = format!(
            "class CPlayer {{}}\n\
             struct SData {{}}\n\
             enum EDir {{ North = 0 }}\n\
             {annotation}(CPlayer)\n"
        );
        let doc = make_doc(&source);
        let mut index = WorkspaceIndex::default();
        index.update_document("file:///test.ws", &doc);

        // Cursor on the 'C' of 'CPlayer': past the annotation name and '('.
        let character = annotation.len() as u32 + 1;
        let completions = annotation_arg_completions(
            &doc,
            &SymbolDb::new(&index, &WorkspaceIndex::default()),
            SourcePosition { line: 3, character },
        );

        let names: Vec<&str> = completions.iter().map(|d| d.symbol.name.as_str()).collect();
        if should_fire {
            assert!(
                names.contains(&"CPlayer"),
                "{annotation}: class should be offered inside parens"
            );
            assert!(
                !names.contains(&"SData"),
                "{annotation}: struct should not be offered inside parens"
            );
            assert!(
                !names.contains(&"EDir"),
                "{annotation}: enum should not be offered inside parens"
            );
        } else {
            assert!(
                completions.is_empty(),
                "{annotation}: unknown annotation must not get class completion"
            );
        }
    }
}

#[test]
fn annotation_arg_completions_empty_outside_annotation() {
    let source = concat!("class CPlayer {}\n", "function Test() {}\n",);
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let completions = annotation_arg_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 1,
            character: 9,
        },
    );

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire outside an annotation"
    );
}

#[test]
fn annotation_arg_completions_empty_after_closing_paren() {
    // Cursor is after the ')' — should not offer anything.
    let source = "@wrapMethod(CPlayer) \n";
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///test.ws", &doc);

    let completions = annotation_arg_completions(
        &doc,
        &SymbolDb::new(&index, &WorkspaceIndex::default()),
        SourcePosition {
            line: 0,
            character: 21,
        },
    );

    assert!(
        completions.is_empty(),
        "annotation_arg_completions must not fire after the closing paren"
    );
}
