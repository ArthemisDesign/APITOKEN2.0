//! Default-off request-facing Gemini Batch dependencies.
use super::{GeminiBatchAuthority, GeminiBatchDataKeyring, GeminiGateway};
use std::sync::Arc;
#[derive(Clone)]
pub struct GeminiBatchPublicFacade {
    authority: GeminiBatchAuthority,
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
        gateway: Arc<GeminiGateway>,
        keys: Arc<GeminiBatchDataKeyring>,
    ) -> Arc<Self> {
        Arc::new(Self {
            authority,
            gateway,
            keys,
        })
    }
    pub(crate) fn authority(&self) -> &GeminiBatchAuthority {
        &self.authority
    }
    pub(crate) fn gateway(&self) -> &GeminiGateway {
        &self.gateway
    }
    pub(crate) fn keys(&self) -> &GeminiBatchDataKeyring {
        &self.keys
    }
}
