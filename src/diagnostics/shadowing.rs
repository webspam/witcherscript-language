use std::collections::HashMap;

use crate::resolve::WorkspaceIndex;
use crate::script_env::ScriptEnvironment;
use crate::symbols::{enclosing_callable_id, AccessLevel, Symbol, SymbolKind};

use super::{RelatedLocation, Severity, WorkspaceDiagnostic};

const EXEMPT_ANNOTATIONS: &[&str] = &["wrapMethod", "replaceMethod"];

pub fn collect_shadowing_diagnostics(
    index: &WorkspaceIndex,
    script_env: &ScriptEnvironment,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();

    for (uri, syms) in index.documents() {
        for sym in syms {
            if is_in_exempt_callable(sym, syms) {
                continue;
            }
            match sym.kind {
                SymbolKind::Parameter | SymbolKind::Variable => {
                    if let Some(diag) = script_global_shadow(sym, script_env) {
                        result.entry(uri.to_string()).or_default().push(diag);
                    }
                    if sym.kind == SymbolKind::Variable {
                        if let Some(diag) = class_field_shadow(sym, syms, index) {
                            result.entry(uri.to_string()).or_default().push(diag);
                        }
                    }
                }
                SymbolKind::Field => {
                    if let Some(diag) = field_shadows_script_global(sym, script_env) {
                        result.entry(uri.to_string()).or_default().push(diag);
                    }
                }
                _ => {}
            }
        }
    }

    result
}

fn script_global_shadow(sym: &Symbol, env: &ScriptEnvironment) -> Option<WorkspaceDiagnostic> {
    let global = env.find(&sym.name)?;
    let label = if sym.kind == SymbolKind::Parameter {
        "Parameter"
    } else {
        "Local"
    };
    Some(WorkspaceDiagnostic {
        kind: "shadows_script_global".to_string(),
        message: format!(
            "{label} '{}' shadows the engine global declared in redscripts.ini",
            sym.name
        ),
        severity: Severity::Warning,
        range: sym.selection_range,
        related: vec![RelatedLocation {
            uri: global.ini_uri.clone(),
            range: global.symbol.selection_range,
            message: format!("'{}' declared here", sym.name),
        }],
        data: None,
    })
}

fn field_shadows_script_global(
    sym: &Symbol,
    env: &ScriptEnvironment,
) -> Option<WorkspaceDiagnostic> {
    let global = env.find(&sym.name)?;
    Some(WorkspaceDiagnostic {
        kind: "shadows_script_global".to_string(),
        message: format!(
            "Field '{}' shadows the engine global declared in redscripts.ini",
            sym.name
        ),
        severity: Severity::Warning,
        range: sym.selection_range,
        related: vec![RelatedLocation {
            uri: global.ini_uri.clone(),
            range: global.symbol.selection_range,
            message: format!("'{}' declared here", sym.name),
        }],
        data: None,
    })
}

fn class_field_shadow(
    sym: &Symbol,
    doc_symbols: &[Symbol],
    index: &WorkspaceIndex,
) -> Option<WorkspaceDiagnostic> {
    let callable = doc_symbols.get(enclosing_callable_id(doc_symbols, sym)?.0)?;
    let class_sym = callable.container.and_then(|id| doc_symbols.get(id.0))?;
    if !matches!(
        class_sym.kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::State
    ) {
        return None;
    }
    let field = index.direct_member_of(class_sym.name.as_str(), &sym.name, AccessLevel::Private)?;
    if field.symbol.kind != SymbolKind::Field {
        return None;
    }
    Some(WorkspaceDiagnostic {
        kind: "shadows_class_field".to_string(),
        message: format!(
            "Local '{}' shadows field declared in {}",
            sym.name, class_sym.name
        ),
        severity: Severity::Warning,
        range: sym.selection_range,
        related: vec![RelatedLocation {
            uri: field.uri.clone(),
            range: field.symbol.selection_range,
            message: format!("Field '{}' declared here", sym.name),
        }],
        data: None,
    })
}

fn is_in_exempt_callable(sym: &Symbol, doc_symbols: &[Symbol]) -> bool {
    let mut current = sym.container;
    while let Some(id) = current {
        let Some(parent) = doc_symbols.get(id.0) else {
            return false;
        };
        if matches!(
            parent.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Event
        ) && parent
            .annotations
            .iter()
            .any(|a| EXEMPT_ANNOTATIONS.contains(&a.name.as_str()))
        {
            return true;
        }
        current = parent.container;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::collect_shadowing_diagnostics;
    use crate::document::parse_document;
    use crate::line_index::{SourcePosition, SourceRange};
    use crate::resolve::WorkspaceIndex;
    use crate::script_env::{ScriptEnvironment, ScriptGlobal};
    use crate::symbols::{AccessLevel, Symbol, SymbolId, SymbolKind};

    fn index(docs: &[(&str, &str)]) -> WorkspaceIndex {
        let mut idx = WorkspaceIndex::default();
        for (uri, src) in docs {
            let doc = parse_document(*src).expect("parse should succeed");
            idx.update_document(*uri, &doc);
        }
        idx
    }

    fn env(names_and_types: &[(&str, &str)]) -> ScriptEnvironment {
        let globals = names_and_types
            .iter()
            .map(|(name, ty)| ScriptGlobal {
                name: name.to_string(),
                type_name: ty.to_string(),
                ini_uri: "file:///redscripts.ini".to_string(),
                symbol: Symbol {
                    id: SymbolId(0),
                    name: name.to_string(),
                    kind: SymbolKind::Variable,
                    range: dummy_range(),
                    selection_range: dummy_range(),
                    byte_range: 0..0,
                    selection_byte_range: 0..0,
                    container: None,
                    container_name: None,
                    type_annotation: Some(ty.to_string()),
                    signature: None,
                    base_class: None,
                    owner_class: None,
                    flavour: None,
                    annotations: Vec::new(),
                    access: AccessLevel::Public,
                    is_optional: false,
                    is_out: false,
                    is_state_machine: false,
                },
            })
            .collect();
        ScriptEnvironment::new(globals)
    }

    fn dummy_range() -> SourceRange {
        SourceRange {
            start: SourcePosition {
                line: 0,
                character: 0,
            },
            end: SourcePosition {
                line: 0,
                character: 0,
            },
        }
    }

    #[test]
    fn param_shadows_script_global() {
        let idx = index(&[("file:///a.ws", "function F(thePlayer : CR4Player) {}\n")]);
        let env = env(&[("thePlayer", "CR4Player")]);

        let result = collect_shadowing_diagnostics(&idx, &env);

        let a = result.get("file:///a.ws").expect("a.ws flagged");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, "shadows_script_global");
        assert!(a[0].message.contains("thePlayer"));
        assert_eq!(a[0].related.len(), 1);
        assert_eq!(a[0].related[0].uri, "file:///redscripts.ini");
    }

    #[test]
    fn local_shadows_script_global() {
        let idx = index(&[(
            "file:///a.ws",
            "function F() {\n  var theGame : CR4Game;\n}\n",
        )]);
        let env = env(&[("theGame", "CR4Game")]);

        let result = collect_shadowing_diagnostics(&idx, &env);

        let a = result.get("file:///a.ws").expect("a.ws flagged");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, "shadows_script_global");
        assert!(a[0].message.contains("theGame"));
    }

    #[test]
    fn local_shadows_enclosing_class_field() {
        let idx = index(&[(
            "file:///a.ws",
            "class C {\n  var x : int;\n  function F() {\n    var x : int;\n  }\n}\n",
        )]);
        let env = env(&[]);

        let result = collect_shadowing_diagnostics(&idx, &env);

        let a = result.get("file:///a.ws").expect("a.ws flagged");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, "shadows_class_field");
        assert!(a[0].message.contains("Local 'x'"));
        assert!(a[0].message.contains("C"));
    }

    #[test]
    fn field_shadows_script_global() {
        let idx = index(&[(
            "file:///a.ws",
            "class C {\n  var thePlayer : CR4Player;\n}\n",
        )]);
        let env = env(&[("thePlayer", "CR4Player")]);

        let result = collect_shadowing_diagnostics(&idx, &env);

        let a = result.get("file:///a.ws").expect("a.ws flagged");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, "shadows_script_global");
        assert!(a[0].message.contains("Field 'thePlayer'"));
    }

    #[test]
    fn wrap_method_exempt() {
        let idx = index(&[(
            "file:///a.ws",
            "@wrapMethod(CR4Player)\nfunction GetTarget(thePlayer : CR4Player) {\n  var theGame : CR4Game;\n}\n",
        )]);
        let env = env(&[("thePlayer", "CR4Player"), ("theGame", "CR4Game")]);

        let result = collect_shadowing_diagnostics(&idx, &env);

        assert!(result.is_empty(), "@wrapMethod must suppress shadowing");
    }

    #[test]
    fn replace_method_exempt() {
        let idx = index(&[(
            "file:///a.ws",
            "@replaceMethod(CR4Player)\nfunction GetTarget(thePlayer : CR4Player) {}\n",
        )]);
        let env = env(&[("thePlayer", "CR4Player")]);

        let result = collect_shadowing_diagnostics(&idx, &env);

        assert!(result.is_empty(), "@replaceMethod must suppress shadowing");
    }

    #[test]
    fn clean_no_warnings() {
        let idx = index(&[(
            "file:///a.ws",
            "class C {\n  var y : int;\n  function F(z : int) {\n    var w : int;\n  }\n}\n",
        )]);
        let env = env(&[("thePlayer", "CR4Player")]);

        assert!(collect_shadowing_diagnostics(&idx, &env).is_empty());
    }

    #[test]
    fn does_not_warn_when_local_matches_unrelated_class_field() {
        let idx = index(&[(
            "file:///a.ws",
            "class Other {\n  var x : int;\n}\nfunction F() {\n  var x : int;\n}\n",
        )]);
        let env = env(&[]);

        assert!(
            collect_shadowing_diagnostics(&idx, &env).is_empty(),
            "local in a top-level function is not shadowing an unrelated class's field"
        );
    }
}
