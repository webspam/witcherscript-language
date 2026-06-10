use std::time::Duration;

use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification,
};
use lsp_types::request::Request;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Url,
    VersionedTextDocumentIdentifier,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::time::timeout;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct JsonRpcClient<R, W> {
    write: W,
    read: BufReader<R>,
    next_id: i64,
    hold_config: bool,
    held_config_requests: Vec<Value>,
}

#[allow(dead_code)]
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> JsonRpcClient<R, W> {
    pub(crate) fn new(read: R, write: W) -> Self {
        Self {
            write,
            read: BufReader::new(read),
            next_id: 1,
            hold_config: false,
            held_config_requests: Vec::new(),
        }
    }

    pub(crate) fn hold_config_replies(&mut self) {
        self.hold_config = true;
    }

    pub(crate) async fn release_config_replies(&mut self) {
        self.hold_config = false;
        for request in std::mem::take(&mut self.held_config_requests) {
            let reply = Self::config_reply(&request);
            self.send_raw(&reply).await;
        }
    }

    pub(crate) async fn raw_request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_raw(&msg).await;

        let result = timeout(REQUEST_TIMEOUT, async {
            loop {
                let v = self.read_raw().await;
                // A response carries our id and no `method`. A server->client request also has an
                // id (its own namespace) and must be answered, not mistaken for our response.
                let is_response = v.get("method").is_none()
                    && v.get("id").and_then(serde_json::Value::as_i64) == Some(id);
                if is_response {
                    return v;
                }
                if let Some(reply) = self.handle_inbound(v) {
                    self.send_raw(&reply).await;
                }
            }
        })
        .await;
        result.unwrap_or_else(|_| panic!("raw request {method} timed out"))
    }

    pub(crate) async fn request<Req: Request>(&mut self, params: Req::Params) -> Req::Result {
        let params = serde_json::to_value(params).expect("serialize request params");
        let v = self.raw_request(Req::METHOD, params).await;
        if let Some(err) = v.get("error") {
            panic!("request {} returned error: {err}", Req::METHOD);
        }
        let result = v.get("result").cloned().unwrap_or(Value::Null);
        serde_json::from_value::<Req::Result>(result)
            .unwrap_or_else(|e| panic!("decode failed for {}: {e}\nresponse: {v}", Req::METHOD))
    }

    pub(crate) async fn notify<N: Notification>(&mut self, params: N::Params) {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": N::METHOD,
            "params": params,
        });
        self.send_raw(&msg).await;
    }

    pub(crate) async fn did_open(&mut self, uri: &Url, text: &str) {
        self.notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "witcherscript".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
    }

    pub(crate) async fn did_change_full(&mut self, uri: &Url, version: i32, text: &str) {
        self.notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_string(),
            }],
        })
        .await;
    }

    pub(crate) async fn did_close(&mut self, uri: &Url) {
        self.notify::<DidCloseTextDocument>(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;
    }

    // Answers every server->client request while waiting, so a blocked handler (e.g. fetch_config) unblocks.
    pub(crate) async fn wait_for_server_request(&mut self, method: &str) -> bool {
        timeout(REQUEST_TIMEOUT, async {
            loop {
                let v = self.read_raw().await;
                let is_request = v.get("id").is_some() && v.get("method").is_some();
                if is_request {
                    if let Some(reply) = self.handle_inbound(v.clone()) {
                        self.send_raw(&reply).await;
                    }
                    if v.get("method").and_then(|m| m.as_str()) == Some(method) {
                        return true;
                    }
                }
            }
        })
        .await
        .unwrap_or(false)
    }

    async fn send_raw(&mut self, msg: &Value) {
        let body = serde_json::to_vec(msg).expect("serialize message");
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.write
            .write_all(header.as_bytes())
            .await
            .expect("write header");
        self.write.write_all(&body).await.expect("write body");
        self.write.flush().await.expect("flush");
    }

    async fn read_raw(&mut self) -> Value {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let n = self.read.read_line(&mut line).await.expect("read header");
            assert!(n != 0, "server closed connection");
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some(v) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(v.trim().parse().expect("Content-Length is a number"));
            }
        }
        let n = content_length.expect("missing Content-Length header");
        let mut buf = vec![0u8; n];
        self.read.read_exact(&mut buf).await.expect("read body");
        serde_json::from_slice(&buf).expect("parse JSON")
    }

    fn handle_inbound(&mut self, v: Value) -> Option<Value> {
        let method = v.get("method").and_then(|m| m.as_str())?;
        v.get("id")?;
        if method == "workspace/configuration" {
            if self.hold_config {
                self.held_config_requests.push(v);
                return None;
            }
            return Some(Self::config_reply(&v));
        }
        Some(json!({ "jsonrpc": "2.0", "id": v.get("id"), "result": Value::Null }))
    }

    fn config_reply(request: &Value) -> Value {
        let count = request
            .pointer("/params/items")
            .and_then(|i| i.as_array())
            .map_or(0, std::vec::Vec::len);
        json!({
            "jsonrpc": "2.0",
            "id": request.get("id"),
            "result": Value::Array(vec![Value::Null; count]),
        })
    }
}
