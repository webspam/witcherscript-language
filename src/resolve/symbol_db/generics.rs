use crate::resolve::Definition;

pub(super) fn generic_lookup_target(container: &str) -> (&str, Option<&str>) {
    match super::super::parse_generic_type(container) {
        Some((ctor, elem)) => (ctor, Some(elem)),
        None => (container, None),
    }
}

fn substitute_placeholder(s: &str, placeholder: &str, replacement: &str) -> String {
    let bytes = s.as_bytes();
    let plen = placeholder.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(placeholder.as_bytes()) {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + plen;
            let after_ok = after_idx >= bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                out.push_str(replacement);
                i += plen;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub(super) fn substitute_in_definition(
    mut def: Definition,
    container_instance: &str,
    element: &str,
) -> Definition {
    let p = crate::builtins::GENERIC_ELEMENT_PLACEHOLDER;
    if let Some(t) = def.symbol.type_annotation.take() {
        def.symbol.type_annotation = Some(substitute_placeholder(&t, p, element));
    }
    if let Some(s) = def.symbol.signature.take() {
        def.symbol.signature = Some(substitute_placeholder(&s, p, element));
    }
    if def.symbol.container_name.is_some() {
        def.symbol.container_name = Some(container_instance.to_string());
    }
    def
}
