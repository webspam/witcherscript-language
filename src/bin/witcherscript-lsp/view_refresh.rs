use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use lsp_types::request::{
    CodeLensRefresh, Request as LspRequest, SemanticTokensRefresh, WorkspaceDiagnosticRefresh,
};
use tracing::trace;

use crate::backend::Backend;

// A refresh round-trip is near-instant for a healthy client; this only reclaims a silent one.
const VIEW_REFRESH_TIMEOUT: Duration = Duration::from_secs(10);

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
            // Each send is independent: a client that stalls one channel must not block the others.
            self.spawn_refresh::<SemanticTokensRefresh>(
                &self.client_supports_semantic_tokens_refresh,
                "semanticTokens",
            );
            self.spawn_refresh::<CodeLensRefresh>(
                &self.client_supports_code_lens_refresh,
                "codeLens",
            );
            self.spawn_refresh::<WorkspaceDiagnosticRefresh>(
                &self.client_supports_pull_diagnostics,
                "diagnostic",
            );
        }
    }

    fn spawn_refresh<R: LspRequest<Params = ()>>(
        &self,
        supported: &AtomicBool,
        refresh: &'static str,
    ) where
        R::Result: Send,
    {
        if !supported.load(Ordering::Acquire) {
            return;
        }
        let client = self.client.clone();
        // Detached + timed so a connected-but-silent client cannot wedge or leak the refresher.
        crate::spawn_logged("view refresh", async move {
            if tokio::time::timeout(VIEW_REFRESH_TIMEOUT, Self::send_refresh::<R>(&client))
                .await
                .is_err()
            {
                trace!(op = "view_refresh", refresh, "timed out");
            }
        });
    }
}
