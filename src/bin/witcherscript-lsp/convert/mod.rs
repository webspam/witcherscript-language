mod completions;
mod diagnostics;
mod file_ops;
mod highlights;
mod inlay_hints;
mod positions;
mod refactor;
mod symbols;

pub(crate) use completions::{
    annotation_name_items, builtin_type_item, class_body_kw_item, completion_item,
    keyword_snippet_item, replace_method_snippet, script_body_item, signature_help_response,
    this_super_item, type_completion_item, wrap_method_snippet,
};
pub(crate) use diagnostics::{
    base_script_conflict_code_actions, lsp_diagnostics, lsp_workspace_diagnostic,
};
pub(crate) use file_ops::{
    created_files_to_watched, deleted_files_to_watched, renamed_files_to_watched, workspace_roots,
};
pub(crate) use highlights::document_highlight;
pub(crate) use inlay_hints::inlay_hint;
pub(crate) use positions::{lsp_range, source_position, source_range};
pub(crate) use refactor::refactor_code_actions;
pub(crate) use symbols::{document_symbols, hover_markdown, workspace_symbol};

#[cfg(test)]
mod file_operation_conversion;
