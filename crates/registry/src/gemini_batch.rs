//! PostgreSQL-only Gemini Batch authority domain model.
//!
//! This module owns validation and secret-free persistence shapes. Encryption and provider
//! execution remain callers' responsibilities; ciphertext and opaque profile identifiers are
//! accepted only as already-produced values.

use crate::{ProviderTurnCalibrationEvent, PROVIDER_GOOGLE};
use anyhow::{bail, Context, Result};

pub const GEMINI_BATCH_DISPATCH_LEADER: &str = "gemini_batch_dispatch";
pub const MAX_BATCH_PAGE_SIZE: i64 = 1_000;
pub const MAX_BATCH_FILE_CHUNK_PAGE_SIZE: i64 = 128;
pub const MAX_BATCH_ACTIVE_ITEMS_PER_ACCOUNT: i64 = 16;
pub const MAX_BATCH_PRUNE_LIMIT: usize = 5_000;
pub const MAX_BATCH_FILE_CHUNK_BYTES: i64 = 8 * 1024 * 1024;
pub const MAX_BATCH_FILE_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub const MAX_BATCH_ACCOUNT_FILE_BYTES: i64 = 20 * 1024 * 1024 * 1024;
pub const MAX_BATCH_NONTERMINAL_JOBS: i64 = 100;
pub const MAX_BATCH_REFERENCED_FILE_BYTES: i64 = MAX_BATCH_FILE_BYTES;
pub const BATCH_RESULT_RETENTION_SECS: i64 = 42 * 24 * 60 * 60;

#[derive(Debug)]
pub struct GeminiBatchUnsupported;
impl std::fmt::Display for GeminiBatchUnsupported {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Gemini Batch requires PostgreSQL authority")
    }
}
impl std::error::Error for GeminiBatchUnsupported {}
pub fn is_gemini_batch_unsupported(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<GeminiBatchUnsupported>())
}

#[derive(Debug)]
pub struct GeminiBatchIdempotencyConflict;
impl std::fmt::Display for GeminiBatchIdempotencyConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Gemini Batch idempotency digest replay conflict")
    }
}
impl std::error::Error for GeminiBatchIdempotencyConflict {}
pub fn is_gemini_batch_idempotency_conflict(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<GeminiBatchIdempotencyConflict>())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiBatchInputKind {
    Inline,
    File,
}
impl GeminiBatchInputKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::File => "file",
        }
    }
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "inline" => Ok(Self::Inline),
            "file" => Ok(Self::File),
            _ => bail!("invalid Gemini Batch input kind"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiBatchItemState {
    Queued,
    Claimed,
    Dispatching,
    SettlementPending,
    Succeeded,
    Failed,
    Indeterminate,
    Canceled,
}
impl GeminiBatchItemState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Dispatching => "dispatching",
            Self::SettlementPending => "settlement_pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Indeterminate => "indeterminate",
            Self::Canceled => "canceled",
        }
    }
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "claimed" => Ok(Self::Claimed),
            "dispatching" => Ok(Self::Dispatching),
            "settlement_pending" => Ok(Self::SettlementPending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "indeterminate" => Ok(Self::Indeterminate),
            "canceled" => Ok(Self::Canceled),
            _ => bail!("invalid Gemini Batch item state"),
        }
    }
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Indeterminate | Self::Canceled
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiBatchTerminalClass {
    Success,
    ClientError,
    Quota,
    Auth,
    Timeout,
    Transport,
    UpstreamError,
    ProtocolError,
    Indeterminate,
    Canceled,
    Expired,
}
impl GeminiBatchTerminalClass {
    #[allow(dead_code)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ClientError => "client_error",
            Self::Quota => "quota",
            Self::Auth => "auth",
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::UpstreamError => "upstream_error",
            Self::ProtocolError => "protocol_error",
            Self::Indeterminate => "indeterminate",
            Self::Canceled => "canceled",
            Self::Expired => "expired",
        }
    }
    pub(crate) fn parse(v: &str) -> Result<Self> {
        match v {
            "success" => Ok(Self::Success),
            "client_error" => Ok(Self::ClientError),
            "quota" => Ok(Self::Quota),
            "auth" => Ok(Self::Auth),
            "timeout" => Ok(Self::Timeout),
            "transport" => Ok(Self::Transport),
            "upstream_error" => Ok(Self::UpstreamError),
            "protocol_error" => Ok(Self::ProtocolError),
            "indeterminate" => Ok(Self::Indeterminate),
            "canceled" => Ok(Self::Canceled),
            "expired" => Ok(Self::Expired),
            _ => bail!("invalid Gemini Batch terminal class"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiBatchJobState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeminiBatchStats {
    pub request_count: i64,
    pub successful_request_count: i64,
    pub failed_request_count: i64,
    pub pending_request_count: i64,
}

#[derive(Clone, PartialEq, Eq)]
pub struct GeminiBatchEncryptedBlob {
    pub kind: String,
    pub key_id: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub plaintext_len: i64,
    pub plaintext_digest: [u8; 32],
    pub retention_ts: i64,
}
impl std::fmt::Debug for GeminiBatchEncryptedBlob {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeminiBatchEncryptedBlob")
            .field("kind", &self.kind)
            .field("key_id", &self.key_id)
            .field("nonce", &"[REDACTED]")
            .field("ciphertext", &"[REDACTED]")
            .field("plaintext_len", &self.plaintext_len)
            .field("plaintext_digest", &"[REDACTED]")
            .field("retention_ts", &self.retention_ts)
            .finish()
    }
}
impl GeminiBatchEncryptedBlob {
    pub fn validate(&self, created: i64) -> Result<()> {
        if !matches!(
            self.kind.as_str(),
            "request" | "metadata" | "result" | "error"
        ) || self.key_id.is_empty()
            || self.key_id.len() > 128
            || self.nonce.len() != 24
            || self.ciphertext.len() as i64 != self.plaintext_len + 16
            || self.plaintext_len < 0
            || self.retention_ts < created
        {
            bail!("invalid Gemini Batch encrypted blob")
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchCreateItem {
    pub item_index: i64,
    pub request_id: String,
    pub logical_request_id: String,
    pub execution_group_id: String,
    pub client_key: Option<String>,
    pub request_digest: [u8; 32],
    pub input_file_id: Option<String>,
    pub referenced_file_ids: Vec<String>,
    pub hold_nano: i64,
    pub payable_multiplier_bp: i64,
    pub priced_ts: i64,
    pub tariff_family: String,
    pub tariff_version: i64,
    pub tariff_schedule_id: String,
    pub request_blob: GeminiBatchEncryptedBlob,
    pub metadata_blob: Option<GeminiBatchEncryptedBlob>,
}
impl GeminiBatchCreateItem {
    pub fn validate(&self, created: i64) -> Result<()> {
        if self.item_index < 0
            || self.request_id.is_empty()
            || self.logical_request_id.is_empty()
            || self.execution_group_id.is_empty()
            || self.client_key.as_ref().is_some_and(|v| v.len() > 512)
            || self.hold_nano < 0
            || !(0..=10000).contains(&self.payable_multiplier_bp)
            || self.priced_ts <= 0
            || self.tariff_family.is_empty()
            || self.tariff_version <= 0
            || self.tariff_schedule_id.is_empty()
        {
            bail!("invalid Gemini Batch create item")
        }
        if self.request_blob.kind != "request" {
            bail!("invalid request blob kind")
        }
        self.request_blob.validate(created)?;
        if let Some(v) = &self.metadata_blob {
            if v.kind != "metadata" {
                bail!("invalid metadata blob kind")
            }
            v.validate(created)?
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchCreate {
    pub job_id: String,
    pub account_id: String,
    pub creator_key_id: String,
    pub public_model: String,
    pub display_name: String,
    pub canonical_request_digest: [u8; 32],
    pub idempotency_digest: Option<[u8; 32]>,
    pub priority: i64,
    pub input_kind: GeminiBatchInputKind,
    pub input_file_id: Option<String>,
    pub schema_version: i32,
    pub encryption_policy_version: i32,
    pub create_ts: i64,
    pub deadline_ts: i64,
    pub items: Vec<GeminiBatchCreateItem>,
}
impl GeminiBatchCreate {
    pub fn validate(&self) -> Result<i64> {
        if self.job_id.is_empty()
            || self.account_id.is_empty()
            || self.creator_key_id.is_empty()
            || self.public_model.is_empty()
            || self.display_name.is_empty()
            || self.schema_version <= 0
            || self.encryption_policy_version <= 0
            || self.create_ts <= 0
            || self.deadline_ts <= self.create_ts
            || self.items.is_empty()
            || (self.input_kind == GeminiBatchInputKind::File) != self.input_file_id.is_some()
        {
            bail!("invalid Gemini Batch create")
        };
        let mut sum = 0i64;
        for (i, v) in self.items.iter().enumerate() {
            v.validate(self.create_ts)?;
            if v.item_index != i64::try_from(i).context("item index overflow")? {
                bail!("non-contiguous item index")
            }
            sum = sum
                .checked_add(v.hold_nano)
                .context("aggregate hold overflow")?
        }
        Ok(sum)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeminiBatchCreateOutcome {
    Created { balance_nano: i64 },
    Replay { job_id: String },
    RejectedFunds,
    RejectedLimit,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchItem {
    pub job_id: String,
    pub item_index: i64,
    pub request_id: String,
    pub logical_request_id: String,
    pub execution_group_id: String,
    pub creator_key_id: String,
    pub state: GeminiBatchItemState,
    pub terminal_class: Option<GeminiBatchTerminalClass>,
    pub claim_generation: i64,
    pub worker_instance: Option<String>,
    pub worker_epoch: Option<i64>,
    pub lease_until: Option<i64>,
    pub selected_profile_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchJob {
    pub job_id: String,
    pub account_id: String,
    pub creator_key_id: String,
    pub public_model: String,
    pub display_name: String,
    pub priority: i64,
    pub input_kind: GeminiBatchInputKind,
    pub input_file_id: Option<String>,
    pub output_file_id: Option<String>,
    pub cancel_requested_ts: Option<i64>,
    pub create_ts: i64,
    pub update_ts: i64,
    pub deadline_ts: i64,
    pub completed_ts: Option<i64>,
    pub delete_ts: Option<i64>,
    pub result_expiration_ts: Option<i64>,
    pub state: GeminiBatchJobState,
    pub stats: GeminiBatchStats,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchJobDetail {
    pub job: GeminiBatchJob,
    pub items: Vec<GeminiBatchItem>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchPageCursor {
    pub create_ts: i64,
    pub job_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchJobPage {
    pub jobs: Vec<GeminiBatchJob>,
    pub next_cursor: Option<GeminiBatchPageCursor>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchClaim {
    pub job_id: String,
    pub account_id: String,
    pub item_index: i64,
    pub request_id: String,
    pub claim_generation: i64,
    pub lease_until: i64,
    pub profile_id: String,
}

/// Complete secret-bearing worker input returned only by the atomic claim transaction.
///
/// Its custom `Debug` deliberately omits request/metadata ciphertext and opaque file identifiers.
#[derive(Clone, PartialEq, Eq)]
pub struct GeminiBatchClaimedItem {
    pub claim: GeminiBatchClaim,
    pub public_model: String,
    pub request_blob: GeminiBatchEncryptedBlob,
    pub metadata_blob: Option<GeminiBatchEncryptedBlob>,
    pub hold_nano: i64,
    pub payable_multiplier_bp: i64,
    pub priced_ts: i64,
    pub tariff_family: String,
    pub tariff_version: i64,
    pub tariff_schedule_id: String,
    pub creator_key_id: String,
    pub input_file_id: Option<String>,
    pub referenced_file_ids: Vec<String>,
}
impl std::fmt::Debug for GeminiBatchClaimedItem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeminiBatchClaimedItem")
            .field("claim", &self.claim)
            .field("public_model", &self.public_model)
            .field("request_blob", &"[REDACTED]")
            .field(
                "metadata_blob",
                &self.metadata_blob.as_ref().map(|_| "[REDACTED]"),
            )
            .field("hold_nano", &self.hold_nano)
            .field("payable_multiplier_bp", &self.payable_multiplier_bp)
            .field("priced_ts", &self.priced_ts)
            .field("tariff_family", &self.tariff_family)
            .field("tariff_version", &self.tariff_version)
            .field("tariff_schedule_id", &self.tariff_schedule_id)
            .field("creator_key_id", &self.creator_key_id)
            .field(
                "input_file_id",
                &self.input_file_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("referenced_file_count", &self.referenced_file_ids.len())
            .finish()
    }
}

/// A stale claim whose money must be resolved by the ordinary settlement authority.
///
/// Recovery never releases a hold or terminalizes an item directly. The consumer chooses the
/// fleet's unknown-usage charge policy, builds encrypted error output, and enqueues a fenced
/// [`GeminiBatchSettlementIntent`] through the normal settlement path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchRecoveryCandidate {
    pub job_id: String,
    pub account_id: String,
    pub item_index: i64,
    pub request_id: String,
    pub claim_generation: i64,
    pub profile_id: String,
    pub hold_nano: i64,
    pub disposition: GeminiBatchSettlementDisposition,
    pub terminal_state: GeminiBatchItemState,
    pub terminal_class: GeminiBatchTerminalClass,
    pub actual_send_evidence: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct GeminiBatchFileChunk {
    pub chunk_index: i64,
    pub key_id: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub plaintext_len: i64,
    pub plaintext_digest: [u8; 32],
    pub created_ts: i64,
}
impl std::fmt::Debug for GeminiBatchFileChunk {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeminiBatchFileChunk")
            .field("chunk_index", &self.chunk_index)
            .field("key_id", &self.key_id)
            .field("nonce", &"[REDACTED]")
            .field("ciphertext", &"[REDACTED]")
            .field("plaintext_len", &self.plaintext_len)
            .field("plaintext_digest", &"[REDACTED]")
            .field("created_ts", &self.created_ts)
            .finish()
    }
}
impl GeminiBatchFileChunk {
    pub fn validate(&self) -> Result<()> {
        if self.chunk_index < 0
            || self.key_id.is_empty()
            || self.nonce.len() != 24
            || !(0..=MAX_BATCH_FILE_CHUNK_BYTES).contains(&self.plaintext_len)
            || self.ciphertext.len() as i64 != self.plaintext_len + 16
            || self.created_ts <= 0
        {
            bail!("invalid file chunk")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchFileCreate {
    pub file_id: String,
    pub account_id: String,
    pub display_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub sha256_digest: [u8; 32],
    pub source_kind: String,
    pub create_ts: i64,
    pub expiration_ts: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiBatchFileCreateOutcome {
    Created,
    Replay,
    Unavailable,
    RejectedQuota,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeminiBatchFileCompletion {
    pub completed_ts: i64,
    /// Whole plaintext SHA-256 computed by the trusted streaming encryption producer.
    pub whole_file_sha256_digest: [u8; 32],
    /// Domain-separated digest of the exact ordered chunk manifest. Registry recomputes this from
    /// durable chunk indices, plaintext lengths and per-chunk plaintext digests before activation.
    pub chunk_manifest_digest: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchFile {
    pub file_id: String,
    pub account_id: String,
    pub display_name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub sha256_digest: [u8; 32],
    pub source_kind: String,
    pub state: String,
    pub storage_kind: String,
    pub create_ts: i64,
    pub update_ts: i64,
    pub expiration_ts: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchFileChunkPage {
    pub chunks: Vec<GeminiBatchFileChunk>,
    pub next_chunk_index: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiBatchSettlementDisposition {
    Settle,
    Cancel,
    Indeterminate,
    Expire,
}
impl GeminiBatchSettlementDisposition {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Settle => "settle",
            Self::Cancel => "cancel",
            Self::Indeterminate => "indeterminate",
            Self::Expire => "expire",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchUsage {
    pub input_tokens: i64,
    pub tool_prompt_tokens: i64,
    pub audio_input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cached_audio_input_tokens: i64,
    pub output_tokens: i64,
    pub thinking_output_tokens: i64,
    pub image_output_tokens: i64,
    pub search_queries: i64,
    pub grounded_search_prompts: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiBatchSettlementIntent {
    pub job_id: String,
    pub item_index: i64,
    pub request_id: String,
    pub claim_generation: i64,
    pub disposition: GeminiBatchSettlementDisposition,
    pub actual_nano: i64,
    pub charge_basis_nano: i64,
    pub real_nano: i64,
    pub usage: Option<GeminiBatchUsage>,
    pub result_blob: GeminiBatchEncryptedBlob,
    pub terminal_state: GeminiBatchItemState,
    pub terminal_class: GeminiBatchTerminalClass,
    pub calibration: Option<ProviderTurnCalibrationEvent>,
    pub completed_ts: i64,
}
impl GeminiBatchSettlementIntent {
    pub fn validate(&self) -> Result<()> {
        if self.job_id.is_empty()
            || self.item_index < 0
            || self.request_id.is_empty()
            || self.claim_generation <= 0
            || self.actual_nano < 0
            || self.charge_basis_nano < 0
            || self.real_nano < 0
            || self.completed_ts <= 0
            || !self.terminal_state.is_terminal()
        {
            bail!("invalid settlement intent")
        }
        let usage_is_valid = self.usage.as_ref().is_none_or(|usage| {
            let values = [
                usage.input_tokens,
                usage.tool_prompt_tokens,
                usage.audio_input_tokens,
                usage.cached_input_tokens,
                usage.cached_audio_input_tokens,
                usage.output_tokens,
                usage.thinking_output_tokens,
                usage.image_output_tokens,
                usage.search_queries,
                usage.grounded_search_prompts,
            ];
            values.iter().all(|value| *value >= 0)
                && usage.tool_prompt_tokens <= usage.input_tokens
                && usage.cached_audio_input_tokens <= usage.cached_input_tokens
                && usage.thinking_output_tokens <= usage.output_tokens
        });
        if !usage_is_valid {
            bail!("invalid Gemini Batch usage")
        }
        let (expected_state, expected_class, expected_blob, measured) = match self.disposition {
            GeminiBatchSettlementDisposition::Settle => (
                GeminiBatchItemState::Succeeded,
                GeminiBatchTerminalClass::Success,
                "result",
                true,
            ),
            GeminiBatchSettlementDisposition::Cancel => (
                GeminiBatchItemState::Canceled,
                GeminiBatchTerminalClass::Canceled,
                "error",
                false,
            ),
            GeminiBatchSettlementDisposition::Indeterminate => (
                GeminiBatchItemState::Indeterminate,
                GeminiBatchTerminalClass::Indeterminate,
                "error",
                false,
            ),
            GeminiBatchSettlementDisposition::Expire => (
                GeminiBatchItemState::Canceled,
                GeminiBatchTerminalClass::Expired,
                "error",
                false,
            ),
        };
        if self.terminal_state != expected_state
            || self.terminal_class != expected_class
            || self.result_blob.kind != expected_blob
            || measured != self.usage.is_some()
            || measured != self.calibration.is_some()
            || (!measured && (self.charge_basis_nano != 0 || self.real_nano != 0))
        {
            bail!("settlement disposition shape mismatch")
        }
        if let Some(calibration) = &self.calibration {
            if calibration.provider != PROVIDER_GOOGLE
                || calibration.request_id != self.request_id
                || calibration.completed_at != self.completed_ts
            {
                bail!("calibration identity mismatch")
            }
            crate::validate_provider_turn_calibration_event(calibration)?;
        }
        self.result_blob.validate(self.completed_ts)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeminiBatchCancelResult {
    pub cancel_requested: bool,
    pub canceled_items: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeminiBatchPruneReport {
    pub blobs: usize,
    pub chunks: usize,
    pub files: usize,
    pub items: usize,
    pub jobs: usize,
}

/// Fleet-wide, fixed-cardinality operational snapshot for Gemini Batch.
///
/// The report deliberately contains no account, job, item, model, file, or profile identity. It is
/// safe to expose through Prometheus and the protected fleet summary without cardinality growing
/// with customer activity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeminiBatchOperationalReport {
    pub queued_items: i64,
    pub oldest_queued_age_seconds: i64,
    pub active_items: i64,
    pub completed_items: i64,
    pub error_items: i64,
    pub indeterminate_items: i64,
    pub settlement_pending: i64,
    pub settlement_oldest_age_seconds: i64,
    pub settlement_retries: i64,
    pub file_bytes: i64,
    pub file_chunks: i64,
}
