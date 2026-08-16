//! Privacy-minimal request-observability domain types.
//!
//! These values contain only bounded structural evidence. They deliberately have no fields for
//! prompts, message bodies, tool payloads, schemas, arbitrary metadata, headers, addresses, user
//! agents, credentials, full API keys, email addresses, or provider subjects.

use anyhow::{bail, Result};

pub const REQUEST_FACT_ADMISSION_SCHEMA_VERSION: i32 = 1;
pub const REQUEST_FACT_TERMINAL_SCHEMA_VERSION: i32 = 1;
pub const MAX_REQUEST_FACT_BATCH: usize = 128;

pub const MAX_REQUEST_FACT_ID_LEN: usize = 36;
pub const MAX_REQUEST_FACT_ACCOUNT_ID_LEN: usize = 128;
pub const MAX_REQUEST_FACT_KEY_ID_LEN: usize = 128;
pub const MAX_REQUEST_FACT_CLIENT_VERSION_LEN: usize = 64;
pub const MAX_REQUEST_FACT_CLASS_LEN: usize = 64;
pub const MAX_REQUEST_FACT_MODEL_LEN: usize = 256;
pub const MAX_REQUEST_FACT_SERVICE_TIER_LEN: usize = 64;
pub const MAX_REQUEST_FACT_UPSTREAM_ID_LEN: usize = 256;
pub const MAX_REQUEST_FACT_FAILURE_CLASS_LEN: usize = 128;

pub const TOOL_CLASS_CUSTOM_FUNCTION: i32 = 1;
pub const TOOL_CLASS_CUSTOM_TOOL: i32 = 2;
pub const TOOL_CLASS_WEB_SEARCH: i32 = 4;
pub const TOOL_CLASS_COMPUTER: i32 = 8;
pub const TOOL_CLASS_CODE_EXECUTION: i32 = 16;
pub const TOOL_CLASS_MCP: i32 = 32;
pub const TOOL_CLASS_OTHER_REVIEWED: i32 = 64;
pub const TOOL_CLASS_MASK: i32 = TOOL_CLASS_CUSTOM_FUNCTION
    | TOOL_CLASS_CUSTOM_TOOL
    | TOOL_CLASS_WEB_SEARCH
    | TOOL_CLASS_COMPUTER
    | TOOL_CLASS_CODE_EXECUTION
    | TOOL_CLASS_MCP
    | TOOL_CLASS_OTHER_REVIEWED;

pub const MODALITY_TEXT: i32 = 1;
pub const MODALITY_IMAGE: i32 = 2;
pub const MODALITY_AUDIO: i32 = 4;
pub const MODALITY_VIDEO: i32 = 8;
pub const MODALITY_PDF: i32 = 16;
pub const MODALITY_MASK: i32 =
    MODALITY_TEXT | MODALITY_IMAGE | MODALITY_AUDIO | MODALITY_VIDEO | MODALITY_PDF;

macro_rules! closed_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $value),+ }
            }

            pub fn parse(value: &str) -> Result<Self> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => bail!(concat!(stringify!($name), " is outside its closed vocabulary")),
                }
            }
        }
    };
}

closed_enum!(ClientKind {
    ClaudeCode => "claude_code",
    OpenCode => "opencode",
    CodexCli => "codex_cli",
    Cursor => "cursor",
    Sdk => "sdk",
    Custom => "custom",
    Unknown => "unknown",
});
closed_enum!(ClientSource {
    Explicit => "explicit",
    Heuristic => "heuristic",
    Unknown => "unknown",
});
closed_enum!(ToolChoiceMode {
    Auto => "auto",
    Required => "required",
    None => "none",
    Named => "named",
    Unknown => "unknown",
});
closed_enum!(ProviderTerminalClass {
    Success => "success",
    ClientError => "client_error",
    Quota => "quota",
    Auth => "auth",
    Timeout => "timeout",
    Transport => "transport",
    UpstreamError => "upstream_error",
    ProtocolError => "protocol_error",
    Unknown => "unknown",
});
closed_enum!(DeliveryState {
    NotStarted => "not_started",
    Started => "started",
    Completed => "completed",
    Interrupted => "interrupted",
    Unknown => "unknown",
});
closed_enum!(BillingOutcome {
    Winner => "winner",
    Loser => "loser",
    ZeroMetered => "zero_metered",
    Canceled => "canceled",
    Reconciled => "reconciled",
    NotApplicable => "not_applicable",
    Unknown => "unknown",
});

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestFactAdmission {
    pub logical_request_id: String,
    pub billing_request_id: String,
    pub execution_group_id: Option<String>,
    pub attempt: i32,
    pub account_id: String,
    pub key_id: String,
    pub client_kind: ClientKind,
    pub client_source: ClientSource,
    pub client_version: Option<String>,
    pub provider_plane: String,
    pub route_class: String,
    pub request_class: String,
    pub requested_model: Option<String>,
    pub executable_model: Option<String>,
    pub stream_flag: bool,
    pub tools_declared_count: Option<i32>,
    pub tool_classes: Option<i32>,
    pub tool_choice_mode: Option<ToolChoiceMode>,
    pub parallel_tools_requested: Option<bool>,
    pub tool_results_in_input: Option<bool>,
    pub structured_output_flag: Option<bool>,
    pub reasoning_flag: Option<bool>,
    pub service_tier: Option<String>,
    pub input_modalities: Option<i32>,
    pub output_modalities: Option<i32>,
    pub admitted_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestFactTerminalEvidence {
    pub terminal_at: i64,
    pub http_status_code: Option<i32>,
    pub provider_terminal_class: ProviderTerminalClass,
    pub delivery_state: DeliveryState,
    pub downstream_disconnect: Option<bool>,
    pub upstream_request_id: Option<String>,
    pub first_public_byte_at: Option<i64>,
    pub internal_attempt_count: Option<i32>,
    pub failure_class: Option<String>,
    pub tool_calls_in_output: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRequestFact {
    pub logical_request_id: String,
    pub billing_request_id: Option<String>,
    pub execution_group_id: Option<String>,
    pub attempt: i32,
    pub account_id: String,
    pub key_id: String,
    pub client_kind: ClientKind,
    pub client_source: ClientSource,
    pub client_version: Option<String>,
    pub provider_plane: String,
    pub route_class: String,
    pub request_class: String,
    pub requested_model: Option<String>,
    pub executable_model: Option<String>,
    pub stream_flag: bool,
    pub tools_declared_count: Option<i32>,
    pub tool_classes: Option<i32>,
    pub tool_choice_mode: Option<ToolChoiceMode>,
    pub parallel_tools_requested: Option<bool>,
    pub tool_results_in_input: Option<bool>,
    pub structured_output_flag: Option<bool>,
    pub reasoning_flag: Option<bool>,
    pub service_tier: Option<String>,
    pub input_modalities: Option<i32>,
    pub output_modalities: Option<i32>,
    pub admitted_at: i64,
    pub terminal: RequestFactTerminalEvidence,
}

fn validate_bounded_ascii(
    value: &str,
    label: &str,
    max_len: usize,
    allow_empty: bool,
) -> Result<()> {
    if (!allow_empty && value.is_empty()) || value.len() > max_len {
        bail!("{label} has an invalid length");
    }
    if !value.is_ascii() || value.bytes().any(|byte| byte.is_ascii_control()) {
        bail!("{label} must be bounded printable ASCII on one line");
    }
    Ok(())
}

fn validate_optional_ascii(value: Option<&str>, label: &str, max_len: usize) -> Result<()> {
    if let Some(value) = value {
        validate_bounded_ascii(value, label, max_len, false)?;
    }
    Ok(())
}

pub fn is_canonical_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != MAX_REQUEST_FACT_ID_LEN
        || bytes[8] != b'-'
        || bytes[13] != b'-'
        || bytes[18] != b'-'
        || bytes[23] != b'-'
        || bytes[14] != b'4'
        || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
    {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 8 | 13 | 18 | 23) || matches!(byte, b'0'..=b'9' | b'a'..=b'f')
    })
}

impl RequestFactAdmission {
    pub fn validate(&self) -> Result<()> {
        if !is_canonical_uuid_v4(&self.logical_request_id)
            || !is_canonical_uuid_v4(&self.billing_request_id)
            || self
                .execution_group_id
                .as_deref()
                .is_some_and(|value| !is_canonical_uuid_v4(value))
        {
            bail!("request-fact identities must be canonical lowercase UUIDv4");
        }
        if self.attempt <= 0 || self.admitted_at < 0 {
            bail!("request-fact attempt and admission time must be nonnegative/positive");
        }
        validate_bounded_ascii(
            &self.account_id,
            "request-fact account id",
            MAX_REQUEST_FACT_ACCOUNT_ID_LEN,
            false,
        )?;
        validate_bounded_ascii(
            &self.key_id,
            "request-fact key id",
            MAX_REQUEST_FACT_KEY_ID_LEN,
            false,
        )?;
        validate_optional_ascii(
            self.client_version.as_deref(),
            "request-fact client version",
            MAX_REQUEST_FACT_CLIENT_VERSION_LEN,
        )?;
        for (value, label) in [
            (&self.provider_plane, "request-fact provider plane"),
            (&self.route_class, "request-fact route class"),
            (&self.request_class, "request-fact request class"),
        ] {
            validate_bounded_ascii(value, label, MAX_REQUEST_FACT_CLASS_LEN, false)?;
        }
        validate_optional_ascii(
            self.requested_model.as_deref(),
            "request-fact requested model",
            MAX_REQUEST_FACT_MODEL_LEN,
        )?;
        validate_optional_ascii(
            self.executable_model.as_deref(),
            "request-fact executable model",
            MAX_REQUEST_FACT_MODEL_LEN,
        )?;
        validate_optional_ascii(
            self.service_tier.as_deref(),
            "request-fact service tier",
            MAX_REQUEST_FACT_SERVICE_TIER_LEN,
        )?;
        if self.tools_declared_count.is_some_and(|value| value < 0) {
            bail!("request-fact tool count must be nonnegative");
        }
        if self
            .tool_classes
            .is_some_and(|value| value < 0 || value & !TOOL_CLASS_MASK != 0)
        {
            bail!("request-fact tool bitset contains unknown bits");
        }
        for (value, label) in [
            (self.input_modalities, "input"),
            (self.output_modalities, "output"),
        ] {
            if value.is_some_and(|value| value < 0 || value & !MODALITY_MASK != 0) {
                bail!("request-fact {label} modality bitset contains unknown bits");
            }
        }
        Ok(())
    }
}

impl RequestFactTerminalEvidence {
    pub fn validate(&self, admitted_at: i64) -> Result<()> {
        self.validate_with_delivery(admitted_at, None)
    }

    pub fn validate_with_delivery(
        &self,
        admitted_at: i64,
        delivery_started_at: Option<i64>,
    ) -> Result<()> {
        if admitted_at < 0 || self.terminal_at < 0 || self.terminal_at < admitted_at {
            bail!("request-fact terminal time precedes admission");
        }
        if delivery_started_at.is_some_and(|value| value < admitted_at || value > self.terminal_at)
        {
            bail!("request-fact delivery time is outside the lifecycle");
        }
        if self
            .http_status_code
            .is_some_and(|value| !(100..=599).contains(&value))
        {
            bail!("request-fact HTTP status is outside 100..599");
        }
        if self.first_public_byte_at.is_some_and(|value| {
            value < admitted_at
                || value > self.terminal_at
                || delivery_started_at.is_some_and(|delivery| value < delivery)
        }) {
            bail!("request-fact first-public-byte time is outside the lifecycle");
        }
        if self.internal_attempt_count.is_some_and(|value| value < 0) {
            bail!("request-fact internal attempt count must be nonnegative");
        }
        validate_optional_ascii(
            self.upstream_request_id.as_deref(),
            "request-fact upstream request id",
            MAX_REQUEST_FACT_UPSTREAM_ID_LEN,
        )?;
        validate_optional_ascii(
            self.failure_class.as_deref(),
            "request-fact failure class",
            MAX_REQUEST_FACT_FAILURE_CLASS_LEN,
        )?;
        Ok(())
    }
}

impl TerminalRequestFact {
    pub fn validate(&self) -> Result<()> {
        if !is_canonical_uuid_v4(&self.logical_request_id)
            || self
                .billing_request_id
                .as_deref()
                .is_some_and(|value| !is_canonical_uuid_v4(value))
            || self
                .execution_group_id
                .as_deref()
                .is_some_and(|value| !is_canonical_uuid_v4(value))
        {
            bail!("terminal request-fact identities must be canonical lowercase UUIDv4");
        }
        // Reuse admission validation with a private placeholder only when this event has no billing
        // identity. The placeholder is never persisted; it keeps the structural validation total.
        let admission = RequestFactAdmission {
            logical_request_id: self.logical_request_id.clone(),
            billing_request_id: self
                .billing_request_id
                .clone()
                .unwrap_or_else(|| "00000000-0000-4000-8000-000000000000".to_owned()),
            execution_group_id: self.execution_group_id.clone(),
            attempt: self.attempt,
            account_id: self.account_id.clone(),
            key_id: self.key_id.clone(),
            client_kind: self.client_kind,
            client_source: self.client_source,
            client_version: self.client_version.clone(),
            provider_plane: self.provider_plane.clone(),
            route_class: self.route_class.clone(),
            request_class: self.request_class.clone(),
            requested_model: self.requested_model.clone(),
            executable_model: self.executable_model.clone(),
            stream_flag: self.stream_flag,
            tools_declared_count: self.tools_declared_count,
            tool_classes: self.tool_classes,
            tool_choice_mode: self.tool_choice_mode,
            parallel_tools_requested: self.parallel_tools_requested,
            tool_results_in_input: self.tool_results_in_input,
            structured_output_flag: self.structured_output_flag,
            reasoning_flag: self.reasoning_flag,
            service_tier: self.service_tier.clone(),
            input_modalities: self.input_modalities,
            output_modalities: self.output_modalities,
            admitted_at: self.admitted_at,
        };
        admission.validate()?;
        self.terminal.validate(self.admitted_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGICAL: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const BILLING: &str = "bbbbbbbb-bbbb-4bbb-9bbb-bbbbbbbbbbbb";

    fn admission() -> RequestFactAdmission {
        RequestFactAdmission {
            logical_request_id: LOGICAL.into(),
            billing_request_id: BILLING.into(),
            execution_group_id: None,
            attempt: 1,
            account_id: "account_1".into(),
            key_id: "key_1".into(),
            client_kind: ClientKind::Unknown,
            client_source: ClientSource::Unknown,
            client_version: None,
            provider_plane: "anthropic".into(),
            route_class: "direct".into(),
            request_class: "messages".into(),
            requested_model: Some("claude-test".into()),
            executable_model: Some("claude-test".into()),
            stream_flag: true,
            tools_declared_count: None,
            tool_classes: None,
            tool_choice_mode: None,
            parallel_tools_requested: None,
            tool_results_in_input: None,
            structured_output_flag: None,
            reasoning_flag: None,
            service_tier: None,
            input_modalities: None,
            output_modalities: None,
            admitted_at: 10,
        }
    }

    fn terminal() -> RequestFactTerminalEvidence {
        RequestFactTerminalEvidence {
            terminal_at: 12,
            http_status_code: Some(200),
            provider_terminal_class: ProviderTerminalClass::Success,
            delivery_state: DeliveryState::Completed,
            downstream_disconnect: None,
            upstream_request_id: None,
            first_public_byte_at: Some(11),
            internal_attempt_count: None,
            failure_class: None,
            tool_calls_in_output: None,
        }
    }

    #[test]
    fn closed_vocabularies_roundtrip_and_reject_unknown_values() {
        for value in [
            ClientKind::ClaudeCode,
            ClientKind::OpenCode,
            ClientKind::CodexCli,
            ClientKind::Cursor,
            ClientKind::Sdk,
            ClientKind::Custom,
            ClientKind::Unknown,
        ] {
            assert_eq!(ClientKind::parse(value.as_str()).unwrap(), value);
        }
        for value in [
            ClientSource::Explicit,
            ClientSource::Heuristic,
            ClientSource::Unknown,
        ] {
            assert_eq!(ClientSource::parse(value.as_str()).unwrap(), value);
        }
        for value in [
            ToolChoiceMode::Auto,
            ToolChoiceMode::Required,
            ToolChoiceMode::None,
            ToolChoiceMode::Named,
            ToolChoiceMode::Unknown,
        ] {
            assert_eq!(ToolChoiceMode::parse(value.as_str()).unwrap(), value);
        }
        for value in [
            ProviderTerminalClass::Success,
            ProviderTerminalClass::ClientError,
            ProviderTerminalClass::Quota,
            ProviderTerminalClass::Auth,
            ProviderTerminalClass::Timeout,
            ProviderTerminalClass::Transport,
            ProviderTerminalClass::UpstreamError,
            ProviderTerminalClass::ProtocolError,
            ProviderTerminalClass::Unknown,
        ] {
            assert_eq!(ProviderTerminalClass::parse(value.as_str()).unwrap(), value);
        }
        for value in [
            DeliveryState::NotStarted,
            DeliveryState::Started,
            DeliveryState::Completed,
            DeliveryState::Interrupted,
            DeliveryState::Unknown,
        ] {
            assert_eq!(DeliveryState::parse(value.as_str()).unwrap(), value);
        }
        for value in [
            BillingOutcome::Winner,
            BillingOutcome::Loser,
            BillingOutcome::ZeroMetered,
            BillingOutcome::Canceled,
            BillingOutcome::Reconciled,
            BillingOutcome::NotApplicable,
            BillingOutcome::Unknown,
        ] {
            assert_eq!(BillingOutcome::parse(value.as_str()).unwrap(), value);
        }
        assert!(ClientKind::parse("raw-user-agent").is_err());
        assert!(ClientSource::parse("header").is_err());
        assert!(ToolChoiceMode::parse("freeform-json").is_err());
        assert!(ProviderTerminalClass::parse("provider-secret").is_err());
        assert!(DeliveryState::parse("maybe").is_err());
        assert!(BillingOutcome::parse("settled").is_err());
    }

    #[test]
    fn admission_validation_rejects_every_untrusted_shape() {
        let mut value = admission();
        value.logical_request_id = "not-a-uuid".into();
        assert!(value.validate().is_err());
        let mut value = admission();
        value.billing_request_id = LOGICAL.to_uppercase();
        assert!(value.validate().is_err());
        let mut value = admission();
        value.execution_group_id = Some("cccccccc-cccc-4ccc-7ccc-cccccccccccc".into());
        assert!(value.validate().is_err());
        let mut value = admission();
        value.attempt = 0;
        assert!(value.validate().is_err());
        let mut value = admission();
        value.account_id.clear();
        assert!(value.validate().is_err());
        let mut value = admission();
        value.key_id = "x".repeat(MAX_REQUEST_FACT_KEY_ID_LEN + 1);
        assert!(value.validate().is_err());
        let mut value = admission();
        value.client_version = Some("raw\nheader".into());
        assert!(value.validate().is_err());
        let mut value = admission();
        value.provider_plane = "☃".into();
        assert!(value.validate().is_err());
        let mut value = admission();
        value.requested_model = Some("x".repeat(MAX_REQUEST_FACT_MODEL_LEN + 1));
        assert!(value.validate().is_err());
        let mut value = admission();
        value.tools_declared_count = Some(-1);
        assert!(value.validate().is_err());
        let mut value = admission();
        value.tool_classes = Some(TOOL_CLASS_MASK | 128);
        assert!(value.validate().is_err());
        let mut value = admission();
        value.input_modalities = Some(MODALITY_MASK | 32);
        assert!(value.validate().is_err());
        let mut value = admission();
        value.output_modalities = Some(MODALITY_MASK | 32);
        assert!(value.validate().is_err());
        let mut value = admission();
        value.route_class.clear();
        assert!(value.validate().is_err());
        let mut value = admission();
        value.service_tier = Some(
            "tier
name"
                .into(),
        );
        assert!(value.validate().is_err());
        let mut value = admission();
        value.executable_model = Some("".into());
        assert!(value.validate().is_err());
        let mut value = admission();
        value.admitted_at = -1;
        assert!(value.validate().is_err());
        assert!(admission().validate().is_ok());
    }

    #[test]
    fn terminal_validation_rejects_status_counts_strings_and_time_inversions() {
        let mut value = terminal();
        value.http_status_code = Some(99);
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.internal_attempt_count = Some(-1);
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.upstream_request_id = Some("upstream\nid".into());
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.failure_class = Some("x".repeat(MAX_REQUEST_FACT_FAILURE_CLASS_LEN + 1));
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.terminal_at = 9;
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.first_public_byte_at = Some(13);
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.first_public_byte_at = Some(9);
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.upstream_request_id = Some("x".repeat(MAX_REQUEST_FACT_UPSTREAM_ID_LEN + 1));
        assert!(value.validate(10).is_err());
        let mut value = terminal();
        value.terminal_at = 10;
        assert!(value.validate_with_delivery(10, Some(11)).is_err());
        let mut value = terminal();
        value.first_public_byte_at = Some(10);
        assert!(value.validate_with_delivery(10, Some(11)).is_err());
        assert!(terminal().validate(-1).is_err());
        assert!(terminal().validate(10).is_ok());
    }

    #[test]
    fn terminal_fact_has_no_private_content_surface_and_checks_billing_identity() {
        let base = admission();
        let value = TerminalRequestFact {
            logical_request_id: base.logical_request_id,
            billing_request_id: Some(base.billing_request_id),
            execution_group_id: base.execution_group_id,
            attempt: base.attempt,
            account_id: base.account_id,
            key_id: base.key_id,
            client_kind: base.client_kind,
            client_source: base.client_source,
            client_version: base.client_version,
            provider_plane: base.provider_plane,
            route_class: base.route_class,
            request_class: base.request_class,
            requested_model: base.requested_model,
            executable_model: base.executable_model,
            stream_flag: base.stream_flag,
            tools_declared_count: base.tools_declared_count,
            tool_classes: base.tool_classes,
            tool_choice_mode: base.tool_choice_mode,
            parallel_tools_requested: base.parallel_tools_requested,
            tool_results_in_input: base.tool_results_in_input,
            structured_output_flag: base.structured_output_flag,
            reasoning_flag: base.reasoning_flag,
            service_tier: base.service_tier,
            input_modalities: base.input_modalities,
            output_modalities: base.output_modalities,
            admitted_at: base.admitted_at,
            terminal: terminal(),
        };
        assert!(value.validate().is_ok());
        let mut invalid = value;
        invalid.billing_request_id = Some("not-a-uuid".into());
        assert!(invalid.validate().is_err());
    }
}
