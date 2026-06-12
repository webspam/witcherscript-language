use super::super::resolve_definition;
use crate::symbols::SymbolKind;
use crate::test_support::{TestDb, script_env};

#[test]
fn definition_at_selection_resolves_script_global_by_name() {
    let t = TestDb::new("function Test() {}\n");
    let env = script_env("theGame", "CR4Game");
    let db = t.db().with_script_env(&env);
    let def = db
        .definition_at_selection("file:///redscripts.ini", &(0..7), "theGame")
        .expect("script global must resolve to its own symbol");
    assert_eq!(def.symbol.name, "theGame");
    assert_eq!(def.uri, "file:///redscripts.ini");
}

#[test]
fn script_global_resolves_to_ini_when_class_not_loaded() {
    let t = TestDb::new("function Test() {\n t$0heGame;\n}\n");
    let (uri, pos) = t.cursor();
    let env = script_env("theGame", "CR4Game");
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db().with_script_env(&env), pos)
        .expect("should resolve to ini");
    assert_eq!(def.uri, "file:///redscripts.ini");
    assert_eq!(def.symbol.name, "theGame");
}

#[test]
fn script_global_redirects_to_class_when_loaded() {
    let t = TestDb::new("function Test() {\n t$0heGame;\n}\n")
        .with_base_doc("file:///r4game.ws", "class CR4Game {}\n");
    let (uri, pos) = t.cursor();
    let env = script_env("theGame", "CR4Game");
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db().with_script_env(&env), pos)
        .expect("should redirect to class");
    assert_eq!(def.symbol.name, "CR4Game");
    assert_eq!(def.uri, "file:///r4game.ws");
}

#[test]
fn member_access_on_script_global_resolves_method() {
    let t = TestDb::new("function Test() {\n theGame.Ge$0tPlayer();\n}\n").with_base_doc(
        "file:///r4game.ws",
        "class CR4Game {\n public function GetPlayer() : CR4Player {}\n}\n",
    );
    let (uri, pos) = t.cursor();
    let env = script_env("theGame", "CR4Game");
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db().with_script_env(&env), pos)
        .expect("GetPlayer should resolve");
    assert_eq!(def.symbol.name, "GetPlayer");
}

#[test]
fn local_var_with_same_name_as_script_global_resolves_to_local() {
    let t = TestDb::new("function Test() {\n    var theGame : CR4Game;\n    $0theGame;\n}\n")
        .with_base_doc("file:///r4game.ws", "class CR4Game {}\n");
    let (uri, pos) = t.cursor();
    let env = script_env("theGame", "CR4Game");
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db().with_script_env(&env), pos)
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
