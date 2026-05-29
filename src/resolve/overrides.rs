use crate::line_index::SourceRange;
use crate::symbols::Symbol;

use super::{Definition, WorkspaceIndex};

/// A workspace top-level declaration that shadows a same-named base-game symbol.
#[derive(Debug, Clone)]
pub struct OverriddenSymbol {
    /// Selection range of the local declaration; the lens anchor.
    pub range: SourceRange,
    /// The base-game symbol the local declaration overrides.
    pub base: Definition,
}

/// Pair each top-level symbol in `symbols` with the base-game definition it shadows, if any.
///
/// `base` is queried directly rather than through `shadowed_base()`: a legacy override file's
/// vanilla URI is suppressed there, which would hide the very symbol the lens navigates to.
pub fn overridden_top_level(symbols: &[Symbol], base: &WorkspaceIndex) -> Vec<OverriddenSymbol> {
    symbols
        .iter()
        .filter(|sym| sym.container.is_none())
        .filter_map(|sym| {
            base_match(base, sym).map(|base| OverriddenSymbol {
                range: sym.selection_range,
                base,
            })
        })
        .collect()
}

fn base_match(base: &WorkspaceIndex, sym: &Symbol) -> Option<Definition> {
    base.all_top_level_with_name(&sym.name)
        .iter()
        .find(|def| def.symbol.kind == sym.kind)
        .cloned()
        .or_else(|| base.find_top_level(&sym.name))
}
