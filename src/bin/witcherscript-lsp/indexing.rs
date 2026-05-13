use std::fs;
use std::sync::atomic::Ordering;
use std::time::Instant;

use rayon::prelude::*;
use serde_json::Value;
use tower_lsp::lsp_types::{ConfigurationItem, Position, Url};
use tracing::{debug, error, info, warn};
use witcherscript_parser::document::{parse_document, ParsedDocument};
use witcherscript_parser::files::collect_witcherscript_files;
use witcherscript_parser::resolve::{resolve_definition, Definition, SymbolDb};
use witcherscript_parser::script_env::parse_script_environment;

use crate::backend::Backend;
use crate::convert::{lsp_diagnostics, read_script_file, source_position};
use crate::logging::{level_from_str, level_to_u8};

impl Backend {
    pub(crate) async fn update_open_document(&self, uri: Url, text: String) {
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
            }
        }
    }

    pub(crate) async fn index_workspace(&self) {
        let roots = self.workspace_roots.lock().await.clone();
        if roots.is_empty() {
            return;
        }

        info!(roots = ?roots, "indexing workspace");
        let start = Instant::now();

        let Ok(files) = collect_witcherscript_files(&roots) else {
            warn!("failed to collect workspace files");
            return;
        };

        let file_count = files.len();

        let parsed: Vec<(String, ParsedDocument)> = files
            .iter()
            .filter_map(|path| {
                let source = fs::read_to_string(path)
                    .map_err(|_| warn!(path = %path.display(), "failed to read workspace file"))
                    .ok()?;
                let document = parse_document(source)
                    .map_err(|_| warn!(path = %path.display(), "failed to parse workspace file"))
                    .ok()?;
                let uri = Url::from_file_path(path)
                    .map_err(|_| warn!(path = %path.display(), "failed to convert path to URI"))
                    .ok()?;
                debug!(uri = %uri, "indexed workspace file");
                Some((uri.to_string(), document))
            })
            .collect();

        let indexed = parsed.len();
        {
            let mut index = self.workspace_index.lock().await;
            let mut docs = self.workspace_documents.lock().await;
            for (uri, document) in parsed {
                index.update_document(uri.as_str(), &document);
                docs.insert(uri, document);
            }
        }

        info!(
            indexed,
            file_count,
            elapsed_ms = start.elapsed().as_millis(),
            "workspace indexed"
        );
    }

    /// Pull `witcherscript.gameDirectory`, `witcherscript.logLevel`, and formatter
    /// settings from the client via `workspace/configuration`.
    pub(crate) async fn fetch_config(&self) {
        let items = vec![
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.gameDirectory".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.logLevel".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.lineLimit".to_string()),
            },
            ConfigurationItem {
                scope_uri: None,
                section: Some("witcherscript.formatter.compactColon".to_string()),
            },
        ];
        let Ok(values) = self.client.configuration(items).await else {
            warn!("workspace/configuration request failed");
            return;
        };
        let mut iter = values.into_iter();
        if let Some(Value::String(path_str)) = iter.next() {
            if !path_str.is_empty() {
                *self.base_scripts_path.lock().await = Some(std::path::PathBuf::from(path_str));
            }
        }
        if let Some(Value::String(level_str)) = iter.next() {
            let new_level = level_to_u8(level_from_str(&level_str));
            self.log_level.store(new_level, Ordering::Relaxed);
            info!(level = %level_str, "log level updated");
        }
        if let Some(Value::Number(n)) = iter.next() {
            if let Some(limit) = n.as_u64() {
                self.formatter_line_limit
                    .store(limit as u32, Ordering::Relaxed);
            }
        }
        if let Some(Value::Bool(compact)) = iter.next() {
            self.formatter_compact_colon
                .store(compact, Ordering::Relaxed);
        }
    }

    pub(crate) async fn resolve_at(&self, uri: &Url, position: Position) -> Option<Definition> {
        let documents = self.documents.lock().await;
        let document = documents.get(uri)?;
        let workspace = self.workspace_index.lock().await;
        let base = self.base_scripts_index.lock().await;
        let script_env = self.script_env.lock().await;
        let db = SymbolDb::new(&workspace, &base).with_script_env(&script_env);
        resolve_definition(uri.as_str(), document, &db, source_position(position))
    }

    /// Parse all `.ws` files under `base_scripts_path` in parallel and store their
    /// symbols in `base_scripts_index`. No-ops if no path is configured.
    pub(crate) async fn index_base_scripts(&self) {
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

        info!(path = %path.display(), "indexing base scripts");
        let start = Instant::now();

        let Ok(files) = collect_witcherscript_files(&[path]) else {
            warn!("failed to collect base script files");
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

        let elapsed_ms = start.elapsed().as_millis();
        info!(
            indexed,
            file_count,
            elapsed_ms,
            elapsed_secs = format!("{:.1}", elapsed_ms as f32 / 1000.0),
            "base scripts indexed"
        );
    }
}
