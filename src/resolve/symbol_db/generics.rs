use crate::resolve::Definition;
use crate::types::Type;

pub(super) fn generic_lookup_target(container: &str) -> (&str, Option<&str>) {
    match crate::types::parse_generic_type(container) {
        Some((ctor, elem)) => (ctor, Some(elem)),
        None => (container, None),
    }
}

pub(super) fn substitute_in_definition(
    mut def: Definition,
    container_instance: &str,
    element: &str,
) -> Definition {
    let p = crate::builtins::GENERIC_ELEMENT_PLACEHOLDER;
    if let Some(t) = def.symbol.type_annotation.take() {
        let element_type = Type::from_annotation(element);
        def.symbol.type_annotation = Some(t.substitute_named(p, &element_type));
    }
    if def.symbol.container_name.is_some() {
        def.symbol.container_name = Some(container_instance.to_string());
    }
    def
}
