use crate::document::{parse_document, ParsedDocument};
use crate::resolve::WorkspaceIndex;

const ARRAY_WS: &str = include_str!("../builtins/array.ws");
const ENUMS_WS: &str = include_str!("../builtins/enums.ws");

pub const BUILTIN_ARRAY_URI: &str = "witcherscript-builtin:/array.ws";
pub const BUILTIN_ENUMS_URI: &str = "witcherscript-builtin:/enums.ws";

pub const GENERIC_ELEMENT_PLACEHOLDER: &str = "T";

const CLASS_BUILTINS: &[(&str, &str)] = &[(
    "witcherscript-builtin:/CR4HudModule.ws",
    include_str!("../builtins/CR4HudModule.ws"),
)];

pub fn load_builtins_index() -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();
    insert_builtin(&mut index, BUILTIN_ARRAY_URI, ARRAY_WS);
    insert_builtin(&mut index, BUILTIN_ENUMS_URI, ENUMS_WS);
    for &(uri, source) in CLASS_BUILTINS {
        insert_builtin(&mut index, uri, source);
    }
    index
}

pub fn builtin_source(uri: &str) -> Option<&'static str> {
    match uri {
        BUILTIN_ARRAY_URI => Some(ARRAY_WS),
        BUILTIN_ENUMS_URI => Some(ENUMS_WS),
        _ => CLASS_BUILTINS
            .iter()
            .find(|(u, _)| *u == uri)
            .map(|&(_, source)| source),
    }
}

fn insert_builtin(index: &mut WorkspaceIndex, uri: &str, source: &'static str) {
    let doc: ParsedDocument =
        parse_document(source).expect("builtin sources must parse cleanly at build time");
    index.update_document(uri, &doc);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::SymbolKind;

    #[test]
    fn array_class_is_indexed() {
        let index = load_builtins_index();
        let def = index.find_top_level("array").expect("array class indexed");
        assert_eq!(def.symbol.kind, SymbolKind::Class);
        assert_eq!(def.uri, BUILTIN_ARRAY_URI);
    }

    #[test]
    fn array_members_are_indexed_with_placeholder_types() {
        let index = load_builtins_index();
        let push_back = index
            .direct_member_of("array", "PushBack", crate::symbols::AccessLevel::Public)
            .expect("PushBack indexed");
        assert_eq!(push_back.symbol.kind, SymbolKind::Method);
        let params = index.full_parameters_of(BUILTIN_ARRAY_URI, push_back.symbol.id);
        let types: Vec<_> = params
            .iter()
            .map(|s| s.type_annotation.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(types, vec!["T"]);
    }

    #[test]
    fn last_method_returns_placeholder() {
        let index = load_builtins_index();
        let last = index
            .direct_member_of("array", "Last", crate::symbols::AccessLevel::Public)
            .expect("Last indexed");
        assert_eq!(last.symbol.type_annotation.as_deref(), Some("T"));
    }
}
