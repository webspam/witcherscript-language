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

pub(crate) enum LegacyScriptStatusNotification {}

impl Notification for LegacyScriptStatusNotification {
    type Params = LegacyScriptStatusParams;
    const METHOD: &'static str = "witcherscript/legacyScriptStatus";
}
