use lsp_types::CodeActionOrCommand;
use witcherscript_language::formatter::{
    analyze_if, format_if_with_layout, if_stmt_on_keyword, IfLayout,
};

use super::{RefactorContext, Refactoring};

const COLLAPSE_TITLE: &str = "Collapse if/else to single-line bodies";
const EXPAND_TITLE: &str = "Expand if/else to block bodies";

pub(super) struct IfLayoutRefactoring;

impl Refactoring for IfLayoutRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        let Some(if_node) = if_stmt_on_keyword(ctx.root(), ctx.cursor()) else {
            return Vec::new();
        };
        let options = ctx.options();
        let toggle = analyze_if(if_node, ctx.source(), options);
        let mut actions = Vec::new();
        if toggle.can_collapse {
            let text = format_if_with_layout(if_node, ctx.source(), options, IfLayout::Collapse);
            actions.push(ctx.rewrite(COLLAPSE_TITLE, if_node, text, true));
        }
        if toggle.can_expand {
            let text = format_if_with_layout(if_node, ctx.source(), options, IfLayout::Expand);
            actions.push(ctx.rewrite(EXPAND_TITLE, if_node, text, false));
        }
        actions
    }
}
