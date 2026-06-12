use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Command, Position, TextEdit, Url,
    WorkspaceEdit,
};
use witcherscript_language::line_index::LineIndex;
use witcherscript_language::resolve::{Splice, VariableExtraction, extract_variable};

use super::super::lsp_range;
use super::{RefactorContext, Refactoring};

// A bare rename races VS Code's cursor placement, so a custom command repositions before renaming.
const EXTRACT_COMMAND: &str = "witcherscript.extractVariable";

fn apply_edits(source: &str, edits: &[Splice]) -> String {
    let mut splices: Vec<&Splice> = edits.iter().collect();
    splices.sort_by_key(|s| std::cmp::Reverse(s.range.start));
    let mut applied = source.to_string();
    for splice in splices {
        applied.replace_range(splice.range.clone(), &splice.text);
    }
    applied
}

// Post-edit position of the original selection's left-most byte, now the start of the new var name.
fn rename_position(source: &str, extraction: &VariableExtraction) -> Position {
    let shift: usize = extraction
        .edits
        .iter()
        .filter(|s| s.range.end <= extraction.name_anchor)
        .map(|s| s.text.len() - s.range.len())
        .sum();
    let applied = apply_edits(source, &extraction.edits);
    let byte = extraction.name_anchor + shift;
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
