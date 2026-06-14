use expect_test::expect;

use witcherscript_language::resolve::resolve_definition;
use witcherscript_language::test_support::TestDb;

use crate::convert::hover_markdown;

fn markdown_at_cursor(fixture: &str) -> String {
    markdown_at_cursor_with(fixture, false)
}

fn markdown_at_cursor_with(fixture: &str, compact_colon: bool) -> String {
    let t = TestDb::new(fixture);
    let (uri, pos) = t.cursor();
    let db = t.db();
    let def =
        resolve_definition(&uri, t.doc_for(&uri), &db, pos).expect("symbol must resolve at cursor");
    hover_markdown(&def, &db, compact_colon)
}

#[test]
fn formats_local_variable_as_markdown() {
    let actual = markdown_at_cursor(
        "//- /example.ws\n\
         function Make() {\n var dataObject : CScriptedFlashObject;\n $0dataObject = dataObject;\n}\n",
    );
    expect![[r"
        ```witcherscript
        var dataObject : CScriptedFlashObject
        ```

        Defined in [example.ws:2](file:///example.ws#L2)"]]
    .assert_eq(&actual);
    assert!(!actual.contains("Defined in file://"));
}

#[test]
fn formats_annotated_function_with_annotation_first() {
    let actual = markdown_at_cursor(
        "//- /fov.ws\n\
         @wrapMethod(CR4Player)\nfunction $0OnSpawned(spawnData : SEntitySpawnData) {\n}\n",
    );
    expect![[r"
        ```witcherscript
        @wrapMethod(CR4Player)
        function OnSpawned(spawnData : SEntitySpawnData)
        ```

        Defined in [fov.ws:2](file:///fov.ws#L2)"]]
    .assert_eq(&actual);
}

#[test]
fn formats_parameter_with_parenthesised_label() {
    let actual = markdown_at_cursor(
        "//- /example.ws\n\
         function Make(spawnData : SEntitySpawnData) {\n $0spawnData = spawnData;\n}\n",
    );
    expect![[r"
        ```witcherscript
        (parameter) spawnData : SEntitySpawnData
        ```

        Defined in [example.ws:1](file:///example.ws#L1)"]]
    .assert_eq(&actual);
}

#[test]
fn formats_method_with_owning_class_prefix() {
    let actual = markdown_at_cursor(
        "//- /example.ws\n\
         class CExample {\n public function $0DoThing(x : int) : bool {}\n}\n",
    );
    expect![[r"
        ```witcherscript
        (method) CExample.DoThing(x : int) : bool
        ```

        Defined in [example.ws:2](file:///example.ws#L2)"]]
    .assert_eq(&actual);
}

#[test]
fn formats_class_hover_with_extends_on_single_line() {
    let t = TestDb::new("class Y {}\nclass $0X extends Y {}\n");
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos).expect("class must resolve");
    let text = witcherscript_language::resolve::hover_text(&def, &t.db(), false);
    expect!["class X extends Y"].assert_eq(&text);
}

#[test]
fn inherited_method_hover_includes_defining_class_and_return_type() {
    let t = TestDb::new(
        "//- /a.ws\n\
         class A extends B {\n function Test() {\n  $0Inherited();\n }\n}\n\
         //- /b.ws\n\
         class B {\n public function Inherited() : int {}\n}\n",
    );
    let (uri, pos) = t.cursor();
    let def = resolve_definition(&uri, t.doc_for(&uri), &t.db(), pos)
        .expect("inherited method must resolve");
    let text = witcherscript_language::resolve::hover_text(&def, &t.db(), false);
    assert!(text.starts_with("(method) "), "got {text:?}");
    assert!(text.contains("B."), "got {text:?}");
    assert!(text.contains("Inherited"), "got {text:?}");
    assert!(text.contains("int"), "got {text:?}");
}

#[test]
fn field_hover_uses_compact_colon_when_enabled() {
    let actual = markdown_at_cursor_with(
        "//- /example.ws\n\
         class CExample {\n var $0count : int;\n}\n",
        true,
    );
    expect![[r"
        ```witcherscript
        (field) count: int
        ```

        Defined in [example.ws:2](file:///example.ws#L2)"]]
    .assert_eq(&actual);
}

#[test]
fn field_hover_includes_name_and_type() {
    let actual = markdown_at_cursor(
        "//- /example.ws\n\
         class CExample {\n protected editable var $0ignore : bool;\n}\n",
    );
    expect![[r"
        ```witcherscript
        (field) protected editable ignore : bool
        ```

        Defined in [example.ws:2](file:///example.ws#L2)"]]
    .assert_eq(&actual);
}
