mod native;
mod native_layout;

use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use crate::document::{ParsedDocument, parse_document};
use crate::resolve::{Definition, WorkspaceIndex};
use crate::symbols::SymbolOrigin;

const BUILTIN_URI_PREFIX: &str = "witcherscript-builtin:/";
pub const BUILTIN_ARRAY_URI: &str = "witcherscript-builtin:/array.ws";
pub const BUILTIN_MAP_URI: &str = "witcherscript-builtin:/map.ws";

pub const GENERIC_ELEMENT_PLACEHOLDER: &str = "T";
pub const MAP_KEY_PLACEHOLDER: &str = "K";

const BUILTIN_DOCUMENT_EXTENSION: &str = ".ws";

const GENERIC_SOURCES: &[(&str, &str)] = &[
    (BUILTIN_ARRAY_URI, include_str!("../builtins/array.ws")),
    (BUILTIN_MAP_URI, include_str!("../builtins/map.ws")),
];

pub(crate) fn builtin_uri(name: &str) -> String {
    format!("{BUILTIN_URI_PREFIX}{name}{BUILTIN_DOCUMENT_EXTENSION}")
}

pub fn is_builtin_uri(uri: &str) -> bool {
    uri.starts_with(BUILTIN_URI_PREFIX)
}

fn generic_source(uri: &str) -> Option<&'static str> {
    GENERIC_SOURCES
        .iter()
        .find(|(generic_uri, _)| *generic_uri == uri)
        .map(|(_, source)| *source)
}

/// `array` and `map` (only valid with type arguments) are not bare-writable type names, so they must stay out of type completion.
pub fn is_non_type_builtin(uri: &str) -> bool {
    generic_source(uri).is_some()
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

fn generic_document(source: &str) -> ParsedDocument {
    // Invariant: generic builtin sources are fixed embedded sources covered by the builtins tests
    let mut document =
        parse_document(source).expect("builtin sources must parse cleanly at build time");
    document.symbols.set_origin(SymbolOrigin::Builtin);
    document
}

fn build_builtins_index() -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();
    index.begin_bulk_catalog_update();
    for (name, symbols) in native::documents() {
        index.insert_synthetic(builtin_uri(name), symbols);
    }
    for &(uri, source) in GENERIC_SOURCES {
        index.update_document(uri, &generic_document(source));
    }
    index.end_bulk_catalog_update();
    index
}

pub fn builtin_source(uri: &str) -> Option<Cow<'static, str>> {
    if let Some(source) = generic_source(uri) {
        return Some(Cow::Borrowed(source));
    }
    let name = uri
        .strip_prefix(BUILTIN_URI_PREFIX)?
        .strip_suffix(BUILTIN_DOCUMENT_EXTENSION)?;
    native::render(name).map(Cow::Owned)
}

#[cfg(test)]
mod tests;
