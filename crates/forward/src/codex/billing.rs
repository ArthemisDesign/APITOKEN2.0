//! Shared customer admission and exact API-equivalent settlement for Codex turns.

use super::openai_image_snapshot::{
    openai_image_quote, OpenAiImageOperation, OpenAiImageQuoteInput,
};
use super::{CodexModel, CodexUsage};
use crate::execution::{ClientAttribution, LogicalRequestId, RequestLifecycleClock};
use crate::metrics::Metrics;
use crate::pricing::{tariff_book, EnginePricingRequestId};
use crate::proxy::{authorize, Authz, HoldGuard};
use crate::request_classification::RequestClassification;
use crate::state::AppState;
use anyhow::Context as _;
use axum::http::HeaderMap;
use registry::request_facts::{
    DeliveryState, ProviderTerminalClass, RequestFactAdmission, RequestFactTerminalEvidence,
    TerminalRequestFact,
};
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Unauthorized,
    Unavailable,
    LowBalance,
}

type CodexReserveResult = (
    String,
    i64,
    i64,
    Option<i64>,
    Option<tariff_book::PinnedTariff>,
    Option<CodexBillableFactContext>,
);

struct Reservation {
    billing: std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: String,
    key: String,
    mult_bp: i64,
    hold: i64,
    tariff_priced_ts: Option<i64>,
    /// The hot tariff override version admission priced the hold with; settlement replays exactly
    /// this version. `None` is the compiled constants, byte-identical to before.
    pinned_tariff: Option<tariff_book::PinnedTariff>,
    policy_fast: Option<bool>,
    request_id: String,
    guard: HoldGuard,
    request_fact: Option<CodexBillableFactContext>,
}

/// Owns the exact billing reservation until a non-streaming response is returned or a streaming
/// upstream task fully finishes. Codex capacity is governed by its upstream subscription pool;
/// local global/per-key concurrency ceilings are intentionally not applied to this provider.
pub(crate) struct CodexAdmission {
    reservation: Option<Reservation>,
}

/// Closed, content-free route evidence created only after the owning generation parser accepts.
/// Its mutable members carry only a checked submission count and an explicit disconnect latch.
pub(crate) struct CodexBillableRequestSpec {
    route: CodexBillableRoute,
    requested_model: Option<String>,
    executable_model: Option<String>,
    stream_flag: bool,
    classification: RequestClassification,
    attempts: super::CodexAttemptObserver,
    downstream_disconnect: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
enum CodexBillableRoute {
    NativeResponses,
    NativeChat,
    UniversalMessages,
}

struct CodexBillableFactContext {
    admitted_at: i64,
    lifecycle_clock: RequestLifecycleClock,
    attempts: super::CodexAttemptObserver,
    downstream_disconnect: Arc<AtomicBool>,
}

impl fmt::Debug for CodexBillableRequestSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CodexBillableRequestSpec(<redacted>)")
    }
}

impl fmt::Debug for CodexBillableFactContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CodexBillableFactContext(<redacted>)")
    }
}

impl CodexBillableRequestSpec {
    pub(crate) fn native_responses(
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self {
            route: CodexBillableRoute::NativeResponses,
            requested_model,
            executable_model,
            stream_flag,
            classification,
            attempts: super::CodexAttemptObserver::default(),
            downstream_disconnect: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn native_chat(
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self {
            route: CodexBillableRoute::NativeChat,
            requested_model,
            executable_model,
            stream_flag,
            classification,
            attempts: super::CodexAttemptObserver::default(),
            downstream_disconnect: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn universal_messages(
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self {
            route: CodexBillableRoute::UniversalMessages,
            requested_model,
            executable_model,
            stream_flag,
            classification,
            attempts: super::CodexAttemptObserver::default(),
            downstream_disconnect: Arc::new(AtomicBool::new(false)),
        }
    }

    fn route_classes(&self) -> (&'static str, &'static str) {
        match self.route {
            CodexBillableRoute::NativeResponses => ("native", "responses"),
            CodexBillableRoute::NativeChat => ("native", "chat"),
            CodexBillableRoute::UniversalMessages => ("universal", "messages"),
        }
    }
}

pub(crate) struct OpenAiImageAdmission {
    reservation: Option<Reservation>,
}

pub(crate) struct PendingCodexAdmission {
    tenant_scope: String,
    authz: Authz,
    execution: registry::ExecutionAttempt,
}

/// Privacy-minimal, immutable attribution captured from one successful metered admission. This
/// deliberately carries the authoritative non-secret key identity but never the raw credential.
pub(crate) struct CodexRequestFactSeed {
    logical_request_id: String,
    client_attribution: ClientAttribution,
    execution: registry::ExecutionAttempt,
    account_id: String,
    key_id: String,
    admitted_at: i64,
    lifecycle_clock: RequestLifecycleClock,
}

/// Content-free route-specific evidence accepted by the owning parser. Keeping the vocabulary typed
/// prevents callers from manufacturing arbitrary provider, route, or request-class strings.
enum CodexRequestFactSpec {
    UniversalCountTokens {
        requested_model: Option<String>,
        executable_model: Option<String>,
    },
    NativeInputTokens {
        requested_model: Option<String>,
        executable_model: Option<String>,
        classification: Option<RequestClassification>,
    },
}

impl CodexRequestFactSeed {
    #[cfg(test)]
    pub(crate) fn for_test(
        logical_request_id: &str,
        client_attribution: ClientAttribution,
        execution: registry::ExecutionAttempt,
        account_id: &str,
        key_id: &str,
        admitted_at: i64,
        lifecycle_clock: RequestLifecycleClock,
    ) -> Self {
        Self {
            logical_request_id: logical_request_id.into(),
            client_attribution,
            execution,
            account_id: account_id.into(),
            key_id: key_id.into(),
            admitted_at,
            lifecycle_clock,
        }
    }

    fn into_billable_admission(
        self,
        billing_request_id: String,
        spec: &CodexBillableRequestSpec,
    ) -> (RequestFactAdmission, CodexBillableFactContext) {
        let (route_class, request_class) = spec.route_classes();
        let admission = RequestFactAdmission {
            logical_request_id: self.logical_request_id,
            billing_request_id,
            execution_group_id: self.execution.group_id().map(str::to_owned),
            attempt: self.execution.attempt(),
            account_id: self.account_id,
            key_id: self.key_id,
            client_kind: self.client_attribution.kind(),
            client_source: self.client_attribution.source(),
            client_version: self.client_attribution.version().map(str::to_owned),
            provider_plane: "openai".into(),
            route_class: route_class.into(),
            request_class: request_class.into(),
            requested_model: spec.requested_model.clone(),
            executable_model: spec.executable_model.clone(),
            stream_flag: spec.stream_flag,
            tools_declared_count: spec.classification.tools_declared_count(),
            tool_classes: spec.classification.tool_classes(),
            tool_choice_mode: spec.classification.tool_choice_mode(),
            parallel_tools_requested: spec.classification.parallel_tools_requested(),
            tool_results_in_input: spec.classification.tool_results_in_input(),
            structured_output_flag: spec.classification.structured_output_flag(),
            reasoning_flag: spec.classification.reasoning_flag(),
            service_tier: spec.classification.service_tier().map(str::to_owned),
            input_modalities: spec.classification.input_modalities(),
            output_modalities: spec.classification.output_modalities(),
            admitted_at: self.admitted_at,
        };
        let context = CodexBillableFactContext {
            admitted_at: self.admitted_at,
            lifecycle_clock: self.lifecycle_clock,
            attempts: spec.attempts.clone(),
            downstream_disconnect: spec.downstream_disconnect.clone(),
        };
        (admission, context)
    }

    pub(crate) fn terminal_fact(
        self,
        status: axum::http::StatusCode,
        requested_model: Option<String>,
        executable_model: Option<String>,
    ) -> TerminalRequestFact {
        self.terminal_fact_for(
            status,
            CodexRequestFactSpec::UniversalCountTokens {
                requested_model,
                executable_model,
            },
        )
    }

    pub(crate) fn terminal_input_tokens_fact(
        self,
        status: axum::http::StatusCode,
        requested_model: Option<String>,
        executable_model: Option<String>,
        classification: Option<RequestClassification>,
    ) -> TerminalRequestFact {
        self.terminal_fact_for(
            status,
            CodexRequestFactSpec::NativeInputTokens {
                requested_model,
                executable_model,
                classification,
            },
        )
    }

    fn terminal_fact_for(
        self,
        status: axum::http::StatusCode,
        spec: CodexRequestFactSpec,
    ) -> TerminalRequestFact {
        let provider_terminal_class = match status.as_u16() {
            200..=299 => ProviderTerminalClass::Success,
            401 | 403 => ProviderTerminalClass::Auth,
            429 => ProviderTerminalClass::Quota,
            400..=499 => ProviderTerminalClass::ClientError,
            _ => ProviderTerminalClass::Unknown,
        };
        let delivery_state = if status.is_success() {
            DeliveryState::Completed
        } else {
            DeliveryState::NotStarted
        };
        let terminal_at = pool::now().max(self.admitted_at);
        let first_public_byte_at = self
            .lifecycle_clock
            .seal_first_public_byte_for_terminal(self.admitted_at, terminal_at);
        let (route_class, request_class, requested_model, executable_model, classification) =
            match spec {
                CodexRequestFactSpec::UniversalCountTokens {
                    requested_model,
                    executable_model,
                } => (
                    "universal",
                    "count_tokens",
                    requested_model,
                    executable_model,
                    None,
                ),
                CodexRequestFactSpec::NativeInputTokens {
                    requested_model,
                    executable_model,
                    classification,
                } => (
                    "native",
                    "input_tokens",
                    requested_model,
                    executable_model,
                    classification,
                ),
            };
        let tools_declared_count = classification
            .as_ref()
            .and_then(RequestClassification::tools_declared_count);
        let tool_classes = classification
            .as_ref()
            .and_then(RequestClassification::tool_classes);
        let tool_choice_mode = classification
            .as_ref()
            .and_then(RequestClassification::tool_choice_mode);
        let parallel_tools_requested = classification
            .as_ref()
            .and_then(RequestClassification::parallel_tools_requested);
        let tool_results_in_input = classification
            .as_ref()
            .and_then(RequestClassification::tool_results_in_input);
        let structured_output_flag = classification
            .as_ref()
            .and_then(RequestClassification::structured_output_flag);
        let reasoning_flag = classification
            .as_ref()
            .and_then(RequestClassification::reasoning_flag);
        let service_tier = classification
            .as_ref()
            .and_then(RequestClassification::service_tier)
            .map(str::to_owned);
        let input_modalities = classification
            .as_ref()
            .and_then(RequestClassification::input_modalities);
        let output_modalities = classification
            .as_ref()
            .and_then(RequestClassification::output_modalities);
        TerminalRequestFact {
            logical_request_id: self.logical_request_id,
            billing_request_id: None,
            execution_group_id: self.execution.group_id().map(str::to_owned),
            attempt: self.execution.attempt(),
            account_id: self.account_id,
            key_id: self.key_id,
            client_kind: self.client_attribution.kind(),
            client_source: self.client_attribution.source(),
            client_version: self.client_attribution.version().map(str::to_owned),
            provider_plane: "openai".into(),
            route_class: route_class.into(),
            request_class: request_class.into(),
            requested_model,
            executable_model,
            stream_flag: false,
            tools_declared_count,
            tool_classes,
            tool_choice_mode,
            parallel_tools_requested,
            tool_results_in_input,
            structured_output_flag,
            reasoning_flag,
            service_tier,
            input_modalities,
            output_modalities,
            admitted_at: self.admitted_at,
            terminal: RequestFactTerminalEvidence {
                terminal_at,
                http_status_code: Some(i32::from(status.as_u16())),
                provider_terminal_class,
                delivery_state,
                downstream_disconnect: None,
                upstream_request_id: None,
                first_public_byte_at,
                internal_attempt_count: Some(0),
                failure_class: None,
                tool_calls_in_output: None,
            },
        }
    }
}

impl PendingCodexAdmission {
    pub(crate) fn tenant_scope(&self) -> &str {
        &self.tenant_scope
    }

    /// Snapshot fact attribution without widening `Authz` or exposing the raw key to a protocol
    /// adapter. Missing typed logical or lifecycle context is an instrumentation gap and therefore
    /// omits the fact.
    pub(crate) fn request_fact_seed(
        &self,
        logical_request_id: Option<&LogicalRequestId>,
        client_attribution: Option<&ClientAttribution>,
        lifecycle_clock: Option<&RequestLifecycleClock>,
        admitted_at: i64,
    ) -> Option<CodexRequestFactSeed> {
        let logical_request_id = logical_request_id?;
        let lifecycle_clock = lifecycle_clock?;
        let Authz::Metered {
            account_id, key_id, ..
        } = &self.authz
        else {
            return None;
        };
        Some(CodexRequestFactSeed {
            logical_request_id: logical_request_id.as_str().to_owned(),
            client_attribution: client_attribution
                .cloned()
                .unwrap_or_else(ClientAttribution::unknown_for_internal_use),
            execution: self.execution.clone(),
            account_id: account_id.clone(),
            key_id: key_id.clone(),
            admitted_at,
            lifecycle_clock: lifecycle_clock.clone(),
        })
    }

    pub(crate) async fn reserve(
        self,
        app: &AppState,
        model: &CodexModel,
        estimated_input_tokens: u64,
        requested_output_tokens: Option<u64>,
        reserve_overhead_tokens: u64,
        fast: bool,
        billable_fact: Option<(CodexRequestFactSeed, CodexBillableRequestSpec)>,
    ) -> Result<CodexAdmission, AdmissionError> {
        let reservation = match (&self.authz, &app.billing) {
            (
                Authz::Metered {
                    account_id,
                    key,
                    available_nano,
                    ..
                },
                Some(billing),
            ) => {
                // SQLite intentionally preserves the legacy money path and never admits an
                // analytics fact. PostgreSQL is identified by its owned command metrics carrier.
                let billable_fact = billing
                    .pg_command_stats()
                    .is_some()
                    .then_some(billable_fact)
                    .flatten();
                let (
                    request_id,
                    hold,
                    reservation_mult_bp,
                    tariff_priced_ts,
                    pinned_tariff,
                    request_fact,
                ) = reserve_codex_metered(
                    billing,
                    account_id,
                    key,
                    model,
                    estimated_input_tokens,
                    requested_output_tokens,
                    reserve_overhead_tokens,
                    fast,
                    self.authz.mult_for(registry::PROVIDER_OPENAI),
                    *available_nano,
                    &self.execution,
                    billable_fact,
                )
                .await?;
                Some(Reservation {
                    billing: billing.clone(),
                    account_id: account_id.clone(),
                    key: key.clone(),
                    mult_bp: reservation_mult_bp,
                    hold,
                    tariff_priced_ts,
                    pinned_tariff,
                    policy_fast: tariff_priced_ts.map(|_| fast),
                    request_id: request_id.clone(),
                    request_fact,
                    guard: HoldGuard::new(
                        Some(billing.clone()),
                        account_id.clone(),
                        key.clone(),
                        hold,
                        request_id,
                    ),
                })
            }
            _ => None,
        };
        Ok(CodexAdmission { reservation })
    }

    pub(crate) async fn reserve_image(
        self,
        app: &AppState,
        requested_model_id: &str,
        operation: OpenAiImageOperation,
    ) -> Result<OpenAiImageAdmission, AdmissionError> {
        let reservation = match (&self.authz, &app.billing) {
            (
                Authz::Metered {
                    account_id,
                    key,
                    available_nano,
                    ..
                },
                Some(billing),
            ) => Some(
                reserve_openai_image_metered(
                    billing,
                    account_id,
                    key,
                    requested_model_id,
                    operation,
                    self.authz.mult_for(registry::PROVIDER_OPENAI),
                    *available_nano,
                    &self.execution,
                )
                .await?,
            ),
            _ => None,
        };
        Ok(OpenAiImageAdmission { reservation })
    }
}

#[allow(clippy::too_many_arguments)]
async fn reserve_openai_image_metered(
    billing: &std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: &str,
    key: &str,
    requested_model_id: &str,
    operation: OpenAiImageOperation,
    mult_bp: i64,
    available_nano: i64,
    execution: &registry::ExecutionAttempt,
) -> Result<Reservation, AdmissionError> {
    let request_id = crate::upstream::fresh_request_id();
    let typed_request_id = EnginePricingRequestId::from_engine_uuid_v4(&request_id)
        .ok_or(AdmissionError::Unavailable)?;
    if mult_bp > 0 && available_nano <= 0 {
        return Err(AdmissionError::LowBalance);
    }
    let quote = openai_image_quote(OpenAiImageQuoteInput {
        request_id: typed_request_id,
        account_id: account_id.to_owned(),
        requested_model_id: requested_model_id.to_owned(),
        quote_ts: pool::now(),
        payable_multiplier_bp: mult_bp,
        operation,
        available_nano,
    })
    .map_err(|error| {
        elog::error(
            "codex-billing",
            format!("OpenAI image quote failed: {error:#}"),
        );
        AdmissionError::Unavailable
    })?
    .ok_or(AdmissionError::LowBalance)?;
    let pinned_tariff = quote.pinned_tariff();
    let snapshot = quote.into_snapshot();
    let hold = snapshot.charged_hold_nano();
    let payable_multiplier_bp = snapshot.payable_multiplier_bp();
    match billing
        .reserve_priced_request_for_execution(
            &request_id,
            account_id,
            key,
            hold,
            execution.clone(),
            registry::PROVIDER_OPENAI,
            payable_multiplier_bp,
        )
        .await
    {
        Ok(Some(_)) => Ok(image_reservation(
            billing,
            account_id,
            key,
            request_id,
            hold,
            payable_multiplier_bp,
            None,
            pinned_tariff,
        )),
        Ok(None) => Err(AdmissionError::LowBalance),
        Err(error) => {
            elog::error(
                "codex-billing",
                format!("OpenAI image billing reservation failed: {error:#}"),
            );
            Err(AdmissionError::Unavailable)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn image_reservation(
    billing: &std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: &str,
    key: &str,
    request_id: String,
    hold: i64,
    mult_bp: i64,
    tariff_priced_ts: Option<i64>,
    pinned_tariff: Option<tariff_book::PinnedTariff>,
) -> Reservation {
    Reservation {
        billing: billing.clone(),
        account_id: account_id.to_owned(),
        key: key.to_owned(),
        mult_bp,
        hold,
        tariff_priced_ts,
        pinned_tariff,
        policy_fast: None,
        request_fact: None,
        guard: HoldGuard::new(
            Some(billing.clone()),
            account_id.to_owned(),
            key.to_owned(),
            hold,
            request_id.clone(),
        ),
        request_id,
    }
}

#[allow(clippy::too_many_arguments)]
async fn reserve_codex_metered(
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    reserve_overhead_tokens: u64,
    fast: bool,
    mult_bp: i64,
    available_nano: i64,
    execution: &registry::ExecutionAttempt,
    billable_fact: Option<(CodexRequestFactSeed, CodexBillableRequestSpec)>,
) -> Result<CodexReserveResult, AdmissionError> {
    let request_id = crate::upstream::fresh_request_id();
    if mult_bp > 0 && available_nano <= 0 {
        return Err(AdmissionError::LowBalance);
    }
    reserve_codex_legacy(
        billing,
        account_id,
        key,
        model,
        estimated_input_tokens,
        requested_output_tokens,
        reserve_overhead_tokens,
        fast,
        mult_bp,
        available_nano,
        &request_id,
        execution,
        billable_fact,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn reserve_codex_legacy(
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    reserve_overhead_tokens: u64,
    fast: bool,
    mult_bp: i64,
    available_nano: i64,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
    billable_fact: Option<(CodexRequestFactSeed, CodexBillableRequestSpec)>,
) -> Result<CodexReserveResult, AdmissionError> {
    let estimated = estimated_input_tokens.saturating_add(reserve_overhead_tokens);
    let now = pool::now();
    // Hot tariff override: the matched catalog family resolves against the process-wide book; an
    // override replaces only the base vector (long-context/Fast modifiers stay code-applied on
    // top) and pins `<family>/v<version>` for settlement.
    let compiled = effective_prices(model, now);
    let resolved = match metering::codex_matched_tariff_at(&model.id, now) {
        Some((family, _)) => tariff_book::reserve_base(
            &tariff_book::snapshot(),
            family,
            now,
            compiled,
            tariff_book::as_codex,
        ),
        None => tariff_book::ReserveBase {
            prices: compiled,
            pin: None,
        },
    };
    let base = reserve_cost_with_prices(
        model,
        resolved.prices,
        estimated,
        requested_output_tokens,
        fast,
    );
    let hold = if mult_bp <= 0 {
        0
    } else {
        metering::apply_multiplier(base, mult_bp).clamp(1, i64::MAX as i128) as i64
    };
    // Preserve the scalar admission contract exactly: a conservative full-output estimate is
    // capped to the account balance. Exact settlement retains the full charge; registry caps only
    // collection at the shared account floor and records any remainder as uncollected.
    let hold = hold.min(available_nano.max(1));
    let (request_fact_admission, request_fact_context) = match billable_fact {
        Some((seed, spec)) => {
            let (admission, context) = seed.into_billable_admission(request_id.to_owned(), &spec);
            (Some(admission), Some(context))
        }
        None => (None, None),
    };
    let reserved = match request_fact_admission {
        Some(admission) => {
            billing
                .reserve_priced_request_for_execution_with_fact(
                    request_id,
                    account_id,
                    key,
                    hold,
                    execution.clone(),
                    registry::PROVIDER_OPENAI,
                    mult_bp,
                    admission,
                )
                .await
        }
        None => {
            billing
                .reserve_priced_request_for_execution(
                    request_id,
                    account_id,
                    key,
                    hold,
                    execution.clone(),
                    registry::PROVIDER_OPENAI,
                    mult_bp,
                )
                .await
        }
    };
    match reserved {
        Ok(Some(_)) => Ok((
            request_id.to_owned(),
            hold,
            mult_bp,
            None,
            resolved.pin,
            request_fact_context,
        )),
        Ok(None) => Err(AdmissionError::LowBalance),
        Err(error) => {
            elog::error(
                "codex-billing",
                format!("Codex billing reservation failed: {error:#}"),
            );
            Err(AdmissionError::Unavailable)
        }
    }
}

impl OpenAiImageAdmission {
    /// Return the engine-owned money identity for a metered image request. Public success responses
    /// expose this value as `x-request-id`, so an operator can correlate the exact reservation,
    /// release snapshot, usage event, and terminal settlement without relying on upstream metadata.
    pub(crate) fn request_id(&self) -> Option<&str> {
        self.reservation
            .as_ref()
            .map(|reservation| reservation.request_id.as_str())
    }

    pub(crate) async fn mark_delivering(&mut self) -> Result<(), AdmissionError> {
        let Some(reservation) = &mut self.reservation else {
            return Ok(());
        };
        match reservation
            .billing
            .mark_delivering(&reservation.request_id, 3_600)
            .await
        {
            Ok(true) => {
                // From this point a cancellation or transport loss is ambiguous: normal durable
                // recovery must charge the full hold instead of HoldGuard refunding an executed turn.
                reservation.guard.disarm();
                Ok(())
            }
            Ok(false) | Err(_) => Err(AdmissionError::Unavailable),
        }
    }

    pub(crate) fn settle(mut self, model_id: &str, usage: &metering::OpenAiImageUsage) {
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let priced_ts = reservation.tariff_priced_ts.unwrap_or_else(pool::now);
        let (charge, usage_event) = match metering::openai_image_tariff(model_id) {
            // No tariff means the turn was never measured, and an unmeasured turn is not billed
            // at the admission ceiling.
            Err(_) => (
                crate::settlement_policy::unknown_usage_charge(reservation.hold),
                None,
            ),
            Ok(tariff) => {
                // Hot tariff override: replay the exact version pinned at admission; the single
                // image family means a served id outside it is already rejected above. A pinned
                // version missing from the book is an integrity error: leave the reservation to
                // durable recovery, never reprice at compiled.
                let prices = match tariff_book::charge_base(
                    &tariff_book::snapshot(),
                    reservation.pinned_tariff.as_ref(),
                    Some(metering::openai_image_tariff_family()),
                    priced_ts,
                    tariff.prices,
                    tariff_book::as_openai_image,
                ) {
                    tariff_book::ChargeBase::Compiled(prices)
                    | tariff_book::ChargeBase::Override(prices, _) => prices,
                    tariff_book::ChargeBase::MissingPinned => {
                        elog::error(
                            "codex-billing",
                            format!(
                                "pinned tariff {} is absent from the override book; settlement left to recovery",
                                reservation
                                    .pinned_tariff
                                    .as_ref()
                                    .map(|pin| pin.schedule_id.as_str())
                                    .unwrap_or("<unknown>")
                            ),
                        );
                        reservation.guard.disarm();
                        return;
                    }
                };
                settled_openai_image_charge_with_prices(
                    model_id,
                    usage,
                    reservation.hold,
                    reservation.mult_bp,
                    priced_ts,
                    prices,
                )
            }
        };
        reservation.billing.settle_detached(
            &reservation.request_id,
            &reservation.account_id,
            &reservation.key,
            reservation.hold,
            charge,
            None,
            usage_event,
        );
        reservation.guard.disarm();
        if charge > 0 {
            elog::info(
                "codex-billing",
                format!(
                    "OpenAI image request charged {} [{}]",
                    metering::nano_to_usd_string(charge as i128),
                    model_id
                ),
            );
        }
    }

    /// A successful provider turn with malformed terminal usage cannot be refunded: execution is
    /// already authoritative. Leave the reservation for normal full-hold recovery rather than
    /// inventing token counts or making an executed image free.
    pub(crate) fn retain_full_hold(mut self) {
        if let Some(mut reservation) = self.reservation.take() {
            reservation.guard.disarm();
        }
    }
}

#[cfg(test)]
fn settled_openai_image_charge(
    model_id: &str,
    usage: &metering::OpenAiImageUsage,
    hold: i64,
    mult_bp: i64,
    priced_ts: i64,
) -> (i64, Option<registry::UsageEventInput>) {
    // No tariff and no computable cost both mean the turn was never measured, and an unmeasured
    // turn is not billed at the admission ceiling.
    let Ok(tariff) = metering::openai_image_tariff(model_id) else {
        return (crate::settlement_policy::unknown_usage_charge(hold), None);
    };
    settled_openai_image_charge_with_prices(
        model_id,
        usage,
        hold,
        mult_bp,
        priced_ts,
        tariff.prices,
    )
}

/// `settled_openai_image_charge` under one explicit rate card: settlement replays the exact
/// override version admission pinned, or the compiled tariff when no override applies.
#[allow(clippy::too_many_arguments)]
fn settled_openai_image_charge_with_prices(
    model_id: &str,
    usage: &metering::OpenAiImageUsage,
    hold: i64,
    mult_bp: i64,
    priced_ts: i64,
    prices: metering::OpenAiImagePrices,
) -> (i64, Option<registry::UsageEventInput>) {
    let Ok(real_nano) = metering::openai_image_cost_nanodollars(usage, &prices) else {
        return (crate::settlement_policy::unknown_usage_charge(hold), None);
    };
    let computed_charge = metering::apply_multiplier(real_nano, mult_bp);
    let charge = computed_charge.clamp(0, i64::MAX as i128) as i64;
    let fresh_text = usage
        .total_text_input_tokens
        .saturating_sub(usage.cached_text_input_tokens);
    let fresh_image = usage
        .total_image_input_tokens
        .saturating_sub(usage.cached_image_input_tokens);
    let input_nano = i128::from(fresh_text) * prices.fresh_text_input
        + i128::from(fresh_image) * prices.fresh_image_input;
    let cache_read_nano = i128::from(usage.cached_text_input_tokens) * prices.cached_text_input
        + i128::from(usage.cached_image_input_tokens) * prices.cached_image_input;
    let output_nano = i128::from(usage.image_output_tokens) * prices.image_output;
    let input_tokens = usage
        .total_text_input_tokens
        .saturating_add(usage.total_image_input_tokens);
    let cache_read_tokens = usage
        .cached_text_input_tokens
        .saturating_add(usage.cached_image_input_tokens);
    let usage_event = (real_nano > 0).then(|| registry::UsageEventInput {
        model: model_id.to_owned(),
        provider: registry::PROVIDER_OPENAI.to_owned(),
        input_tokens: input_tokens.min(i64::MAX as u64) as i64,
        output_tokens: usage.image_output_tokens.min(i64::MAX as u64) as i64,
        cache_read_tokens: cache_read_tokens.min(i64::MAX as u64) as i64,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        web_search_requests: 0,
        real_nano: real_nano.min(i64::MAX as i128) as i64,
        charge_basis_nano: real_nano.min(i64::MAX as i128) as i64,
        speed: "standard".to_owned(),
        inference_geo: String::new(),
        input_nano: input_nano.min(i64::MAX as i128) as i64,
        output_nano: output_nano.min(i64::MAX as i128) as i64,
        cache_read_nano: cache_read_nano.min(i64::MAX as i128) as i64,
        cache_write_5m_nano: 0,
        cache_write_1h_nano: 0,
        web_search_nano: 0,
        priced_ts,
    });
    (charge, usage_event)
}

fn codex_tool_calls_in_output(result: &super::CodexTurnResult) -> Option<bool> {
    let mut saw_tool_call = false;
    for item in &result.output {
        match item.get("type").and_then(serde_json::Value::as_str) {
            Some("function_call" | "custom_tool_call") => saw_tool_call = true,
            Some("message" | "reasoning") => {}
            // `CodexTurnResult` is provider-parsed output, so an unreviewed future item type makes
            // the existential false non-exhaustive rather than silently treating it as non-tool.
            _ => return None,
        }
    }
    Some(saw_tool_call)
}

fn codex_process_error_terminal(
    error: &super::ProcessError,
) -> (ProviderTerminalClass, DeliveryState) {
    match error {
        super::ProcessError::BadRequest | super::ProcessError::ContextWindowExceeded => {
            (ProviderTerminalClass::ClientError, DeliveryState::Unknown)
        }
        super::ProcessError::UsageLimitExceeded { .. } => {
            (ProviderTerminalClass::Quota, DeliveryState::Unknown)
        }
        super::ProcessError::AuthenticationRequired | super::ProcessError::SubscriptionRequired => {
            (ProviderTerminalClass::Auth, DeliveryState::Unknown)
        }
        super::ProcessError::Timeout(_) => {
            (ProviderTerminalClass::Timeout, DeliveryState::Interrupted)
        }
        super::ProcessError::Closed => {
            (ProviderTerminalClass::Transport, DeliveryState::Interrupted)
        }
        super::ProcessError::Protocol(_) => (
            ProviderTerminalClass::ProtocolError,
            DeliveryState::Interrupted,
        ),
        super::ProcessError::ExternalFallbackFailed { .. } => {
            (ProviderTerminalClass::Unknown, DeliveryState::Unknown)
        }
        super::ProcessError::Disabled | super::ProcessError::InvalidConfig(_) => {
            (ProviderTerminalClass::Unknown, DeliveryState::NotStarted)
        }
    }
}

impl CodexBillableFactContext {
    fn terminal_evidence(
        &self,
        http_status_code: Option<i32>,
        provider_terminal_class: ProviderTerminalClass,
        delivery_state: DeliveryState,
        downstream_disconnect: Option<bool>,
        attempts_exhaustive: bool,
        tool_calls_in_output: Option<bool>,
    ) -> RequestFactTerminalEvidence {
        let terminal_at = pool::now().max(self.admitted_at);
        RequestFactTerminalEvidence {
            terminal_at,
            http_status_code,
            provider_terminal_class,
            delivery_state,
            downstream_disconnect: downstream_disconnect.filter(|observed| *observed).or_else(
                || {
                    self.downstream_disconnect
                        .load(Ordering::Acquire)
                        .then_some(true)
                },
            ),
            upstream_request_id: None,
            first_public_byte_at: self
                .lifecycle_clock
                .seal_first_public_byte_for_terminal(self.admitted_at, terminal_at),
            internal_attempt_count: attempts_exhaustive
                .then(|| self.attempts.exhaustive_i32())
                .flatten(),
            failure_class: None,
            tool_calls_in_output,
        }
    }
}

#[cfg(test)]
static FAIL_CODEX_DELIVERY_MARKER_FOR_TEST: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn fail_next_codex_delivery_marker_for_test() {
    FAIL_CODEX_DELIVERY_MARKER_FOR_TEST.store(true, Ordering::Release);
}

impl CodexAdmission {
    pub(crate) fn attempt_observer(&self) -> Option<super::CodexAttemptObserver> {
        self.reservation
            .as_ref()
            .and_then(|reservation| reservation.request_fact.as_ref())
            .map(|context| context.attempts.clone())
    }

    pub(crate) fn record_downstream_disconnect(&self) {
        if let Some(context) = self
            .reservation
            .as_ref()
            .and_then(|reservation| reservation.request_fact.as_ref())
        {
            context.downstream_disconnect.store(true, Ordering::Release);
        }
    }

    /// Record only an explicit receiver closure. A frame-send timeout is backpressure evidence,
    /// not proof that the downstream disconnected, and must therefore remain `NULL` in the fact.
    pub(crate) fn record_downstream_disconnect_if_closed<T>(
        &self,
        sender: &tokio::sync::mpsc::Sender<T>,
    ) {
        if sender.is_closed() {
            self.record_downstream_disconnect();
        }
    }

    pub(crate) async fn mark_delivering(&self) -> Result<(), AdmissionError> {
        #[cfg(test)]
        if FAIL_CODEX_DELIVERY_MARKER_FOR_TEST.swap(false, Ordering::AcqRel) {
            return Err(AdmissionError::Unavailable);
        }
        let Some(reservation) = &self.reservation else {
            return Ok(());
        };
        let result = if reservation.request_fact.is_some() {
            reservation
                .billing
                .mark_delivering_with_request_fact(&reservation.request_id, 3_600)
                .await
        } else {
            reservation
                .billing
                .mark_delivering(&reservation.request_id, 3_600)
                .await
        };
        match result {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(AdmissionError::Unavailable),
        }
    }

    pub(crate) fn settle_error(mut self, error: &super::ProcessError) {
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let Some(context) = reservation.request_fact.take() else {
            return;
        };
        let (provider_class, delivery_state) = codex_process_error_terminal(error);
        let evidence =
            context.terminal_evidence(None, provider_class, delivery_state, None, true, None);
        submit_fact_aware_refund(&mut reservation, evidence, "error");
    }

    /// Terminalize a panicked/cancelled runner task without pretending that its JoinError came
    /// from the provider. The shared observer remains exhaustive because the joined task is over;
    /// provider status, delivery and tool output stay unknown, while an already observed receiver
    /// closure is retained by `terminal_evidence`.
    pub(crate) fn settle_join_error(mut self) {
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let Some(context) = reservation.request_fact.take() else {
            return;
        };
        let evidence = context.terminal_evidence(
            None,
            ProviderTerminalClass::Unknown,
            DeliveryState::Unknown,
            None,
            true,
            None,
        );
        submit_fact_aware_refund(&mut reservation, evidence, "join-error");
    }

    pub(crate) fn settle(
        self,
        model: &CodexModel,
        result: &super::CodexTurnResult,
        requested_output_tokens: Option<u64>,
        fast: bool,
        downstream_disconnect: Option<bool>,
    ) {
        self.settle_success(
            model,
            result,
            requested_output_tokens,
            fast,
            Some(200),
            DeliveryState::Completed,
            downstream_disconnect,
        );
    }

    /// A completed provider turn remains authoritative even if the durable delivery marker fails.
    /// Settle exact usage once, but report the actual conservative 503 and unknown delivery state;
    /// no success response was handed to the caller and no delivery fact is invented.
    pub(crate) fn settle_after_delivery_marker_failure(
        self,
        model: &CodexModel,
        result: &super::CodexTurnResult,
        requested_output_tokens: Option<u64>,
        fast: bool,
    ) {
        self.settle_success(
            model,
            result,
            requested_output_tokens,
            fast,
            Some(503),
            DeliveryState::Unknown,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn settle_success(
        mut self,
        model: &CodexModel,
        result: &super::CodexTurnResult,
        requested_output_tokens: Option<u64>,
        fast: bool,
        http_status_code: Option<i32>,
        delivery_state: DeliveryState,
        downstream_disconnect: Option<bool>,
    ) {
        let usage = &result.usage;
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let priced_ts = reservation.tariff_priced_ts.unwrap_or_else(pool::now);
        let effective_fast = reservation.policy_fast.unwrap_or(fast);
        // Hot tariff override: replay the exact version pinned at admission; a cross-family serve
        // reprices by the served model's family at the pinned priced timestamp; an empty book is
        // byte-identical to the compiled constants. A pinned version missing from the book is an
        // integrity error — the reservation is left to durable recovery, never repriced at
        // compiled. This sync settle cannot await the bounded refresh retry the async planes
        // perform; the miss is structurally unreachable because the pinning reserve read the row
        // from this process.
        let compiled = effective_prices(model, priced_ts);
        let served_family =
            metering::codex_matched_tariff_at(&model.id, priced_ts).map(|(family, _)| family);
        let prices = match tariff_book::charge_base(
            &tariff_book::snapshot(),
            reservation.pinned_tariff.as_ref(),
            served_family,
            priced_ts,
            compiled,
            tariff_book::as_codex,
        ) {
            tariff_book::ChargeBase::Compiled(prices)
            | tariff_book::ChargeBase::Override(prices, _) => prices,
            tariff_book::ChargeBase::MissingPinned => {
                elog::error(
                    "codex-billing",
                    format!(
                        "pinned tariff {} is absent from the override book; settlement left to recovery",
                        reservation
                            .pinned_tariff
                            .as_ref()
                            .map(|pin| pin.schedule_id.as_str())
                            .unwrap_or("<unknown>")
                    ),
                );
                reservation.guard.disarm();
                return;
            }
        };
        let (charge, usage_event) = settled_charge_with_prices(
            model,
            usage,
            reservation.hold,
            reservation.mult_bp,
            requested_output_tokens,
            priced_ts,
            effective_fast,
            prices,
        );
        let settlement = match reservation.request_fact.take() {
            Some(context) => {
                let evidence = context.terminal_evidence(
                    http_status_code,
                    ProviderTerminalClass::Success,
                    delivery_state,
                    downstream_disconnect,
                    true,
                    codex_tool_calls_in_output(result),
                );
                reservation.billing.settle_detached_with_request_fact(
                    &reservation.request_id,
                    &reservation.account_id,
                    &reservation.key,
                    reservation.hold,
                    charge,
                    None,
                    usage_event,
                    evidence,
                )
            }
            None => {
                reservation.billing.settle_detached(
                    &reservation.request_id,
                    &reservation.account_id,
                    &reservation.key,
                    reservation.hold,
                    charge,
                    None,
                    usage_event,
                );
                Ok(())
            }
        };
        if let Err(error) = settlement {
            elog::error(
                "codex-billing",
                format!("Codex request-fact settlement rejected: {error:#}"),
            );
        }
        reservation.guard.disarm();
        if charge > 0 {
            elog::info(
                "codex-billing",
                format!(
                    "OpenAI-compatible request: −{} [{}]",
                    metering::nano_to_usd_string(charge as i128),
                    model.id
                ),
            );
        }
    }
}

fn submit_fact_aware_refund(
    reservation: &mut Reservation,
    evidence: RequestFactTerminalEvidence,
    reason: &'static str,
) {
    if let Err(error) = reservation.billing.settle_detached_with_request_fact(
        &reservation.request_id,
        &reservation.account_id,
        &reservation.key,
        reservation.hold,
        0,
        None,
        None,
        evidence,
    ) {
        elog::error(
            "codex-billing",
            format!("Codex request-fact {reason} settlement rejected: {error:#}"),
        );
    }
    reservation.guard.disarm();
}

impl Drop for CodexAdmission {
    fn drop(&mut self) {
        let Some(reservation) = self.reservation.as_mut() else {
            return;
        };
        let Some(context) = reservation.request_fact.take() else {
            return;
        };
        let evidence = context.terminal_evidence(
            None,
            ProviderTerminalClass::Unknown,
            DeliveryState::Unknown,
            None,
            false,
            None,
        );
        if let Err(error) = reservation.billing.settle_detached_with_request_fact(
            &reservation.request_id,
            &reservation.account_id,
            &reservation.key,
            reservation.hold,
            0,
            None,
            None,
            evidence,
        ) {
            elog::error(
                "codex-billing",
                format!("Codex request-fact cancellation evidence rejected: {error:#}"),
            );
        }
        reservation.guard.disarm();
    }
}

/// Compute the exact customer debit and immutable provider-usage record before handing either to
/// the asynchronous billing actor. Keeping this boundary pure makes the only Codex money mutation
/// exhaustively testable without substituting a second pricing implementation in the test.
#[cfg(test)]
fn settled_charge(
    model: &CodexModel,
    usage: &CodexUsage,
    hold: i64,
    mult_bp: i64,
    requested_output_tokens: Option<u64>,
    now: i64,
    fast: bool,
) -> (i64, Option<registry::UsageEventInput>) {
    settled_charge_with_prices(
        model,
        usage,
        hold,
        mult_bp,
        requested_output_tokens,
        now,
        fast,
        effective_prices(model, now),
    )
}

/// `settled_charge` under one explicit rate card: settlement replays the exact override version
/// admission pinned, or the compiled vector when no override applies.
#[allow(clippy::too_many_arguments)]
fn settled_charge_with_prices(
    model: &CodexModel,
    usage: &CodexUsage,
    _hold: i64,
    mult_bp: i64,
    requested_output_tokens: Option<u64>,
    now: i64,
    fast: bool,
    prices: metering::CodexPrices,
) -> (i64, Option<registry::UsageEventInput>) {
    let priced = price_usage_with_prices(usage, prices, fast);
    // Honest billing: the transport cannot hard-stop generation, so the model may emit more output
    // than the client's requested cap. The provider truly consumed those tokens (the immutable
    // usage_event and window calibration below record the real figures), but the customer is never
    // charged past the ceiling it asked for — the overage is absorbed by the pool, matching the
    // real API where hitting `max_tokens` stops generation and bills only up to the cap.
    let charge_basis_nano = match requested_output_tokens {
        Some(cap) if usage.output_tokens > cap => {
            let mut capped_usage = usage.clone();
            capped_usage.output_tokens = cap;
            price_usage_with_prices(&capped_usage, prices, fast).real_nano
        }
        _ => priced.real_nano,
    };
    let computed_charge = metering::apply_multiplier(charge_basis_nano, mult_bp);
    let charge = computed_charge.clamp(0, i64::MAX as i128) as i64;
    let usage_event = (priced.real_nano > 0).then(|| registry::UsageEventInput {
        model: model.id.clone(),
        provider: registry::PROVIDER_OPENAI.to_string(),
        input_tokens: priced.normal_input.min(i64::MAX as u64) as i64,
        output_tokens: usage.output_tokens.min(i64::MAX as u64) as i64,
        cache_read_tokens: priced.cached_input.min(i64::MAX as u64) as i64,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: priced.cache_write_input.min(i64::MAX as u64) as i64,
        web_search_requests: 0,
        real_nano: priced.real_nano.min(i64::MAX as i128) as i64,
        // What the customer is billed for: the capped slice when the model overshot the ceiling it
        // asked for. The ledger's multiplier invariant is checked against this, while the full
        // `real_nano` above keeps the pool's absorbed overage measurable.
        charge_basis_nano: charge_basis_nano.min(i64::MAX as i128) as i64,
        speed: if fast { "fast" } else { "standard" }.to_string(),
        inference_geo: String::new(),
        input_nano: priced.input_nano.min(i64::MAX as i128) as i64,
        output_nano: priced.output_nano.min(i64::MAX as i128) as i64,
        cache_read_nano: priced.cached_nano.min(i64::MAX as i128) as i64,
        cache_write_5m_nano: 0,
        cache_write_1h_nano: priced.cache_write_nano.min(i64::MAX as i128) as i64,
        web_search_nano: 0,
        priced_ts: now,
    });
    (charge, usage_event)
}

pub(crate) async fn begin_admission(
    app: &AppState,
    headers: &HeaderMap,
    peer: &SocketAddr,
) -> Result<PendingCodexAdmission, AdmissionError> {
    if !app.authority_ready.load(Ordering::Acquire) {
        return Err(AdmissionError::Unavailable);
    }
    let execution = crate::execution::parse_execution_attempt(headers).map_err(|error| {
        elog::error(
            "codex-billing",
            format!(
                "OpenAI execution identity rejected class={}",
                error.as_str()
            ),
        );
        AdmissionError::Unavailable
    })?;
    let authz = authorize(app, headers, peer).await;
    let tenant_scope = match &authz {
        Authz::Admin { affinity_scope } => affinity_scope.clone(),
        // Balance is enforced only after model-specific release resolution. Service assignments
        // use meter_only and must remain admissible at zero balance.
        Authz::Metered { account_id, .. } => account_id.clone(),
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return Err(AdmissionError::Unauthorized);
        }
        Authz::Unavailable => return Err(AdmissionError::Unavailable),
    };
    Metrics::inc(&app.metrics.requests);

    Ok(PendingCodexAdmission {
        tenant_scope,
        authz,
        execution,
    })
}

/// Rates for this model as of `now`. The pinned, effective-dated catalog in `metering` is
/// authoritative, so a reviewed price change takes effect at its own epoch without a restart. The
/// config-resolved rate remains the fallback for a model the catalog no longer advertises, which
/// keeps an in-flight settlement priced exactly as it was admitted.
pub(super) fn effective_prices(model: &CodexModel, now: i64) -> metering::CodexPrices {
    metering::codex_prices_at(&model.id, now).unwrap_or(model.prices)
}

#[cfg(test)]
pub(super) fn reserve_cost(
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    now: i64,
    fast: bool,
) -> i128 {
    reserve_cost_with_prices(
        model,
        effective_prices(model, now),
        estimated_input_tokens,
        requested_output_tokens,
        fast,
    )
}

/// The reserve formula under one explicit rate card: the hot tariff override book substitutes the
/// base vector here, while the long-context and Fast modifiers stay code-applied on top.
pub(super) fn reserve_cost_with_prices(
    model: &CodexModel,
    prices: metering::CodexPrices,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    fast: bool,
) -> i128 {
    let long = estimated_input_tokens > prices.long_context_threshold;
    let input_rate = prices.input.max(prices.cache_write_input);
    let input_rate = if long {
        metering::apply_multiplier(input_rate, prices.long_input_basis_points)
    } else {
        input_rate
    };
    let output_rate = if long {
        metering::apply_multiplier(prices.output, prices.long_output_basis_points)
    } else {
        prices.output
    };
    // Reserve the output leg against the client's requested cap when it supplied one, not the
    // model's full max_output_tokens. Settlement bills output capped to the same figure, so the
    // hold stays a correct ceiling while a small `max_tokens` no longer holds the worst-case
    // ~$4 that false-402'd low-balance clients running several turns at once.
    let output_tokens = requested_output_tokens
        .unwrap_or(model.max_output_tokens)
        .min(model.max_output_tokens);
    let standard = (estimated_input_tokens as i128)
        .saturating_mul(input_rate)
        .saturating_add((output_tokens as i128).saturating_mul(output_rate));
    apply_fast_multiplier(prices, standard, fast)
}

/// Exact official-price cost of one completed turn, used for per-home window-capacity
/// calibration. Pure pricing only: customer money moves exclusively through `settled_charge`.
#[cfg(test)]
pub(crate) fn price_real_nano(
    model: &CodexModel,
    usage: &CodexUsage,
    now: i64,
    fast: bool,
) -> i128 {
    price_usage(model, usage, now, fast).real_nano
}

/// Build one complete immutable dual-ledger event. Reasoning is a subset of output and cached
/// input is a subset of input; both are recorded diagnostically but never added twice. Integer
/// conversions fail closed so hostile counters cannot saturate into plausible evidence.
pub(crate) fn price_calibration_event(
    request_id: &str,
    home_id: &str,
    model: &CodexModel,
    usage: &CodexUsage,
    completed_at: i64,
    fast: bool,
    provider_reported_tier: Option<&str>,
) -> anyhow::Result<registry::CodexTurnCalibrationEvent> {
    if usage.input_tokens == 0 && usage.output_tokens == 0 {
        anyhow::bail!("Codex turn completed without billable usage evidence");
    }
    // Hot tariff override: capacity evidence prices the official replacement cost from the same
    // book the customer settlement replays, on both the USD and the native credit ledgers; the
    // Fast multiplier stays the compiled model capability (overrides carry rates, not gates).
    let book = tariff_book::snapshot();
    let compiled = effective_prices(model, completed_at);
    let (prices, api_schedule_id) = match metering::codex_matched_tariff_at(&model.id, completed_at)
    {
        Some((family, compiled)) => match book.resolve(family, completed_at) {
            Some((pin, payload)) => match tariff_book::as_codex(&payload) {
                Some(prices) => (prices, pin.schedule_id),
                None => (compiled, String::new()),
            },
            None => (compiled, String::new()),
        },
        None => (compiled, String::new()),
    };
    let priced = price_usage_with_prices(usage, prices, fast);
    let service_tier = if fast {
        metering::CodexServiceTier::Fast
    } else {
        metering::CodexServiceTier::Standard
    };
    let tariff = metering::codex_tariff_capability_at(
        &model.id,
        completed_at,
        service_tier,
        usage.input_tokens,
    )
    .map_err(|error| anyhow::anyhow!("Codex tariff identity unavailable: {error:?}"))?;
    let api_tariff_schedule_id = if api_schedule_id.is_empty() {
        tariff.tariff_schedule_id.as_str().to_owned()
    } else {
        api_schedule_id
    };
    let (credit_rates, credit_schedule_id) =
        match metering::codex_matched_credit_rates_at(&model.id) {
            Some((family, compiled_rates)) => match book.resolve(family, completed_at) {
                Some((pin, payload)) => match tariff_book::as_codex_credits(&payload) {
                    Some(rates) => (rates, pin.schedule_id),
                    None => (
                        compiled_rates,
                        metering::CODEX_CREDIT_SCHEDULE_ID.to_owned(),
                    ),
                },
                None => (
                    compiled_rates,
                    metering::CODEX_CREDIT_SCHEDULE_ID.to_owned(),
                ),
            },
            None => anyhow::bail!("Codex subscription credit rate unavailable"),
        };
    let fast_multiplier = if fast {
        metering::codex_subscription_fast_multiplier_basis_points(&model.id)
            .context("Codex subscription credit rate unavailable")?
    } else {
        10_000
    };
    let credits = metering::codex_credit_cost_nano_with_rates(
        &credit_rates,
        fast_multiplier,
        usage.input_tokens,
        priced.cached_input,
        usage.output_tokens,
    );
    let token = |value: u64, name: &'static str| {
        i64::try_from(value).with_context(|| format!("Codex {name} exceeds bigint"))
    };
    let money = |value: i128, name: &'static str| {
        i64::try_from(value).with_context(|| format!("Codex {name} exceeds bigint"))
    };
    Ok(registry::CodexTurnCalibrationEvent {
        request_id: request_id.to_owned(),
        home_id: home_id.to_owned(),
        model_id: model.id.clone(),
        service_tier: if fast { "fast" } else { "standard" }.to_owned(),
        provider_reported_tier: provider_reported_tier.map(str::to_owned),
        api_tariff_schedule_id,
        credit_schedule_id,
        completed_at,
        input_tokens: token(usage.input_tokens, "input tokens")?,
        cached_input_tokens: token(priced.cached_input, "cached input tokens")?,
        cache_write_input_tokens: token(priced.cache_write_input, "cache-write input tokens")?,
        output_tokens: token(usage.output_tokens, "output tokens")?,
        reasoning_output_tokens: token(
            usage.reasoning_output_tokens.min(usage.output_tokens),
            "reasoning output tokens",
        )?,
        api_input_nanousd: money(priced.input_nano, "API input cost")?,
        api_cached_input_nanousd: money(priced.cached_nano, "API cached-input cost")?,
        api_cache_write_nanousd: money(priced.cache_write_nano, "API cache-write cost")?,
        api_output_nanousd: money(priced.output_nano, "API output cost")?,
        api_total_nanousd: money(priced.real_nano, "API total cost")?,
        chatgpt_input_nanocredits: money(credits.input_credit_nano, "ChatGPT input credits")?,
        chatgpt_cached_input_nanocredits: money(
            credits.cached_input_credit_nano,
            "ChatGPT cached-input credits",
        )?,
        chatgpt_output_nanocredits: money(credits.output_credit_nano, "ChatGPT output credits")?,
        chatgpt_total_nanocredits: money(credits.total_credit_nano, "ChatGPT total credits")?,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PricedUsage {
    normal_input: u64,
    cached_input: u64,
    cache_write_input: u64,
    input_nano: i128,
    cached_nano: i128,
    cache_write_nano: i128,
    output_nano: i128,
    real_nano: i128,
}

#[cfg(test)]
fn price_usage(model: &CodexModel, usage: &CodexUsage, now: i64, fast: bool) -> PricedUsage {
    price_usage_with_prices(usage, effective_prices(model, now), fast)
}

/// The settlement formula under one explicit rate card: the same math as `price_usage`, priced
/// from the exact override vector admission pinned (or the compiled vector when none applies).
fn price_usage_with_prices(
    usage: &CodexUsage,
    prices: metering::CodexPrices,
    fast: bool,
) -> PricedUsage {
    let cached_input = usage.cached_input_tokens.min(usage.input_tokens);
    let remaining = usage.input_tokens.saturating_sub(cached_input);
    let cache_write_input = usage.cache_write_input_tokens.min(remaining);
    let normal_input = remaining.saturating_sub(cache_write_input);
    let long = usage.input_tokens > prices.long_context_threshold;
    let input_multiplier = if long {
        prices.long_input_basis_points
    } else {
        10_000
    };
    let output_multiplier = if long {
        prices.long_output_basis_points
    } else {
        10_000
    };
    let input_nano = metering::apply_multiplier(
        (normal_input as i128).saturating_mul(prices.input),
        input_multiplier,
    );
    let input_nano = apply_fast_multiplier(prices, input_nano, fast);
    let cached_nano = metering::apply_multiplier(
        (cached_input as i128).saturating_mul(prices.cached_input),
        input_multiplier,
    );
    let cached_nano = apply_fast_multiplier(prices, cached_nano, fast);
    let cache_write_nano = metering::apply_multiplier(
        (cache_write_input as i128).saturating_mul(prices.cache_write_input),
        input_multiplier,
    );
    let cache_write_nano = apply_fast_multiplier(prices, cache_write_nano, fast);
    let output_nano = metering::apply_multiplier(
        (usage.output_tokens as i128).saturating_mul(prices.output),
        output_multiplier,
    );
    let output_nano = apply_fast_multiplier(prices, output_nano, fast);
    PricedUsage {
        normal_input,
        cached_input,
        cache_write_input,
        input_nano,
        cached_nano,
        cache_write_nano,
        output_nano,
        real_nano: input_nano
            .saturating_add(cached_nano)
            .saturating_add(cache_write_nano)
            .saturating_add(output_nano),
    }
}

fn apply_fast_multiplier(prices: metering::CodexPrices, amount: i128, fast: bool) -> i128 {
    if !fast {
        return amount;
    }
    metering::apply_multiplier(amount, prices.api_fast_multiplier_basis_points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::billing::AsyncBilling;
    use crate::codex::CodexPrices;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_SETTLEMENT_DB: AtomicU64 = AtomicU64::new(0);

    fn model() -> CodexModel {
        CodexModel {
            id: "gpt-5.6-sol".to_string(),
            upstream: "gpt-5.6-sol".to_string(),
            created: 0,
            owned_by: "test".to_string(),
            max_output_tokens: 128_000,
            reasoning_efforts: vec!["medium".to_string()],
            input_modalities: vec!["text".to_string(), "image".to_string()],
            output_modalities: vec!["text".to_string()],
            tool_calling: true,
            structured_outputs: true,
            fast_multiplier_basis_points: Some(25_000),
            prices: CodexPrices {
                input: 5_000,
                cached_input: 500,
                cache_write_input: 6_250,
                output: 30_000,
                api_fast_multiplier_basis_points: 25_000,
                long_context_threshold: 272_000,
                long_input_basis_points: 20_000,
                long_output_basis_points: 15_000,
            },
        }
    }

    fn turn_result(usage: CodexUsage) -> super::super::CodexTurnResult {
        super::super::CodexTurnResult {
            output: Vec::new(),
            usage,
            effective_service_tier: None,
            provider_reported_service_tier: None,
        }
    }

    fn settlement_model() -> CodexModel {
        let mut model = model();
        // Deliberately stay outside the effective-dated production catalog so every expected value
        // below is pinned to this fixture rather than changing when a reviewed catalog epoch lands.
        model.id = "gpt-settlement-test".to_string();
        model.upstream = model.id.clone();
        model
    }

    async fn reserved_admission(
        mult_bp: i64,
        topup: i64,
        hold: i64,
        request_id: &str,
    ) -> (CodexAdmission, Arc<AsyncBilling>, PathBuf) {
        const ACCOUNT: &str = "codex-settlement-account";
        const KEY: &str = "sk-pool-codex-settlement";

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // macOS can return the same wall-clock tick to tests that start in parallel. The
        // process-local sequence prevents two billing actors from opening the same SQLite fixture.
        let sequence = NEXT_SETTLEMENT_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-settlement-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start settlement test billing"),
        );
        billing
            .create_account(ACCOUNT, None, mult_bp)
            .await
            .unwrap();
        billing
            .topup(ACCOUNT, topup, Some("codex-settlement-seed"))
            .await
            .unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        assert!(billing
            .reserve_request(request_id, ACCOUNT, KEY, hold)
            .await
            .unwrap()
            .is_some());

        let admission = CodexAdmission {
            reservation: Some(Reservation {
                billing: Arc::clone(&billing),
                account_id: ACCOUNT.to_string(),
                key: KEY.to_string(),
                mult_bp,
                hold,
                tariff_priced_ts: None,
                pinned_tariff: None,
                policy_fast: None,
                request_id: request_id.to_string(),
                request_fact: None,
                guard: HoldGuard::new(
                    Some(Arc::clone(&billing)),
                    ACCOUNT.to_string(),
                    KEY.to_string(),
                    hold,
                    request_id.to_string(),
                ),
            }),
        };
        (admission, billing, path)
    }

    #[tokio::test]
    async fn zero_multiplier_text_and_image_admission_reserve_no_balance() {
        const ACCOUNT: &str = "codex-meter-only-account";
        const KEY: &str = "sk-pool-codex-meter-only";
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_SETTLEMENT_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-meter-only-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = Arc::new(AsyncBilling::start(path_string.clone(), 1).unwrap());
        billing.create_account(ACCOUNT, None, 0).await.unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();

        let (text_request_id, text_hold, text_multiplier, _, _, _) = reserve_codex_metered(
            &billing,
            ACCOUNT,
            KEY,
            &model(),
            100,
            Some(64),
            0,
            false,
            0,
            0,
            &registry::ExecutionAttempt::direct(),
            None,
        )
        .await
        .expect("zero-multiplier text request must not require balance");
        assert_eq!((text_hold, text_multiplier), (0, 0));

        let mut image = reserve_openai_image_metered(
            &billing,
            ACCOUNT,
            KEY,
            metering::GPT_IMAGE_2_ALIAS,
            OpenAiImageOperation::Generation,
            0,
            0,
            &registry::ExecutionAttempt::direct(),
        )
        .await
        .expect("zero-multiplier image request must not require balance");
        assert_eq!((image.hold, image.mult_bp), (0, 0));
        let image_request_id = image.request_id.clone();

        assert_eq!(
            billing
                .settle_request(&text_request_id, ACCOUNT, KEY, 0, 0, None)
                .await
                .unwrap(),
            Some(0),
        );
        assert_eq!(
            billing
                .settle_request(&image_request_id, ACCOUNT, KEY, 0, 0, None)
                .await
                .unwrap(),
            Some(0),
        );
        image.guard.disarm();
        drop(image);
        billing.flush().await.unwrap();

        let connection = registry::open(&path_string).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM billing_reservations
                      WHERE request_id IN (?1,?2) AND hold_nano=0 AND actual_nano=0
                        AND provider='openai' AND payable_multiplier_bp=0 AND state='canceled'",
                    (text_request_id, image_request_id),
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2,
        );
        assert_eq!(
            billing
                .account(ACCOUNT)
                .await
                .unwrap()
                .unwrap()
                .balance_nano,
            0,
        );

        drop(connection);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn prices_non_cached_cached_and_cache_write_without_double_counting() {
        let priced = price_usage(
            &model(),
            &CodexUsage {
                input_tokens: 1_000,
                cached_input_tokens: 400,
                cache_write_input_tokens: 100,
                output_tokens: 20,
                ..CodexUsage::default()
            },
            0,
            false,
        );
        assert_eq!(priced.normal_input, 500);
        assert_eq!(priced.cached_input, 400);
        assert_eq!(priced.cache_write_input, 100);
        assert_eq!(priced.input_nano, 500 * 5_000);
        assert_eq!(priced.cached_nano, 400 * 500);
        assert_eq!(priced.cache_write_nano, 100 * 6_250);
        assert_eq!(priced.output_nano, 20 * 30_000);
        assert_eq!(
            priced.real_nano,
            500 * 5_000 + 400 * 500 + 100 * 6_250 + 20 * 30_000
        );
    }

    #[test]
    fn long_context_multiplier_applies_to_all_input_buckets_and_output() {
        let priced = price_usage(
            &model(),
            &CodexUsage {
                input_tokens: 300_000,
                cached_input_tokens: 100_000,
                cache_write_input_tokens: 50_000,
                output_tokens: 10,
                ..CodexUsage::default()
            },
            0,
            false,
        );
        assert_eq!(priced.input_nano, 150_000 * 5_000 * 2);
        assert_eq!(priced.cached_nano, 100_000 * 500 * 2);
        assert_eq!(priced.cache_write_nano, 50_000 * 6_250 * 2);
        assert_eq!(priced.output_nano, 10 * 30_000 * 3 / 2);
    }

    #[test]
    fn reserve_covers_full_output_and_cache_write_rate() {
        let hold = reserve_cost(&model(), 1_000, None, 0, false);
        assert_eq!(hold, 1_000 * 6_250 + 128_000 * 30_000);
    }

    #[test]
    fn reserve_output_leg_follows_the_requested_cap() {
        let full = reserve_cost(&model(), 1_000, None, 0, false);
        // A small requested cap holds only that many output tokens, not the model's 128k max.
        let capped = reserve_cost(&model(), 1_000, Some(500), 0, false);
        assert_eq!(capped, 1_000 * 6_250 + 500 * 30_000);
        assert!(capped < full);
        // A cap above the model maximum is clamped to the model maximum.
        assert_eq!(
            reserve_cost(&model(), 1_000, Some(10_000_000), 0, false),
            full
        );
    }

    #[test]
    fn fast_mode_multiplies_reserve_settlement_ledger_and_capacity_spend() {
        let usage = CodexUsage {
            input_tokens: 100,
            output_tokens: 10,
            ..CodexUsage::default()
        };
        // Admission reserves every estimated input token at the most expensive possible input
        // bucket (a GPT-5.6 cache write); exact settlement below uses the actual fresh-input bucket.
        let fast_reserve = (100 * 6_250 + 10 * 30_000) * 5 / 2;
        let fast_usage = (100 * 5_000 + 10 * 30_000) * 5 / 2;

        assert_eq!(
            reserve_cost(&model(), 100, Some(10), 0, true),
            fast_reserve as i128
        );
        assert_eq!(
            price_real_nano(&model(), &usage, 0, true),
            fast_usage as i128
        );

        let (charge, event) = settled_charge(&model(), &usage, i64::MAX, 10_000, None, 0, true);
        assert_eq!(charge, fast_usage);
        let event = event.expect("fast usage must produce a usage event");
        assert_eq!(event.speed, "fast");
        assert_eq!(event.input_nano, 100 * 5_000 * 5 / 2);
        assert_eq!(event.output_nano, 10 * 30_000 * 5 / 2);
        assert_eq!(event.real_nano, fast_usage);
    }

    #[test]
    fn calibration_event_keeps_exact_api_credit_legs_and_reasoning_as_output_subset() {
        let usage = CodexUsage {
            input_tokens: 1_000,
            cached_input_tokens: 400,
            cache_write_input_tokens: 100,
            output_tokens: 100,
            reasoning_output_tokens: 80,
            ..CodexUsage::default()
        };
        let event = price_calibration_event(
            "turn-1",
            "home-1",
            &model(),
            &usage,
            1_785_369_601,
            true,
            Some("priority"),
        )
        .unwrap();
        assert_eq!(event.service_tier, "fast");
        assert_eq!(event.provider_reported_tier.as_deref(), Some("priority"));
        assert_eq!(
            event.api_tariff_schedule_id,
            "openai/gpt-5.6-sol/2026-07-30/v2"
        );
        assert_eq!(event.credit_schedule_id, metering::CODEX_CREDIT_SCHEDULE_ID);
        assert_eq!(event.reasoning_output_tokens, 80);
        // API Fast is 2x in this epoch.
        assert_eq!(event.api_input_nanousd, 5_000_000);
        assert_eq!(event.api_cached_input_nanousd, 400_000);
        assert_eq!(event.api_cache_write_nanousd, 1_250_000);
        assert_eq!(event.api_output_nanousd, 6_000_000);
        assert_eq!(event.api_total_nanousd, 12_650_000);
        // Subscription Fast is independently 2.5x; cache writes use fresh-input credits and
        // reasoning is not added again because it is already in output_tokens.
        assert_eq!(event.chatgpt_input_nanocredits, 187_500_000);
        assert_eq!(event.chatgpt_cached_input_nanocredits, 12_500_000);
        assert_eq!(event.chatgpt_output_nanocredits, 187_500_000);
        assert_eq!(event.chatgpt_total_nanocredits, 387_500_000);
    }

    #[test]
    fn settlement_caps_billed_output_but_records_actual_usage() {
        // The model generated 1_000 output tokens; the client asked for at most 100.
        let usage = CodexUsage {
            input_tokens: 100,
            output_tokens: 1_000,
            ..CodexUsage::default()
        };
        // Uncapped: bill every generated token (fixture rates: input 5_000, output 30_000).
        let (uncapped, _) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            10_000,
            None,
            0,
            false,
        );
        assert_eq!(uncapped, 100 * 5_000 + 1_000 * 30_000);
        // Honest billing: charge only up to the requested 100 output tokens.
        let (capped, event) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            10_000,
            Some(100),
            0,
            false,
        );
        assert_eq!(capped, 100 * 5_000 + 100 * 30_000);
        assert!(capped < uncapped);
        // The immutable provider-usage record still reflects the real consumption, so window
        // calibration and accounting stay truthful even though the customer paid less.
        let event = event.expect("positive usage must produce a usage event");
        assert_eq!(event.output_tokens, 1_000);
        assert_eq!(event.real_nano, 100 * 5_000 + 1_000 * 30_000);
    }

    #[test]
    fn pricing_comes_from_the_audited_catalog_not_the_config_copy() {
        // A config whose embedded rate drifted from the pinned catalog must still bill the
        // catalog rate: `metering` is the single audited source of money.
        let mut drifted = model();
        drifted.prices.input = 1;
        drifted.prices.output = 1;
        let usage = CodexUsage {
            input_tokens: 1_000,
            output_tokens: 20,
            ..CodexUsage::default()
        };
        let priced = price_usage(&drifted, &usage, 0, false);
        assert_eq!(priced.input_nano, 1_000 * 5_000);
        assert_eq!(priced.output_nano, 20 * 30_000);
    }

    /// Settlement replays the exact override version pinned at admission, not the compiled
    /// constants and not a newer version.
    #[tokio::test]
    async fn settlement_replays_the_pinned_override_version() {
        let _lock = tariff_book::GLOBAL_BOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // effective_from = i64::MAX keeps the row invisible to every timestamped resolve — and so
        // to every concurrently running test; only the exact pinned-version lookup sees it.
        tariff_book::install_global_rows_for_test(vec![tariff_book::test_row(
            "openai/codex/gpt-5.6-sol",
            2,
            i64::MAX,
            serde_json::json!({
                "input": "10000",
                "cached_input": "1000",
                "cache_write_input": "12500",
                "output": "60000",
                "api_fast_multiplier_basis_points": 25000,
                "long_context_threshold": 272000,
                "long_input_basis_points": 20000,
                "long_output_basis_points": 15000
            }),
        )]);
        const TOPUP: i64 = 1_000_000_000;
        let (mut admission, billing, path) =
            reserved_admission(10_000, TOPUP, 500_000_000, "codex-pinned-override").await;
        admission
            .reservation
            .as_mut()
            .expect("reservation")
            .pinned_tariff = Some(tariff_book::PinnedTariff {
            family: "openai/codex/gpt-5.6-sol".to_owned(),
            version: 2,
            schedule_id: "openai/codex/gpt-5.6-sol/v2".to_owned(),
        });
        admission.settle(
            &model(),
            &turn_result(CodexUsage {
                input_tokens: 1_000,
                output_tokens: 20,
                ..CodexUsage::default()
            }),
            None,
            false,
            None,
        );
        billing.flush().await.unwrap();
        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        let usage = billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);
        tariff_book::clear_global_book_for_test();
        // 1_000 × 10_000 + 20 × 60_000 at the override card (×1.0 multiplier) — the compiled card
        // would have charged 5_600_000.
        assert_eq!(account.spent_nano, 11_200_000);
        assert_eq!(account.reserved_nano, 0);
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].charge_nano, 11_200_000);
    }

    /// A pinned version the book cannot produce is an integrity error: nothing is settled at
    /// compiled prices and the reservation is left for durable recovery.
    #[tokio::test]
    async fn a_missing_pinned_override_version_never_settles_at_compiled() {
        let _lock = tariff_book::GLOBAL_BOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        tariff_book::clear_global_book_for_test();
        const TOPUP: i64 = 1_000_000_000;
        const HOLD: i64 = 500_000_000;
        let (mut admission, billing, path) =
            reserved_admission(10_000, TOPUP, HOLD, "codex-missing-pinned-override").await;
        admission
            .reservation
            .as_mut()
            .expect("reservation")
            .pinned_tariff = Some(tariff_book::PinnedTariff {
            family: "openai/codex/gpt-5.6-sol".to_owned(),
            version: 7,
            schedule_id: "openai/codex/gpt-5.6-sol/v7".to_owned(),
        });
        admission.settle(
            &model(),
            &turn_result(CodexUsage {
                input_tokens: 1_000,
                output_tokens: 20,
                ..CodexUsage::default()
            }),
            None,
            false,
            None,
        );
        billing.flush().await.unwrap();
        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);
        tariff_book::clear_global_book_for_test();
        // No settle happened at all: the hold stays reserved for the reconciler, nothing is spent.
        assert_eq!(account.spent_nano, 0);
        assert_eq!(account.reserved_nano, HOLD);
    }

    #[test]
    fn a_model_outside_the_catalog_keeps_the_rate_it_was_admitted_with() {
        // Nothing can configure such a model today, but an in-flight settlement for a model the
        // catalog stopped advertising must never silently reprice to a default.
        let mut retired = model();
        retired.id = "gpt-retired".to_string();
        retired.prices.input = 7_000;
        let priced = price_usage(
            &retired,
            &CodexUsage {
                input_tokens: 10,
                ..CodexUsage::default()
            },
            0,
            false,
        );
        assert_eq!(priced.input_nano, 10 * 7_000);
    }

    #[test]
    fn settlement_applies_the_customer_multiplier_exactly_once_and_builds_openai_usage() {
        let usage = CodexUsage {
            input_tokens: 1_000,
            cached_input_tokens: 400,
            cache_write_input_tokens: 100,
            output_tokens: 20,
            ..CodexUsage::default()
        };
        let (charge, event) = settled_charge(
            &settlement_model(),
            &usage,
            10_000_000,
            5_000,
            None,
            123,
            false,
        );

        // Official cost: 500*5000 + 400*500 + 100*6250 + 20*30000 = 3,925,000.
        // The customer multiplier is 0.5 and must be applied once, not squared.
        assert_eq!(charge, 1_962_500);
        let event = event.expect("positive usage must produce an immutable usage event");
        assert_eq!(event.model, "gpt-settlement-test");
        assert_eq!(event.provider, registry::PROVIDER_OPENAI);
        assert_eq!(event.input_tokens, 500);
        assert_eq!(event.cache_read_tokens, 400);
        assert_eq!(event.cache_write_1h_tokens, 100);
        assert_eq!(event.output_tokens, 20);
        assert_eq!(event.input_nano, 2_500_000);
        assert_eq!(event.cache_read_nano, 200_000);
        assert_eq!(event.cache_write_1h_nano, 625_000);
        assert_eq!(event.output_nano, 600_000);
        assert_eq!(
            event.real_nano,
            event.input_nano
                + event.cache_read_nano
                + event.cache_write_1h_nano
                + event.output_nano
        );
        assert_eq!(event.priced_ts, 123);
    }

    #[test]
    fn settlement_retains_full_actual_beyond_the_admission_hold() {
        let usage = CodexUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..CodexUsage::default()
        };
        let hold = 17;
        let (charge, event) =
            settled_charge(&settlement_model(), &usage, hold, 10_000, None, 456, false);
        let event = event.expect("priced usage must remain auditable");
        assert_eq!(charge, event.charge_basis_nano);
        assert!(charge as i128 > hold as i128 + metering::OVERDRAFT_NANO);
    }

    /// A customer that caps its turn with `max_tokens` is billed to that ceiling and no further, but
    /// the ledger must still be able to prove the amount matches the multiplier it declares.
    /// Recording the full generation as the official price put those two numbers on different bases
    /// and made the production invariant check report a money defect on rows where both were right.
    #[test]
    fn a_capped_turn_reports_the_billed_basis_and_keeps_the_full_cost_visible() {
        let usage = CodexUsage {
            input_tokens: 300,
            output_tokens: 20_000,
            ..CodexUsage::default()
        };
        let (charge, event) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            1_500,
            Some(4_000),
            0,
            false,
        );
        let event = event.expect("a priced turn emits a usage event");

        // The full generation stays recorded — that is what the pool actually paid for.
        assert_eq!(event.output_tokens, 20_000);
        assert!(event.real_nano > event.charge_basis_nano);
        // The charge is exactly the declared multiplier of the billed basis: the invariant that
        // production checks on every settled row.
        assert_eq!(
            charge as i128,
            metering::apply_multiplier(event.charge_basis_nano as i128, 1_500)
        );

        // An uncapped turn leaves both figures identical, so nothing changes elsewhere.
        let (_, uncapped) =
            settled_charge(&settlement_model(), &usage, i64::MAX, 1_500, None, 0, false);
        let uncapped = uncapped.expect("a priced turn emits a usage event");
        assert_eq!(uncapped.real_nano, uncapped.charge_basis_nano);
    }

    #[test]
    fn zero_usage_neither_charges_nor_writes_a_usage_event() {
        let (charge, event) = settled_charge(
            &settlement_model(),
            &CodexUsage::default(),
            1_000_000,
            10_000,
            None,
            789,
            false,
        );
        assert_eq!(charge, 0);
        assert!(event.is_none());
    }

    #[test]
    fn settlement_saturates_hostile_token_counts_instead_of_wrapping() {
        let usage = CodexUsage {
            input_tokens: u64::MAX,
            cached_input_tokens: u64::MAX,
            cache_write_input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            ..CodexUsage::default()
        };
        let (charge, event) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            10_000,
            None,
            999,
            false,
        );
        assert_eq!(charge, i64::MAX);
        let event = event.unwrap();
        assert_eq!(event.input_tokens, 0);
        assert_eq!(event.cache_read_tokens, i64::MAX);
        assert_eq!(event.cache_write_1h_tokens, 0);
        assert_eq!(event.output_tokens, i64::MAX);
        assert_eq!(event.real_nano, i64::MAX);
        assert_eq!(event.cache_read_nano, i64::MAX);
        assert_eq!(event.output_nano, i64::MAX);
    }

    #[tokio::test]
    async fn codex_admission_settle_debits_once_and_persists_provider_usage() {
        const TOPUP: i64 = 20_000_000;
        let (admission, billing, path) =
            reserved_admission(5_000, TOPUP, 10_000_000, "codex-settle").await;
        let usage = CodexUsage {
            input_tokens: 1_000,
            cached_input_tokens: 400,
            cache_write_input_tokens: 100,
            output_tokens: 20,
            ..CodexUsage::default()
        };

        admission.settle(&settlement_model(), &turn_result(usage), None, false, None);
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP - 1_962_500);
        assert_eq!(account.spent_nano, 1_962_500);
        assert_eq!(account.reserved_nano, 0);

        let ledger = billing
            .ledger("codex-settlement-account", 10)
            .await
            .unwrap();
        let charges = ledger
            .iter()
            .filter(|row| row.kind == "charge")
            .collect::<Vec<_>>();
        assert_eq!(charges.len(), 1);
        assert_eq!(charges[0].amount_nano, 1_962_500);
        assert_eq!(charges[0].model.as_deref(), Some("gpt-settlement-test"));

        let usage_rows = billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap();
        assert_eq!(usage_rows.len(), 1);
        assert_eq!(usage_rows[0].charge_nano, 1_962_500);
        assert_eq!(usage_rows[0].real_nano, 3_925_000);
        let providers = billing.spend_by_provider(0).await.unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider, registry::PROVIDER_OPENAI);
        assert_eq!(providers[0].requests, 1);
        assert_eq!(providers[0].charge_nano, 1_962_500);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn openai_image_settlement_preserves_modality_cost_buckets() {
        let usage = metering::OpenAiImageUsage {
            total_text_input_tokens: 100,
            cached_text_input_tokens: 40,
            total_image_input_tokens: 200,
            cached_image_input_tokens: 50,
            image_output_tokens: 10,
        };
        let (charge, event) = settled_openai_image_charge(
            metering::GPT_IMAGE_2_ALIAS,
            &usage,
            10_000_000,
            5_000,
            1_800_000_000,
        );
        assert_eq!(charge, 975_000);
        let event = event.unwrap();
        assert_eq!(event.provider, registry::PROVIDER_OPENAI);
        assert_eq!(event.model, metering::GPT_IMAGE_2_ALIAS);
        assert_eq!(event.input_tokens, 300);
        assert_eq!(event.output_tokens, 10);
        assert_eq!(event.cache_read_tokens, 90);
        assert_eq!(event.input_nano, 1_500_000);
        assert_eq!(event.cache_read_nano, 150_000);
        assert_eq!(event.output_nano, 300_000);
        assert_eq!(event.real_nano, 1_950_000);
        assert_eq!(event.priced_ts, 1_800_000_000);
    }

    #[test]
    fn openai_image_settlement_retains_full_actual_beyond_the_hold() {
        let usage = metering::OpenAiImageUsage {
            image_output_tokens: 100_000,
            ..Default::default()
        };
        let hold = 1;
        let (charge, event) = settled_openai_image_charge(
            metering::GPT_IMAGE_2_ALIAS,
            &usage,
            hold,
            10_000,
            1_800_000_000,
        );
        let event = event.expect("priced image usage must remain auditable");
        assert_eq!(charge, event.charge_basis_nano);
        assert!(charge as i128 > hold as i128 + metering::OVERDRAFT_NANO);
    }

    #[tokio::test]
    async fn image_settlement_closes_hold_and_persists_exact_usage() {
        const TOPUP: i64 = 20_000_000;
        const HOLD: i64 = 10_000_000;
        let (mut codex_admission, billing, path) =
            reserved_admission(5_000, TOPUP, HOLD, "image-exact-settlement").await;
        let mut admission = OpenAiImageAdmission {
            reservation: codex_admission.reservation.take(),
        };
        assert_eq!(admission.request_id(), Some("image-exact-settlement"));
        admission.mark_delivering().await.unwrap();
        admission.settle(
            metering::GPT_IMAGE_2_ALIAS,
            &metering::OpenAiImageUsage {
                total_text_input_tokens: 100,
                cached_text_input_tokens: 40,
                total_image_input_tokens: 200,
                cached_image_input_tokens: 50,
                image_output_tokens: 10,
            },
        );
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP - 975_000);
        assert_eq!(account.spent_nano, 975_000);
        assert_eq!(account.reserved_nano, 0);
        let usage = billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].provider, registry::PROVIDER_OPENAI);
        assert_eq!(usage[0].model, metering::GPT_IMAGE_2_ALIAS);
        assert_eq!(usage[0].charge_nano, 975_000);
        assert_eq!(usage[0].real_nano, 1_950_000);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn image_delivery_fence_prevents_cancellation_refund() {
        const TOPUP: i64 = 20_000_000;
        const HOLD: i64 = 10_000_000;
        let (mut codex_admission, billing, path) =
            reserved_admission(10_000, TOPUP, HOLD, "image-delivery-fence").await;
        let mut admission = OpenAiImageAdmission {
            reservation: codex_admission.reservation.take(),
        };

        admission.mark_delivering().await.unwrap();
        drop(admission);
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP - HOLD);
        assert_eq!(account.spent_nano, 0);
        assert_eq!(account.reserved_nano, HOLD);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn dropping_codex_admission_without_settle_refunds_the_hold_once() {
        const TOPUP: i64 = 20_000_000;
        let (admission, billing, path) =
            reserved_admission(10_000, TOPUP, 10_000_000, "codex-drop").await;

        drop(admission);
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP);
        assert_eq!(account.spent_nano, 0);
        assert_eq!(account.reserved_nano, 0);
        assert!(billing
            .ledger("codex-settlement-account", 10)
            .await
            .unwrap()
            .iter()
            .all(|row| row.kind != "charge"));
        assert!(billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap()
            .is_empty());

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn request_fact_specs_keep_shared_terminal_status_semantics() {
        let seed = || {
            CodexRequestFactSeed::for_test(
                "55555555-5555-4555-8555-555555555555",
                ClientAttribution::unknown_for_internal_use(),
                registry::ExecutionAttempt::direct(),
                "account",
                "key_nonsecret",
                pool::now(),
                RequestLifecycleClock::default(),
            )
        };
        let universal =
            seed().terminal_fact(axum::http::StatusCode::SERVICE_UNAVAILABLE, None, None);
        let native = seed().terminal_input_tokens_fact(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            None,
            None,
            None,
        );
        for fact in [&universal, &native] {
            assert_eq!(fact.terminal.http_status_code, Some(503));
            assert_eq!(
                fact.terminal.provider_terminal_class,
                ProviderTerminalClass::Unknown
            );
            assert_eq!(fact.terminal.delivery_state, DeliveryState::NotStarted);
            assert_eq!(fact.terminal.internal_attempt_count, Some(0));
        }
        assert_eq!(universal.route_class, "universal");
        assert_eq!(universal.request_class, "count_tokens");
        assert_eq!(native.route_class, "native");
        assert_eq!(native.request_class, "input_tokens");
    }

    fn fact_context() -> CodexBillableFactContext {
        CodexBillableFactContext {
            admitted_at: pool::now(),
            lifecycle_clock: RequestLifecycleClock::default(),
            attempts: super::super::CodexAttemptObserver::default(),
            downstream_disconnect: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn process_error_terminal_mapping_is_conservative_and_total() {
        let cases = [
            (
                super::super::ProcessError::BadRequest,
                ProviderTerminalClass::ClientError,
                DeliveryState::Unknown,
            ),
            (
                super::super::ProcessError::ContextWindowExceeded,
                ProviderTerminalClass::ClientError,
                DeliveryState::Unknown,
            ),
            (
                super::super::ProcessError::UsageLimitExceeded { retry_after: None },
                ProviderTerminalClass::Quota,
                DeliveryState::Unknown,
            ),
            (
                super::super::ProcessError::AuthenticationRequired,
                ProviderTerminalClass::Auth,
                DeliveryState::Unknown,
            ),
            (
                super::super::ProcessError::SubscriptionRequired,
                ProviderTerminalClass::Auth,
                DeliveryState::Unknown,
            ),
            (
                super::super::ProcessError::Timeout("test"),
                ProviderTerminalClass::Timeout,
                DeliveryState::Interrupted,
            ),
            (
                super::super::ProcessError::Closed,
                ProviderTerminalClass::Transport,
                DeliveryState::Interrupted,
            ),
            (
                super::super::ProcessError::Protocol("test".into()),
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
            ),
            (
                super::super::ProcessError::Disabled,
                ProviderTerminalClass::Unknown,
                DeliveryState::NotStarted,
            ),
            (
                super::super::ProcessError::InvalidConfig("test".into()),
                ProviderTerminalClass::Unknown,
                DeliveryState::NotStarted,
            ),
        ];
        for (error, provider, delivery) in cases {
            assert_eq!(codex_process_error_terminal(&error), (provider, delivery));
        }
        let fallback = super::super::ProcessError::ExternalFallbackFailed {
            local: Box::new(super::super::ProcessError::BadRequest),
        };
        assert_eq!(
            codex_process_error_terminal(&fallback),
            (ProviderTerminalClass::Unknown, DeliveryState::Unknown)
        );
    }

    #[test]
    fn successful_output_tool_evidence_is_true_false_or_unknown_only_when_exhaustive() {
        let result = |output| super::super::CodexTurnResult {
            output,
            usage: CodexUsage::default(),
            effective_service_tier: None,
            provider_reported_service_tier: None,
        };
        assert_eq!(codex_tool_calls_in_output(&result(Vec::new())), Some(false));
        assert_eq!(
            codex_tool_calls_in_output(&result(vec![serde_json::json!({"type":"message"})])),
            Some(false)
        );
        assert_eq!(
            codex_tool_calls_in_output(&result(vec![serde_json::json!({"type":"function_call"})])),
            Some(true)
        );
        assert_eq!(
            codex_tool_calls_in_output(&result(vec![serde_json::json!({"type":"future_output"})])),
            None
        );
    }

    #[test]
    fn attempt_observer_overflow_and_join_terminalization_preserve_only_exact_evidence() {
        let context = fact_context();
        context.attempts.record_send();
        context.attempts.record_send();
        context.downstream_disconnect.store(true, Ordering::Release);
        let join = context.terminal_evidence(
            None,
            ProviderTerminalClass::Unknown,
            DeliveryState::Unknown,
            None,
            true,
            None,
        );
        assert_eq!(join.internal_attempt_count, Some(2));
        assert_eq!(join.http_status_code, None);
        assert_eq!(join.provider_terminal_class, ProviderTerminalClass::Unknown);
        assert_eq!(join.delivery_state, DeliveryState::Unknown);
        assert_eq!(join.downstream_disconnect, Some(true));
        assert_eq!(join.tool_calls_in_output, None);

        let overflow = fact_context();
        overflow.attempts.set_count_for_test(i32::MAX as usize + 1);
        assert_eq!(
            overflow
                .terminal_evidence(
                    None,
                    ProviderTerminalClass::Unknown,
                    DeliveryState::Unknown,
                    None,
                    true,
                    None
                )
                .internal_attempt_count,
            None
        );
        let saturated = fact_context();
        saturated.attempts.set_count_for_test(usize::MAX);
        saturated.attempts.record_send();
        assert_eq!(saturated.attempts.exhaustive_i32(), None);
    }
}
