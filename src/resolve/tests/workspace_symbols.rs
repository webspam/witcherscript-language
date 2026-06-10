use super::make_doc;
use crate::resolve::{WorkspaceIndex, workspace_symbols};
use crate::test_support::{TestDb, def_names};

const LIMIT: usize = 256;

#[test]
fn finds_top_level_symbols_across_files() {
    let t = TestDb::new(concat!(
        "//- /a.ws\n",
        "class FooBar {}\n",
        "//- /b.ws\n",
        "function FooBaz() {}\n",
    ));

    let results = workspace_symbols(&[&t.workspace], "Foo", LIMIT);
    let names = def_names(&results);

    assert!(names.contains(&"FooBar"), "should find class: {names:?}");
    assert!(names.contains(&"FooBaz"), "should find function: {names:?}");
}

#[test]
fn finds_member_with_container_name() {
    let t = TestDb::new("class Foo {\n  function DoThing() {}\n  var theField : int;\n}\n");

    let method = workspace_symbols(&[&t.workspace], "DoThing", LIMIT);
    assert_eq!(def_names(&method), vec!["DoThing"]);
    assert_eq!(
        method[0].symbol.container_name.as_deref(),
        Some("Foo"),
        "method should carry its container name"
    );

    let field = workspace_symbols(&[&t.workspace], "theField", LIMIT);
    assert_eq!(def_names(&field), vec!["theField"], "fields are included");
    assert_eq!(field[0].symbol.container_name.as_deref(), Some("Foo"));
}

#[test]
fn finds_enum_member() {
    let t = TestDb::new("enum Direction { North, South }\n");

    let results = workspace_symbols(&[&t.workspace], "North", LIMIT);
    assert_eq!(def_names(&results), vec!["North"]);
    assert_eq!(
        results[0].symbol.container_name.as_deref(),
        Some("Direction")
    );
}

#[test]
fn excludes_locals_and_parameters() {
    let t = TestDb::new("function Run(theArg : int) {\n  var theLocal : int;\n}\n");

    assert!(
        workspace_symbols(&[&t.workspace], "theArg", LIMIT).is_empty(),
        "parameters must not appear in workspace symbols"
    );
    assert!(
        workspace_symbols(&[&t.workspace], "theLocal", LIMIT).is_empty(),
        "local variables must not appear in workspace symbols"
    );
}

#[test]
fn matching_is_case_insensitive() {
    let t = TestDb::new("class PlayerWitcher {}\n");

    let results = workspace_symbols(&[&t.workspace], "playerwitcher", LIMIT);
    assert_eq!(def_names(&results), vec!["PlayerWitcher"]);
}

#[test]
fn matches_subsequence() {
    let t = TestDb::new("function GetMyPlayer() {}\n");

    let results = workspace_symbols(&[&t.workspace], "GMP", LIMIT);
    assert_eq!(
        def_names(&results),
        vec!["GetMyPlayer"],
        "non-contiguous subsequence should match"
    );
}

#[test]
fn empty_query_returns_empty() {
    let t = TestDb::new("class Foo {}\nfunction Bar() {}\n");

    assert!(workspace_symbols(&[&t.workspace], "", LIMIT).is_empty());
    assert!(
        workspace_symbols(&[&t.workspace], "   ", LIMIT).is_empty(),
        "whitespace-only query is treated as empty"
    );
}

#[test]
fn ranks_exact_prefix_substring_then_subsequence() {
    let t = TestDb::new(concat!(
        "function Get() {}\n",
        "function GetPlayer() {}\n",
        "function TargetGet() {}\n",
        "function GreatExpectationsThing() {}\n",
    ));

    let results = workspace_symbols(&[&t.workspace], "get", LIMIT);
    assert_eq!(
        def_names(&results),
        vec!["Get", "GetPlayer", "TargetGet", "GreatExpectationsThing"],
        "exact > prefix > substring > subsequence"
    );
}

#[test]
fn respects_limit() {
    let t = TestDb::new(concat!(
        "function FaA() {}\n",
        "function FbB() {}\n",
        "function FcC() {}\n",
    ));

    let results = workspace_symbols(&[&t.workspace], "F", 2);
    assert_eq!(results.len(), 2, "result count is capped at the limit");
}

#[test]
fn earlier_index_tier_outranks_later_on_equal_score() {
    let mut workspace = WorkspaceIndex::default();
    workspace.update_document("file:///mod/a.ws", &make_doc("class Shared {}\n"));

    let mut base = WorkspaceIndex::default();
    base.update_document("file:///base/b.ws", &make_doc("class Shared {}\n"));

    let results = workspace_symbols(&[&workspace, &base], "Shared", LIMIT);

    assert_eq!(def_names(&results), vec!["Shared", "Shared"]);
    assert_eq!(
        results[0].uri, "file:///mod/a.ws",
        "workspace tier should rank before base tier on an equal-score match"
    );
    assert_eq!(results[1].uri, "file:///base/b.ws");
}
