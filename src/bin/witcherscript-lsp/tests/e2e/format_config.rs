use std::fs;

use lsp_types::request::Formatting;
use lsp_types::{
    DocumentFormattingParams, FormattingOptions, TextDocumentIdentifier, TextEdit, Url,
    WorkDoneProgressParams,
};
use serde_json::json;

use super::harness::LspClientBuilder;
use crate::tests::support::LocalTempDir;

#[tokio::test]
async fn wsformat_toml_overrides_vscode_formatter_settings() {
    let tmp = LocalTempDir::under_target("wsformat-config");
    let ws_root = tmp.path().join("ws");
    let scripts = ws_root.join("scripts");
    fs::create_dir_all(&scripts).expect("mkdir scripts");
    fs::write(
        ws_root.join(".wsformat.toml"),
        "colon_spacing = \"spaced\"\n",
    )
    .expect("write .wsformat.toml");
    let script = scripts.join("script.ws");
    let source = "function f(){var x:int;}\n";
    fs::write(&script, source).expect("write script");

    let mut client = LspClientBuilder::new()
        .root(&ws_root)
        .config_override("witcherscript.formatter.compactColon", json!(true))
        .spawn()
        .await;

    let uri = Url::from_file_path(&script).expect("script path -> URI");
    client.open(&uri, source).await;

    let edits: Option<Vec<TextEdit>> = client
        .request::<Formatting>(DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                ..FormattingOptions::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await;

    let new_text = edits
        .expect("formatting returns edits")
        .into_iter()
        .next()
        .expect("a whole-document edit")
        .new_text;
    assert!(
        new_text.contains("var x : int;"),
        ".wsformat.toml colon_spacing=spaced must override VS Code compactColon=true, got:\n{new_text}"
    );
}
