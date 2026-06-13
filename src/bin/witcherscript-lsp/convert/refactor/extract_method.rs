use lsp_types::CodeActionOrCommand;
use witcherscript_language::resolve::extract_method;

use super::{RefactorContext, Refactoring, extraction_code_action};

pub(super) struct ExtractMethodRefactoring;

impl Refactoring for ExtractMethodRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        if ctx.selection.is_empty() {
            return Vec::new();
        }
        let Some(extraction) = extract_method(
            ctx.canonical_uri,
            ctx.document,
            ctx.db,
            ctx.selection.clone(),
            ctx.options,
        ) else {
            return Vec::new();
        };
        vec![extraction_code_action(
            ctx,
            &extraction,
            "Extract to method",
            "Rename extracted method",
        )]
    }
}
