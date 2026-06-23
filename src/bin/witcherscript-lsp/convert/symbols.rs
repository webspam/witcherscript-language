use std::fmt::Write;

use lsp_types::{DocumentSymbol, Location, OneOf, Url, WorkspaceSymbol};
use witcherscript_language::formatter::ColonSpacing;
use witcherscript_language::resolve::{Definition, SymbolDb, hover_doc, hover_text};
use witcherscript_language::symbols::{DocumentSymbols, SymbolId, SymbolKind};

use super::positions::lsp_range;

#[allow(deprecated)]
pub(crate) fn document_symbols(
    symbols: &DocumentSymbols,
    container: Option<SymbolId>,
) -> Vec<DocumentSymbol> {
    symbols
        .children_of(container)
        .filter(|symbol| symbol.kind.is_outline())
        // VS Code rejects DocumentSymbols with empty names; skip them silently.
        .filter(|symbol| !symbol.name.is_empty())
        .map(|symbol| DocumentSymbol {
            name: symbol.name.clone(),
            detail: symbol
                .display_detail()
                .or_else(|| symbol.type_annotation.as_ref().map(ToString::to_string)),
            kind: lsp_symbol_kind(symbol.kind),
            tags: None,
            deprecated: None,
            range: lsp_range(symbol.range),
            selection_range: lsp_range(symbol.selection_range),
            children: Some(document_symbols(symbols, Some(symbol.id))),
        })
        .collect()
}

pub(crate) fn workspace_symbol(definition: &Definition) -> Option<WorkspaceSymbol> {
    let uri = match Url::parse(&definition.uri) {
        Ok(uri) => uri,
        Err(err) => {
            tracing::warn!(uri = %definition.uri, %err, "indexed symbol uri failed to parse; skipping");
            return None;
        }
    };
    let symbol = &definition.symbol;
    Some(WorkspaceSymbol {
        name: symbol.name.clone(),
        kind: lsp_symbol_kind(symbol.kind),
        tags: None,
        container_name: symbol.container_name.clone(),
        location: OneOf::Left(Location {
            uri,
            range: lsp_range(symbol.selection_range),
        }),
        data: None,
    })
}

fn lsp_symbol_kind(kind: SymbolKind) -> lsp_types::SymbolKind {
    match kind {
        SymbolKind::Class => lsp_types::SymbolKind::CLASS,
        SymbolKind::Struct => lsp_types::SymbolKind::STRUCT,
        SymbolKind::Enum => lsp_types::SymbolKind::ENUM,
        SymbolKind::EnumMember => lsp_types::SymbolKind::ENUM_MEMBER,
        SymbolKind::Function => lsp_types::SymbolKind::FUNCTION,
        SymbolKind::Method | SymbolKind::Event => lsp_types::SymbolKind::METHOD,
        SymbolKind::Field => lsp_types::SymbolKind::FIELD,
        SymbolKind::Variable | SymbolKind::Parameter => lsp_types::SymbolKind::VARIABLE,
        SymbolKind::State | SymbolKind::NativeType => lsp_types::SymbolKind::OBJECT,
    }
}

pub(crate) fn hover_markdown(
    definition: &Definition,
    db: &SymbolDb,
    colon: ColonSpacing,
) -> String {
    let mut markdown = format!(
        "```witcherscript\n{}\n```",
        hover_text(definition, db, colon)
    );
    if let Some(doc) = hover_doc(definition, db) {
        write!(markdown, "\n\n{doc}").unwrap();
    }
    write!(
        markdown,
        "\n\nDefined in {}",
        hover_location_markdown(definition)
    )
    .unwrap();
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
