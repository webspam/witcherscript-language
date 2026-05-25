use lsp_types::{ParameterLabel, SymbolKind as LspSymbolKind};
use witcherscript_language::line_index::SourcePosition;
use witcherscript_language::resolve::SignatureHelpInfo;
use witcherscript_language::test_support::TestDb;

use crate::convert::{
    document_symbols, lsp_diagnostics, lsp_workspace_diagnostic, signature_help_response,
};

#[test]
fn maps_core_diagnostics_to_lsp_diagnostics() {
    let t = TestDb::new("function Bad() {\n a = 1;\n var b : int;\n}\n");
    let diagnostics = lsp_diagnostics(t.primary_doc());

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].source.as_deref(), Some("witcherscript"));
    assert_eq!(
        diagnostics[0].message,
        "Local variable declarations must precede executable statements"
    );
}

#[test]
fn ternary_diagnostic_maps_to_warning() {
    let t = TestDb::new("function Pick() {\n var x : int;\n x = true ? 1 : 2;\n}\n");
    let diagnostics = lsp_diagnostics(t.primary_doc());

    let ternary = diagnostics
        .iter()
        .find(|d| {
            d.code
                == Some(lsp_types::NumberOrString::String(
                    "ternary_cond_expr".to_string(),
                ))
        })
        .expect("expected a ternary_cond_expr diagnostic");
    assert_eq!(
        ternary.severity,
        Some(lsp_types::DiagnosticSeverity::WARNING)
    );
}

#[test]
fn signature_help_response_maps_label_offsets_and_active_parameter() {
    let info = SignatureHelpInfo {
        label: "Find(name : string, range : float)".to_string(),
        parameters: vec![(5, 18), (20, 33)],
        active_parameter: Some(1),
    };

    let help = signature_help_response(info);

    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.active_signature, Some(0));
    assert_eq!(help.active_parameter, Some(1));

    let signature = &help.signatures[0];
    assert_eq!(signature.label, "Find(name : string, range : float)");
    let params = signature.parameters.as_ref().expect("parameters present");
    assert_eq!(params.len(), 2);
    assert!(matches!(
        params[0].label,
        ParameterLabel::LabelOffsets([5, 18])
    ));
    assert!(matches!(
        params[1].label,
        ParameterLabel::LabelOffsets([20, 33])
    ));
}

#[test]
fn maps_symbols_to_lsp_document_symbols() {
    let t = TestDb::new("class CExample {\n var value : int;\n}\n");
    let symbols = document_symbols(&t.primary_doc().symbols, None, t.primary_uri());

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "CExample");
    assert_eq!(symbols[0].kind, LspSymbolKind::CLASS);
    assert_eq!(
        symbols[0]
            .children
            .as_ref()
            .expect("class should have child symbols")[0]
            .name,
        "value"
    );
}

#[test]
fn workspace_diagnostic_carries_related_information() {
    use witcherscript_language::diagnostics::{RelatedLocation, Severity, WorkspaceDiagnostic};
    use witcherscript_language::line_index::SourceRange;

    let range = SourceRange {
        start: SourcePosition {
            line: 0,
            character: 6,
        },
        end: SourcePosition {
            line: 0,
            character: 9,
        },
    };
    let diagnostic = WorkspaceDiagnostic {
        kind: "duplicate_symbol".to_string(),
        message: "A class or function with that name already exists.".to_string(),
        severity: Severity::Error,
        range,
        related: vec![RelatedLocation {
            uri: "file:///other.ws".to_string(),
            range,
            message: "'Foo' also declared here".to_string(),
        }],
        data: None,
    };

    let lsp = lsp_workspace_diagnostic(&diagnostic);

    assert_eq!(lsp.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
    assert_eq!(
        lsp.code,
        Some(lsp_types::NumberOrString::String(
            "duplicate_symbol".to_string()
        ))
    );
    let related = lsp
        .related_information
        .expect("related_information should be present");
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].location.uri.as_str(), "file:///other.ws");
    assert_eq!(related[0].message, "'Foo' also declared here");
    assert_eq!(related[0].location.range.start.character, 6);
}
