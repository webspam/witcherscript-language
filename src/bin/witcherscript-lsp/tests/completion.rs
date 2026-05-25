use witcherscript_language::test_support::TestDb;

use crate::convert::completion_item;

#[test]
fn completion_item_method_has_method_kind() {
    use lsp_types::CompletionItemKind;
    use witcherscript_language::resolve::completion_members;

    let t = TestDb::new(concat!(
        "class CExample {\n",
        "  public function DoThing() {}\n",
        "}\n",
        "function Test() {\n",
        "  var e : CExample;\n",
        "  e.$0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let members = completion_members(&uri, t.doc_for(&uri), &t.db(), pos);

    assert!(!members.is_empty(), "should have completion members");
    let (_, def) = &members[0];
    let item = completion_item(def, &[]);
    assert_eq!(item.label, "DoThing");
    assert_eq!(item.kind, Some(CompletionItemKind::METHOD));
    assert_eq!(item.insert_text.as_deref(), Some("DoThing()"));
    assert!(
        item.command.is_none(),
        "paramless callable should not trigger parameter hints"
    );
}

#[test]
fn completion_item_snippet_includes_param_placeholders() {
    use lsp_types::{CompletionItemKind, InsertTextFormat};
    use witcherscript_language::resolve::completion_members;

    let t = TestDb::new(concat!(
        "class CExample {\n",
        "  public function Find(findName : string, range : float) : int {}\n",
        "}\n",
        "function Test() {\n",
        "  var e : CExample;\n",
        "  e.$0\n",
        "}\n",
    ));
    let (uri, pos) = t.cursor();
    let db = t.db();
    let members = completion_members(&uri, t.doc_for(&uri), &db, pos);

    let (_, find_def) = members
        .iter()
        .find(|(_, d)| d.symbol.name == "Find")
        .expect("Find should appear in completions");
    let params = db.parameters_of(&find_def.uri, find_def.symbol.id);
    let item = completion_item(find_def, &params);

    assert_eq!(item.kind, Some(CompletionItemKind::METHOD));
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
    assert_eq!(
        item.insert_text.as_deref(),
        Some("Find(${1:findName}, ${2:range})$0")
    );
    assert_eq!(
        item.command.as_ref().map(|c| c.command.as_str()),
        Some("editor.action.triggerParameterHints"),
        "callable with params should open signature help after insertion"
    );
}

mod builtin_source_request {
    use crate::backend::builtin_source_response;
    use async_lsp::ErrorCode;
    use witcherscript_language::builtins::BUILTIN_ARRAY_URI;

    #[test]
    fn returns_array_text_for_array_uri() {
        let response = builtin_source_response(BUILTIN_ARRAY_URI).expect("should succeed");
        let text = response
            .get("text")
            .and_then(|v| v.as_str())
            .expect("response has text field");
        assert!(text.contains("class array"));
        assert!(text.contains("PushBack"));
    }

    #[test]
    fn returns_null_for_unknown_uri() {
        let response = builtin_source_response("file:///not/a/builtin.ws").expect("should succeed");
        assert!(response.is_null());
    }

    #[test]
    fn errors_when_uri_is_empty() {
        let err = builtin_source_response("").expect_err("should reject empty uri");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }
}
