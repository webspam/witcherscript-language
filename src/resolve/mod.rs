use std::collections::HashSet;

use crate::symbols::Symbol;

mod ast;
mod completion;
mod db;
mod definition;
mod inference;
mod references;
mod signature;

#[cfg(test)]
mod tests;

pub use ast::{BUILTIN_TYPES, BUILTIN_TYPE_COMPLETIONS};
pub use completion::{
    after_wrap_method_completions, annotation_arg_completions, annotation_name_completions,
    class_body_keyword_completions, class_header_keyword_completions, completion_members,
    expression_completions, extends_completions, script_body_completions, state_owner_completions,
    statement_completions, type_completions, AfterWrapMethodCompletions, ExpressionCompletions,
    StatementCompletions,
};
pub use db::{ObservedKey, SymbolDb, WorkspaceIndex};
pub use definition::{
    classify_definition_at_ident, resolve_all_definitions, resolve_definition,
    resolve_definition_at_byte, resolve_definition_at_ident,
};
pub use inference::infer_expr_type_memo;
pub use references::find_references;
pub use signature::{hover_text, signature_help, SignatureHelpInfo};

#[derive(Debug, Default, Clone)]
pub struct ObservationSet {
    pub top_level: HashSet<String>,
    pub members: HashSet<(String, String)>,
    pub enum_variants: HashSet<String>,
}

impl ObservationSet {
    pub fn is_empty(&self) -> bool {
        self.top_level.is_empty() && self.members.is_empty() && self.enum_variants.is_empty()
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

pub(super) fn annotation_target_class(symbol: &Symbol) -> Option<&str> {
    symbol
        .annotations
        .iter()
        .find(|a| MEMBER_INJECTING_ANNOTATIONS.contains(&a.name.as_str()))
        .and_then(|a| a.argument.as_deref())
}

pub fn parse_generic_type(s: &str) -> Option<(&str, &str)> {
    let trimmed = s.trim();
    let lt = trimmed.find('<')?;
    if !trimmed.ends_with('>') {
        return None;
    }
    let ctor = trimmed[..lt].trim();
    let element = trimmed[lt + 1..trimmed.len() - 1].trim();
    if ctor.is_empty() || element.is_empty() {
        return None;
    }
    Some((ctor, element))
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
