use lsp_types::CodeActionOrCommand;
use witcherscript_language::resolve::{join_declaration, split_declaration};

use super::{RefactorContext, Refactoring, splice_rewrite_action};

pub(super) struct JoinDeclarationRefactoring;

impl Refactoring for JoinDeclarationRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        // A selection means the user is extracting, so offer this only at a plain cursor.
        if !ctx.selection.is_empty() {
            return Vec::new();
        }
        let Some(model) = ctx.body_model() else {
            return Vec::new();
        };
        let Some(edits) = join_declaration(model, ctx.cursor()) else {
            return Vec::new();
        };
        vec![splice_rewrite_action(
            ctx,
            &edits,
            "Join declaration and assignment",
        )]
    }
}

pub(super) struct SplitDeclarationRefactoring;

impl Refactoring for SplitDeclarationRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        if !ctx.selection.is_empty() {
            return Vec::new();
        }
        let Some(model) = ctx.body_model() else {
            return Vec::new();
        };
        let Some(edits) = split_declaration(model, ctx.cursor()) else {
            return Vec::new();
        };
        vec![splice_rewrite_action(
            ctx,
            &edits,
            "Split declaration and initializer",
        )]
    }
}
