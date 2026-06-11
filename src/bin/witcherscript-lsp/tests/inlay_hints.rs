use lsp_types::{
    InlayHintParams, Position, Range, TextDocumentIdentifier, Url, WorkDoneProgressParams,
};

use crate::tests::support::{make_backend, open_params};

fn inlay_hint_params(uri: &Url) -> InlayHintParams {
    InlayHintParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: u32::MAX,
                character: 0,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    }
}

#[test]
fn inlay_hints_setting_toggles_hints() {
    let backend = make_backend();
    let uri: Url = "file:///main.ws".parse().unwrap();
    backend._did_open(open_params(
        &uri,
        "function Foo(target : int) {}\nfunction Bar() { Foo(1); }\n",
    ));

    let enabled = backend
        ._inlay_hint(inlay_hint_params(&uri))
        .expect("handler ok")
        .expect("hints present when enabled");
    assert_eq!(enabled.len(), 1, "default-on config yields the hint");

    backend.update_config(|c| c.inlay_hints = false);
    let disabled = backend
        ._inlay_hint(inlay_hint_params(&uri))
        .expect("handler ok");
    assert!(
        disabled.is_none(),
        "disabling the setting suppresses all hints"
    );
}
