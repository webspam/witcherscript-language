use std::ops::Range;

use lsp_types::{CodeActionKind, CodeActionOrCommand};
use witcherscript_language::formatter::FormatOptions;
use witcherscript_language::resolve::{
    BodyModel, Extraction, extract_function, extract_method, extract_variable,
};

use super::{RefactorContext, Refactoring, RenameAfter, refactor_action};

type ExtractFn = fn(&BodyModel, Range<usize>, FormatOptions) -> Option<Extraction>;

fn extract_action(
    ctx: &RefactorContext,
    extract: ExtractFn,
    title: &str,
    rename_title: &str,
) -> Vec<CodeActionOrCommand> {
    if ctx.selection.is_empty() {
        return Vec::new();
    }
    let Some(model) = ctx.body_model() else {
        return Vec::new();
    };
    let Some(extraction) = extract(model, ctx.selection.clone(), ctx.options) else {
        return Vec::new();
    };
    vec![refactor_action(
        ctx,
        &extraction.plan,
        CodeActionKind::REFACTOR_EXTRACT,
        title,
        Some(RenameAfter {
            cursor: extraction.cursor,
            command_title: rename_title,
        }),
    )]
}

pub(super) struct ExtractVariableRefactoring;

impl Refactoring for ExtractVariableRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        extract_action(
            ctx,
            extract_variable,
            "Extract to variable",
            "Rename extracted variable",
        )
    }
}

pub(super) struct ExtractMethodRefactoring;

impl Refactoring for ExtractMethodRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        extract_action(
            ctx,
            extract_method,
            "Extract to method",
            "Rename extracted method",
        )
    }
}

pub(super) struct ExtractFunctionRefactoring;

impl Refactoring for ExtractFunctionRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        extract_action(
            ctx,
            extract_function,
            "Extract to function",
            "Rename extracted function",
        )
    }
}
