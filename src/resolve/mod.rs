use std::collections::HashSet;

use crate::symbols::Symbol;

mod assignability;
mod ast;
mod completion;
mod completion_catalog;
mod definition;
mod document_highlight;
mod extract_callable;
mod extract_common;
mod extract_var;
mod inference;
mod inlay_hints;
mod inline_var;
mod name_context;
mod overrides;
mod reaching_defs;
mod references;
mod shadowed_base;
mod signature;
mod state_classes;
mod subscription_registry;
mod symbol_db;
mod type_definition;
mod workspace_index;
mod workspace_symbols;

#[cfg(test)]
mod tests;

pub(crate) use assignability::{Assignability, assignability};
pub use ast::BUILTIN_TYPE_COMPLETIONS;
pub use completion::{
    ExpressionCompletions, OverrideBody, OverrideCompletion, StatementCompletions,
    annotation_arg_completions, annotation_name_completions, class_body_keyword_completions,
    class_header_keyword_completions, completion_members, default_or_hint_member_completions,
    expression_completions, extends_completions, merged_global_completions,
    new_lifetime_completions, new_type_completions, override_completions, position_in_comment,
    script_body_completions, state_owner_completions, statement_completions, type_completions,
    type_completions_arc,
};
pub use completion_catalog::{
    CompletionCatalog, global_catalog_changed, merge_ws_base, merge_ws_base_three,
};
pub(crate) use definition::callee_params;
pub use definition::{
    classify_definition_at_ident, resolve_all_definitions, resolve_definition,
    resolve_definition_at_byte, resolve_definition_at_ident,
};
pub use document_highlight::{HighlightKind, document_highlights};
pub use extract_callable::{extract_function, extract_method};
pub use extract_common::{Extraction, Splice};
pub use extract_var::extract_variable;
pub(crate) use inference::infer_type;
pub(crate) use inference::infer_type_memo;
pub use inlay_hints::{InlayHintInfo, inlay_hints};
pub use inline_var::{InlineConfidence, InlineScope, Inlining, inline_variable};
pub use name_context::{NameContext, classify_ident_context};
pub use overrides::{OverriddenSymbol, overridden_top_level};
pub use references::find_references;
pub use signature::{
    SignatureHelpInfo, hover_text, render_parameters, render_signature, signature_help,
};
pub use subscription_registry::SubscriptionRegistry;
pub use symbol_db::{FilteredBaseCatalogs, SymbolDb};
pub use type_definition::resolve_type_definition;
pub use workspace_index::{DocContribution, ObservedKey, WorkspaceIndex};
pub use workspace_symbols::workspace_symbols;

#[derive(Debug, Default, Clone)]
pub struct ObservationSet {
    pub top_level: HashSet<String>,
    pub members: HashSet<(String, String)>,
    pub enum_members: HashSet<String>,
}

impl ObservationSet {
    pub fn is_empty(&self) -> bool {
        self.top_level.is_empty() && self.members.is_empty() && self.enum_members.is_empty()
    }
}

// AGENTS.md key invariant #3.
pub(super) const MAX_INHERITANCE_DEPTH: usize = 32;

#[derive(Debug, Clone)]
pub struct Definition {
    pub uri: String,
    pub symbol: Symbol,
}

const MEMBER_INJECTING_ANNOTATIONS: &[&str] =
    &["addMethod", "wrapMethod", "replaceMethod", "addField"];

pub(crate) fn annotation_target_class(symbol: &Symbol) -> Option<&str> {
    symbol
        .annotations
        .iter()
        .find(|a| MEMBER_INJECTING_ANNOTATIONS.contains(&a.name.as_str()))
        .and_then(|a| a.argument.as_deref())
}

pub(super) fn dedup_by_name(defs: impl Iterator<Item = Definition>) -> Vec<Definition> {
    let mut seen: std::collections::HashMap<String, Definition> = std::collections::HashMap::new();
    for def in defs {
        seen.entry(def.symbol.name.clone()).or_insert(def);
    }
    seen.into_values().collect()
}

/// `Definition` has no `Eq`; identity is `(uri, selection byte range)`.
pub(super) fn dedup_definitions(defs: Vec<Definition>) -> Vec<Definition> {
    let mut seen: Vec<(String, std::ops::Range<usize>)> = Vec::new();
    let mut result = Vec::new();
    for def in defs {
        let key = (def.uri.clone(), def.symbol.selection_byte_range.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        result.push(def);
    }
    result
}
