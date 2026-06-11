use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Command, Position, TextEdit, Url,
    WorkspaceEdit,
};
use witcherscript_language::line_index::LineIndex;
use witcherscript_language::resolve::{VariableExtraction, extract_variable};

use super::super::lsp_range;
use super::{RefactorContext, Refactoring};

// A bare rename races VS Code's cursor placement, so a custom command repositions before renaming.
const EXTRACT_COMMAND: &str = "witcherscript.extractVariable";

// Post-edit position of the original selection's left-most byte, now the start of the new var name.
fn rename_position(source: &str, extraction: &VariableExtraction) -> Position {
    let mut applied =
        String::with_capacity(source.len() + extraction.new_text.len() + extraction.name.len());
    applied.push_str(&source[..extraction.insert_at]);
    applied.push_str(&extraction.new_text);
    applied.push_str(&source[extraction.insert_at..extraction.replace_range.start]);
    applied.push_str(&extraction.name);
    applied.push_str(&source[extraction.replace_range.end..]);

    let byte = extraction.replace_range.start + extraction.new_text.len();
    let p = LineIndex::new(&applied).byte_to_position(&applied, byte);
    Position {
        line: p.line,
        character: p.character,
    }
}

fn extract_command(uri: &Url, position: Position) -> Command {
    Command {
        title: "Rename extracted variable".to_string(),
        command: EXTRACT_COMMAND.to_string(),
        arguments: Some(vec![
            serde_json::to_value(uri).expect("Url serializes"),
            serde_json::to_value(position).expect("Position serializes"),
        ]),
    }
}

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
        let insert = TextEdit {
            range: lsp_range(line_index.byte_range_to_range(
                source,
                extraction.insert_at,
                extraction.insert_at,
            )),
            new_text: extraction.new_text,
        };
        let replace = TextEdit {
            range: lsp_range(line_index.byte_range_to_range(
                source,
                extraction.replace_range.start,
                extraction.replace_range.end,
            )),
            new_text: extraction.name,
        };
        let mut changes = HashMap::new();
        changes.insert(ctx.uri.clone(), vec![insert, replace]);
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
