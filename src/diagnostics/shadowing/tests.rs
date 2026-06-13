use super::collect_shadowing_diagnostics;
use crate::line_index::{SourcePosition, SourceRange};
use crate::script_env::{ScriptEnvironment, ScriptGlobal};
use crate::symbols::{AccessLevel, Specifiers, Symbol, SymbolId, SymbolKind};
use crate::test_support::TestDb;
use crate::types::Type;

fn env(names_and_types: &[(&str, &str)]) -> ScriptEnvironment {
    let globals = names_and_types
        .iter()
        .map(|(name, ty)| ScriptGlobal {
            name: name.to_string(),
            type_name: ty.to_string(),
            ini_uri: "file:///redscripts.ini".to_string(),
            symbol: Symbol {
                id: SymbolId(0),
                name: name.to_string(),
                kind: SymbolKind::Variable,
                range: dummy_range(),
                selection_range: dummy_range(),
                byte_range: 0..0,
                selection_byte_range: 0..0,
                container: None,
                container_name: None,
                type_annotation: Some(Type::from_annotation(ty)),
                base_class: None,
                owner_class: None,
                flavour: None,
                annotations: Vec::new(),
                access: AccessLevel::Public,
                specifiers: Specifiers::default(),
            },
        })
        .collect();
    ScriptEnvironment::new(globals)
}

fn dummy_range() -> SourceRange {
    SourceRange {
        start: SourcePosition {
            line: 0,
            character: 0,
        },
        end: SourcePosition {
            line: 0,
            character: 0,
        },
    }
}

#[test]
fn param_shadows_script_global() {
    let t = TestDb::new("function F(thePlayer : CR4Player) {}\n");
    let env = env(&[("thePlayer", "CR4Player")]);

    let result = collect_shadowing_diagnostics(&t.workspace, &env);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].kind, "shadows_script_global");
    assert!(a[0].message.contains("thePlayer"));
    assert_eq!(a[0].related.len(), 1);
    assert_eq!(a[0].related[0].uri, "file:///redscripts.ini");
}

#[test]
fn local_shadows_script_global() {
    let t = TestDb::new("function F() {\n  var theGame : CR4Game;\n}\n");
    let env = env(&[("theGame", "CR4Game")]);

    let result = collect_shadowing_diagnostics(&t.workspace, &env);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].kind, "shadows_script_global");
    assert!(a[0].message.contains("theGame"));
}

#[test]
fn local_shadows_enclosing_class_field() {
    let t = TestDb::new("class C {\n  var x : int;\n  function F() {\n    var x : int;\n  }\n}\n");
    let env = env(&[]);

    let result = collect_shadowing_diagnostics(&t.workspace, &env);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].kind, "shadows_class_field");
    assert!(a[0].message.contains("Local 'x'"));
    assert!(a[0].message.contains('C'));
}

#[test]
fn field_shadows_script_global() {
    let t = TestDb::new("class C {\n  var thePlayer : CR4Player;\n}\n");
    let env = env(&[("thePlayer", "CR4Player")]);

    let result = collect_shadowing_diagnostics(&t.workspace, &env);

    let a = result.get(t.primary_uri()).expect("primary file flagged");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].kind, "shadows_script_global");
    assert!(a[0].message.contains("Field 'thePlayer'"));
}

#[test]
fn wrap_method_exempt() {
    let t = TestDb::new(
        "@wrapMethod(CR4Player)\nfunction GetTarget(thePlayer : CR4Player) {\n  var theGame : CR4Game;\n}\n",
    );
    let env = env(&[("thePlayer", "CR4Player"), ("theGame", "CR4Game")]);

    let result = collect_shadowing_diagnostics(&t.workspace, &env);

    assert!(result.is_empty(), "@wrapMethod must suppress shadowing");
}

#[test]
fn replace_method_exempt() {
    let t =
        TestDb::new("@replaceMethod(CR4Player)\nfunction GetTarget(thePlayer : CR4Player) {}\n");
    let env = env(&[("thePlayer", "CR4Player")]);

    let result = collect_shadowing_diagnostics(&t.workspace, &env);

    assert!(result.is_empty(), "@replaceMethod must suppress shadowing");
}

#[test]
fn clean_no_warnings() {
    let t = TestDb::new(
        "class C {\n  var y : int;\n  function F(z : int) {\n    var w : int;\n  }\n}\n",
    );
    let env = env(&[("thePlayer", "CR4Player")]);

    assert!(collect_shadowing_diagnostics(&t.workspace, &env).is_empty());
}

#[test]
fn does_not_warn_when_local_matches_unrelated_class_field() {
    let t = TestDb::new("class Other {\n  var x : int;\n}\nfunction F() {\n  var x : int;\n}\n");
    let env = env(&[]);

    assert!(
        collect_shadowing_diagnostics(&t.workspace, &env).is_empty(),
        "local in a top-level function is not shadowing an unrelated class's field"
    );
}
