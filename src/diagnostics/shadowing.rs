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
mod tests;
