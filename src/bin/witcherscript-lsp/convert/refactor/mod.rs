use std::collections::HashMap;
use std::ops::Range;

use lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, Url, WorkspaceEdit};
use tree_sitter::Node;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::formatter::FormatOptions;
use witcherscript_language::resolve::SymbolDb;

use super::lsp_range;

mod extract_var;
mod if_stmt;
mod switch;

// Adding a construct means writing a `Refactoring` impl and listing it here.
const REFACTORINGS: &[&dyn Refactoring] = &[
    &switch::SwitchLayoutRefactoring,
    &if_stmt::IfLayoutRefactoring,
    &extract_var::ExtractVariableRefactoring,
];

// A cursor-driven "rewrite this construct" code action. Each impl locates its own target node
// from the cursor and returns 0..N rewrites; an impl that does not apply returns an empty vec.
trait Refactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand>;
}

enum Preference {
    Preferred,
    Alternative,
}

pub(crate) fn refactor_code_actions<'a>(
    uri: &'a Url,
    canonical_uri: &'a str,
    document: &'a ParsedDocument,
    db: &'a SymbolDb<'a>,
    selection: Range<usize>,
    options: FormatOptions,
) -> Vec<CodeActionOrCommand> {
    let ctx = RefactorContext {
        uri,
        canonical_uri,
        document,
        db,
        selection,
        options,
    };
    REFACTORINGS.iter().flat_map(|r| r.actions(&ctx)).collect()
}

struct RefactorContext<'a> {
    uri: &'a Url,
    canonical_uri: &'a str,
    document: &'a ParsedDocument,
    db: &'a SymbolDb<'a>,
    selection: Range<usize>,
    options: FormatOptions,
}

impl<'a> RefactorContext<'a> {
    fn root(&self) -> Node<'a> {
        self.document.tree.root_node()
    }

    fn cursor(&self) -> usize {
        self.selection.start
    }

    fn source(&self) -> &'a str {
        &self.document.source
    }

    fn options(&self) -> FormatOptions {
        self.options
    }

    // A REFACTOR_REWRITE action replacing `node`'s range with `new_text` in this document.
    fn rewrite(
        &self,
        title: &str,
        node: Node,
        new_text: String,
        preference: &Preference,
    ) -> CodeActionOrCommand {
        let range = lsp_range(self.document.line_index.byte_range_to_range(
            &self.document.source,
            node.start_byte(),
            node.end_byte(),
        ));
        let mut changes = HashMap::new();
        changes.insert(self.uri.clone(), vec![TextEdit { range, new_text }]);
        CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_string(),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            }),
            is_preferred: matches!(preference, Preference::Preferred).then_some(true),
            ..CodeAction::default()
        })
    }
}
