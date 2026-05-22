use lsp_types::notification::Notification;

use crate::file_scope::FileScope;

// camelCase to match the VS Code client's TypeScript interface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileScopeStatusParams {
    pub uri: String,
    pub scope: FileScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_script_path: Option<String>,
}

pub(crate) enum FileScopeStatusNotification {}

impl Notification for FileScopeStatusNotification {
    type Params = FileScopeStatusParams;
    const METHOD: &'static str = "witcherscript/fileScopeStatus";
}
