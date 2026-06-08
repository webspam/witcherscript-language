use lsp_types::CodeActionOrCommand;
use witcherscript_language::formatter::{
    analyze_switch, format_switch_with_layout, switch_stmt_at, SwitchLayout,
};

use super::{Preference, RefactorContext, Refactoring};

const COLLAPSE_TITLE: &str = "Collapse switch cases to a single line";
const EXPAND_TITLE: &str = "Expand switch cases onto multiple lines";

pub(super) struct SwitchLayoutRefactoring;

impl Refactoring for SwitchLayoutRefactoring {
    fn actions(&self, ctx: &RefactorContext) -> Vec<CodeActionOrCommand> {
        let Some(switch) = switch_stmt_at(ctx.root(), ctx.cursor()) else {
            return Vec::new();
        };
        let options = ctx.options();
        let toggle = analyze_switch(switch, ctx.source(), options);
        let mut actions = Vec::new();
        if toggle.can_collapse {
            let text =
                format_switch_with_layout(switch, ctx.source(), options, SwitchLayout::Collapse);
            actions.push(ctx.rewrite(COLLAPSE_TITLE, switch, text, Preference::Preferred));
        }
        if toggle.can_expand {
            let text =
                format_switch_with_layout(switch, ctx.source(), options, SwitchLayout::Expand);
            actions.push(ctx.rewrite(EXPAND_TITLE, switch, text, Preference::Alternative));
        }
        actions
    }
}
