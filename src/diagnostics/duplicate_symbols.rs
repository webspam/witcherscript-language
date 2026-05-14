use std::collections::HashMap;

use crate::resolve::WorkspaceIndex;
use crate::symbols::{Symbol, SymbolKind};

use super::{RelatedLocation, WorkspaceDiagnostic};

/// Flag any top-level declaration whose name collides with another top-level
/// declaration anywhere in the workspace (class vs function, class vs class,
/// same-file or cross-file). Returns the diagnostics keyed by document URI.
pub fn collect_duplicate_symbol_diagnostics(
    index: &WorkspaceIndex,
) -> HashMap<String, Vec<WorkspaceDiagnostic>> {
    let mut by_name: HashMap<&str, Vec<(&str, &Symbol)>> = HashMap::new();
    for (uri, sym) in index.all_top_level() {
        if !is_declaration_kind(sym.kind) {
            continue;
        }
        // Modding-annotation functions (@addMethod/@wrapMethod/...) are member
        // injections, not fresh global names.
        if !sym.annotations.is_empty() {
            continue;
        }
        by_name
            .entry(sym.name.as_str())
            .or_default()
            .push((uri, sym));
    }

    let mut result: HashMap<String, Vec<WorkspaceDiagnostic>> = HashMap::new();
    for (name, occurrences) in by_name {
        if occurrences.len() < 2 {
            continue;
        }
        for (i, (uri, sym)) in occurrences.iter().enumerate() {
            let related = occurrences
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, (other_uri, other_sym))| RelatedLocation {
                    uri: other_uri.to_string(),
                    range: other_sym.selection_range,
                    message: format!("'{name}' also declared here"),
                })
                .collect();
            result
                .entry(uri.to_string())
                .or_default()
                .push(WorkspaceDiagnostic {
                    kind: "duplicate_symbol".to_string(),
                    message: "A class or function with that name already exists.".to_string(),
                    range: sym.selection_range,
                    related,
                });
        }
    }
    result
}

fn is_declaration_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::State
            | SymbolKind::Function
            | SymbolKind::Event
    )
}

#[cfg(test)]
mod tests {
    use super::collect_duplicate_symbol_diagnostics;
    use crate::document::parse_document;
    use crate::resolve::WorkspaceIndex;

    fn index(docs: &[(&str, &str)]) -> WorkspaceIndex {
        let mut idx = WorkspaceIndex::default();
        for (uri, src) in docs {
            let doc = parse_document(*src).expect("parse should succeed");
            idx.update_document(*uri, &doc);
        }
        idx
    }

    #[test]
    fn flags_cross_file_class_and_function_conflict() {
        let idx = index(&[
            ("file:///a.ws", "class Foo {}\n"),
            ("file:///b.ws", "function Foo() {}\n"),
        ]);

        let result = collect_duplicate_symbol_diagnostics(&idx);

        let a = result.get("file:///a.ws").expect("a.ws should be flagged");
        let b = result.get("file:///b.ws").expect("b.ws should be flagged");
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].kind, "duplicate_symbol");
        assert_eq!(
            a[0].message,
            "A class or function with that name already exists."
        );
        assert_eq!(a[0].related.len(), 1);
        assert_eq!(a[0].related[0].uri, "file:///b.ws");
        assert_eq!(b[0].related[0].uri, "file:///a.ws");
    }

    #[test]
    fn flags_same_file_duplicate() {
        let idx = index(&[("file:///a.ws", "class Foo {}\nclass Foo {}\n")]);

        let result = collect_duplicate_symbol_diagnostics(&idx);

        let a = result.get("file:///a.ws").expect("a.ws should be flagged");
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|d| d.related.len() == 1));
    }

    #[test]
    fn no_duplicates_returns_empty() {
        let idx = index(&[
            ("file:///a.ws", "class Foo {}\n"),
            ("file:///b.ws", "function Bar() {}\n"),
        ]);

        assert!(collect_duplicate_symbol_diagnostics(&idx).is_empty());
    }

    #[test]
    fn annotated_member_injection_is_excluded() {
        let idx = index(&[
            ("file:///a.ws", "class Foo {}\n"),
            (
                "file:///b.ws",
                "@wrapMethod(CR4Player)\nfunction Foo() {}\n",
            ),
        ]);

        assert!(
            collect_duplicate_symbol_diagnostics(&idx).is_empty(),
            "an @wrapMethod function must not collide with a class name"
        );
    }
}
