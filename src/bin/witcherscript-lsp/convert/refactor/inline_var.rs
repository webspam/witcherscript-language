use lsp_types::{CodeActionKind, CodeActionOrCommand};
use witcherscript_language::resolve::{Confidence, InlineScope, inline_variable};

use super::{RefactorContext, Refactoring, splice_code_action};

pub(super) struct InlineVariableRefactoring;

impl Refactoring for InlineVariableRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        let Some(model) = ctx.body_model() else {
            return Vec::new();
        };
        let Some(inlining) = inline_variable(model, ctx.cursor()) else {
            return Vec::new();
        };
        let title = match (&inlining.scope, &inlining.plan.confidence) {
            (InlineScope::AllUsages, Confidence::Verified) => "Inline variable (all)",
            (InlineScope::AllUsages, Confidence::Unverified) => "Inline variable (all, unsafe)",
            (InlineScope::SingleUsage, Confidence::Verified) => "Inline variable",
            (InlineScope::SingleUsage, Confidence::Unverified) => "Inline variable (unsafe)",
        };
        vec![splice_code_action(
            ctx,
            &inlining.plan.edits,
            CodeActionKind::REFACTOR_INLINE,
            title,
        )]
    }
}
