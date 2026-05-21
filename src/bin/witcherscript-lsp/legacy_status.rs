use lsp_types::notification::Notification;

// camelCase to match the VS Code client's TypeScript interface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyScriptStatusParams {
    pub uri: String,
    pub replaces_base_script: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_script_path: Option<String>,
}

impl LegacyScriptStatusParams {
    pub(crate) fn new(uri: String, replaced_script_path: Option<String>) -> Self {
        Self {
            uri,
            replaces_base_script: replaced_script_path.is_some(),
            replaced_script_path,
        }
    }
}

pub(crate) enum LegacyScriptStatusNotification {}

impl Notification for LegacyScriptStatusNotification {
    type Params = LegacyScriptStatusParams;
    const METHOD: &'static str = "witcherscript/legacyScriptStatus";
}
