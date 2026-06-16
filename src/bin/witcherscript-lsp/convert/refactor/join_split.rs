use lsp_types::{CodeActionKind, CodeActionOrCommand};
use witcherscript_language::resolve::{Confidence, join_declaration, split_declaration};

use super::{RefactorContext, Refactoring, refactor_action};

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
        let Some(plan) = join_declaration(model, ctx.cursor()) else {
            return Vec::new();
        };
        let title = match plan.confidence {
            Confidence::Verified => "Join declaration and assignment",
            Confidence::Unverified => "Join declaration and assignment (unsafe)",
        };
        vec![refactor_action(
            ctx,
            &plan,
            CodeActionKind::REFACTOR_REWRITE,
            title,
            None,
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
        let Some(plan) = split_declaration(model, ctx.cursor()) else {
            return Vec::new();
        };
        let title = match plan.confidence {
            Confidence::Verified => "Split declaration and initializer",
            Confidence::Unverified => "Split declaration and initializer (unsafe)",
        };
        vec![refactor_action(
            ctx,
            &plan,
            CodeActionKind::REFACTOR_REWRITE,
            title,
            None,
        )]
    }
}
