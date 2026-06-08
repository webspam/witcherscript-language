mod base_shadowing;
mod builtin_array;
mod builtin_classes;
mod builtin_enums;
mod builtin_native_types;
mod chaining;
mod completion_annotation_arg;
mod completion_annotation_body;
mod completion_annotation_name;
mod completion_annotation_replace_global;
mod completion_annotation_wrap;
mod completion_comment;
mod completion_default_hint;
mod completion_keywords;
mod completion_members;
mod completion_new;
mod completion_script_keywords;
mod completion_statement;
mod completion_type;
mod definition;
mod index;
mod inheritance;
mod inlay_hints;
mod overrides;
mod parameters;
mod references;
mod script_globals;
mod signature_help;
mod state_classes;

use crate::document::{ParsedDocument, parse_document};

pub(super) fn make_doc(source: &str) -> ParsedDocument {
    parse_document(source).expect("parse should succeed")
}
