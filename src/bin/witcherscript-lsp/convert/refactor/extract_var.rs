use std::collections::HashMap;

use lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, WorkspaceEdit};
use witcherscript_language::resolve::extract_variable;

use super::super::lsp_range;
use super::{RefactorContext, Refactoring, extract_command, rename_position};

pub(super) struct ExtractVariableRefactoring;

impl Refactoring for ExtractVariableRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        if ctx.selection.is_empty() {
            return Vec::new();
        }
        let Some(extraction) = extract_variable(
            ctx.canonical_uri,
            ctx.document,
            ctx.db,
            ctx.selection.clone(),
            ctx.options,
        ) else {
            return Vec::new();
        };
        let source = &ctx.document.source;
        let line_index = &ctx.document.line_index;
        let position = rename_position(source, &extraction);
        let edits = extraction
            .edits
            .iter()
            .map(|splice| TextEdit {
                range: lsp_range(line_index.byte_range_to_range(
                    source,
                    splice.range.start,
                    splice.range.end,
                )),
                new_text: splice.text.clone(),
            })
            .collect();
        let mut changes = HashMap::new();
        changes.insert(ctx.uri.clone(), edits);
        let edit = WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        };
        vec![CodeActionOrCommand::CodeAction(CodeAction {
            title: "Extract to variable".to_string(),
            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
            edit: Some(edit),
            command: Some(extract_command(ctx.uri, position)),
            ..CodeAction::default()
        })]
    }
}
