use std::sync::Arc;

use async_lsp::ErrorCode;
use lsp_types::WorkDoneProgressParams;
use lsp_types::{
    CodeLens, CodeLensParams, Command, Location, PartialResultParams, Position, Range,
    TextDocumentIdentifier, Url,
};

use super::legacy_helpers::indexed_legacy_override;
use crate::backend::Backend;
use crate::queries::ReferenceLensData;
use crate::tests::support::{make_backend, open_params};

fn code_lens_params(uri: &Url) -> CodeLensParams {
    CodeLensParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn enable_references_lens(backend: &Backend) {
    let mut cfg = (**backend.config.load()).clone();
    cfg.code_lens_references = true;
    backend.config.store(Arc::new(cfg));
}

#[tokio::test]
async fn lens_links_legacy_override_to_base_definition() {
    let (_temp, backend, override_url, _new_url) =
        indexed_legacy_override("ws_code_lens_override").await;
    backend._did_open(open_params(
        &override_url,
        "class CR4Player {}\n// legacy\n",
    ));

    let lenses = backend
        ._code_lens(code_lens_params(&override_url))
        .expect("code_lens ok")
        .expect("overriding symbol yields a lens");

    assert_eq!(lenses.len(), 1, "one overriding top-level symbol");
    let command = lenses[0].command.as_ref().expect("lens carries a command");
    assert_eq!(
        command.command, "witcherscript.goToBaseDefinition",
        "lens invokes the navigation command"
    );
    let arg = command.arguments.as_ref().expect("command has arguments")[0].clone();
    let target: Location = serde_json::from_value(arg).expect("argument is a Location");
    assert_ne!(
        target.uri, override_url,
        "lens navigates to the base script, not the override itself"
    );
    assert!(
        target.uri.as_str().contains("content0"),
        "target points at the vanilla base script"
    );
}

#[tokio::test]
async fn lens_suppressed_when_feature_disabled() {
    let (_temp, backend, override_url, _new_url) =
        indexed_legacy_override("ws_code_lens_disabled").await;
    backend._did_open(open_params(
        &override_url,
        "class CR4Player {}\n// legacy\n",
    ));

    let mut cfg = (**backend.config.load()).clone();
    cfg.code_lens_overridden_symbols = false;
    backend.config.store(Arc::new(cfg));

    let result = backend
        ._code_lens(code_lens_params(&override_url))
        .expect("code_lens ok");
    assert!(result.is_none(), "disabled feature yields no lenses");
}

#[tokio::test]
async fn lens_shown_for_override_inside_workspace_root() {
    let (temp, backend, override_url, _new_url) =
        indexed_legacy_override("ws_code_lens_in_workspace").await;
    // Inside an open workspace root the override classifies as InProject, not LegacyOverride; the lens must still appear.
    backend.set_workspace_roots(vec![temp.path().to_path_buf()]);
    backend._did_open(open_params(
        &override_url,
        "class CR4Player {}\n// legacy\n",
    ));

    let lenses = backend
        ._code_lens(code_lens_params(&override_url))
        .expect("code_lens ok")
        .expect("override inside the workspace still yields a lens");
    assert_eq!(lenses.len(), 1, "one overriding top-level symbol");
}

#[tokio::test]
async fn no_lens_for_brand_new_legacy_file() {
    let (_temp, backend, _override_url, new_url) =
        indexed_legacy_override("ws_code_lens_new").await;
    backend._did_open(open_params(&new_url, "class CMyNewMod {}\n"));

    let result = backend
        ._code_lens(code_lens_params(&new_url))
        .expect("code_lens ok");
    assert!(
        result.is_none(),
        "a brand-new legacy file overrides no base script"
    );
}

#[tokio::test]
async fn references_lens_emits_unresolved_data_lens() {
    let backend = make_backend();
    enable_references_lens(&backend);
    let uri = Url::parse("file:///refs_main.ws").expect("uri parses");
    backend._did_open(open_params(
        &uri,
        "function Foo() {}\nfunction Bar() { Foo(); }\n",
    ));

    let lenses = backend
        ._code_lens(code_lens_params(&uri))
        .expect("code_lens ok")
        .expect("references lenses present");

    assert!(
        lenses.iter().all(|l| l.command.is_none()),
        "phase-1 reference lenses carry no command"
    );
    for lens in &lenses {
        let data: ReferenceLensData = serde_json::from_value(
            lens.data
                .clone()
                .expect("phase-1 lens carries resolve data"),
        )
        .expect("data deserializes as ReferenceLensData");
        assert_eq!(
            data.position, lens.range.start,
            "resolve data position anchors the lens identifier"
        );
        assert_eq!(data.uri, uri, "resolve data carries the document uri");
    }
}

// Pre-index resolves must answer immediately: parked resolves hold request-cap slots and can deadlock the main loop.
#[tokio::test]
async fn reference_lens_resolves_before_initial_index() {
    let backend = make_backend();
    enable_references_lens(&backend);
    let uri = Url::parse("file:///refs_gate.ws").expect("uri parses");
    backend._did_open(open_params(
        &uri,
        "function Foo() {}\nfunction Bar() { Foo(); }\n",
    ));

    let lenses = backend
        ._code_lens(code_lens_params(&uri))
        .expect("code_lens ok")
        .expect("references lenses present");
    let foo_lens = lenses
        .into_iter()
        .find(|l| l.range.start.line == 0)
        .expect("lens for Foo on line 0");

    let resolved = backend
        ._code_lens_resolve(foo_lens)
        .await
        .expect("pre-index resolve must answer, not park");
    let command = resolved.command.expect("resolved lens carries a command");
    assert_eq!(
        command.title, "1 reference",
        "open documents are indexed on did_open, so Foo's caller is counted pre-index"
    );
}

#[tokio::test]
async fn references_lens_resolve_fills_count() {
    let backend = make_backend();
    enable_references_lens(&backend);
    backend
        .initial_index_done
        .store(true, std::sync::atomic::Ordering::Release);
    let uri = Url::parse("file:///refs_count.ws").expect("uri parses");
    backend._did_open(open_params(
        &uri,
        "function Foo() {}\nfunction Bar() {}\nfunction Baz() { Foo(); Foo(); Bar(); }\n",
    ));

    let lenses = backend
        ._code_lens(code_lens_params(&uri))
        .expect("code_lens ok")
        .expect("references lenses present");

    let mut titles = Vec::new();
    for lens in lenses {
        let resolved = backend._code_lens_resolve(lens).await.expect("resolve ok");
        let command = resolved.command.expect("resolved lens carries a command");
        assert_eq!(
            command.command, "witcherscript.showReferences",
            "reference lens invokes the show-references command"
        );
        assert_eq!(
            command
                .arguments
                .as_ref()
                .expect("command has arguments")
                .len(),
            3,
            "show-references carries uri, position and locations"
        );
        titles.push(command.title);
    }
    titles.sort();
    assert_eq!(
        titles,
        vec![
            "0 references".to_string(),
            "1 reference".to_string(),
            "2 references".to_string(),
        ],
        "counts cover zero (Baz), singular (Bar) and plural (Foo)"
    );
}

#[tokio::test]
async fn references_lens_resolve_passthrough_for_gamedef() {
    let backend = make_backend();
    let lens = CodeLens {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 5,
            },
        },
        command: Some(Command {
            title: "game definition".to_string(),
            command: "witcherscript.goToBaseDefinition".to_string(),
            arguments: Some(vec![]),
        }),
        data: None,
    };

    let resolved = backend._code_lens_resolve(lens).await.expect("resolve ok");
    let command = resolved.command.expect("command preserved");
    assert_eq!(
        command.command, "witcherscript.goToBaseDefinition",
        "a fully-built lens with no data passes through unchanged"
    );
}

#[tokio::test]
async fn references_lens_resolve_rejects_malformed_data() {
    let backend = make_backend();
    let lens = CodeLens {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        command: None,
        data: Some(serde_json::json!({ "bogus": true })),
    };

    let err = backend
        ._code_lens_resolve(lens)
        .await
        .expect_err("malformed data is rejected");
    assert_eq!(
        err.code,
        ErrorCode::INVALID_PARAMS,
        "malformed reference lens data fails loud"
    );
}

#[tokio::test]
async fn references_lens_suppressed_when_disabled() {
    let backend = make_backend();
    let uri = Url::parse("file:///refs_off.ws").expect("uri parses");
    backend._did_open(open_params(&uri, "function Foo() {}\n"));

    let result = backend
        ._code_lens(code_lens_params(&uri))
        .expect("code_lens ok");
    assert!(
        result.is_none(),
        "no references lens when the feature is off and the file overrides nothing"
    );
}

#[tokio::test]
async fn references_lens_precedes_game_definition_lens() {
    let (_temp, backend, override_url, _new_url) =
        indexed_legacy_override("ws_code_lens_order").await;
    enable_references_lens(&backend);
    backend._did_open(open_params(
        &override_url,
        "class CR4Player {}\n// legacy\n",
    ));

    let lenses = backend
        ._code_lens(code_lens_params(&override_url))
        .expect("code_lens ok")
        .expect("both lenses present");

    assert_eq!(
        lenses.len(),
        2,
        "one references lens and one game-definition lens"
    );
    assert!(
        lenses[0].command.is_none(),
        "the references lens is emitted first so it stays leftmost"
    );
    assert_eq!(
        lenses[1]
            .command
            .as_ref()
            .expect("game-definition lens carries a command")
            .command,
        "witcherscript.goToBaseDefinition",
        "the game-definition lens renders to the right of the references lens"
    );
}

#[tokio::test]
async fn references_lens_appears_on_non_override_file() {
    let backend = make_backend();
    enable_references_lens(&backend);
    let uri = Url::parse("file:///refs_plain.ws").expect("uri parses");
    backend._did_open(open_params(&uri, "function Foo() {}\n"));

    let lenses = backend
        ._code_lens(code_lens_params(&uri))
        .expect("code_lens ok")
        .expect("a plain file still gets reference lenses");
    assert_eq!(lenses.len(), 1, "one eligible top-level function");
    assert!(
        lenses.iter().all(|l| l.command.is_none()),
        "references lens decouples from the game-definition override gate"
    );
}
