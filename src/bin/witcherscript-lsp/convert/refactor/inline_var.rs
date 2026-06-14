use lsp_types::CodeActionOrCommand;
use witcherscript_language::resolve::{InlineScope, inline_variable};

use super::{RefactorContext, Refactoring, inline_code_action};

pub(super) struct InlineVariableRefactoring;

impl Refactoring for InlineVariableRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        let Some(inlining) = inline_variable(ctx.canonical_uri, ctx.document, ctx.db, ctx.cursor())
        else {
            return Vec::new();
        };
        let title = match inlining.scope {
            InlineScope::AllUsages => "Inline variable (all)",
            InlineScope::SingleUsage => "Inline variable",
        };
        vec![inline_code_action(ctx, &inlining, title)]
    }
}
