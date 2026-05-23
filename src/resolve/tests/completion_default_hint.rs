use super::super::default_or_hint_member_completions;
use super::{make_doc, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;

fn names_at(source: &str, line: u32, character: u32) -> Vec<String> {
    let doc = make_doc(source);
    let mut index = WorkspaceIndex::default();
    index.update_document("file:///t.ws", &doc);
    let base = WorkspaceIndex::default();
    let db = SymbolDb::new(&index, &base);
    default_or_hint_member_completions(&doc, &db, SourcePosition { line, character })
        .into_iter()
        .map(|d| d.symbol.name)
        .collect()
}

#[test]
fn offers_private_inherited_field_in_default_or_hint_position() {
    struct Case {
        name: &'static str,
        source: &'static str,
        line: u32,
        character: u32,
    }
    let cases = [
        Case {
            name: "default keyword",
            source: "class Super { private var hidden : int; }\n\
                     class Sub extends Super { default  = 1; }\n",
            line: 1,
            character: 34,
        },
        Case {
            name: "hint keyword",
            source: "class Super { private var hidden : int; }\n\
                     class Sub extends Super { hint  = \"tip\"; }\n",
            line: 1,
            character: 31,
        },
        Case {
            name: "defaults block",
            source: "class Super { private var hidden : int; }\n\
                     class Sub extends Super { defaults {  = 1; } }\n",
            line: 1,
            character: 37,
        },
    ];
    for c in cases {
        let names = names_at(c.source, c.line, c.character);
        assert!(
            names.iter().any(|n| n == "hidden"),
            "case {}: private inherited field should be offered, got {names:?}",
            c.name,
        );
    }
}

#[test]
fn does_not_offer_outside_default_or_hint_member_position() {
    struct Case {
        name: &'static str,
        source: &'static str,
        line: u32,
        character: u32,
    }
    let cases = [
        Case {
            name: "value position after `=`",
            source: "class A { var known : int; default known = ; }\n",
            line: 0,
            character: 43,
        },
        Case {
            name: "function body",
            source: "class A { var f : int; function R() {  } }\n",
            line: 0,
            character: 38,
        },
    ];
    for c in cases {
        let names = names_at(c.source, c.line, c.character);
        assert!(
            names.is_empty(),
            "case {}: should not trigger default-member completion, got {names:?}",
            c.name,
        );
    }
}
