use witcherscript_language::formatter::ColonSpacing;
use witcherscript_language::test_support::TestDb;

use crate::convert::completion_item;

#[test]
fn annotation_name_items_reopen_suggestions_for_class_name() {
    use crate::convert::annotation_name_items;

    let items = annotation_name_items();
    assert!(
        !items.is_empty(),
        "annotation name items should be produced"
    );
    for item in &items {
        assert_eq!(
            item.command.as_ref().map(|c| c.command.as_str()),
            Some("editor.action.triggerSuggest"),
            "{} should reopen suggestions for its class-name argument",
            item.label
        );
        assert!(
            item.text_edit.is_none(),
            "{} must insert via insert_text, not a replace-range that deletes the typed @",
            item.label
        );
        let insert = item
            .insert_text
            .as_deref()
            .unwrap_or_else(|| panic!("{} should carry insert_text", item.label));
        assert!(
            !insert.starts_with('@'),
            "{} must not re-insert the @ (would double it), got {insert:?}",
            item.label
        );
        assert!(
            insert.ends_with("($1)"),
            "{} must land the cursor in empty parens, got {insert:?}",
            item.label
        );
    }
}

#[test]
fn script_body_annotation_items_do_not_reinsert_the_at_sign() {
    use crate::convert::script_body_item;

    for label in ["@addField", "@addMethod", "@wrapMethod", "@replaceMethod"] {
        let item = script_body_item(label);
        let insert = item
            .insert_text
            .as_deref()
            .unwrap_or_else(|| panic!("{label} should carry insert_text"));
        assert!(
            !insert.starts_with('@'),
            "{label} must not re-insert the @ (the typed trigger @ stays), got {insert:?}"
        );
    }
}

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
    let origin: lsp_types::Url = uri.parse().expect("test uri parses");
    let item = completion_item(def, &t.db(), &origin, ColonSpacing::Spaced);
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
    let origin: lsp_types::Url = uri.parse().expect("test uri parses");
    let item = completion_item(find_def, &db, &origin, ColonSpacing::Spaced);

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

#[test]
fn completion_item_snippet_excludes_optional_params() {
    use witcherscript_language::resolve::completion_members;

    let t = TestDb::new(concat!(
        "class CExample {\n",
        "  public function Find(findName : string, optional range : float) : int {}\n",
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
    let origin: lsp_types::Url = uri.parse().expect("test uri parses");
    let item = completion_item(find_def, &db, &origin, ColonSpacing::Spaced);

    assert_eq!(
        item.insert_text.as_deref(),
        Some("Find(${1:findName})$0"),
        "optional parameter must not become a snippet slot"
    );
    assert_eq!(
        item.detail.as_deref(),
        Some("(findName : string, optional range : float) : int"),
        "detail must render the full parameter list"
    );
}

#[test]
fn completion_item_defers_documentation_to_resolve() {
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
    let origin: lsp_types::Url = uri.parse().expect("test uri parses");
    let item = completion_item(def, &t.db(), &origin, ColonSpacing::Spaced);
    assert!(
        item.documentation.is_none(),
        "documentation must defer to completionItem/resolve"
    );
    assert!(item.data.is_some(), "item must carry resolve data");
}

mod resolve {
    use std::sync::Arc;

    use arc_swap::ArcSwap;
    use async_lsp::ClientSocket;
    use async_lsp::router::Router;
    use lsp_types::{
        CompletionItem, DidOpenTextDocumentParams, Documentation, TextDocumentItem, Url,
    };
    use witcherscript_language::files::canonical_uri;

    use crate::backend::Backend;
    use crate::config::{Config, DiagnosticsScope};
    use crate::convert::CompletionItemData;

    const SOURCE: &str = "class CExample {\n  public function DoThing() {}\n}\n";

    fn opened_backend(uri: &Url) -> Backend {
        let (_main_loop, client) =
            async_lsp::MainLoop::new_server(|_client: ClientSocket| Router::<()>::new(()));
        let config = Arc::new(ArcSwap::from_pointee(Config {
            diagnostics_scope: DiagnosticsScope::None,
            ..Config::default()
        }));
        let backend = Backend::new(client, config);
        backend._did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "witcherscript".to_string(),
                version: 1,
                text: SOURCE.to_string(),
            },
        });
        backend
    }

    fn do_thing_data(uri: &Url, selection: std::ops::Range<usize>) -> CompletionItemData {
        CompletionItemData {
            origin: uri.clone(),
            def_uri: canonical_uri(uri),
            selection,
            name: "DoThing".to_string(),
            container: None,
        }
    }

    fn documentation_text(item: &CompletionItem) -> Option<&str> {
        match item.documentation.as_ref()? {
            Documentation::MarkupContent(markup) => Some(&markup.value),
            Documentation::String(s) => Some(s),
        }
    }

    #[test]
    fn resolve_fills_documentation_from_carried_data() {
        let uri: Url = "file:///main.ws".parse().unwrap();
        let backend = opened_backend(&uri);
        let start = SOURCE.find("DoThing").expect("fixture contains DoThing");
        let data = do_thing_data(&uri, start..start + "DoThing".len());

        let resolved = backend
            ._completion_item_resolve_blocking(CompletionItem::default(), &data)
            .expect("resolve succeeds");
        let text = documentation_text(&resolved).expect("resolve must fill documentation");
        assert!(
            text.contains("(method) CExample.DoThing()"),
            "documentation must carry the hover markdown, got {text:?}"
        );
    }

    #[test]
    fn resolve_falls_back_by_name_when_selection_is_stale() {
        let uri: Url = "file:///main.ws".parse().unwrap();
        let backend = opened_backend(&uri);
        let data = do_thing_data(&uri, 0..1);

        let resolved = backend
            ._completion_item_resolve_blocking(CompletionItem::default(), &data)
            .expect("resolve succeeds");
        assert!(
            documentation_text(&resolved).is_some_and(|text| text.contains("DoThing")),
            "stale selection must fall back to name lookup"
        );
    }

    #[test]
    fn resolve_returns_item_unchanged_when_symbol_is_gone() {
        let uri: Url = "file:///main.ws".parse().unwrap();
        let backend = opened_backend(&uri);
        let data = CompletionItemData {
            name: "NoSuchSymbol".to_string(),
            ..do_thing_data(&uri, 0..1)
        };

        let resolved = backend
            ._completion_item_resolve_blocking(CompletionItem::default(), &data)
            .expect("resolve succeeds");
        assert!(
            resolved.documentation.is_none(),
            "missing symbol must leave the item without documentation"
        );
    }

    #[tokio::test]
    async fn resolve_passes_dataless_item_through_unchanged() {
        let uri: Url = "file:///main.ws".parse().unwrap();
        let backend = opened_backend(&uri);
        let item = CompletionItem {
            label: "var".to_string(),
            ..CompletionItem::default()
        };

        let resolved = backend
            ._completion_item_resolve(item.clone())
            .await
            .expect("resolve succeeds");
        assert_eq!(
            resolved, item,
            "an item without data must pass through unchanged"
        );
    }
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
