use std::cell::OnceCell;
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
use witcherscript_language::resolve::{BodyModel, EditPlan, Splice, SymbolDb};

use super::lsp_range;

mod extract;
mod if_stmt;
mod inline_var;
mod join_split;
mod switch;

// A bare rename races VS Code's cursor placement, so a custom command repositions before renaming.
// The command is extraction-agnostic; reusing it for every extract action spares an extension release.
const EXTRACT_COMMAND: &str = "witcherscript.extractVariable";

struct RenameAfter<'a> {
    cursor: usize,
    command_title: &'a str,
}

fn rename_position(source: &str, plan: &EditPlan, cursor: usize) -> Position {
    let applied = plan.apply(source);
    let p = LineIndex::new(&applied).byte_to_position(&applied, cursor);
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

fn refactor_action(
    ctx: &RefactorContext,
    plan: &EditPlan,
    kind: CodeActionKind,
    title: &str,
    rename: Option<RenameAfter>,
) -> CodeActionOrCommand {
    let command = rename.map(|r| {
        let position = rename_position(&ctx.document.source, plan, r.cursor);
        extract_command(r.command_title, ctx.uri, position)
    });
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(kind),
        edit: Some(workspace_edit_from_splices(ctx, &plan.edits)),
        command,
        ..CodeAction::default()
    })
}

// Adding a construct means writing a `Refactoring` impl and listing it here.
const REFACTORINGS: &[&dyn Refactoring] = &[
    &switch::SwitchLayoutRefactoring,
    &if_stmt::IfLayoutRefactoring,
    &extract::ExtractVariableRefactoring,
    &extract::ExtractMethodRefactoring,
    &extract::ExtractFunctionRefactoring,
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
        body_model: OnceCell::new(),
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
    // Built once per request, then borrowed by every body-level action (dispatch is single-threaded).
    body_model: OnceCell<Option<BodyModel<'a>>>,
}

impl<'a> RefactorContext<'a> {
    fn root(&self) -> Node<'a> {
        self.document.tree.root_node()
    }

    fn cursor(&self) -> usize {
        self.selection.start
    }

    fn body_model(&self) -> Option<&BodyModel<'a>> {
        self.body_model
            .get_or_init(|| {
                BodyModel::enclosing(self.canonical_uri, self.document, self.db, self.cursor())
            })
            .as_ref()
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
        let splice = Splice {
            range: node.byte_range(),
            text: new_text,
        };
        CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_string(),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            edit: Some(workspace_edit_from_splices(self, &[splice])),
            is_preferred: matches!(preference, Preference::Preferred).then_some(true),
            ..CodeAction::default()
        })
    }
}
