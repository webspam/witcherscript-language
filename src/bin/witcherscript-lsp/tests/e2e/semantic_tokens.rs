use async_lsp::ErrorCode;
use lsp_types::request::{
    Request, SemanticTokensFullDeltaRequest, SemanticTokensFullRequest, SemanticTokensRangeRequest,
};
use lsp_types::{
    PartialResultParams, Position, Range, SemanticToken, SemanticTokens, SemanticTokensDeltaParams,
    SemanticTokensEdit, SemanticTokensFullDeltaResult, SemanticTokensFullOptions,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, SemanticTokensServerCapabilities, TextDocumentIdentifier, Url,
    WorkDoneProgressParams,
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

pub(crate) fn absolute_tokens(tokens: &[SemanticToken]) -> Vec<(u32, u32, u32, u32, u32)> {
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

fn delta_params(uri: &Url, previous_result_id: &str) -> SemanticTokensDeltaParams {
    SemanticTokensDeltaParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        previous_result_id: previous_result_id.to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

// Edit positions are flat-u32-array indices; the server only emits whole-token (5-aligned) edits.
fn apply_edits(previous: &[SemanticToken], edits: &[SemanticTokensEdit]) -> Vec<SemanticToken> {
    let mut result = previous.to_vec();
    for edit in edits {
        let start = (edit.start / 5) as usize;
        let delete = (edit.delete_count / 5) as usize;
        result.splice(start..start + delete, edit.data.clone().unwrap_or_default());
    }
    result
}

// Retries CONTENT_MODIFIED like a real client: a did_change may still be queued server-side.
async fn delta_request_settled(
    client: &mut LspClient,
    params: &SemanticTokensDeltaParams,
) -> SemanticTokensFullDeltaResult {
    let params = serde_json::to_value(params).expect("serialize delta params");
    for _ in 0..50 {
        let v = client
            .raw_request(SemanticTokensFullDeltaRequest::METHOD, params.clone())
            .await;
        if let Some(err) = v.get("error") {
            let code = err.get("code").and_then(serde_json::Value::as_i64);
            assert_eq!(
                code,
                Some(i64::from(ErrorCode::CONTENT_MODIFIED.0)),
                "unexpected delta error: {err}"
            );
            continue;
        }
        let result = v.get("result").cloned().expect("delta response has result");
        return serde_json::from_value(result).expect("decode delta result");
    }
    panic!("semanticTokens/full/delta kept returning ContentModified");
}

#[tokio::test]
async fn advertises_full_delta_support() {
    let client = LspClient::spawn().await;
    assert_eq!(
        semantic_tokens_options(&client).full,
        Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
        "server must advertise semantic tokens full/delta support",
    );
}

#[tokio::test]
async fn delta_edits_transform_previous_tokens_into_current() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, TWO_FUNCTIONS).await;

    let full = full_tokens(&mut client, &uri).await;
    let previous_id = full
        .result_id
        .clone()
        .expect("full response must mint a result_id");

    client
        .change_full(&uri, 2, "function First() {}\nfunction Renamed() {}\n")
        .await;

    let result = delta_request_settled(&mut client, &delta_params(&uri, &previous_id)).await;
    let SemanticTokensFullDeltaResult::TokensDelta(delta) = result else {
        panic!("known previous_result_id must produce a delta, got {result:?}");
    };

    let patched = apply_edits(&full.data, &delta.edits);
    let fresh = full_tokens(&mut client, &uri).await;
    assert_eq!(
        patched, fresh.data,
        "applying the delta edits must reproduce the current full tokens",
    );
}

#[tokio::test]
async fn delta_with_unknown_previous_result_id_returns_full_tokens() {
    let uri: Url = "file:///main.ws".parse().unwrap();
    let mut client = LspClient::spawn().await;
    client.open(&uri, TWO_FUNCTIONS).await;

    let full = full_tokens(&mut client, &uri).await;
    let result = delta_request_settled(&mut client, &delta_params(&uri, "no-such-id")).await;
    let SemanticTokensFullDeltaResult::Tokens(tokens) = result else {
        panic!("unknown previous_result_id must produce a full payload, got {result:?}");
    };
    assert_eq!(
        tokens.data, full.data,
        "fallback full payload must carry the complete token data",
    );
    assert!(
        tokens.result_id.is_some(),
        "fallback full payload must mint a fresh result_id",
    );
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
