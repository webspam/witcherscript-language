//! One-call editor simulation: open a workspace fixture, then drive any feature and get a deterministic snapshot struct back.

mod battery;
mod features;
mod model;
mod scenarios;
mod workspace;

use lsp_types::{Position, Url};

use super::harness::{LspClient, LspClientBuilder};
use workspace::{LoadedFile, LoadedWorkspace};

pub(crate) enum WorkspaceFixture {
    Minimal,
    BaseLayering,
    MultiRoot,
    EmitterMod,
}

impl WorkspaceFixture {
    fn dir_name(self) -> &'static str {
        match self {
            WorkspaceFixture::Minimal => "minimal",
            WorkspaceFixture::BaseLayering => "base_layering",
            WorkspaceFixture::MultiRoot => "multi_root",
            WorkspaceFixture::EmitterMod => "emitter_mod",
        }
    }
}

pub(crate) struct EditorSession {
    client: LspClient,
    workspace: LoadedWorkspace,
}

impl EditorSession {
    pub(crate) async fn open(fixture: WorkspaceFixture) -> Self {
        let workspace = LoadedWorkspace::materialize(fixture.dir_name());
        let mut builder = LspClientBuilder::new();
        for root in workspace.workspace_roots() {
            builder = builder.root(root);
        }
        for (section, value) in workspace.config_overrides() {
            builder = builder.config_override(section, value.clone());
        }
        let mut client = builder.spawn().await;
        for file in workspace.files() {
            client.open(&file.uri, &file.text).await;
        }
        client.wait_until_indexed().await;
        Self { client, workspace }
    }

    pub(crate) fn rel_paths(&self) -> Vec<String> {
        self.workspace
            .files()
            .iter()
            .map(|f| f.rel.clone())
            .collect()
    }

    fn file(&self, rel: &str) -> &LoadedFile {
        self.workspace
            .files()
            .iter()
            .find(|f| f.rel == rel)
            .unwrap_or_else(|| panic!("no fixture file at {rel:?}"))
    }

    pub(crate) fn uri_of(&self, rel: &str) -> Url {
        self.file(rel).uri.clone()
    }

    pub(crate) fn cursor_in(&self, rel: &str) -> Position {
        self.file(rel)
            .cursor
            .unwrap_or_else(|| panic!("fixture file {rel:?} has no $0 cursor"))
    }
}
