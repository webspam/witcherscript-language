mod fixture;

pub use fixture::{Fixture, FixtureFile};

use crate::document::{ParsedDocument, parse_document};
use crate::line_index::{SourcePosition, SourceRange};
use crate::resolve::{Definition, SymbolDb, WorkspaceIndex};
use crate::script_env::{ScriptEnvironment, ScriptGlobal};
use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};
use crate::types::Type;

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

    #[must_use]
    pub fn with_base_doc(mut self, uri: &str, source: &str) -> Self {
        let doc = parse_document(source).expect("test_support: base source must parse");
        self.base.update_document(uri, &doc);
        self
    }

    #[must_use]
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
        self.docs.iter().find(|(u, _)| u == uri).map_or_else(
            || panic!("test_support: no document for uri {uri:?}"),
            |(_, d)| d,
        )
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

pub fn script_env(name: &str, type_name: &str) -> ScriptEnvironment {
    let start = SourcePosition {
        line: 1,
        character: 0,
    };
    let end = SourcePosition {
        line: 1,
        character: name.len() as u32,
    };
    let range = SourceRange { start, end };
    ScriptEnvironment::new(vec![ScriptGlobal {
        name: name.to_string(),
        type_name: type_name.to_string(),
        ini_uri: "file:///redscripts.ini".to_string(),
        symbol: Symbol {
            id: SymbolId(0),
            name: name.to_string(),
            kind: SymbolKind::Variable,
            range,
            selection_range: range,
            byte_range: 0..name.len(),
            selection_byte_range: 0..name.len(),
            container: None,
            container_name: None,
            type_annotation: Some(Type::from_annotation(type_name)),
            signature: None,
            base_class: None,
            owner_class: None,
            flavour: None,
            annotations: Vec::new(),
            access: AccessLevel::Public,
            is_optional: false,
            is_out: false,
            is_state_machine: false,
            is_abstract: false,
        },
    }])
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
