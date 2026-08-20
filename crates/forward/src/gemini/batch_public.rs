//! Default-off request-facing Gemini Batch dependencies.
use super::{GeminiBatchAuthority, GeminiBatchDataKeyring, GeminiBatchIngest, GeminiGateway};
use std::sync::Arc;
#[derive(Clone)]
pub struct GeminiBatchPublicFacade {
    authority: GeminiBatchAuthority,
    ingest: GeminiBatchIngest,
    gateway: Arc<GeminiGateway>,
    keys: Arc<GeminiBatchDataKeyring>,
}
impl std::fmt::Debug for GeminiBatchPublicFacade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiBatchPublicFacade")
            .field("enabled", &true)
            .field("keys", &"REDACTED")
            .finish()
    }
}
impl GeminiBatchPublicFacade {
    pub fn new(
        authority: GeminiBatchAuthority,
        ingest: GeminiBatchIngest,
        gateway: Arc<GeminiGateway>,
        keys: Arc<GeminiBatchDataKeyring>,
    ) -> Arc<Self> {
        Arc::new(Self {
            authority,
            ingest,
            gateway,
            keys,
        })
    }
    pub(crate) fn authority(&self) -> &GeminiBatchAuthority {
        &self.authority
    }
    pub(crate) fn ingest(&self) -> &GeminiBatchIngest {
        &self.ingest
    }
    pub(crate) fn gateway(&self) -> &GeminiGateway {
        &self.gateway
    }
    pub(crate) fn keys(&self) -> &GeminiBatchDataKeyring {
        &self.keys
    }
}
