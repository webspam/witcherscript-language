use std::sync::atomic::Ordering;

use lsp_types::{
    InlayHintLabel, InlayHintParams, Position, Range, TextDocumentIdentifier, Url,
    WorkDoneProgressParams,
};

use crate::tests::support::{change_params, make_backend, open_params};

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

#[test]
fn hints_reflect_a_queued_unpublished_edit() {
    let backend = make_backend();
    backend.edit_writer_spawned.store(true, Ordering::Release);
    let uri: Url = "file:///queued_hints.ws".parse().unwrap();
    backend._did_open(open_params(
        &uri,
        "function Foo(target : int) {}\nfunction Bar() { }\n",
    ));
    backend._did_change(change_params(&uri, 2, (1, 17), (1, 17), "Foo(1); "));
    assert!(
        backend.pending_target_for(&uri).is_some(),
        "the edit must still be queued for this test to exercise the race"
    );

    let hints = backend
        ._inlay_hint(inlay_hint_params(&uri))
        .expect("a queued edit must not fail the request")
        .expect("hints computed against the queued text");

    let positions_and_labels: Vec<_> = hints
        .iter()
        .map(|hint| {
            let InlayHintLabel::String(label) = &hint.label else {
                panic!("expected a plain string label, got {:?}", hint.label);
            };
            (hint.position, label.as_str())
        })
        .collect();
    assert_eq!(
        positions_and_labels,
        vec![(
            Position {
                line: 1,
                character: 21
            },
            "target:"
        )],
        "the hint sits on the argument typed in the queued edit",
    );
}
