use std::fs;
use std::path::PathBuf;

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, DocumentSymbol,
    Documentation, InitializeParams, InsertTextFormat, MarkupContent, MarkupKind, Position, Range,
    Url,
};
use tracing::warn;
use witcherscript_parser::document::ParsedDocument;
use witcherscript_parser::files::is_witcherscript_file;
use witcherscript_parser::line_index::{SourcePosition, SourceRange};
use witcherscript_parser::resolve::{hover_text, Definition, SymbolDb};
use witcherscript_parser::symbols::{DocumentSymbols, Symbol, SymbolId, SymbolKind};

/// Read a WitcherScript source file, handling UTF-16LE/BE BOMs produced by the
/// Witcher 3 toolchain. Falls back to UTF-8 when no BOM is present.
pub(crate) fn read_script_file(path: &std::path::Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        // UTF-16 LE
        let words: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&words)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        // UTF-16 BE
        let words: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&words)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e));
    }
    String::from_utf8(bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub(crate) fn workspace_roots(params: InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = params.workspace_folders {
        return folders
            .into_iter()
            .filter_map(|folder| folder.uri.to_file_path().ok())
            .collect();
    }

    params
        .root_uri
        .and_then(|uri| uri.to_file_path().ok())
        .filter(|path| path.is_dir() || is_witcherscript_file(path))
        .into_iter()
        .collect()
}

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
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(tower_lsp::lsp_types::NumberOrString::String(
                diagnostic.kind.clone(),
            )),
            source: Some("witcherscript".to_string()),
            message: diagnostic.message.clone(),
            ..Diagnostic::default()
        })
        .collect()
}

#[allow(deprecated)]
pub(crate) fn document_symbols(
    symbols: &DocumentSymbols,
    container: Option<SymbolId>,
    uri: &str,
) -> Vec<DocumentSymbol> {
    symbols
        .children_of(container)
        .filter(|symbol| is_outline_symbol(symbol))
        .filter(|symbol| {
            if symbol.name.is_empty() {
                warn!(
                    "skipping {:?} symbol with empty name at line {} in {uri} (parse error in source)",
                    symbol.kind,
                    symbol.range.start.line + 1,
                );
                false
            } else {
                true
            }
        })
        .map(|symbol| DocumentSymbol {
            name: symbol.name.clone(),
            detail: symbol
                .detail
                .clone()
                .or_else(|| symbol.type_annotation.clone()),
            kind: lsp_symbol_kind(symbol.kind),
            tags: None,
            deprecated: None,
            range: lsp_range(symbol.range),
            selection_range: lsp_range(symbol.selection_range),
            children: Some(document_symbols(symbols, Some(symbol.id), uri)),
        })
        .collect()
}

fn is_outline_symbol(symbol: &Symbol) -> bool {
    !matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Parameter)
}

fn lsp_symbol_kind(kind: SymbolKind) -> tower_lsp::lsp_types::SymbolKind {
    match kind {
        SymbolKind::Class => tower_lsp::lsp_types::SymbolKind::CLASS,
        SymbolKind::Struct => tower_lsp::lsp_types::SymbolKind::STRUCT,
        SymbolKind::Enum => tower_lsp::lsp_types::SymbolKind::ENUM,
        SymbolKind::EnumVariant => tower_lsp::lsp_types::SymbolKind::ENUM_MEMBER,
        SymbolKind::Function => tower_lsp::lsp_types::SymbolKind::FUNCTION,
        SymbolKind::Method | SymbolKind::Event => tower_lsp::lsp_types::SymbolKind::METHOD,
        SymbolKind::Field => tower_lsp::lsp_types::SymbolKind::FIELD,
        SymbolKind::Variable => tower_lsp::lsp_types::SymbolKind::VARIABLE,
        SymbolKind::Parameter => tower_lsp::lsp_types::SymbolKind::VARIABLE,
        SymbolKind::State => tower_lsp::lsp_types::SymbolKind::OBJECT,
    }
}

pub(crate) fn lsp_range(range: SourceRange) -> Range {
    Range {
        start: Position {
            line: range.start.line,
            character: range.start.character,
        },
        end: Position {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

pub(crate) fn source_position(position: Position) -> SourcePosition {
    SourcePosition {
        line: position.line,
        character: position.character,
    }
}

pub(crate) fn completion_item(definition: &Definition, params: &[String]) -> CompletionItem {
    let symbol = &definition.symbol;
    let is_callable = matches!(
        symbol.kind,
        SymbolKind::Method | SymbolKind::Event | SymbolKind::Function
    );
    let kind = Some(match symbol.kind {
        SymbolKind::Method | SymbolKind::Event => CompletionItemKind::METHOD,
        SymbolKind::Field => CompletionItemKind::FIELD,
        SymbolKind::Function => CompletionItemKind::FUNCTION,
        SymbolKind::Variable | SymbolKind::Parameter => CompletionItemKind::VARIABLE,
        _ => CompletionItemKind::TEXT,
    });
    let detail = symbol
        .signature
        .clone()
        .or_else(|| symbol.type_annotation.clone());
    let (insert_text, insert_text_format) = if is_callable {
        if params.is_empty() {
            (
                Some(format!("{}()", symbol.name)),
                Some(InsertTextFormat::SNIPPET),
            )
        } else {
            let args = params
                .iter()
                .enumerate()
                .map(|(i, name)| format!("${{{}:{}}}", i + 1, name))
                .collect::<Vec<_>>()
                .join(", ");
            (
                Some(format!("{}({})$0", symbol.name, args)),
                Some(InsertTextFormat::SNIPPET),
            )
        }
    } else {
        (None, None)
    };
    CompletionItem {
        label: symbol.name.clone(),
        kind,
        detail,
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: hover_markdown(definition),
        })),
        insert_text,
        insert_text_format,
        ..CompletionItem::default()
    }
}

pub(crate) fn wrap_method_snippet(method: &Definition, db: &SymbolDb) -> String {
    let params = db.full_parameters_of(&method.uri, method.symbol.id);
    let param_list = params
        .iter()
        .map(|p| {
            let mut s = String::new();
            if p.is_optional {
                s.push_str("optional ");
            }
            if p.is_out {
                s.push_str("out ");
            }
            s.push_str(&p.name);
            if let Some(ty) = &p.type_annotation {
                s.push_str(" : ");
                s.push_str(ty);
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}({}) {{\n\t$0\n}}", method.symbol.name, param_list)
}

pub(crate) fn type_completion_item(definition: &Definition) -> CompletionItem {
    let symbol = &definition.symbol;
    let kind = Some(match symbol.kind {
        SymbolKind::Struct => CompletionItemKind::STRUCT,
        SymbolKind::Enum => CompletionItemKind::ENUM,
        _ => CompletionItemKind::CLASS,
    });
    CompletionItem {
        label: symbol.name.clone(),
        kind,
        detail: symbol.detail.clone(),
        ..CompletionItem::default()
    }
}

pub(crate) fn builtin_type_item(name: &str) -> CompletionItem {
    CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..CompletionItem::default()
    }
}

pub(crate) fn this_super_item(name: &str) -> CompletionItem {
    CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::VARIABLE),
        sort_text: Some(format!("0_{name}")),
        ..CompletionItem::default()
    }
}

pub(crate) fn class_body_kw_item(keyword: &str) -> CompletionItem {
    let (snippet, sort_prefix): (Option<&str>, &str) = match keyword {
        "var" => (Some("var ${1:varName} : ${2:int};"), "0"),
        "function" => (Some("function ${1:Name}($2) {\n\t$0\n}"), "0"),
        "event" => (Some("event On${1}($2) {\n\t$0\n}"), "0"),
        "autobind" => (Some("autobind ${1:name} : ${2:Type} = single;"), "0"),
        "default" => (Some("default ${1:field} = ${2:value};"), "0"),
        "defaults" => (Some("defaults {\n\t${1:field} = ${2:value};\n}"), "0"),
        "hint" => (Some("hint ${1:field} = \"${2:tooltip}\";"), "0"),
        _ => (None, "1"),
    };
    CompletionItem {
        label: keyword.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        insert_text: snippet.map(str::to_string),
        insert_text_format: snippet.map(|_| InsertTextFormat::SNIPPET),
        sort_text: Some(format!("{sort_prefix}_{keyword}")),
        ..CompletionItem::default()
    }
}

pub(crate) fn keyword_snippet_item(label: &str, snippet: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        insert_text: Some(snippet.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some(format!("0_{label}")),
        ..CompletionItem::default()
    }
}

pub(crate) fn hover_markdown(definition: &Definition) -> String {
    let mut markdown = format!("```witcherscript\n{}\n```", hover_text(definition));
    markdown.push_str(&format!(
        "\n\nDefined in {}",
        hover_location_markdown(definition)
    ));
    markdown
}

fn hover_location_markdown(definition: &Definition) -> String {
    let line = definition.symbol.selection_range.start.line + 1;
    let Ok(mut uri) = Url::parse(&definition.uri) else {
        return format!("`{}:{line}`", definition.uri);
    };

    let label = uri
        .to_file_path()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .or_else(|| {
            uri.path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| definition.uri.clone());

    uri.set_fragment(Some(&format!("L{line}")));

    format!("[{label}:{line}]({uri})")
}
