use std::sync::atomic::Ordering;

use lsp_types::request::{CodeLensRefresh, SemanticTokensRefresh, WorkspaceDiagnosticRefresh};
use tracing::trace;

use crate::backend::Backend;

impl Backend {
    pub(crate) fn spawn_view_refresher(&self) {
        if self.view_refresher_spawned.swap(true, Ordering::AcqRel) {
            return;
        }
        let backend = self.clone();
        tokio::spawn(async move {
            backend.run_view_refresher().await;
        });
    }

    async fn run_view_refresher(&self) {
        let mut last_emitted: u64 = 0;
        loop {
            self.views_dirty.notified().await;
            let version = self.state_version.load(Ordering::Acquire);
            if version == last_emitted {
                continue;
            }
            last_emitted = version;
            trace!(op = "view_refresh", version, "emit");
            // Highest visual impact first; diagnostics is pull-model so the version bump already staled it.
            if self
                .client_supports_semantic_tokens_refresh
                .load(Ordering::Acquire)
            {
                Self::send_refresh::<SemanticTokensRefresh>(&self.client).await;
            }
            if self
                .client_supports_code_lens_refresh
                .load(Ordering::Acquire)
            {
                Self::send_refresh::<CodeLensRefresh>(&self.client).await;
            }
            if self
                .client_supports_pull_diagnostics
                .load(Ordering::Acquire)
            {
                Self::send_refresh::<WorkspaceDiagnosticRefresh>(&self.client).await;
            }
        }
    }
}
