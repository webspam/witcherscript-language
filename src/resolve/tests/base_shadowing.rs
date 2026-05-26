use std::collections::HashSet;

use crate::document::parse_document;
use crate::line_index::SourcePosition;
use crate::resolve::{find_references, resolve_definition, SymbolDb, WorkspaceIndex};

#[test]
fn suppressed_base_stays_in_index_but_not_in_find_top_level() {
    let base_doc = parse_document("class CR4Player {}\n").expect("parse");
    let mod_doc = parse_document("class CR4Player {}\n// mod\n").expect("parse");
    let base_uri = "file:///game/content/content0/scripts/game/r4Player.ws";
    let mod_uri = "file:///mod/legacy/game/r4Player.ws";
    let mut base = WorkspaceIndex::default();
    let mut workspace = WorkspaceIndex::default();
    base.update_document(base_uri, &base_doc);
    workspace.update_document(mod_uri, &mod_doc);
    let mut suppressed = HashSet::new();
    suppressed.insert(base_uri.to_string());
    let db = SymbolDb::new(&workspace, &base).with_suppressed_base_uris(&suppressed);
    let def = resolve_definition(
        mod_uri,
        &mod_doc,
        &db,
        SourcePosition {
            line: 0,
            character: 6,
        },
    )
    .expect("class in mod");
    assert_eq!(def.uri, mod_uri);
    assert!(base.find_top_level("CR4Player").is_some());
}

#[test]
fn find_references_includes_shadowed_base_file() {
    let base_doc =
        parse_document("class CR4Player { function Foo() { IsCiri(); } }\n").expect("parse");
    let mod_doc = parse_document("class CR4Player {}\n").expect("parse");
    let base_uri = "file:///game/content/content0/scripts/game/r4Player.ws";
    let mod_uri = "file:///mod/legacy/game/r4Player.ws";
    let mut base = WorkspaceIndex::default();
    let mut workspace = WorkspaceIndex::default();
    base.update_document(base_uri, &base_doc);
    workspace.update_document(mod_uri, &mod_doc);
    let mut suppressed = HashSet::new();
    suppressed.insert(base_uri.to_string());
    let db = SymbolDb::new(&workspace, &base).with_suppressed_base_uris(&suppressed);
    let def = resolve_definition(
        mod_uri,
        &mod_doc,
        &db,
        SourcePosition {
            line: 0,
            character: 6,
        },
    )
    .expect("class in mod");
    let search = [(base_uri, &base_doc), (mod_uri, &mod_doc)];
    let refs = find_references(&def, &mod_doc, &search, &db, true);
    assert!(
        refs.iter().any(|(uri, _)| uri == base_uri),
        "references must include the shadowed vanilla base file, got {:?}",
        refs
    );
}

#[test]
fn find_references_skips_unrelated_idents_in_shadowed_base_file() {
    let base_doc = parse_document("class CR4Player { function Bar() { var CR4Player = 1; } }\n")
        .expect("parse");
    let mod_doc = parse_document("class CR4Player {}\n").expect("parse");
    let base_uri = "file:///game/content/content0/scripts/game/r4Player.ws";
    let mod_uri = "file:///mod/legacy/game/r4Player.ws";
    let mut base = WorkspaceIndex::default();
    let mut workspace = WorkspaceIndex::default();
    base.update_document(base_uri, &base_doc);
    workspace.update_document(mod_uri, &mod_doc);
    let mut suppressed = HashSet::new();
    suppressed.insert(base_uri.to_string());
    let db = SymbolDb::new(&workspace, &base).with_suppressed_base_uris(&suppressed);
    let def = resolve_definition(
        mod_uri,
        &mod_doc,
        &db,
        SourcePosition {
            line: 0,
            character: 6,
        },
    )
    .expect("class in mod");
    let search = [(base_uri, &base_doc), (mod_uri, &mod_doc)];
    let refs = find_references(&def, &mod_doc, &search, &db, false);
    let local_start = base_doc.source.find("var CR4Player").unwrap() + "var ".len();
    let local_end = local_start + "CR4Player".len();
    let local_range =
        base_doc
            .line_index
            .byte_range_to_range(&base_doc.source, local_start, local_end);
    assert!(
        !refs
            .iter()
            .any(|(uri, r)| uri == base_uri && *r == local_range),
        "unrelated locals in the shadowed base file must not count as references, got {:?}",
        refs
    );
}
