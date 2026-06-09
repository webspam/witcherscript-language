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

enum SetTo<T> {
    Unset,
    Set(Option<T>),
}

pub(crate) struct CompilationBuilder {
    pub(crate) base: Arc<Compilation>,
    workspace_index: Option<WorkspaceIndex>,
    loose_index: Option<WorkspaceIndex>,
    base_scripts_index: Option<WorkspaceIndex>,
    script_env: Option<ScriptEnvironment>,
    suppressed_base_uris: Option<HashSet<String>>,
    filtered_base_catalogs: SetTo<FilteredBaseCatalogs>,
    documents: Option<HashMap<Url, Arc<ParsedDocument>>>,
    workspace_documents: Option<HashMap<String, Arc<ParsedDocument>>>,
    base_scripts_documents: Option<HashMap<String, Arc<ParsedDocument>>>,
}

fn cow_clone_mut<'a, T: Clone>(slot: &'a mut Option<T>, base: &Arc<T>) -> &'a mut T {
    slot.get_or_insert_with(|| (**base).clone())
}

fn resolve<T: Clone>(slot: Option<T>, base: &Arc<T>) -> Arc<T> {
    slot.map_or_else(|| base.clone(), Arc::new)
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
            filtered_base_catalogs: SetTo::Unset,
            documents: None,
            workspace_documents: None,
            base_scripts_documents: None,
        }
    }

    pub(crate) fn workspace_index_mut(&mut self) -> &mut WorkspaceIndex {
        cow_clone_mut(&mut self.workspace_index, &self.base.workspace_index)
    }

    pub(crate) fn loose_index_mut(&mut self) -> &mut WorkspaceIndex {
        cow_clone_mut(&mut self.loose_index, &self.base.loose_index)
    }

    pub(crate) fn base_scripts_index_mut(&mut self) -> &mut WorkspaceIndex {
        cow_clone_mut(&mut self.base_scripts_index, &self.base.base_scripts_index)
    }

    pub(crate) fn set_base_scripts_index(&mut self, idx: WorkspaceIndex) {
        self.base_scripts_index = Some(idx);
    }

    pub(crate) fn script_env_mut(&mut self) -> &mut ScriptEnvironment {
        cow_clone_mut(&mut self.script_env, &self.base.script_env)
    }

    pub(crate) fn set_suppressed_base_uris(&mut self, v: HashSet<String>) {
        self.suppressed_base_uris = Some(v);
    }

    pub(crate) fn set_filtered_base_catalogs(&mut self, v: Option<FilteredBaseCatalogs>) {
        self.filtered_base_catalogs = SetTo::Set(v);
    }

    pub(crate) fn documents_mut(&mut self) -> &mut HashMap<Url, Arc<ParsedDocument>> {
        cow_clone_mut(&mut self.documents, &self.base.documents)
    }

    pub(crate) fn workspace_documents_mut(&mut self) -> &mut HashMap<String, Arc<ParsedDocument>> {
        cow_clone_mut(
            &mut self.workspace_documents,
            &self.base.workspace_documents,
        )
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
        let index = cow_clone_mut(&mut self.workspace_index, ws_base);
        let docs = cow_clone_mut(&mut self.workspace_documents, docs_base);
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
        let index = cow_clone_mut(&mut self.base_scripts_index, idx_base);
        let docs = cow_clone_mut(&mut self.base_scripts_documents, docs_base);
        (index, docs)
    }

    pub(crate) fn set_base_scripts_documents(
        &mut self,
        docs: HashMap<String, Arc<ParsedDocument>>,
    ) {
        self.base_scripts_documents = Some(docs);
    }

    // `documents` is excluded: the views derive from indices, so an overlay-only swap must not refresh.
    pub(crate) fn changes_views(&self) -> bool {
        self.workspace_index.is_some()
            || self.loose_index.is_some()
            || self.base_scripts_index.is_some()
            || self.script_env.is_some()
            || self.suppressed_base_uris.is_some()
            || !matches!(self.filtered_base_catalogs, SetTo::Unset)
            || self.workspace_documents.is_some()
            || self.base_scripts_documents.is_some()
    }

    pub(crate) fn finish(self) -> Compilation {
        Compilation {
            workspace_index: resolve(self.workspace_index, &self.base.workspace_index),
            loose_index: resolve(self.loose_index, &self.base.loose_index),
            base_scripts_index: resolve(self.base_scripts_index, &self.base.base_scripts_index),
            script_env: resolve(self.script_env, &self.base.script_env),
            suppressed_base_uris: resolve(
                self.suppressed_base_uris,
                &self.base.suppressed_base_uris,
            ),
            filtered_base_catalogs: match self.filtered_base_catalogs {
                SetTo::Set(Some(cats)) => Some(Arc::new(cats)),
                SetTo::Set(None) => None,
                SetTo::Unset => self.base.filtered_base_catalogs.clone(),
            },
            documents: resolve(self.documents, &self.base.documents),
            workspace_documents: resolve(self.workspace_documents, &self.base.workspace_documents),
            base_scripts_documents: resolve(
                self.base_scripts_documents,
                &self.base.base_scripts_documents,
            ),
        }
    }
}
