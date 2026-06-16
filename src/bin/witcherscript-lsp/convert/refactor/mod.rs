use std::collections::HashMap;
use std::ops::Range;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Command, Position, TextEdit, Url,
    WorkspaceEdit,
};
use tree_sitter::Node;
use witcherscript_language::document::ParsedDocument;
use witcherscript_language::formatter::FormatOptions;
use witcherscript_language::line_index::LineIndex;
use witcherscript_language::resolve::{Extraction, Inlining, Splice, SymbolDb};

use super::lsp_range;

mod extract_func;
mod extract_method;
mod extract_var;
mod if_stmt;
mod inline_var;
mod join_split;
mod switch;

// A bare rename races VS Code's cursor placement, so a custom command repositions before renaming.
// The command is extraction-agnostic; reusing it for every extract action spares an extension release.
const EXTRACT_COMMAND: &str = "witcherscript.extractVariable";

fn rename_position(source: &str, extraction: &Extraction) -> Position {
    let applied = extraction.apply(source);
    let p = LineIndex::new(&applied).byte_to_position(&applied, extraction.cursor);
    Position {
        line: p.line,
        character: p.character,
    }
}

fn extract_command(title: &str, uri: &Url, position: Position) -> Command {
    Command {
        title: title.to_string(),
        command: EXTRACT_COMMAND.to_string(),
        arguments: Some(vec![
            serde_json::to_value(uri).expect("Url serializes"),
            serde_json::to_value(position).expect("Position serializes"),
        ]),
    }
}

fn workspace_edit_from_splices(ctx: &RefactorContext, splices: &[Splice]) -> WorkspaceEdit {
    let source = &ctx.document.source;
    let line_index = &ctx.document.line_index;
    let edits = splices
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
    WorkspaceEdit {
        changes: Some(changes),
        ..WorkspaceEdit::default()
    }
}

fn extraction_code_action(
    ctx: &RefactorContext,
    extraction: &Extraction,
    title: &str,
    command_title: &str,
) -> CodeActionOrCommand {
    let position = rename_position(&ctx.document.source, extraction);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(workspace_edit_from_splices(ctx, &extraction.edits)),
        command: Some(extract_command(command_title, ctx.uri, position)),
        ..CodeAction::default()
    })
}

fn inline_code_action(
    ctx: &RefactorContext,
    inlining: &Inlining,
    title: &str,
) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::REFACTOR_INLINE),
        edit: Some(workspace_edit_from_splices(ctx, &inlining.edits)),
        ..CodeAction::default()
    })
}

fn splice_rewrite_action(
    ctx: &RefactorContext,
    splices: &[Splice],
    title: &str,
) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(workspace_edit_from_splices(ctx, splices)),
        ..CodeAction::default()
    })
}

// Adding a construct means writing a `Refactoring` impl and listing it here.
const REFACTORINGS: &[&dyn Refactoring] = &[
    &switch::SwitchLayoutRefactoring,
    &if_stmt::IfLayoutRefactoring,
    &extract_var::ExtractVariableRefactoring,
    &extract_method::ExtractMethodRefactoring,
    &extract_func::ExtractFunctionRefactoring,
    &inline_var::InlineVariableRefactoring,
    &join_split::JoinDeclarationRefactoring,
    &join_split::SplitDeclarationRefactoring,
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
