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
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::time::timeout;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct JsonRpcClient<R, W> {
    write: W,
    read: BufReader<R>,
    next_id: i64,
}

#[allow(dead_code)]
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> JsonRpcClient<R, W> {
    pub(crate) fn new(read: R, write: W) -> Self {
        Self {
            write,
            read: BufReader::new(read),
            next_id: 1,
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
                if v.get("id").and_then(|i| i.as_i64()) == Some(id) {
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
            if n == 0 {
                panic!("server closed connection");
            }
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
        let id = v.get("id").cloned()?;
        let result = match method {
            "workspace/configuration" => {
                let count = v
                    .pointer("/params/items")
                    .and_then(|i| i.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                Value::Array(vec![Value::Null; count])
            }
            _ => Value::Null,
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }
}
