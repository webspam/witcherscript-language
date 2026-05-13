use super::super::resolve_definition;
use super::{make_doc, make_env, SymbolDb, WorkspaceIndex};
use crate::line_index::SourcePosition;
use crate::symbols::SymbolKind;

#[test]
fn script_global_resolves_to_ini_when_class_not_loaded() {
    let doc = make_doc("function Test() {\n theGame;\n}\n");
    let env = make_env("theGame", "CR4Game");
    let workspace = WorkspaceIndex::default();
    let base = WorkspaceIndex::default();
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&workspace, &base).with_script_env(&env),
        SourcePosition {
            line: 1,
            character: 2,
        },
    )
    .expect("should resolve to ini");
    assert_eq!(def.uri, "file:///redscripts.ini");
    assert_eq!(def.symbol.name, "theGame");
}

#[test]
fn script_global_redirects_to_class_when_loaded() {
    let doc = make_doc("function Test() {\n theGame;\n}\n");
    let class_doc = make_doc("class CR4Game {}\n");
    let env = make_env("theGame", "CR4Game");
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///r4game.ws", &class_doc);
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&WorkspaceIndex::default(), &base).with_script_env(&env),
        SourcePosition {
            line: 1,
            character: 2,
        },
    )
    .expect("should redirect to class");
    assert_eq!(def.symbol.name, "CR4Game");
    assert_eq!(def.uri, "file:///r4game.ws");
}

#[test]
fn member_access_on_script_global_resolves_method() {
    let doc = make_doc("function Test() {\n theGame.GetPlayer();\n}\n");
    let class_doc = make_doc("class CR4Game {\n public function GetPlayer() : CR4Player {}\n}\n");
    let env = make_env("theGame", "CR4Game");
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///r4game.ws", &class_doc);
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&WorkspaceIndex::default(), &base).with_script_env(&env),
        SourcePosition {
            line: 1,
            character: 11,
        },
    )
    .expect("GetPlayer should resolve");
    assert_eq!(def.symbol.name, "GetPlayer");
}

#[test]
fn local_var_with_same_name_as_script_global_resolves_to_local() {
    let doc = make_doc("function Test() {\n    var theGame : CR4Game;\n    theGame;\n}\n");
    let class_doc = make_doc("class CR4Game {}\n");
    let env = make_env("theGame", "CR4Game");
    let mut base = WorkspaceIndex::default();
    base.update_document("file:///r4game.ws", &class_doc);
    let def = resolve_definition(
        "file:///test.ws",
        &doc,
        &SymbolDb::new(&WorkspaceIndex::default(), &base).with_script_env(&env),
        SourcePosition {
            line: 2,
            character: 4,
        },
    )
    .expect("should resolve to local variable");
    assert_eq!(
        def.symbol.kind,
        SymbolKind::Variable,
        "expected local variable, not class"
    );
    assert_eq!(def.symbol.name, "theGame");
    assert_ne!(
        def.uri, "file:///r4game.ws",
        "should not redirect to class when a local shadows the global"
    );
}
