//! OpenAI-compatible text API backed by the official Codex app-server protocol.
//!
//! The transport speaks newline-delimited JSON-RPC v2 to a pinned Codex child. It never reads or
//! replays ChatGPT bearer tokens: authentication remains owned by the official Codex profile.

mod api;
mod billing;
mod chat;
mod config;
mod history;
mod process;
mod runner;

pub use api::{model as openai_model, models as openai_models, responses as openai_responses};
pub use chat::completions as openai_chat_completions;
pub use config::{CodexConfig, CodexModel, CodexPrices};
pub use history::{HistoryError, StoredHistory};
pub(crate) use process::{AppServerEvent, CodexProcess, ProcessError};
pub use process::{CodexRateLimitWindow, CodexRateLimits};
pub(crate) use runner::{CodexTurnRequest, CodexTurnResult, CodexUsage, TurnUpdate};

use history::HistoryStore;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

pub(crate) fn new_id(prefix: &str) -> String {
    let mut random = [0u8; 16];
    if getrandom::fill(&mut random).is_err() {
        return format!("{prefix}_{}", crate::upstream::fresh_request_id());
    }
    let mut hex = String::with_capacity(32);
    for byte in random {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("{prefix}_{hex}")
}

#[derive(Clone, Debug)]
pub struct CodexOperationalStatus {
    pub process_live: bool,
    pub rate_limits: Option<CodexRateLimits>,
}

/// Owns and restarts the pinned app-server process. The composition layer calls `preflight` before
/// exposing a configured provider, while later transport failures are recovered lazily. Existing
/// Claude routing is completely independent when Codex is disabled.
pub struct CodexGateway {
    cfg: Arc<CodexConfig>,
    process: Mutex<Option<Arc<CodexProcess>>>,
    process_start: Mutex<()>,
    history: Arc<HistoryStore>,
    turns: Arc<Semaphore>,
}

impl CodexGateway {
    pub fn new(cfg: CodexConfig) -> anyhow::Result<Self> {
        let history = HistoryStore::new(
            cfg.history_redis_url.as_deref(),
            cfg.history_secret.as_deref(),
            cfg.history_ttl_secs,
            cfg.history_local_cap,
            cfg.history_redis_timeout_ms,
        )?;
        Ok(Self {
            turns: Arc::new(Semaphore::new(cfg.max_concurrent_turns.max(1))),
            cfg: Arc::new(cfg),
            process: Mutex::new(None),
            process_start: Mutex::new(()),
            history: Arc::new(history),
        })
    }

    pub fn config(&self) -> &CodexConfig {
        &self.cfg
    }

    pub(crate) fn history(&self) -> &HistoryStore {
        &self.history
    }

    pub(crate) async fn acquire_turn(
        &self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, ProcessError> {
        self.turns
            .clone()
            .try_acquire_owned()
            .map_err(|_| ProcessError::Busy)
    }

    /// Return a live, subscription-authenticated process. Startup is serialized, but normal
    /// JSON-RPC requests and independent turns are multiplexed over the same child.
    pub(crate) async fn process(&self) -> Result<Arc<CodexProcess>, ProcessError> {
        if let Some(process) = self.process.lock().await.as_ref().cloned() {
            if process.is_live() {
                return Ok(process);
            }
        }

        let _start = self.process_start.lock().await;
        if let Some(process) = self.process.lock().await.as_ref().cloned() {
            if process.is_live() {
                return Ok(process);
            }
        }

        let process = Arc::new(CodexProcess::spawn(self.cfg.clone()).await?);
        *self.process.lock().await = Some(process.clone());
        Ok(process)
    }

    /// Verify the pinned executable, start app-server, complete protocol initialization and prove
    /// that the dedicated profile is authenticated with a ChatGPT subscription. Returning only a
    /// diagnostic class keeps account metadata, paths and child messages out of composition logs.
    pub async fn preflight(&self) -> anyhow::Result<()> {
        self.process().await.map(|_| ()).map_err(|error| {
            anyhow::anyhow!(
                "Codex app-server preflight failed [{}]",
                error.diagnostic_class()
            )
        })
    }

    pub(crate) async fn invalidate(&self, process: &Arc<CodexProcess>) {
        let mut current = self.process.lock().await;
        if current
            .as_ref()
            .is_some_and(|candidate| Arc::ptr_eq(candidate, process))
        {
            *current = None;
        }
    }

    /// Read only cached operational state. Metrics collection never starts a provider process or
    /// triggers an authentication or network request.
    pub async fn operational_status(&self) -> CodexOperationalStatus {
        let process = self.process.lock().await.as_ref().cloned();
        match process {
            Some(process) => CodexOperationalStatus {
                process_live: process.is_live(),
                rate_limits: process.rate_limits().await,
            },
            None => CodexOperationalStatus {
                process_live: false,
                rate_limits: None,
            },
        }
    }
}
