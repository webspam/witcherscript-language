use crate::document::ParsedDocument;
use crate::resolve::SymbolDb;
use crate::symbols::Symbol;

use super::{Severity, WorkspaceDiagnostic};

pub(crate) const KIND_UNKNOWN_NATIVE: &str = "unknown_native_import";
pub(crate) const KIND_NATIVE_NOT_IMPORTED: &str = "native_not_imported";

pub(crate) enum NativeName<'a> {
    Member {
        receiver: &'a str,
        name: &'a str,
        noun: &'static str,
    },
    Function(&'a str),
}

pub(crate) fn native_not_imported(
    db: &SymbolDb,
    target: NativeName<'_>,
) -> Option<(&'static str, String, Severity)> {
    let message = match target {
        NativeName::Member {
            receiver,
            name,
            noun,
        } => db
            .find_native_member(receiver, name)
            .map(|_| format!("Native {noun} '{name}' of '{receiver}' is not imported")),
        NativeName::Function(name) => db
            .find_native_top_level(name)
            .map(|_| format!("Native function '{name}' is not imported")),
    }?;
    Some((KIND_NATIVE_NOT_IMPORTED, message, Severity::Info))
}

#[cfg(test)]
fn collect_native_import_diagnostics(
    documents: &[(&str, &ParsedDocument)],
    db: &SymbolDb,
) -> std::collections::HashMap<String, Vec<WorkspaceDiagnostic>> {
    documents
        .iter()
        .filter_map(|(uri, document)| {
            let diagnostics = check_document(document, db);
            (!diagnostics.is_empty()).then(|| ((*uri).to_string(), diagnostics))
        })
        .collect()
}

pub(crate) fn check_document(document: &ParsedDocument, db: &SymbolDb) -> Vec<WorkspaceDiagnostic> {
    if !db.has_builtins() {
        return Vec::new();
    }
    document
        .symbols
        .all()
        .iter()
        .filter(|symbol| symbol.is_script_import())
        .filter(|symbol| db.native_counterpart_of(symbol).is_none())
        .map(|symbol| WorkspaceDiagnostic {
            kind: KIND_UNKNOWN_NATIVE.to_string(),
            message: missing_native_message(symbol),
            severity: Severity::Error,
            range: symbol.selection_range,
            related: vec![],
            data: None,
        })
        .collect()
}

fn missing_native_message(symbol: &Symbol) -> String {
    let name = &symbol.name;
    match &symbol.container_name {
        Some(container) => format!("No native member '{name}' on '{container}' to import"),
        None if symbol.kind.is_type() => format!("No native type '{name}' to import"),
        None => format!("No native function '{name}' to import"),
    }
}

#[cfg(test)]
mod tests;
