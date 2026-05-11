use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use serde_json::Value;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    ConfigurationItem, Diagnostic, DiagnosticSeverity, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InitializeParams, InitializeResult,
    InitializedParams, Location, MarkupContent, MarkupKind, MessageType, OneOf, Position, Range,
    ReferenceParams, SemanticToken, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url, WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{error, warn};
use witcherscript_parser::document::{parse_document, ParsedDocument};
use witcherscript_parser::files::{collect_witcherscript_files, is_witcherscript_file};
use witcherscript_parser::line_index::{SourcePosition, SourceRange};
use witcherscript_parser::resolve::{
    find_references, hover_text, resolve_definition, Definition, SymbolDb, WorkspaceIndex,
};
use witcherscript_parser::script_env::{parse_script_environment, ScriptEnvironment};
use witcherscript_parser::semantic_tokens::{
    collect_semantic_tokens, TOKEN_MODIFIERS, TOKEN_TYPES,
};
use witcherscript_parser::symbols::{DocumentSymbols, Symbol, SymbolId, SymbolKind};

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: Arc<Mutex<HashMap<Url, ParsedDocument>>>,
    workspace_index: Arc<Mutex<WorkspaceIndex>>,
    workspace_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,
    workspace_roots: Arc<Mutex<Vec<PathBuf>>>,
    base_scripts_path: Arc<Mutex<Option<PathBuf>>>,
    base_scripts_index: Arc<Mutex<WorkspaceIndex>>,
    base_scripts_documents: Arc<Mutex<HashMap<String, ParsedDocument>>>,
    script_env: Arc<Mutex<ScriptEnvironment>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Capture base scripts path from initializationOptions if provided.
        // workspace/configuration is pulled after initialized(), but this ensures
        // we have a value even before that round-trip completes.
        if let Some(opts) = &params.initialization_options {
            if let Some(p) = opts.get("gameDirectory").and_then(|v| v.as_str()) {
                if !p.is_empty() {
                    *self.base_scripts_path.lock().await = Some(PathBuf::from(p));
                }
            }
        }

        let roots = workspace_roots(params);
        *self.workspace_roots.lock().await = roots;

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(tower_lsp::lsp_types::HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: TOKEN_TYPES
                                    .iter()
                                    .map(|s| tower_lsp::lsp_types::SemanticTokenType::new(s))
                                    .collect(),
                                token_modifiers: TOKEN_MODIFIERS
                                    .iter()
                                    .map(|s| tower_lsp::lsp_types::SemanticTokenModifier::new(s))
                                    .collect(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            ..SemanticTokensOptions::default()
                        },
                    ),
                ),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: None,
                    }),
                    ..WorkspaceServerCapabilities::default()
                }),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.index_workspace().await;
        // Pull witcherscript.gameDirectory from the client's settings. This may
        // override the value from initializationOptions.
        self.fetch_config().await;
        self.index_base_scripts().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.update_open_document(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().next() {
            self.update_open_document(params.text_document.uri, change.text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.fetch_config().await;
        self.index_base_scripts().await;
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base).with_script_env(&script_env);
        let Some(definition) =
            resolve_definition(uri.as_str(), document, &db, source_position(position))
        else {
            return Ok(None);
        };
        let Ok(target_uri) = Url::parse(&definition.uri) else {
            return Ok(None);
        };

        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: target_uri,
            range: lsp_range(definition.symbol.selection_range),
        })))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base).with_script_env(&script_env);
        let Some(definition) =
            resolve_definition(uri.as_str(), document, &db, source_position(position))
        else {
            return Ok(None);
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_markdown(&definition),
            }),
            range: Some(lsp_range(definition.symbol.selection_range)),
        }))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
            &document.symbols,
            None,
        ))))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base).with_script_env(&script_env);
        let data = collect_semantic_tokens(
            document.tree.root_node(),
            &document.source,
            &document.line_index,
            &document.symbols,
            &db,
        );
        let tokens: Vec<SemanticToken> = data
            .chunks_exact(5)
            .map(|c| SemanticToken {
                delta_line: c[0],
                delta_start: c[1],
                length: c[2],
                token_type: c[3],
                token_modifiers_bitset: c[4],
            })
            .collect();
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let documents = self.documents.lock().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base).with_script_env(&script_env);

        let ws_bytes = workspace.doc_idents_bytes();
        let base_bytes = base.doc_idents_bytes();
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "ident index memory: workspace {}KB, base {}KB, total {}KB",
                    ws_bytes / 1024,
                    base_bytes / 1024,
                    (ws_bytes + base_bytes) / 1024,
                ),
            )
            .await;

        let Some(definition) =
            resolve_definition(uri.as_str(), document, &db, source_position(position))
        else {
            return Ok(Some(Vec::new()));
        };

        let workspace_docs = self.workspace_documents.lock().await;
        let base_docs = self.base_scripts_documents.lock().await;

        // Merge all indexed documents; open documents take precedence over indexed ones
        // so that unsaved edits are reflected in reference search results.
        let mut merged: HashMap<String, &ParsedDocument> = HashMap::new();
        for (uri, doc) in base_docs.iter() {
            merged.insert(uri.clone(), doc);
        }
        for (uri, doc) in workspace_docs.iter() {
            merged.insert(uri.clone(), doc);
        }
        for (url, doc) in documents.iter() {
            merged.insert(url.to_string(), doc);
        }

        let definition_document = merged.get(&definition.uri).copied().unwrap_or(document);

        let search_docs: Vec<(&str, &ParsedDocument)> = merged
            .iter()
            .map(|(uri, doc)| (uri.as_str(), *doc))
            .collect();

        let refs = find_references(
            &definition,
            definition_document,
            &search_docs,
            &db,
            include_declaration,
        );

        let locations: Vec<Location> = refs
            .into_iter()
            .filter_map(|(ref_uri, range)| {
                Url::parse(&ref_uri).ok().map(|url| Location {
                    uri: url,
                    range: lsp_range(range),
                })
            })
            .collect();

        Ok(Some(locations))
    }
}

impl Backend {
    async fn update_open_document(&self, uri: Url, text: String) {
        match parse_document(text) {
            Ok(document) => {
                let diagnostics = lsp_diagnostics(&document);
                self.workspace_index
                    .lock()
                    .await
                    .update_document(uri.as_str(), &document);
                self.documents.lock().await.insert(uri.clone(), document);
                self.client
                    .publish_diagnostics(uri, diagnostics, None)
                    .await;
            }
            Err(err) => {
                error!(uri = %uri, error = %err, "failed to parse document");
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("failed to parse document: {err}"),
                    )
                    .await;
            }
        }
    }

    async fn index_workspace(&self) {
        let roots = self.workspace_roots.lock().await.clone();
        if roots.is_empty() {
            return;
        }

        let Ok(files) = collect_witcherscript_files(&roots) else {
            warn!("failed to collect workspace files");
            return;
        };

        let mut index = self.workspace_index.lock().await;
        let mut docs = self.workspace_documents.lock().await;
        for path in files {
            let Ok(source) = fs::read_to_string(&path) else {
                warn!(path = %path.display(), "failed to read workspace file");
                continue;
            };
            let Ok(document) = parse_document(source) else {
                warn!(path = %path.display(), "failed to parse workspace file");
                continue;
            };
            let Ok(uri) = Url::from_file_path(&path) else {
                warn!(path = %path.display(), "failed to convert path to URI");
                continue;
            };
            index.update_document(uri.as_str(), &document);
            docs.insert(uri.to_string(), document);
        }
    }

    /// Pull `witcherscript.gameDirectory` from the client via `workspace/configuration`.
    /// Updates `self.base_scripts_path` if a non-empty value is returned.
    async fn fetch_config(&self) {
        let items = vec![ConfigurationItem {
            scope_uri: None,
            section: Some("witcherscript.gameDirectory".to_string()),
        }];
        let Ok(values) = self.client.configuration(items).await else {
            warn!("workspace/configuration request failed");
            return;
        };
        if let Some(Value::String(path_str)) = values.into_iter().next() {
            if !path_str.is_empty() {
                *self.base_scripts_path.lock().await = Some(PathBuf::from(path_str));
            }
        }
    }

    /// Parse all `.ws` files under `base_scripts_path` in parallel and store their
    /// symbols in `base_scripts_index`. No-ops if no path is configured.
    async fn index_base_scripts(&self) {
        let game_dir = {
            let guard = self.base_scripts_path.lock().await;
            match guard.clone() {
                Some(p) => p,
                None => return,
            }
        };

        if let Some(env) = parse_script_environment(&game_dir.join(r"bin\redscripts.ini")) {
            *self.script_env.lock().await = env;
        }

        let path = game_dir.join(r"content\content0\scripts");

        self.client
            .log_message(
                MessageType::INFO,
                format!("Indexing base scripts from {}", path.display()),
            )
            .await;
        let start = Instant::now();

        let Ok(files) = collect_witcherscript_files(&[path]) else {
            warn!("failed to collect base script files");
            self.client
                .log_message(MessageType::WARNING, "Failed to collect base script files")
                .await;
            return;
        };

        let file_count = files.len();

        // Parse files in parallel; each rayon thread gets its own tree-sitter parser
        // via parse_document(), so there is no shared mutable state.
        let parsed: Vec<(String, ParsedDocument)> = files
            .par_iter()
            .filter_map(|path| {
                let source = read_script_file(path).ok()?;
                let document = parse_document(source).ok()?;
                let uri = Url::from_file_path(path).ok()?;
                Some((uri.to_string(), document))
            })
            .collect();

        let indexed = parsed.len();
        {
            let mut index = self.base_scripts_index.lock().await;
            let mut docs = self.base_scripts_documents.lock().await;
            for (uri, document) in parsed {
                index.update_document(uri.as_str(), &document);
                docs.insert(uri, document);
            }
        }

        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "Base scripts indexed: {indexed}/{file_count} files in {:.1}s",
                    start.elapsed().as_secs_f32()
                ),
            )
            .await;
    }
}

/// Read a WitcherScript source file, handling UTF-16LE/BE BOMs produced by the
/// Witcher 3 toolchain. Falls back to UTF-8 when no BOM is present.
fn read_script_file(path: &std::path::Path) -> std::io::Result<String> {
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

fn workspace_roots(params: InitializeParams) -> Vec<PathBuf> {
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

fn lsp_diagnostics(document: &ParsedDocument) -> Vec<Diagnostic> {
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
fn document_symbols(symbols: &DocumentSymbols, container: Option<SymbolId>) -> Vec<DocumentSymbol> {
    symbols
        .children_of(container)
        .filter(|symbol| is_outline_symbol(symbol))
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
            children: Some(document_symbols(symbols, Some(symbol.id))),
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

fn lsp_range(range: SourceRange) -> Range {
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

fn source_position(position: Position) -> SourcePosition {
    SourcePosition {
        line: position.line,
        character: position.character,
    }
}

fn hover_markdown(definition: &Definition) -> String {
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: Arc::new(Mutex::new(HashMap::new())),
        workspace_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
        workspace_documents: Arc::new(Mutex::new(HashMap::new())),
        workspace_roots: Arc::new(Mutex::new(Vec::new())),
        base_scripts_path: Arc::new(Mutex::new(None)),
        base_scripts_index: Arc::new(Mutex::new(WorkspaceIndex::default())),
        base_scripts_documents: Arc::new(Mutex::new(HashMap::new())),
        script_env: Arc::new(Mutex::new(ScriptEnvironment::default())),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::SymbolKind as LspSymbolKind;
    use witcherscript_parser::document::parse_document;
    use witcherscript_parser::line_index::SourcePosition;
    use witcherscript_parser::resolve::{resolve_definition, SymbolDb, WorkspaceIndex};

    use super::{document_symbols, hover_markdown, lsp_diagnostics, read_script_file};

    fn encode_utf16le(s: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE]; // BOM
        for unit in s.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn encode_utf16be(s: &str) -> Vec<u8> {
        let mut bytes = vec![0xFE, 0xFF]; // BOM
        for unit in s.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        bytes
    }

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes).expect("temp file write should succeed");
        path
    }

    #[test]
    fn reads_utf8_script_file() {
        let path = write_temp("ws_test_utf8.ws", b"class CExample {}\n");
        assert_eq!(
            read_script_file(&path).expect("should read"),
            "class CExample {}\n"
        );
    }

    #[test]
    fn reads_utf16le_script_file() {
        let bytes = encode_utf16le("class CExample {}\n");
        let path = write_temp("ws_test_utf16le.ws", &bytes);
        assert_eq!(
            read_script_file(&path).expect("should read"),
            "class CExample {}\n"
        );
    }

    #[test]
    fn reads_utf16be_script_file() {
        let bytes = encode_utf16be("class CExample {}\n");
        let path = write_temp("ws_test_utf16be.ws", &bytes);
        assert_eq!(
            read_script_file(&path).expect("should read"),
            "class CExample {}\n"
        );
    }

    #[test]
    fn returns_error_for_invalid_utf8() {
        let path = write_temp("ws_test_bad.ws", &[0x80, 0x81, 0x82]);
        assert!(read_script_file(&path).is_err());
    }

    #[test]
    fn maps_core_diagnostics_to_lsp_diagnostics() {
        let document = parse_document("function Bad() {\n a = 1;\n var b : int;\n}\n")
            .expect("document should parse");

        let diagnostics = lsp_diagnostics(&document);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source.as_deref(), Some("witcherscript"));
        assert_eq!(
            diagnostics[0].message,
            "local variable declarations must precede executable statements"
        );
    }

    #[test]
    fn maps_symbols_to_lsp_document_symbols() {
        let document = parse_document("class CExample {\n var value : int;\n}\n")
            .expect("document should parse");

        let symbols = document_symbols(&document.symbols, None);

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
    fn formats_hover_contents_as_markdown() {
        let source = "function Make() {\n var dataObject : CScriptedFlashObject;\n dataObject = dataObject;\n}\n";
        let document = parse_document(source).expect("document should parse");
        let mut workspace = WorkspaceIndex::default();
        workspace.update_document("file:///example.ws", &document);

        let definition = resolve_definition(
            "file:///example.ws",
            &document,
            &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
            SourcePosition {
                line: 2,
                character: 2,
            },
        )
        .expect("local variable should resolve");

        let markdown = hover_markdown(&definition);

        assert_eq!(
            markdown,
            "```witcherscript\nvar dataObject : CScriptedFlashObject\n```\n\nDefined in [example.ws:2](file:///example.ws#L2)"
        );
        assert!(!markdown.contains("Defined in file://"));
    }

    #[test]
    fn formats_annotated_function_hover_with_annotation_first() {
        let source =
            "@wrapMethod(CR4Player)\nfunction OnSpawned(spawnData : SEntitySpawnData) {\n}\n";
        let document = parse_document(source).expect("document should parse");
        let mut workspace = WorkspaceIndex::default();
        workspace.update_document("file:///fov.ws", &document);

        let definition = resolve_definition(
            "file:///fov.ws",
            &document,
            &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
            SourcePosition {
                line: 1,
                character: 9,
            },
        )
        .expect("function should resolve");

        let markdown = hover_markdown(&definition);

        assert_eq!(
            markdown,
            "```witcherscript\n@wrapMethod(CR4Player)\nfunction OnSpawned(spawnData : SEntitySpawnData)\n```\n\nDefined in [fov.ws:2](file:///fov.ws#L2)"
        );
    }

    #[test]
    fn formats_parameter_hover_with_parenthesised_label() {
        let source = "function Make(spawnData : SEntitySpawnData) {\n spawnData = spawnData;\n}\n";
        let document = parse_document(source).expect("document should parse");
        let mut workspace = WorkspaceIndex::default();
        workspace.update_document("file:///example.ws", &document);

        let definition = resolve_definition(
            "file:///example.ws",
            &document,
            &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
            SourcePosition {
                line: 1,
                character: 2,
            },
        )
        .expect("parameter should resolve");

        let markdown = hover_markdown(&definition);

        assert_eq!(
            markdown,
            "```witcherscript\n(parameter) spawnData : SEntitySpawnData\n```\n\nDefined in [example.ws:1](file:///example.ws#L1)"
        );
    }

    #[test]
    fn formats_method_hover_with_owning_class_prefix() {
        let source = "class CExample {\n public function DoThing(x : int) : bool {}\n}\n";
        let document = parse_document(source).expect("document should parse");
        let mut workspace = WorkspaceIndex::default();
        workspace.update_document("file:///example.ws", &document);

        let definition = resolve_definition(
            "file:///example.ws",
            &document,
            &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
            SourcePosition {
                line: 1,
                character: 17,
            },
        )
        .expect("method should resolve");

        let markdown = hover_markdown(&definition);

        assert_eq!(
            markdown,
            "```witcherscript\n(method) CExample.DoThing(x : int) : bool\n```\n\nDefined in [example.ws:2](file:///example.ws#L2)"
        );
    }

    #[test]
    fn formats_inherited_method_hover_with_superclass_name() {
        let source_a = "class A extends B {\n function Test() {\n  Inherited();\n }\n}\n";
        let source_b = "class B {\n public function Inherited() : int {}\n}\n";
        let doc_a = parse_document(source_a).expect("document should parse");
        let doc_b = parse_document(source_b).expect("document should parse");
        let mut workspace = WorkspaceIndex::default();
        workspace.update_document("file:///a.ws", &doc_a);
        workspace.update_document("file:///b.ws", &doc_b);

        let definition = resolve_definition(
            "file:///a.ws",
            &doc_a,
            &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
            SourcePosition {
                line: 2,
                character: 3,
            },
        )
        .expect("inherited method should resolve");

        let text = witcherscript_parser::resolve::hover_text(&definition);
        assert_eq!(text, "(method) B.Inherited() : int");
    }

    #[test]
    fn formats_field_hover_with_full_declaration() {
        let source = "class CExample {\n protected editable var ignore : bool;\n}\n";
        let document = parse_document(source).expect("document should parse");
        let mut workspace = WorkspaceIndex::default();
        workspace.update_document("file:///example.ws", &document);

        let definition = resolve_definition(
            "file:///example.ws",
            &document,
            &SymbolDb::new(&workspace, &WorkspaceIndex::default()),
            SourcePosition {
                line: 1,
                character: 25,
            },
        )
        .expect("field should resolve");

        let text = witcherscript_parser::resolve::hover_text(&definition);
        assert!(
            text.starts_with("(field) "),
            "field hover should start with '(field) '"
        );
        assert!(text.contains("ignore"), "field hover should include name");
        assert!(text.contains("bool"), "field hover should include type");
    }
}
