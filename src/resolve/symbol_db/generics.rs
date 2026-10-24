use crate::builtins::{GENERIC_ELEMENT_PLACEHOLDER, MAP_KEY_PLACEHOLDER, MAP_VALUE_PLACEHOLDER};
use crate::resolve::Definition;
use crate::types::{Type, parse_generic_type};

pub(super) fn is_generic_instance(container: &str) -> bool {
    parse_generic_type(container).is_some()
}

pub(super) fn parse_generic_instance(container: &str) -> Option<Type> {
    is_generic_instance(container).then(|| Type::from_annotation(container))
}

fn placeholder_binding<'a>(placeholder: &str, container: &'a Type) -> Option<&'a Type> {
    match container {
        Type::Array(element) if placeholder == GENERIC_ELEMENT_PLACEHOLDER => Some(element),
        Type::Map(key, _) if placeholder == MAP_KEY_PLACEHOLDER => Some(key),
        Type::Map(_, value) if placeholder == MAP_VALUE_PLACEHOLDER => Some(value),
        Type::Array(_)
        | Type::Map(..)
        | Type::Void
        | Type::Primitive(_)
        | Type::Named(_)
        | Type::Null
        | Type::Unknown => None,
    }
}

pub(super) fn substitute_generics(ty: &Type, container: &Type) -> Type {
    match ty {
        Type::Named(name) => placeholder_binding(name, container).unwrap_or(ty).clone(),
        Type::Array(element) => Type::Array(Box::new(substitute_generics(element, container))),
        Type::Map(key, value) => Type::Map(
            Box::new(substitute_generics(key, container)),
            Box::new(substitute_generics(value, container)),
        ),
        Type::Void | Type::Primitive(_) | Type::Null | Type::Unknown => ty.clone(),
    }
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
