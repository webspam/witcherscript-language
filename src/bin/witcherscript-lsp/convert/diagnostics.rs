use std::path::{Path, PathBuf};

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Command, Diagnostic,
    DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, Location, NumberOrString, Url,
};
use witcherscript_language::diagnostics::{
    BASE_SCRIPT_CONFLICT_KIND, Severity, UNUSED_SYMBOL_KIND, WorkspaceDiagnostic,
};
use witcherscript_language::document::ParsedDocument;

use super::positions::lsp_range;

pub(crate) fn lsp_diagnostics(document: &ParsedDocument) -> Vec<Diagnostic> {
    document
        .diagnostics
        .iter()
        .map(|diagnostic| Diagnostic {
            range: lsp_range(document.line_index.byte_range_to_range(
                &document.source,
                diagnostic.byte_range.start,
                diagnostic.byte_range.end,
            )),
            severity: Some(match diagnostic.kind.as_str() {
                "ternary_cond_expr" => DiagnosticSeverity::WARNING,
                _ => DiagnosticSeverity::ERROR,
            }),
            code: Some(lsp_types::NumberOrString::String(diagnostic.kind.clone())),
            source: Some("witcherscript".to_string()),
            message: diagnostic.message.clone(),
            ..Diagnostic::default()
        })
        .collect()
}

pub(crate) fn lsp_workspace_diagnostic(diagnostic: &WorkspaceDiagnostic) -> Diagnostic {
    let related_information: Vec<DiagnosticRelatedInformation> = diagnostic
        .related
        .iter()
        .filter_map(|related| {
            Url::parse(&related.uri)
                .ok()
                .map(|uri| DiagnosticRelatedInformation {
                    location: Location {
                        uri,
                        range: lsp_range(related.range),
                    },
                    message: related.message.clone(),
                })
        })
        .collect();

    Diagnostic {
        range: lsp_range(diagnostic.range),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Info => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        // The Unnecessary tag is what makes editors render the range as faded.
        tags: (diagnostic.kind == UNUSED_SYMBOL_KIND).then(|| vec![DiagnosticTag::UNNECESSARY]),
        code: Some(lsp_types::NumberOrString::String(diagnostic.kind.clone())),
        source: Some("witcherscript".to_string()),
        message: diagnostic.message.clone(),
        related_information: if related_information.is_empty() {
            None
        } else {
            Some(related_information)
        },
        data: diagnostic.data.clone(),
        ..Diagnostic::default()
    }
}

pub(crate) fn base_script_conflict_code_actions(
    diagnostics: &[Diagnostic],
    workspace_roots: &[PathBuf],
) -> Vec<CodeActionOrCommand> {
    let mut actions: Vec<CodeActionOrCommand> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for diagnostic in diagnostics {
        let Some(directory) = conflict_directory(diagnostic) else {
            continue;
        };
        if seen.contains(&directory) {
            continue;
        }
        seen.push(directory.clone());

        let matched: Vec<Diagnostic> = diagnostics
            .iter()
            .filter(|d| conflict_directory(d).as_deref() == Some(directory.as_str()))
            .cloned()
            .collect();
        let display = display_path(&directory, workspace_roots);
        let title = format!("Add '{display}' to legacyScriptDirectories");
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: title.clone(),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(matched),
            is_preferred: Some(true),
            command: Some(add_legacy_dir_command(&directory, title)),
            ..CodeAction::default()
        }));
    }
    actions
}

fn is_base_script_conflict(diagnostic: &Diagnostic) -> bool {
    matches!(&diagnostic.code, Some(NumberOrString::String(code)) if code == BASE_SCRIPT_CONFLICT_KIND)
}

fn conflict_directory(diagnostic: &Diagnostic) -> Option<String> {
    if !is_base_script_conflict(diagnostic) {
        return None;
    }
    Some(
        diagnostic
            .data
            .as_ref()?
            .get("directory")?
            .as_str()?
            .to_string(),
    )
}

fn display_path(directory: &str, workspace_roots: &[PathBuf]) -> String {
    for root in workspace_roots {
        if let Ok(relative) = Path::new(directory).strip_prefix(root) {
            let shown = relative.display().to_string();
            return if shown.is_empty() {
                ".".to_string()
            } else {
                shown
            };
        }
    }
    directory.to_string()
}

fn add_legacy_dir_command(directory: &str, title: String) -> Command {
    Command {
        title,
        command: "witcherscript.addLegacyScriptDirectory".to_string(),
        arguments: Some(vec![serde_json::Value::String(directory.to_string())]),
    }
}
