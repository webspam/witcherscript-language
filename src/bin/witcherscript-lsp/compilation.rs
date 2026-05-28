use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lsp_types::Url;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::resolve::{FilteredBaseCatalogs, WorkspaceIndex};
use witcherscript_language::script_env::ScriptEnvironment;

#[derive(Debug, Default, Clone)]
pub(crate) struct Compilation {
    pub(crate) workspace_index: Arc<WorkspaceIndex>,
    pub(crate) loose_index: Arc<WorkspaceIndex>,
    pub(crate) base_scripts_index: Arc<WorkspaceIndex>,
    pub(crate) script_env: Arc<ScriptEnvironment>,
    pub(crate) suppressed_base_uris: Arc<HashSet<String>>,
    pub(crate) filtered_base_catalogs: Option<Arc<FilteredBaseCatalogs>>,
    pub(crate) documents: Arc<HashMap<Url, Arc<ParsedDocument>>>,
    pub(crate) workspace_documents: Arc<HashMap<String, Arc<ParsedDocument>>>,
    pub(crate) base_scripts_documents: Arc<HashMap<String, Arc<ParsedDocument>>>,
}

pub(crate) struct CompilationBuilder {
    pub(crate) base: Arc<Compilation>,
    workspace_index: Option<WorkspaceIndex>,
    loose_index: Option<WorkspaceIndex>,
    base_scripts_index: Option<WorkspaceIndex>,
    script_env: Option<ScriptEnvironment>,
    suppressed_base_uris: Option<HashSet<String>>,
    // Outer Option flags "set"; inner Option carries the actual value.
    filtered_base_catalogs: Option<Option<FilteredBaseCatalogs>>,
    documents: Option<HashMap<Url, Arc<ParsedDocument>>>,
    workspace_documents: Option<HashMap<String, Arc<ParsedDocument>>>,
    base_scripts_documents: Option<HashMap<String, Arc<ParsedDocument>>>,
}

impl CompilationBuilder {
    pub(crate) fn new(base: Arc<Compilation>) -> Self {
        Self {
            base,
            workspace_index: None,
            loose_index: None,
            base_scripts_index: None,
            script_env: None,
            suppressed_base_uris: None,
            filtered_base_catalogs: None,
            documents: None,
            workspace_documents: None,
            base_scripts_documents: None,
        }
    }

    pub(crate) fn workspace_index_mut(&mut self) -> &mut WorkspaceIndex {
        let base = &self.base.workspace_index;
        self.workspace_index.get_or_insert_with(|| (**base).clone())
    }

    pub(crate) fn loose_index_mut(&mut self) -> &mut WorkspaceIndex {
        let base = &self.base.loose_index;
        self.loose_index.get_or_insert_with(|| (**base).clone())
    }

    pub(crate) fn base_scripts_index_mut(&mut self) -> &mut WorkspaceIndex {
        let base = &self.base.base_scripts_index;
        self.base_scripts_index
            .get_or_insert_with(|| (**base).clone())
    }

    pub(crate) fn replace_base_scripts_index(&mut self, idx: WorkspaceIndex) {
        self.base_scripts_index = Some(idx);
    }

    pub(crate) fn script_env_mut(&mut self) -> &mut ScriptEnvironment {
        let base = &self.base.script_env;
        self.script_env.get_or_insert_with(|| (**base).clone())
    }

    pub(crate) fn set_suppressed_base_uris(&mut self, v: HashSet<String>) {
        self.suppressed_base_uris = Some(v);
    }

    pub(crate) fn set_filtered_base_catalogs(&mut self, v: Option<FilteredBaseCatalogs>) {
        self.filtered_base_catalogs = Some(v);
    }

    pub(crate) fn documents_mut(&mut self) -> &mut HashMap<Url, Arc<ParsedDocument>> {
        let base = &self.base.documents;
        self.documents.get_or_insert_with(|| (**base).clone())
    }

    pub(crate) fn workspace_documents_mut(&mut self) -> &mut HashMap<String, Arc<ParsedDocument>> {
        let base = &self.base.workspace_documents;
        self.workspace_documents
            .get_or_insert_with(|| (**base).clone())
    }

    // Borrow both fields at once; Rust's method-call borrow checker cannot prove the
    // index and docs slots are disjoint when accessed through separate `*_mut` methods.
    pub(crate) fn workspace_index_and_docs_mut(
        &mut self,
    ) -> (
        &mut WorkspaceIndex,
        &mut HashMap<String, Arc<ParsedDocument>>,
    ) {
        let ws_base = &self.base.workspace_index;
        let docs_base = &self.base.workspace_documents;
        let index = self
            .workspace_index
            .get_or_insert_with(|| (**ws_base).clone());
        let docs = self
            .workspace_documents
            .get_or_insert_with(|| (**docs_base).clone());
        (index, docs)
    }

    pub(crate) fn base_scripts_index_and_docs_mut(
        &mut self,
    ) -> (
        &mut WorkspaceIndex,
        &mut HashMap<String, Arc<ParsedDocument>>,
    ) {
        let idx_base = &self.base.base_scripts_index;
        let docs_base = &self.base.base_scripts_documents;
        let index = self
            .base_scripts_index
            .get_or_insert_with(|| (**idx_base).clone());
        let docs = self
            .base_scripts_documents
            .get_or_insert_with(|| (**docs_base).clone());
        (index, docs)
    }

    pub(crate) fn replace_base_scripts_documents(
        &mut self,
        docs: HashMap<String, Arc<ParsedDocument>>,
    ) {
        self.base_scripts_documents = Some(docs);
    }

    pub(crate) fn finish(self) -> Compilation {
        Compilation {
            workspace_index: self
                .workspace_index
                .map(Arc::new)
                .unwrap_or_else(|| self.base.workspace_index.clone()),
            loose_index: self
                .loose_index
                .map(Arc::new)
                .unwrap_or_else(|| self.base.loose_index.clone()),
            base_scripts_index: self
                .base_scripts_index
                .map(Arc::new)
                .unwrap_or_else(|| self.base.base_scripts_index.clone()),
            script_env: self
                .script_env
                .map(Arc::new)
                .unwrap_or_else(|| self.base.script_env.clone()),
            suppressed_base_uris: self
                .suppressed_base_uris
                .map(Arc::new)
                .unwrap_or_else(|| self.base.suppressed_base_uris.clone()),
            filtered_base_catalogs: match self.filtered_base_catalogs {
                Some(Some(cats)) => Some(Arc::new(cats)),
                Some(None) => None,
                None => self.base.filtered_base_catalogs.clone(),
            },
            documents: self
                .documents
                .map(Arc::new)
                .unwrap_or_else(|| self.base.documents.clone()),
            workspace_documents: self
                .workspace_documents
                .map(Arc::new)
                .unwrap_or_else(|| self.base.workspace_documents.clone()),
            base_scripts_documents: self
                .base_scripts_documents
                .map(Arc::new)
                .unwrap_or_else(|| self.base.base_scripts_documents.clone()),
        }
    }
}
