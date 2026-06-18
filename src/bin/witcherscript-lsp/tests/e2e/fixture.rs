//! Converts the shared library fixture parser's output into `lsp_types`, so marker parsing lives in one place.

use std::collections::HashMap;

use lsp_types::{Position, Range, Url};
use witcherscript_language::line_index::{SourcePosition, SourceRange};
use witcherscript_language::test_support::Fixture as CoreFixture;

pub(crate) struct Fixture {
    pub(crate) files: Vec<FixtureFile>,
    pub(crate) cursor: Option<(Url, Position)>,
    pub(crate) spans: HashMap<String, (Url, Range)>,
}

pub(crate) struct FixtureFile {
    pub(crate) uri: Url,
    pub(crate) text: String,
}

impl Fixture {
    pub(crate) fn parse(input: &str) -> Self {
        let core = CoreFixture::parse(input);
        let files = core
            .files
            .into_iter()
            .map(|f| FixtureFile {
                uri: to_uri(&f.uri),
                text: f.text,
            })
            .collect();
        let cursor = core.cursor.map(|(uri, pos)| (to_uri(&uri), to_pos(pos)));
        let spans = core
            .spans
            .into_iter()
            .map(|(label, (uri, range))| (label, (to_uri(&uri), to_range(range))))
            .collect();
        Fixture {
            files,
            cursor,
            spans,
        }
    }

    pub(crate) fn cursor(&self) -> (Url, Position) {
        self.cursor
            .clone()
            .expect("fixture has no $0 cursor marker")
    }

    pub(crate) fn span(&self, label: &str) -> (Url, Range) {
        self.spans
            .get(label)
            .cloned()
            .unwrap_or_else(|| panic!("fixture has no span labelled {label:?}"))
    }
}

pub(crate) fn to_pos(p: SourcePosition) -> Position {
    Position {
        line: p.line,
        character: p.character,
    }
}

pub(crate) fn to_range(r: SourceRange) -> Range {
    Range {
        start: to_pos(r.start),
        end: to_pos(r.end),
    }
}

pub(crate) fn to_uri(s: &str) -> Url {
    Url::parse(s).unwrap_or_else(|e| panic!("fixture: invalid URI {s:?}: {e}"))
}

#[cfg(test)]
mod tests;
