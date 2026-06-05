use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use crate::document::{parse_document, ParsedDocument};
use crate::resolve::{Definition, WorkspaceIndex};
use crate::symbols::SymbolKind;
use crate::types::native_type_names;

pub const BUILTIN_ARRAY_URI: &str = "witcherscript-builtin:/array.ws";
pub const BUILTIN_ENUMS_URI: &str = "witcherscript-builtin:/enums.ws";
pub const BUILTIN_ORPHAN_ENUMS_URI: &str = "witcherscript-builtin:/orphan_enums.ws";
pub const BUILTIN_NATIVE_TYPES_URI: &str = "witcherscript-builtin:/native-types.ws";

pub const GENERIC_ELEMENT_PLACEHOLDER: &str = "T";

static BUILTIN_SOURCES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (BUILTIN_ARRAY_URI, include_str!("../builtins/array.ws")),
        (
            "witcherscript-builtin:/EInputKey.ws",
            include_str!("../builtins/EInputKey.ws"),
        ),
        (
            "witcherscript-builtin:/EShowFlags.ws",
            include_str!("../builtins/EShowFlags.ws"),
        ),
        (BUILTIN_ENUMS_URI, include_str!("../builtins/enums.ws")),
        (
            BUILTIN_ORPHAN_ENUMS_URI,
            include_str!("../builtins/orphan_enums.ws"),
        ),
        (
            "witcherscript-builtin:/CR4HudModule.ws",
            include_str!("../builtins/CR4HudModule.ws"),
        ),
        (
            "witcherscript-builtin:/CGuiObject.ws",
            include_str!("../builtins/CGuiObject.ws"),
        ),
        (
            "witcherscript-builtin:/unknown-classes.ws",
            include_str!("../builtins/unknown-classes.ws"),
        ),
        (
            "witcherscript-builtin:/unknown-enums.ws",
            include_str!("../builtins/unknown-enums.ws"),
        ),
        (
            "witcherscript-builtin:/unknown-interfaces.ws",
            include_str!("../builtins/unknown-interfaces.ws"),
        ),
        (
            "witcherscript-builtin:/unknown-structs.ws",
            include_str!("../builtins/unknown-structs.ws"),
        ),
    ])
});

/// `class` stubs for the native engine types, emitted from `NATIVE_TYPE_ACCEPTS` so the names live in one place.
static NATIVE_TYPES_SOURCE: LazyLock<String> = LazyLock::new(|| {
    native_type_names()
        .map(|name| format!("class {name} {{}}\n"))
        .collect()
});

/// `array` (only valid as `array<T>`) and the orphan-member bucket (a synthetic enum) are not bare-writable type names, so their types must stay out of type completion.
pub fn is_non_type_builtin(uri: &str) -> bool {
    uri == BUILTIN_ARRAY_URI || uri == BUILTIN_ORPHAN_ENUMS_URI
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

fn build_builtins_index() -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();
    for (&uri, &source) in BUILTIN_SOURCES.iter() {
        insert_builtin(&mut index, uri, source);
    }
    insert_builtin(
        &mut index,
        BUILTIN_NATIVE_TYPES_URI,
        NATIVE_TYPES_SOURCE.as_str(),
    );
    index
}

pub fn builtin_source(uri: &str) -> Option<&'static str> {
    if uri == BUILTIN_NATIVE_TYPES_URI {
        return Some(NATIVE_TYPES_SOURCE.as_str());
    }
    BUILTIN_SOURCES.get(uri).copied()
}

fn insert_builtin(index: &mut WorkspaceIndex, uri: &str, source: &str) {
    let mut doc: ParsedDocument =
        parse_document(source).expect("builtin sources must parse cleanly at build time");
    // Native engine types are stubbed as `class` since no native-type syntax exists; re-tag them.
    if uri == BUILTIN_NATIVE_TYPES_URI {
        doc.symbols
            .retag_top_level(SymbolKind::Class, SymbolKind::NativeType);
    }
    index.update_document(uri, &doc);
}

#[cfg(test)]
mod tests;
