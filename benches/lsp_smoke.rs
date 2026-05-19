use std::process::Stdio;

use criterion::{criterion_group, criterion_main, Criterion};
use lsp_types::notification::Initialized;
use lsp_types::request::{GotoDefinition, Initialize, Request};
use lsp_types::{
    ClientCapabilities, GotoDefinitionParams, InitializeParams, InitializedParams,
    PartialResultParams, Position, TextDocumentIdentifier, TextDocumentPositionParams, Url,
    WorkDoneProgressParams,
};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

#[path = "../src/bin/witcherscript-lsp/tests/jsonrpc_client.rs"]
mod jsonrpc_client;

#[path = "common/synth.rs"]
mod synth;

use jsonrpc_client::JsonRpcClient;

const LSP_BIN: &str = env!("CARGO_BIN_EXE_witcherscript-lsp");

async fn spawn_server() -> (Child, JsonRpcClient<ChildStdout, ChildStdin>) {
    let mut child = Command::new(LSP_BIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn LSP binary");
    let stdin = child.stdin.take().expect("take stdin");
    let stdout = child.stdout.take().expect("take stdout");
    let client = JsonRpcClient::new(stdout, stdin);
    (child, client)
}

async fn initialize(rpc: &mut JsonRpcClient<ChildStdout, ChildStdin>) {
    let _: <Initialize as Request>::Result = rpc
        .request::<Initialize>(InitializeParams {
            capabilities: ClientCapabilities::default(),
            ..InitializeParams::default()
        })
        .await;
    rpc.notify::<Initialized>(InitializedParams {}).await;
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime")
}

fn bench_cold_start(c: &mut Criterion) {
    let rt = runtime();
    c.bench_function("lsp/cold_start", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (mut child, mut rpc) = spawn_server().await;
                initialize(&mut rpc).await;
                let _ = child.kill().await;
            });
        });
    });
}

fn bench_did_open(c: &mut Criterion) {
    let rt = runtime();
    let (mut child, mut rpc) = rt.block_on(async {
        let (child, mut rpc) = spawn_server().await;
        initialize(&mut rpc).await;
        (child, rpc)
    });
    let source = synth::synth_file(4, 4);
    let mut counter: u64 = 0;

    c.bench_function("lsp/did_open", |b| {
        b.iter(|| {
            counter += 1;
            let uri = Url::parse(&format!("file:///smoke/open{counter}.ws")).unwrap();
            rt.block_on(async {
                rpc.did_open(&uri, &source).await;
            });
        });
    });

    rt.block_on(async {
        let _ = child.kill().await;
    });
}

fn bench_definition(c: &mut Criterion) {
    let rt = runtime();
    let (mut child, mut rpc, uri) = rt.block_on(async {
        let (child, mut rpc) = spawn_server().await;
        initialize(&mut rpc).await;
        let uri = Url::parse("file:///smoke/definition.ws").unwrap();
        rpc.did_open(&uri, &synth::synth_file(4, 4)).await;
        (child, rpc, uri)
    });
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let canary = rt.block_on(async { rpc.request::<GotoDefinition>(params.clone()).await });
    assert!(
        canary.is_some(),
        "synth layout drifted: cursor no longer lands on a resolvable call site"
    );

    c.bench_function("lsp/definition_request", |b| {
        b.iter(|| {
            rt.block_on(async {
                rpc.request::<GotoDefinition>(params.clone()).await;
            });
        });
    });

    rt.block_on(async {
        let _ = child.kill().await;
    });
}

criterion_group!(benches, bench_cold_start, bench_did_open, bench_definition);
criterion_main!(benches);
