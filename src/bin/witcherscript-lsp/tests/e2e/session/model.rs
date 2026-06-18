//! Snapshot-friendly, deterministic projections of LSP results. URIs are workspace-relative; positions render as `line:col`.

use lsp_types::{
    CompletionItemKind, DiagnosticSeverity, DocumentHighlightKind, InlayHintKind, Position, Range,
    SymbolKind,
};
use serde::Serialize;

pub(crate) fn fmt_pos(p: Position) -> String {
    format!("{}:{}", p.line, p.character)
}

pub(crate) fn fmt_range(r: Range) -> String {
    format!("{}-{}", fmt_pos(r.start), fmt_pos(r.end))
}

#[derive(Debug, Serialize)]
pub(crate) struct SnapLoc {
    pub(crate) file: String,
    pub(crate) range: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DiagSnap {
    pub(crate) range: String,
    pub(crate) severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<String>,
    pub(crate) message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct HoverSnap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range: Option<String>,
    pub(crate) value: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SymbolSnap {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) range: String,
    pub(crate) selection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) children: Vec<SymbolSnap>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WsSymbolSnap {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) location: SnapLoc,
}

#[derive(Debug, Serialize)]
pub(crate) struct TokenSnap {
    pub(crate) range: String,
    pub(crate) token_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) modifiers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompletionItemSnap {
    pub(crate) label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SignatureInfoSnap {
    pub(crate) label: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) parameters: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SignatureSnap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_signature: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_parameter: Option<u32>,
    pub(crate) signatures: Vec<SignatureInfoSnap>,
}

#[derive(Debug, Serialize)]
pub(crate) struct HighlightSnap {
    pub(crate) range: String,
    pub(crate) kind: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct HintSnap {
    pub(crate) position: String,
    pub(crate) label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TextEditSnap {
    pub(crate) range: String,
    pub(crate) new_text: String,
}

pub(crate) fn severity_name(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::WARNING) => "warning",
        Some(DiagnosticSeverity::INFORMATION) => "information",
        Some(DiagnosticSeverity::HINT) => "hint",
        _ => "unknown",
    }
}

pub(crate) fn symbol_kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::FILE => "file",
        SymbolKind::MODULE => "module",
        SymbolKind::NAMESPACE => "namespace",
        SymbolKind::PACKAGE => "package",
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "method",
        SymbolKind::PROPERTY => "property",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "constructor",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "interface",
        SymbolKind::FUNCTION => "function",
        SymbolKind::VARIABLE => "variable",
        SymbolKind::CONSTANT => "constant",
        SymbolKind::STRING => "string",
        SymbolKind::NUMBER => "number",
        SymbolKind::BOOLEAN => "boolean",
        SymbolKind::ARRAY => "array",
        SymbolKind::OBJECT => "object",
        SymbolKind::KEY => "key",
        SymbolKind::NULL => "null",
        SymbolKind::ENUM_MEMBER => "enum_member",
        SymbolKind::STRUCT => "struct",
        SymbolKind::EVENT => "event",
        SymbolKind::OPERATOR => "operator",
        SymbolKind::TYPE_PARAMETER => "type_parameter",
        _ => "other",
    }
}

pub(crate) fn completion_kind_name(kind: CompletionItemKind) -> &'static str {
    match kind {
        CompletionItemKind::CLASS => "class",
        CompletionItemKind::METHOD => "method",
        CompletionItemKind::FIELD => "field",
        CompletionItemKind::PROPERTY => "property",
        CompletionItemKind::ENUM => "enum",
        CompletionItemKind::ENUM_MEMBER => "enum_member",
        CompletionItemKind::FUNCTION => "function",
        CompletionItemKind::VARIABLE => "variable",
        CompletionItemKind::CONSTANT => "constant",
        CompletionItemKind::KEYWORD => "keyword",
        CompletionItemKind::STRUCT => "struct",
        CompletionItemKind::INTERFACE => "interface",
        CompletionItemKind::TYPE_PARAMETER => "type_parameter",
        CompletionItemKind::SNIPPET => "snippet",
        _ => "other",
    }
}

pub(crate) fn highlight_kind_name(kind: Option<DocumentHighlightKind>) -> &'static str {
    match kind {
        Some(DocumentHighlightKind::READ) => "read",
        Some(DocumentHighlightKind::WRITE) => "write",
        _ => "text",
    }
}

pub(crate) fn inlay_kind_name(kind: Option<InlayHintKind>) -> Option<&'static str> {
    match kind {
        Some(InlayHintKind::TYPE) => Some("type"),
        Some(InlayHintKind::PARAMETER) => Some("parameter"),
        _ => None,
    }
}
