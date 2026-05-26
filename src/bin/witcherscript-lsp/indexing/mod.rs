mod helpers;
mod legacy;
mod open_documents;
mod scan;

pub(crate) use helpers::remove_document_all_spellings;

#[cfg(test)]
pub(crate) use helpers::{
    build_index_segments, index_open_document, legacy_base_replacements, legacy_replaces_base,
    mod_shared_imports_dir,
};
