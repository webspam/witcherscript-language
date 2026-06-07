use std::collections::HashMap;

use lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, Url, WorkspaceEdit};
use tree_sitter::Node;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::formatter::FormatOptions;

use super::lsp_range;

mod if_stmt;
mod switch;

const DEFAULT_TAB_WIDTH: u32 = 4;

// Adding a construct means writing a `Refactoring` impl and listing it here.
const REFACTORINGS: &[&dyn Refactoring] = &[
    &switch::SwitchLayoutRefactoring,
    &if_stmt::IfLayoutRefactoring,
];

// A cursor-driven "rewrite this construct" code action. Each impl locates its own target node
// from the cursor and returns 0..N rewrites; an impl that does not apply returns an empty vec.
trait Refactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand>;
}

pub(crate) fn refactor_code_actions(
    uri: &Url,
    document: &ParsedDocument,
    cursor: usize,
    options: FormatOptions,
) -> Vec<CodeActionOrCommand> {
    let ctx = RefactorContext {
        uri,
        document,
        cursor,
        options,
    };
    REFACTORINGS.iter().flat_map(|r| r.actions(&ctx)).collect()
}

/// Indent style for a rewrite, since code-action requests carry no editor formatting hint. Taken
/// from the first indented line; consistent on formatted code, which is what these rewrites run on.
pub(crate) fn infer_indent(source: &str) -> (bool, u32) {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.len() == line.len() {
            continue;
        }
        let indent = &line[..line.len() - trimmed.len()];
        return if indent.starts_with('\t') {
            (true, DEFAULT_TAB_WIDTH)
        } else {
            (false, indent.len() as u32)
        };
    }
    (false, DEFAULT_TAB_WIDTH)
}

struct RefactorContext<'a> {
    uri: &'a Url,
    document: &'a ParsedDocument,
    cursor: usize,
    options: FormatOptions,
}

impl<'a> RefactorContext<'a> {
    fn root(&self) -> Node<'a> {
        self.document.tree.root_node()
    }

    fn cursor(&self) -> usize {
        self.cursor
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
        preferred: bool,
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
            is_preferred: preferred.then_some(true),
            ..CodeAction::default()
        })
    }
}
