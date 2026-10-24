use crate::builtins::{GENERIC_ELEMENT_PLACEHOLDER, MAP_KEY_PLACEHOLDER, MAP_VALUE_PLACEHOLDER};
use crate::resolve::Definition;
use crate::types::{Type, parse_generic_type};

pub(super) fn is_generic_instance(container: &str) -> bool {
    parse_generic_type(container).is_some()
}

pub(super) fn parse_generic_instance(container: &str) -> Option<Type> {
    is_generic_instance(container).then(|| Type::from_annotation(container))
}

fn generic_bindings(container: &Type) -> Vec<(&'static str, &Type)> {
    match container {
        Type::Array(element) => vec![(GENERIC_ELEMENT_PLACEHOLDER, element)],
        Type::Map(key, value) => vec![(MAP_KEY_PLACEHOLDER, key), (MAP_VALUE_PLACEHOLDER, value)],
        _ => Vec::new(),
    }
}

pub(super) fn substitute_generics(ty: &Type, container: &Type) -> Type {
    ty.substitute_placeholders(&generic_bindings(container))
}

pub(super) fn substitute_in_definition(
    mut def: Definition,
    container_instance: &str,
    container: &Type,
) -> Definition {
    if let Some(t) = def.symbol.type_annotation.take() {
        def.symbol.type_annotation = Some(substitute_generics(&t, container));
    }
    if def.symbol.container_name.is_some() {
        def.symbol.container_name = Some(container_instance.to_string());
    }
    def
}
