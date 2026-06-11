use lsp_types::request::{SemanticTokensFullRequest, SemanticTokensRangeRequest};
use lsp_types::{
    PartialResultParams, Position, Range, SemanticToken, SemanticTokens, SemanticTokensParams,
    SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult,
    SemanticTokensServerCapabilities, TextDocumentIdentifier, Url, WorkDoneProgressParams,
};

use super::harness::LspClient;

const TWO_FUNCTIONS: &str = "function First() {}\nfunction Second() {}\n";

fn semantic_tokens_options(client: &LspClient) -> &lsp_types::SemanticTokensOptions {
    match client
        .server_capabilities()
        .semantic_tokens_provider
        .as_ref()
        .expect("server must advertise semanticTokensProvider")
    {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => options,
        SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(options) => {
            &options.semantic_tokens_options
        }
    }
}

fn absolute_tokens(tokens: &[SemanticToken]) -> Vec<(u32, u32, u32, u32, u32)> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut line = 0u32;
    let mut start = 0u32;
    for t in tokens {
        line += t.delta_line;
        start = if t.delta_line > 0 {
            t.delta_start
        } else {
            start + t.delta_start
        };
        out.push((
            line,
            start,
            t.length,
            t.token_type,
            t.token_modifiers_bitset,
        ));
    }
    out
}

async fn full_tokens(client: &mut LspClient, uri: &Url) -> SemanticTokens {
    let response = client
        .request::<SemanticTokensFullRequest>(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("semantic tokens full response");
    let SemanticTokensResult::Tokens(tokens) = response else {
        panic!("expected a full token payload, got {response:?}");
    };
    tokens
}

#[tokio::test]
async fn advertises_range_support() {
    let client = LspClient::spawn().await;
    assert_eq!(
        semantic_tokens_options(&client).range,
        Some(true),
        "server must advertise semantic tokens range support",
    );
}

#[tokio::test]
async fn range_response_matches_full_tokens_within_the_range() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, TWO_FUNCTIONS).await;

    let full = full_tokens(&mut client, &uri).await;
    let response = client
        .request::<SemanticTokensRangeRequest>(SemanticTokensRangeParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::new(Position::new(1, 0), Position::new(2, 0)),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("semantic tokens range response");
    let SemanticTokensRangeResult::Tokens(ranged) = response else {
        panic!("expected a range token payload, got {response:?}");
    };

    let expected: Vec<_> = absolute_tokens(&full.data)
        .into_iter()
        .filter(|&(line, ..)| line == 1)
        .collect();
    assert!(
        !expected.is_empty(),
        "fixture must produce tokens on line 1"
    );
    assert_eq!(
        absolute_tokens(&ranged.data),
        expected,
        "range tokens must equal the full tokens that fall inside the range",
    );
}
