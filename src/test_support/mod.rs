mod fixture;

pub use fixture::{Fixture, FixtureFile};

use crate::document::{parse_document, ParsedDocument};
use crate::line_index::{SourcePosition, SourceRange};
use crate::resolve::{Definition, SymbolDb, WorkspaceIndex};

pub struct TestDb {
    pub docs: Vec<(String, ParsedDocument)>,
    pub workspace: WorkspaceIndex,
    pub base: WorkspaceIndex,
    pub builtins: Option<WorkspaceIndex>,
    pub fixture: Fixture,
}

impl TestDb {
    pub fn new(fixture_src: &str) -> Self {
        let fixture = Fixture::parse(fixture_src);
        let mut docs = Vec::with_capacity(fixture.files.len());
        let mut workspace = WorkspaceIndex::default();
        for file in &fixture.files {
            let doc = parse_document(&file.text).expect("test_support: fixture source must parse");
            workspace.update_document(&file.uri, &doc);
            docs.push((file.uri.clone(), doc));
        }
        Self {
            docs,
            workspace,
            base: WorkspaceIndex::default(),
            builtins: None,
            fixture,
        }
    }

    pub fn db(&self) -> SymbolDb<'_> {
        let db = SymbolDb::new(&self.workspace, &self.base);
        match &self.builtins {
            Some(b) => db.with_builtins(b),
            None => db,
        }
    }

    pub fn with_base_doc(mut self, uri: &str, source: &str) -> Self {
        let doc = parse_document(source).expect("test_support: base source must parse");
        self.base.update_document(uri, &doc);
        self
    }

    pub fn with_builtins_index(mut self) -> Self {
        self.builtins = Some(crate::builtins::load_builtins_index());
        self
    }

    pub fn primary_uri(&self) -> &str {
        &self.docs[0].0
    }

    pub fn primary_doc(&self) -> &ParsedDocument {
        &self.docs[0].1
    }

    pub fn doc_for(&self, uri: &str) -> &ParsedDocument {
        self.docs
            .iter()
            .find(|(u, _)| u == uri)
            .map(|(_, d)| d)
            .unwrap_or_else(|| panic!("test_support: no document for uri {uri:?}"))
    }

    pub fn search_docs(&self) -> Vec<(&str, &ParsedDocument)> {
        self.docs.iter().map(|(u, d)| (u.as_str(), d)).collect()
    }

    pub fn cursor(&self) -> (String, SourcePosition) {
        self.fixture.cursor()
    }

    pub fn cursor_pos(&self) -> SourcePosition {
        self.fixture.cursor().1
    }

    pub fn span(&self, label: &str) -> (String, SourceRange) {
        self.fixture.span(label)
    }
}

pub fn def_names(defs: &[Definition]) -> Vec<&str> {
    defs.iter().map(|d| d.symbol.name.as_str()).collect()
}

pub fn def_names_tiered(defs: &[(u8, Definition)]) -> Vec<&str> {
    defs.iter().map(|(_, d)| d.symbol.name.as_str()).collect()
}

#[track_caller]
pub fn assert_names_contain(actual: &[&str], expected: &[&str]) {
    for name in expected {
        assert!(
            actual.contains(name),
            "expected name {name:?} in {actual:?}"
        );
    }
}

#[track_caller]
pub fn assert_names_exclude(actual: &[&str], excluded: &[&str]) {
    for name in excluded {
        assert!(
            !actual.contains(name),
            "name {name:?} should NOT appear in {actual:?}"
        );
    }
}
