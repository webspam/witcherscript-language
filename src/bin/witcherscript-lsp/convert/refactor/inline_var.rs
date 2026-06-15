use lsp_types::CodeActionOrCommand;
use witcherscript_language::resolve::{InlineConfidence, InlineScope, inline_variable};

use super::{RefactorContext, Refactoring, inline_code_action};

pub(super) struct InlineVariableRefactoring;

impl Refactoring for InlineVariableRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        let Some(inlining) = inline_variable(ctx.canonical_uri, ctx.document, ctx.db, ctx.cursor())
        else {
            return Vec::new();
        };
        let title = match (&inlining.scope, &inlining.confidence) {
            (InlineScope::AllUsages, InlineConfidence::Verified) => "Inline variable (all)",
            (InlineScope::AllUsages, InlineConfidence::Unverified) => {
                "Inline variable (all, unsafe)"
            }
            (InlineScope::SingleUsage, InlineConfidence::Verified) => "Inline variable",
            (InlineScope::SingleUsage, InlineConfidence::Unverified) => "Inline variable (unsafe)",
        };
        vec![inline_code_action(ctx, &inlining, title)]
    }
}
