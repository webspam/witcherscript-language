mod native;
mod native_layout;

use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use crate::document::{ParsedDocument, parse_document};
use crate::resolve::{Definition, WorkspaceIndex};
use crate::symbols::SymbolOrigin;

const BUILTIN_URI_PREFIX: &str = "witcherscript-builtin:/";
pub const BUILTIN_ARRAY_URI: &str = "witcherscript-builtin:/array.ws";

pub const GENERIC_ELEMENT_PLACEHOLDER: &str = "T";

const BUILTIN_DOCUMENT_EXTENSION: &str = ".ws";

const ARRAY_SOURCE: &str = include_str!("../builtins/array.ws");

pub(crate) fn builtin_uri(name: &str) -> String {
    format!("{BUILTIN_URI_PREFIX}{name}{BUILTIN_DOCUMENT_EXTENSION}")
}

pub fn is_builtin_uri(uri: &str) -> bool {
    uri.starts_with(BUILTIN_URI_PREFIX)
}

/// `array` (only valid as `array<T>`) is not a bare-writable type name, so its type must stay out of type completion.
pub fn is_non_type_builtin(uri: &str) -> bool {
    uri == BUILTIN_ARRAY_URI
}

static BUILTINS: LazyLock<(WorkspaceIndex, Arc<[Definition]>)> = LazyLock::new(|| {
    let index = build_builtins_index();
    let types = Arc::from(
        index
            .types_catalog()
            .iter()
            .filter(|d| !is_non_type_builtin(&d.uri))
            .cloned()
            .collect::<Vec<_>>(),
    );
    (index, types)
});

pub fn types_completion_catalog() -> Arc<[Definition]> {
    BUILTINS.1.clone()
}

pub fn load_builtins_index() -> WorkspaceIndex {
    BUILTINS.0.clone()
}

fn array_document() -> ParsedDocument {
    // Invariant: array.ws is a fixed embedded source covered by the builtins tests
    let mut array =
        parse_document(ARRAY_SOURCE).expect("builtin sources must parse cleanly at build time");
    array.symbols.set_origin(SymbolOrigin::Builtin);
    array
}

fn build_builtins_index() -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();
    index.begin_bulk_catalog_update();
    for (name, symbols) in native::documents() {
        index.insert_synthetic(builtin_uri(name), symbols);
    }
    index.update_document(BUILTIN_ARRAY_URI, &array_document());
    index.end_bulk_catalog_update();
    index
}

pub fn builtin_source(uri: &str) -> Option<Cow<'static, str>> {
    if uri == BUILTIN_ARRAY_URI {
        return Some(Cow::Borrowed(ARRAY_SOURCE));
    }
    let name = uri
        .strip_prefix(BUILTIN_URI_PREFIX)?
        .strip_suffix(BUILTIN_DOCUMENT_EXTENSION)?;
    native::render(name).map(Cow::Owned)
}

#[cfg(test)]
mod tests;
