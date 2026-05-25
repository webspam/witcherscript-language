use super::make_doc;
use crate::symbols::{AccessLevel, SymbolKind};
use crate::test_support::TestDb;

#[test]
fn all_top_level_yields_top_level_symbols_across_documents() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class Foo {\n",
        "  function Method() {}\n",
        "  var field : int;\n",
        "}\n",
        "//- /b.ws\n",
        "function Bar() {}\n",
    ));

    let mut found: Vec<(String, String)> = t
        .workspace
        .all_top_level()
        .map(|(uri, sym)| (uri.to_string(), sym.name.clone()))
        .collect();
    found.sort();

    assert_eq!(
        found,
        vec![
            ("file:///a.ws".to_string(), "Foo".to_string()),
            ("file:///b.ws".to_string(), "Bar".to_string()),
        ]
    );
}

#[test]
fn all_top_level_excludes_members() {
    let t = TestDb::new("class Foo {\n  function Method() {}\n  var field : int;\n}\n");

    assert!(
        t.workspace
            .all_top_level()
            .all(|(_, sym)| sym.container.is_none()),
        "all_top_level must not yield members"
    );
    assert!(
        t.workspace
            .all_top_level()
            .all(|(_, sym)| sym.kind != SymbolKind::Method && sym.kind != SymbolKind::Field),
        "all_top_level must not yield methods or fields"
    );
}

#[test]
fn generation_counter_bumps_on_mutation() {
    let doc_a = make_doc("class Foo {}\n");
    let doc_b = make_doc("class Bar {}\n");
    let mut index = crate::resolve::WorkspaceIndex::default();
    let g0 = index.generation();

    index.update_document("file:///a.ws", &doc_a);
    let g1 = index.generation();
    assert_ne!(g0, g1, "update_document should bump generation");

    index.update_document("file:///b.ws", &doc_b);
    let g2 = index.generation();
    assert_ne!(g1, g2, "second update should bump again");

    index.remove_document("file:///b.ws");
    let g3 = index.generation();
    assert_ne!(g2, g3, "remove_document should bump generation");
}

#[test]
fn surface_hash_stable_under_text_only_edits() {
    let original = make_doc("class A { function F() { var x : int; } }\n");
    let with_comment = make_doc("class A { function F() { var x : int; /* hi */ } }\n");
    let with_body_change = make_doc("class A { function F() { var x : int; x = 42; } }\n");

    let mut index = crate::resolve::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &original);
    let h0 = index.surface_hash();

    index.update_document("file:///a.ws", &with_comment);
    assert_eq!(
        h0,
        index.surface_hash(),
        "comment-only edit must not change surface hash"
    );

    index.update_document("file:///a.ws", &with_body_change);
    assert_eq!(
        h0,
        index.surface_hash(),
        "function-body edit must not change surface hash"
    );
}

#[test]
fn surface_hash_ignores_local_variable_and_parameter_churn() {
    let baseline = make_doc("class A { function F() { var x : int; } }\n");
    let mut index = crate::resolve::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &baseline);
    let h0 = index.surface_hash();

    let mid_typing_locals = [
        "class A { function F() { var x : int; var y : float; } }\n",
        "class A { function F() { var x : int; var y : float; var z : string; } }\n",
        "class A { function F() { var renamed : int; } }\n",
        "class A { function F() {} }\n",
    ];
    for src in mid_typing_locals {
        index.update_document("file:///a.ws", &make_doc(src));
        assert_eq!(
            h0,
            index.surface_hash(),
            "local-var churn should not change surface hash: {src}"
        );
    }
}

#[test]
fn surface_hash_changes_on_structural_edit() {
    struct Case {
        name: &'static str,
        before: &'static str,
        after: &'static str,
    }
    let cases = [
        Case {
            name: "rename class",
            before: "class A {}\n",
            after: "class B {}\n",
        },
        Case {
            name: "add method",
            before: "class A { function F() {} }\n",
            after: "class A { function F() {} function G() {} }\n",
        },
        Case {
            name: "change member access",
            before: "class A { public function F() {} }\n",
            after: "class A { private function F() {} }\n",
        },
        Case {
            name: "change base class",
            before: "class A extends Foo {}\n",
            after: "class A extends Bar {}\n",
        },
        Case {
            name: "change parameter signature",
            before: "function F(x : int) {}\n",
            after: "function F(x : float) {}\n",
        },
    ];
    for c in cases {
        let mut index = crate::resolve::WorkspaceIndex::default();
        index.update_document("file:///x.ws", &make_doc(c.before));
        let h_before = index.surface_hash();
        index.update_document("file:///x.ws", &make_doc(c.after));
        let h_after = index.surface_hash();
        assert_ne!(
            h_before, h_after,
            "case `{}`: surface hash should change",
            c.name
        );
    }
}

#[test]
fn surface_hash_does_not_self_cancel_for_identical_docs() {
    let doc = make_doc("class A {}\n");
    let mut index = crate::resolve::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &doc);
    index.update_document("file:///b.ws", &doc);
    assert_ne!(
        0,
        index.surface_hash(),
        "two identical-content docs under different URIs must not XOR to 0"
    );
}

#[test]
fn surface_hash_recovers_after_revert() {
    let original = make_doc("class A {}\n");
    let edited = make_doc("class B {}\n");
    let mut index = crate::resolve::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &original);
    let h0 = index.surface_hash();
    index.update_document("file:///a.ws", &edited);
    assert_ne!(h0, index.surface_hash());
    index.update_document("file:///a.ws", &original);
    assert_eq!(
        h0,
        index.surface_hash(),
        "reverting should restore prior surface hash"
    );
}

#[test]
fn surface_hash_returns_to_zero_after_removing_only_doc() {
    let doc = make_doc("class A {}\n");
    let mut index = crate::resolve::WorkspaceIndex::default();
    assert_eq!(0, index.surface_hash());
    index.update_document("file:///a.ws", &doc);
    assert_ne!(0, index.surface_hash());
    index.remove_document("file:///a.ws");
    assert_eq!(
        0,
        index.surface_hash(),
        "removing the only indexed doc should restore empty hash"
    );
}

#[test]
fn removing_duplicate_top_level_class_keeps_original_visible() {
    let mut t = TestDb::new(
        "\
//- /a.ws
class Foo {}
//- /b.ws
class Foo {}
",
    );

    t.workspace.remove_document("file:///b.ws");

    let def = t
        .workspace
        .find_top_level("Foo")
        .expect("Foo should still resolve to the original after duplicate removed");
    assert_eq!(def.uri, "file:///a.ws");
}

#[test]
fn updating_duplicate_out_of_existence_keeps_original_visible() {
    let mut t = TestDb::new(
        "\
//- /a.ws
class Foo {}
//- /b.ws
class Foo {}
",
    );
    let unrelated = make_doc("class Bar {}\n");

    t.workspace.update_document("file:///b.ws", &unrelated);

    let def = t
        .workspace
        .find_top_level("Foo")
        .expect("Foo should still resolve via the original after duplicate edited away");
    assert_eq!(def.uri, "file:///a.ws");
}

#[test]
fn removing_duplicate_class_restores_superclass_lookup() {
    let mut t = TestDb::new(
        "\
//- /a.ws
class Foo extends Base {}
//- /b.ws
class Foo extends Base {}
",
    );

    t.workspace.remove_document("file:///b.ws");

    assert_eq!(
        t.workspace.superclass_of("Foo").as_deref(),
        Some("Base"),
        "superclass lookup should survive removal of duplicate class"
    );
}

#[test]
fn removing_duplicate_class_member_keeps_original_member_visible() {
    let mut t = TestDb::new(
        "\
//- /a.ws
class Foo { function Bar() {} }
//- /b.ws
class Foo { function Bar() {} }
",
    );

    t.workspace.remove_document("file:///b.ws");

    let def = t
        .workspace
        .direct_member_of("Foo", "Bar", AccessLevel::Public)
        .expect("Bar member should still resolve to the original after duplicate removed");
    assert_eq!(def.uri, "file:///a.ws");
}

#[test]
fn completion_catalog_stable_on_local_var_edit() {
    let baseline = make_doc("class A { function F() { var x : int; } }\n");
    let mut index = crate::resolve::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &baseline);
    let callables = index.callables_catalog();
    let types = index.types_catalog();
    let variants = index.enum_variants_catalog();

    index.update_document(
        "file:///a.ws",
        &make_doc("class A { function F() { var x : int; var y : float; } }\n"),
    );

    assert!(std::sync::Arc::ptr_eq(
        &callables,
        &index.callables_catalog()
    ));
    assert!(std::sync::Arc::ptr_eq(&types, &index.types_catalog()));
    assert!(std::sync::Arc::ptr_eq(
        &variants,
        &index.enum_variants_catalog()
    ));
}

#[test]
fn completion_catalog_rebuilds_on_top_level_change() {
    let mut index = crate::resolve::WorkspaceIndex::default();
    index.update_document("file:///a.ws", &make_doc("function F() {}\n"));
    let before = index.callables_catalog();

    index.update_document("file:///a.ws", &make_doc("function G() {}\n"));
    let after = index.callables_catalog();

    assert!(!std::sync::Arc::ptr_eq(&before, &after));
    let names: Vec<_> = after.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains(&"G"));
    assert!(!names.contains(&"F"));
}

#[test]
fn merged_global_completions_matches_lsp_cache_globals_shape() {
    let t =
        crate::test_support::TestDb::new("function Caller() {\n  $0\n}\n").with_builtins_index();
    let env = super::make_env("theGame", "CR4Game");
    let db = t.db().with_script_env(&env);
    let globals = crate::resolve::merged_global_completions(&db);
    let names: std::collections::HashSet<&str> =
        globals.iter().map(|d| d.symbol.name.as_str()).collect();
    assert!(names.contains("theGame"));
    assert!(names.contains("AD_Front"));
}

#[test]
fn merged_enum_variants_catalog_includes_builtins() {
    let workspace = crate::resolve::WorkspaceIndex::default();
    let base = crate::resolve::WorkspaceIndex::default();
    let builtins = crate::builtins::load_builtins_index();
    let db = crate::resolve::SymbolDb::new(&workspace, &base).with_builtins(&builtins);
    let merged = db.merged_enum_variants_catalog();
    assert!(
        merged.iter().any(|d| d.symbol.name == "AD_Front"),
        "merged enum variants must include builtins; sample missing"
    );
}

#[test]
fn merged_callables_workspace_shadows_base_and_excludes_exec_quest() {
    let mut workspace = crate::resolve::WorkspaceIndex::default();
    workspace.update_document("file:///mod/ws", &make_doc("function WorkspaceFn() {}\n"));

    let mut base = crate::resolve::WorkspaceIndex::default();
    base.update_document(
        "file:///base/a.ws",
        &make_doc(
            "exec function DebugCmd() {}\n\
             quest function QuestFn() {}\n\
             function BaseFn() {}\n\
             function WorkspaceFn() {}\n",
        ),
    );

    let db = crate::resolve::SymbolDb::new(&workspace, &base);
    let merged = db.merged_callables_catalog();
    let names: Vec<_> = merged.iter().map(|d| d.symbol.name.as_str()).collect();

    assert!(names.contains(&"WorkspaceFn"));
    assert!(names.contains(&"BaseFn"));
    assert!(!names.contains(&"DebugCmd"));
    assert!(!names.contains(&"QuestFn"));

    let ws_fn = merged
        .iter()
        .find(|d| d.symbol.name == "WorkspaceFn")
        .expect("workspace function present");
    assert_eq!(ws_fn.uri, "file:///mod/ws");
}

#[test]
fn removing_duplicate_enum_variant_keeps_original_visible() {
    let mut t = TestDb::new(
        "\
//- /a.ws
enum E { V }
//- /b.ws
enum E { V }
",
    );

    t.workspace.remove_document("file:///b.ws");

    let def = t
        .workspace
        .find_enum_variant("V")
        .expect("Enum variant V should still resolve after duplicate enum removed");
    assert_eq!(def.uri, "file:///a.ws");
}
