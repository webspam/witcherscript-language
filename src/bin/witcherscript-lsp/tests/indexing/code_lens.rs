use std::sync::Arc;

use lsp_types::WorkDoneProgressParams;
use lsp_types::{CodeLensParams, Location, PartialResultParams, TextDocumentIdentifier, Url};

use super::legacy_helpers::{indexed_legacy_override, open_params};

fn code_lens_params(uri: &Url) -> CodeLensParams {
    CodeLensParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
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
        .await
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
        .await
        .expect("code_lens ok");
    assert!(result.is_none(), "disabled feature yields no lenses");
}

#[tokio::test]
async fn lens_shown_for_override_inside_workspace_root() {
    let (temp, backend, override_url, _new_url) =
        indexed_legacy_override("ws_code_lens_in_workspace").await;
    // Inside an open workspace root the override classifies as InProject, not LegacyOverride; the lens must still appear.
    *backend.workspace_roots.lock() = vec![temp.path().to_path_buf()];
    backend._did_open(open_params(
        &override_url,
        "class CR4Player {}\n// legacy\n",
    ));

    let lenses = backend
        ._code_lens(code_lens_params(&override_url))
        .await
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
        .await
        .expect("code_lens ok");
    assert!(
        result.is_none(),
        "a brand-new legacy file overrides no base script"
    );
}
