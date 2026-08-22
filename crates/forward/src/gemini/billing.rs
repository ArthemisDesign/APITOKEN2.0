//! Shared customer admission and exact native-Gemini settlement.

use super::config::GeminiModel;
use crate::execution::{ClientAttribution, LogicalRequestId, RequestLifecycleClock};
use crate::metrics::Metrics;
use crate::pricing::{tariff_book, EnginePricingRequestId};
use crate::proxy::{authorize, Authz, HoldGuard};
use crate::request_classification::RequestClassification;
use crate::state::AppState;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use registry::request_facts::{
    DeliveryState, ProviderTerminalClass, RequestFactAdmission, RequestFactTerminalEvidence,
};
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

const CALIBRATION_PROFILE_HEADER: &str = "x-apitoken-calibration-profile";
const CALIBRATION_REQUEST_ID_HEADER: &str = "x-apitoken-calibration-request-id";
const CALIBRATION_NOT_AFTER_HEADER: &str = "x-apitoken-calibration-not-after";
// JavaScript's Number is the Node helper's IPC integer. Keep the seconds value within the exact
// range after conversion to milliseconds so the dispatch attestation cannot be rounded.
const MAX_CALIBRATION_NOT_AFTER: u64 = 9_007_199_254_740;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Unauthorized,
    Unavailable,
    LowBalance,
}

/// Which pricing authority produced the admission hold; selects the settlement rounding
/// contract. Release-v2 settles with the exact contract floor (matching its reserve); the
/// legacy scalar keeps its immutable half-up arithmetic. Legacy strict Gemini is rejected at
type GeminiReserveResult = (
    u64,
    i64,
    i64,
    i64,
    Option<tariff_book::PinnedTariff>,
    Option<GeminiBillableFactContext>,
);

struct Reservation {
    billing: std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: String,
    key: String,
    mult_bp: i64,
    hold: i64,
    /// Admission-time instant used for both compiled and hot-override tariff resolution.
    /// Settlement must not replace it with completion time across an effective-date boundary.
    tariff_priced_ts: i64,
    /// The hot tariff override version admission priced the hold with; settlement replays exactly
    /// this version. `None` is the compiled constants, byte-identical to before.
    pinned_tariff: Option<tariff_book::PinnedTariff>,
    request_id: String,
    guard: HoldGuard,
    request_fact: Option<GeminiBillableFactContext>,
}

pub(crate) struct GeminiAdmission {
    reservation: Option<Reservation>,
    calibration_request_id: String,
    exact_calibration_target: bool,
    calibration_not_after: Option<u64>,
}

pub(crate) struct PendingGeminiAdmission {
    authz: Authz,
    execution: registry::ExecutionAttempt,
    calibration_request_id: String,
    calibration_target: Option<String>,
    calibration_not_after: Option<u64>,
}

/// Privacy-minimal immutable attribution captured from one successful metered Gemini admission.
/// The raw API key and provider credential never enter this seed.
pub(crate) struct GeminiRequestFactSeed {
    pub(super) logical_request_id: String,
    pub(super) client_attribution: ClientAttribution,
    pub(super) execution: registry::ExecutionAttempt,
    pub(super) account_id: String,
    pub(super) key_id: String,
    pub(super) admitted_at: i64,
    pub(super) lifecycle_clock: RequestLifecycleClock,
}

/// Closed, content-free native generation evidence created only after the owning Gemini parser and
/// validator accept the original customer body. The observer counts physical generation POSTs at
/// the transport seam; no request JSON, tool name, provider credential or header is retained.
pub(crate) struct GeminiBillableRequestSpec {
    route: GeminiBillableRoute,
    request_class: GeminiBillableRequestClass,
    requested_model: Option<String>,
    executable_model: Option<String>,
    stream_flag: bool,
    classification: RequestClassification,
    actual_sends: super::ActualSendObserver,
    downstream_disconnect: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
enum GeminiBillableRoute {
    Native,
    Universal,
}

#[derive(Clone, Copy)]
enum GeminiBillableRequestClass {
    Generate,
    StreamGenerate,
    Chat,
    Responses,
    Messages,
}

struct GeminiBillableFactContext {
    admitted_at: i64,
    lifecycle_clock: RequestLifecycleClock,
    actual_sends: super::ActualSendObserver,
    downstream_disconnect: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
struct GeminiTerminalFact {
    http_status_code: Option<i32>,
    provider_terminal_class: ProviderTerminalClass,
    delivery_state: DeliveryState,
    downstream_disconnect: Option<bool>,
    attempts_exhaustive: bool,
    tool_calls_in_output: Option<bool>,
}

impl fmt::Debug for GeminiBillableRequestSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeminiBillableRequestSpec(<redacted>)")
    }
}

impl fmt::Debug for GeminiBillableFactContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeminiBillableFactContext(<redacted>)")
    }
}

impl GeminiBillableRequestSpec {
    pub(crate) fn native(
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self::new(
            GeminiBillableRoute::Native,
            if stream_flag {
                GeminiBillableRequestClass::StreamGenerate
            } else {
                GeminiBillableRequestClass::Generate
            },
            requested_model,
            executable_model,
            stream_flag,
            classification,
        )
    }

    pub(crate) fn universal_chat(
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self::new(
            GeminiBillableRoute::Universal,
            GeminiBillableRequestClass::Chat,
            requested_model,
            executable_model,
            stream_flag,
            classification,
        )
    }

    pub(crate) fn universal_responses(
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self::new(
            GeminiBillableRoute::Universal,
            GeminiBillableRequestClass::Responses,
            requested_model,
            executable_model,
            stream_flag,
            classification,
        )
    }

    pub(crate) fn universal_messages(
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self::new(
            GeminiBillableRoute::Universal,
            GeminiBillableRequestClass::Messages,
            requested_model,
            executable_model,
            stream_flag,
            classification,
        )
    }

    fn new(
        route: GeminiBillableRoute,
        request_class: GeminiBillableRequestClass,
        requested_model: Option<String>,
        executable_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    ) -> Self {
        Self {
            route,
            request_class,
            requested_model,
            executable_model,
            stream_flag,
            classification,
            actual_sends: super::ActualSendObserver::default(),
            downstream_disconnect: Arc::new(AtomicBool::new(false)),
        }
    }

    fn route_class(&self) -> &'static str {
        match self.route {
            GeminiBillableRoute::Native => "native",
            GeminiBillableRoute::Universal => "universal",
        }
    }

    fn request_class(&self) -> &'static str {
        match self.request_class {
            GeminiBillableRequestClass::Generate => "generate",
            GeminiBillableRequestClass::StreamGenerate => "stream_generate",
            GeminiBillableRequestClass::Chat => "chat",
            GeminiBillableRequestClass::Responses => "responses",
            GeminiBillableRequestClass::Messages => "messages",
        }
    }
}

impl GeminiRequestFactSeed {
    fn into_billable_admission(
        self,
        billing_request_id: String,
        spec: &GeminiBillableRequestSpec,
    ) -> (RequestFactAdmission, GeminiBillableFactContext) {
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
            provider_plane: "gemini".into(),
            route_class: spec.route_class().into(),
            request_class: spec.request_class().into(),
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
        let context = GeminiBillableFactContext {
            admitted_at: self.admitted_at,
            lifecycle_clock: self.lifecycle_clock,
            actual_sends: spec.actual_sends.clone(),
            downstream_disconnect: Arc::clone(&spec.downstream_disconnect),
        };
        (admission, context)
    }
}

impl PendingGeminiAdmission {
    /// Request-scoped diagnostic identity created before routing or reserve. It contains no
    /// customer/profile/provider identity and stays stable across every pre-byte rotation attempt.
    pub(crate) fn request_id(&self) -> &str {
        &self.calibration_request_id
    }

    pub(crate) fn affinity_scope(&self) -> Option<&str> {
        self.authz.affinity_scope()
    }

    pub(crate) fn calibration_target(&self) -> Option<&str> {
        self.calibration_target.as_deref()
    }

    /// Absolute Unix-seconds dispatch fence supplied by a controlled exact-profile runner. It is
    /// present only on the deadline-bound exact profile lane and never becomes a provider header.
    pub(crate) fn calibration_not_after(&self) -> Option<u64> {
        self.calibration_not_after
    }

    /// Snapshot content-free request-fact attribution without widening `Authz`. Missing typed
    /// logical/lifecycle context, admin authority and SQLite all deliberately omit the producer.
    pub(crate) fn request_fact_seed(
        &self,
        logical_request_id: Option<&LogicalRequestId>,
        client_attribution: Option<&ClientAttribution>,
        lifecycle_clock: Option<&RequestLifecycleClock>,
        admitted_at: i64,
    ) -> Option<GeminiRequestFactSeed> {
        let logical_request_id = logical_request_id?;
        let lifecycle_clock = lifecycle_clock?;
        let Authz::Metered {
            account_id, key_id, ..
        } = &self.authz
        else {
            return None;
        };
        Some(GeminiRequestFactSeed {
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

    pub(crate) fn without_reserve(self) -> GeminiAdmission {
        GeminiAdmission {
            reservation: None,
            calibration_request_id: self.calibration_request_id,
            exact_calibration_target: self.calibration_target.is_some(),
            calibration_not_after: self.calibration_not_after,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn reserve(
        self,
        app: &AppState,
        model: &GeminiModel,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
        image_output_tokens: u64,
        grounding_enabled: bool,
        allow_output_cap: bool,
        billable_fact: Option<(GeminiRequestFactSeed, GeminiBillableRequestSpec)>,
    ) -> Result<(GeminiAdmission, u64), AdmissionError> {
        let mut effective_output_tokens = requested_output_tokens.max(1);
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
                let request_id = self.calibration_request_id.clone();
                // SQLite intentionally preserves the legacy money path and never admits analytics.
                // PostgreSQL is identified by its owned command-metrics carrier.
                let billable_fact = billing
                    .pg_command_stats()
                    .is_some()
                    .then_some(billable_fact)
                    .flatten();
                let (
                    affordable_output_tokens,
                    hold,
                    reservation_mult_bp,
                    tariff_priced_ts,
                    pinned_tariff,
                    request_fact,
                ) = reserve_gemini_metered(
                    billing,
                    account_id,
                    key,
                    model,
                    estimated_input_tokens,
                    requested_output_tokens,
                    image_output_tokens,
                    grounding_enabled,
                    allow_output_cap,
                    self.authz.mult_for(registry::PROVIDER_GOOGLE),
                    *available_nano,
                    &request_id,
                    &self.execution,
                    billable_fact,
                )
                .await?;
                effective_output_tokens = affordable_output_tokens;
                Some(Reservation {
                    billing: billing.clone(),
                    account_id: account_id.clone(),
                    key: key.clone(),
                    mult_bp: reservation_mult_bp,
                    hold,
                    tariff_priced_ts,
                    pinned_tariff,
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
        Ok((
            GeminiAdmission {
                reservation,
                calibration_request_id: self.calibration_request_id,
                exact_calibration_target: self.calibration_target.is_some(),
                calibration_not_after: self.calibration_not_after,
            },
            effective_output_tokens,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
async fn reserve_gemini_metered(
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
    allow_output_cap: bool,
    mult_bp: i64,
    available_nano: i64,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
    billable_fact: Option<(GeminiRequestFactSeed, GeminiBillableRequestSpec)>,
) -> Result<GeminiReserveResult, AdmissionError> {
    if available_nano <= 0 {
        return Err(AdmissionError::LowBalance);
    }
    reserve_gemini_legacy(
        billing,
        account_id,
        key,
        model,
        estimated_input_tokens,
        requested_output_tokens,
        image_output_tokens,
        grounding_enabled,
        allow_output_cap,
        mult_bp,
        available_nano,
        request_id,
        execution,
        billable_fact,
    )
    .await
}

#[cfg(test)]
static FAIL_GEMINI_DELIVERY_MARKER_FOR_TEST: OnceLock<Mutex<Option<std::thread::ThreadId>>> =
    OnceLock::new();

#[cfg(test)]
pub(crate) fn fail_next_gemini_delivery_marker_for_test() {
    *FAIL_GEMINI_DELIVERY_MARKER_FOR_TEST
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(std::thread::current().id());
}

impl GeminiBillableFactContext {
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
            // The private Antigravity requestId is generated locally and is not provider evidence.
            upstream_request_id: None,
            first_public_byte_at: self
                .lifecycle_clock
                .seal_first_public_byte_for_terminal(self.admitted_at, terminal_at),
            internal_attempt_count: attempts_exhaustive
                .then(|| self.actual_sends.count())
                .flatten()
                .and_then(|count| i32::try_from(count).ok()),
            failure_class: None,
            tool_calls_in_output,
        }
    }
}

impl GeminiAdmission {
    pub(crate) fn requires_usage(&self) -> bool {
        self.reservation.is_some()
    }

    /// The request identity carried into the reservation and the ledger. Not a secret: it is the
    /// same value the customer receives in `x-request-id`, and it is what makes a journal line
    /// joinable to the settlement it describes.
    pub(crate) fn request_id(&self) -> &str {
        &self.calibration_request_id
    }

    pub(crate) fn requests_post_turn_probe(&self) -> bool {
        self.exact_calibration_target
    }

    pub(crate) fn calibration_not_after(&self) -> Option<u64> {
        self.calibration_not_after
    }

    pub(crate) fn actual_send_observer(&self) -> Option<super::ActualSendObserver> {
        self.reservation
            .as_ref()
            .and_then(|reservation| reservation.request_fact.as_ref())
            .map(|context| context.actual_sends.clone())
    }

    pub(crate) fn observed_failure_delivery(&self) -> DeliveryState {
        match self
            .reservation
            .as_ref()
            .and_then(|reservation| reservation.request_fact.as_ref())
            .and_then(|context| context.actual_sends.count())
        {
            Some(0) => DeliveryState::NotStarted,
            Some(_) => DeliveryState::Interrupted,
            None => DeliveryState::Unknown,
        }
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

    pub(crate) async fn mark_delivering(&self) -> Result<(), AdmissionError> {
        #[cfg(test)]
        if FAIL_GEMINI_DELIVERY_MARKER_FOR_TEST
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap()
            .as_ref()
            == Some(&std::thread::current().id())
        {
            *FAIL_GEMINI_DELIVERY_MARKER_FOR_TEST
                .get()
                .unwrap()
                .lock()
                .unwrap() = None;
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

    /// Settle customer money when present and return one exact immutable provider event. Admin
    /// admissions have no reservation but keep the same request identity and calibration vector.
    #[cfg(test)]
    pub(crate) fn settle(
        self,
        model: &GeminiModel,
        usage: Option<&metering::GeminiUsage>,
        profile_id: &str,
    ) -> Option<registry::ProviderTurnCalibrationEvent> {
        self.settle_terminal(
            model,
            usage,
            profile_id,
            Some(200),
            ProviderTerminalClass::Success,
            DeliveryState::Completed,
            None,
            true,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn settle_terminal(
        self,
        model: &GeminiModel,
        usage: Option<&metering::GeminiUsage>,
        profile_id: &str,
        http_status_code: Option<i32>,
        provider_terminal_class: ProviderTerminalClass,
        delivery_state: DeliveryState,
        downstream_disconnect: Option<bool>,
        attempts_exhaustive: bool,
        tool_calls_in_output: Option<bool>,
    ) -> Option<registry::ProviderTurnCalibrationEvent> {
        self.settle_at(
            model,
            usage,
            profile_id,
            pool::now(),
            Some(GeminiTerminalFact {
                http_status_code,
                provider_terminal_class,
                delivery_state,
                downstream_disconnect,
                attempts_exhaustive,
                tool_calls_in_output,
            }),
        )
    }

    pub(crate) fn settle_failure(
        mut self,
        http_status: StatusCode,
        provider_terminal_class: ProviderTerminalClass,
        delivery_state: DeliveryState,
    ) {
        let Some(reservation) = self.reservation.as_mut() else {
            return;
        };
        let Some(context) = reservation.request_fact.take() else {
            return;
        };
        let evidence = context.terminal_evidence(
            Some(i32::from(http_status.as_u16())),
            provider_terminal_class,
            delivery_state,
            None,
            true,
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
                "gemini-billing",
                format!("Gemini request-fact failure settlement rejected: {error:#}"),
            );
        }
        reservation.guard.disarm();
    }

    pub(crate) fn settle_after_delivery_marker_failure(
        self,
        model: &GeminiModel,
        usage: Option<&metering::GeminiUsage>,
        profile_id: &str,
        tool_calls_in_output: Option<bool>,
    ) -> Option<registry::ProviderTurnCalibrationEvent> {
        self.settle_terminal(
            model,
            usage,
            profile_id,
            Some(503),
            ProviderTerminalClass::Success,
            DeliveryState::Unknown,
            None,
            true,
            tool_calls_in_output,
        )
    }

    fn settle_at(
        mut self,
        model: &GeminiModel,
        usage: Option<&metering::GeminiUsage>,
        profile_id: &str,
        completed_at: i64,
        terminal: Option<GeminiTerminalFact>,
    ) -> Option<registry::ProviderTurnCalibrationEvent> {
        if let Some(mut reservation) = self.reservation.take() {
            let priced_ts = reservation.tariff_priced_ts;
            // Hot tariff override: replay the exact version pinned at admission; a cross-family
            // serve reprices by the served model's family at the pinned priced timestamp; an empty
            // book is byte-identical to the compiled constants. A pinned version missing from the
            // book is an integrity error — the reservation is left to durable recovery, never
            // repriced at compiled. This sync settle cannot await the bounded refresh retry the
            // async planes perform; the miss is structurally unreachable because the pinning
            // reserve read the row from this process.
            let compiled = metering::gemini_prices_at(&model.id, priced_ts).unwrap_or(model.prices);
            let served_family =
                metering::gemini_matched_tariff_at(&model.id, priced_ts).map(|(family, _)| family);
            let (prices, override_schedule_id) = match tariff_book::charge_base(
                &tariff_book::snapshot(),
                reservation.pinned_tariff.as_ref(),
                served_family,
                priced_ts,
                compiled,
                tariff_book::as_gemini,
            ) {
                tariff_book::ChargeBase::Compiled(prices) => (prices, None),
                tariff_book::ChargeBase::Override(prices, schedule_id) => {
                    (prices, Some(schedule_id))
                }
                tariff_book::ChargeBase::MissingPinned => {
                    elog::error(
                        "gemini-billing",
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
                    // The exact override rate is part of the admitted economic fact. If its
                    // pinned version vanished, neither customer money nor provider-capacity
                    // calibration may silently fall back to a compiled rate from another epoch.
                    return None;
                }
            };
            let (charge, event) = settled_charge_or_hold_with_prices(
                model,
                usage,
                reservation.hold,
                reservation.mult_bp,
                priced_ts,
                prices,
            );
            let settlement = match (reservation.request_fact.take(), terminal) {
                (Some(context), Some(terminal)) => {
                    let evidence = context.terminal_evidence(
                        terminal.http_status_code,
                        terminal.provider_terminal_class,
                        terminal.delivery_state,
                        terminal.downstream_disconnect,
                        terminal.attempts_exhaustive,
                        terminal.tool_calls_in_output,
                    );
                    reservation.billing.settle_detached_with_request_fact(
                        &reservation.request_id,
                        &reservation.account_id,
                        &reservation.key,
                        reservation.hold,
                        charge,
                        None,
                        event,
                        evidence,
                    )
                }
                (Some(context), None) => {
                    let evidence = context.terminal_evidence(
                        None,
                        ProviderTerminalClass::Unknown,
                        DeliveryState::Unknown,
                        None,
                        false,
                        None,
                    );
                    reservation.billing.settle_detached_with_request_fact(
                        &reservation.request_id,
                        &reservation.account_id,
                        &reservation.key,
                        reservation.hold,
                        charge,
                        None,
                        event,
                        evidence,
                    )
                }
                (None, _) => {
                    reservation.billing.settle_detached(
                        &reservation.request_id,
                        &reservation.account_id,
                        &reservation.key,
                        reservation.hold,
                        charge,
                        None,
                        event,
                    );
                    Ok(())
                }
            };
            if let Err(error) = settlement {
                elog::error(
                    "gemini-billing",
                    format!("Gemini request-fact settlement rejected: {error:#}"),
                );
            }
            reservation.guard.disarm();
            if charge > 0 {
                elog::info(
                    "gemini-billing",
                    format!(
                        "Gemini request: −{} [{}]",
                        metering::nano_to_usd_string(charge as i128),
                        model.id
                    ),
                );
            }
            return usage.filter(|usage| !usage.is_zero()).and_then(|usage| {
                gemini_calibration_event_with_prices(
                    &self.calibration_request_id,
                    profile_id,
                    model,
                    usage,
                    priced_ts,
                    completed_at,
                    prices,
                    override_schedule_id,
                )
            });
        }
        // Admin/unmetered turns carry no pin; capacity evidence still prices the official
        // replacement cost from the book's current override, exactly like a cross-family reprice.
        usage.filter(|usage| !usage.is_zero()).and_then(|usage| {
            let compiled =
                metering::gemini_prices_at(&model.id, completed_at).unwrap_or(model.prices);
            let (prices, schedule_id) =
                match metering::gemini_matched_tariff_at(&model.id, completed_at) {
                    Some((family, compiled)) => {
                        match tariff_book::snapshot().resolve(family, completed_at) {
                            Some((pin, payload)) => match tariff_book::as_gemini(&payload) {
                                Some(prices) => (prices, Some(pin.schedule_id)),
                                None => (compiled, None),
                            },
                            None => (compiled, None),
                        }
                    }
                    None => (compiled, None),
                };
            gemini_calibration_event_with_prices(
                &self.calibration_request_id,
                profile_id,
                model,
                usage,
                completed_at,
                completed_at,
                prices,
                schedule_id,
            )
        })
    }
}

impl Drop for GeminiAdmission {
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
                "gemini-billing",
                format!("Gemini request-fact cancellation evidence rejected: {error:#}"),
            );
        }
        reservation.guard.disarm();
    }
}

#[cfg(test)]
fn gemini_calibration_event(
    request_id: &str,
    profile_id: &str,
    model: &GeminiModel,
    usage: &metering::GeminiUsage,
    now: i64,
) -> Option<registry::ProviderTurnCalibrationEvent> {
    let prices = metering::gemini_prices_at(&model.id, now).unwrap_or(model.prices);
    gemini_calibration_event_with_prices(
        request_id, profile_id, model, usage, now, now, prices, None,
    )
}

/// The calibration event under one explicit rate card and tariff identity: the hot override
/// vector admission pinned (or the book's current resolution for unmetered turns), with the
/// compiled schedule id preserved when no override applies.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gemini_calibration_event_with_prices(
    request_id: &str,
    profile_id: &str,
    model: &GeminiModel,
    usage: &metering::GeminiUsage,
    priced_ts: i64,
    completed_at: i64,
    prices: metering::GeminiPrices,
    override_schedule_id: Option<String>,
) -> Option<registry::ProviderTurnCalibrationEvent> {
    if request_id.is_empty() || profile_id.is_empty() || usage.is_zero() {
        return None;
    }
    let prompt_tokens = usage
        .input_tokens
        .saturating_add(usage.audio_input_tokens)
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cached_audio_input_tokens);
    let long = prompt_tokens > prices.long_context_threshold;
    let (input_rate, audio_rate, cache_rate, cached_audio_rate, output_rate) = if long {
        (
            prices.long_input,
            prices.long_audio_input,
            prices.long_cached_input,
            prices.long_cached_audio_input,
            prices.long_output,
        )
    } else {
        (
            prices.input,
            prices.audio_input,
            prices.cached_input,
            prices.cached_audio_input,
            prices.output,
        )
    };
    let cost = |tokens: u64, rate: i128| i64::try_from(i128::from(tokens).checked_mul(rate)?).ok();
    let api_input_nanousd = cost(usage.input_tokens, input_rate)?;
    let api_audio_input_nanousd = cost(usage.audio_input_tokens, audio_rate)?;
    let api_cache_read_nanousd = cost(usage.cached_input_tokens, cache_rate)?;
    let api_cached_audio_input_nanousd = cost(usage.cached_audio_input_tokens, cached_audio_rate)?;
    let api_output_nanousd = cost(usage.output_tokens, output_rate)?;
    let api_image_output_nanousd = cost(usage.image_output_tokens, prices.image_output)?;
    let api_search_nanousd = i64::try_from(match prices.search {
        metering::GeminiSearchBilling::PerQuery { nano } => {
            i128::from(usage.search_queries).checked_mul(nano)?
        }
        metering::GeminiSearchBilling::PerGroundedPrompt { nano } => {
            i128::from(usage.grounded_search_prompts).checked_mul(nano)?
        }
    })
    .ok()?;
    let api_total_nanousd = [
        api_input_nanousd,
        api_audio_input_nanousd,
        api_cache_read_nanousd,
        api_cached_audio_input_nanousd,
        api_output_nanousd,
        api_image_output_nanousd,
        api_search_nanousd,
    ]
    .into_iter()
    .try_fold(0i64, i64::checked_add)?;
    if api_total_nanousd <= 0 {
        return None;
    }
    Some(registry::ProviderTurnCalibrationEvent {
        provider: registry::PROVIDER_GOOGLE.to_owned(),
        request_id: request_id.to_owned(),
        subject_id: profile_id.to_owned(),
        model_id: model.id.clone(),
        service_tier: "standard".to_owned(),
        inference_geo: "global".to_owned(),
        tariff_schedule_id: override_schedule_id
            .unwrap_or_else(|| metering::gemini::TARIFF_SCHEDULE_ID.to_owned()),
        priced_ts,
        completed_at,
        input_tokens: i64::try_from(usage.input_tokens).ok()?,
        audio_input_tokens: i64::try_from(usage.audio_input_tokens).ok()?,
        cache_read_tokens: i64::try_from(
            usage
                .cached_input_tokens
                .checked_add(usage.cached_audio_input_tokens)?,
        )
        .ok()?,
        cached_audio_input_tokens: i64::try_from(usage.cached_audio_input_tokens).ok()?,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: i64::try_from(usage.output_tokens).ok()?,
        thinking_output_tokens: i64::try_from(usage.thinking_output_tokens).ok()?,
        image_output_tokens: i64::try_from(usage.image_output_tokens).ok()?,
        tool_prompt_tokens: i64::try_from(usage.tool_prompt_tokens).ok()?,
        search_queries: i64::try_from(usage.search_queries).ok()?,
        grounded_search_prompts: i64::try_from(usage.grounded_search_prompts).ok()?,
        api_input_nanousd,
        api_audio_input_nanousd,
        api_cache_read_nanousd,
        api_cached_audio_input_nanousd,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd,
        api_image_output_nanousd,
        api_search_nanousd,
        api_total_nanousd,
    })
}

#[cfg(test)]
fn settled_charge_or_hold(
    model: &GeminiModel,
    usage: Option<&metering::GeminiUsage>,
    hold: i64,
    mult_bp: i64,
    now: i64,
) -> (i64, Option<registry::UsageEventInput>) {
    let prices = metering::gemini_prices_at(&model.id, now).unwrap_or(model.prices);
    settled_charge_or_hold_with_prices(model, usage, hold, mult_bp, now, prices)
}

/// `settled_charge_or_hold` under one explicit rate card: settlement replays the exact override
/// version admission pinned, or the compiled vector when no override applies.
#[allow(clippy::too_many_arguments)]
fn settled_charge_or_hold_with_prices(
    model: &GeminiModel,
    usage: Option<&metering::GeminiUsage>,
    hold: i64,
    mult_bp: i64,
    now: i64,
    prices: metering::GeminiPrices,
) -> (i64, Option<registry::UsageEventInput>) {
    match usage.filter(|usage| !usage.is_zero()) {
        Some(usage) => settled_charge_with_prices(model, usage, hold, mult_bp, now, prices),
        // Usage never arrived. The preflight hold is an admission device, not a price — billing it
        // charged a double-digit multiple of the real turn — so this settles at nothing unless an
        // operator has deliberately re-armed the conservative fallback. No synthetic token event is
        // invented either way: that would corrupt authoritative analytics.
        None => (crate::settlement_policy::unknown_usage_charge(hold), None),
    }
}

fn exactly_one_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a HeaderValue>, AdmissionError> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (None, None) => Ok(None),
        (Some(value), None) => Ok(Some(value)),
        (Some(_), Some(_)) => Err(AdmissionError::Unavailable),
        (None, Some(_)) => unreachable!("HeaderMap iterator cannot skip its first value"),
    }
}

fn parse_calibration_not_after(value: &HeaderValue) -> Result<u64, AdmissionError> {
    let value = value.to_str().map_err(|_| AdmissionError::Unavailable)?;
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(AdmissionError::Unavailable);
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|value| (1..=MAX_CALIBRATION_NOT_AFTER).contains(value))
        .ok_or(AdmissionError::Unavailable)
}

pub(crate) async fn begin_admission(
    app: &AppState,
    headers: &HeaderMap,
    peer: &SocketAddr,
) -> Result<PendingGeminiAdmission, AdmissionError> {
    if !app.authority_ready.load(Ordering::Acquire) {
        return Err(AdmissionError::Unavailable);
    }
    let execution = crate::execution::parse_execution_attempt(headers).map_err(|error| {
        elog::error(
            "gemini-billing",
            format!(
                "Gemini execution identity rejected class={}",
                error.as_str()
            ),
        );
        AdmissionError::Unavailable
    })?;
    let authz = authorize(app, headers, peer).await;
    match &authz {
        Authz::Admin { .. } => {}
        // Model-aware release resolution decides whether this is a balance account or a
        // zero-balance service meter. Legacy strict Gemini remains rejected in reserve().
        Authz::Metered { .. } => {}
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return Err(AdmissionError::Unauthorized);
        }
        Authz::Unavailable => return Err(AdmissionError::Unavailable),
    }
    Metrics::inc(&app.metrics.requests);

    let calibration_profile = exactly_one_header(headers, CALIBRATION_PROFILE_HEADER)?;
    let calibration_request_id_header = exactly_one_header(headers, CALIBRATION_REQUEST_ID_HEADER)?;
    let calibration_not_after_header = exactly_one_header(headers, CALIBRATION_NOT_AFTER_HEADER)?;
    let any_calibration_header = calibration_profile.is_some()
        || calibration_request_id_header.is_some()
        || calibration_not_after_header.is_some();
    if any_calibration_header && !matches!(authz, Authz::Admin { .. }) {
        return Err(AdmissionError::Unavailable);
    }
    let calibration_target = calibration_profile
        .map(|profile| {
            let profile = profile.to_str().map_err(|_| AdmissionError::Unavailable)?;
            gemini_credential::validate_profile_id(profile)
                .map_err(|_| AdmissionError::Unavailable)?;
            Ok(profile.to_owned())
        })
        .transpose()?;
    let calibration_request_id = match (calibration_target.as_ref(), calibration_request_id_header)
    {
        (None, None) => crate::upstream::fresh_request_id(),
        // Preserve the established admin exact-profile contract. Only the new deadline-bound lane
        // requires its caller to supply the canonical request identity explicitly.
        (Some(_), None) if calibration_not_after_header.is_none() => {
            crate::upstream::fresh_request_id()
        }
        (Some(_), Some(request_id)) => {
            let value = request_id
                .to_str()
                .map_err(|_| AdmissionError::Unavailable)?;
            EnginePricingRequestId::from_engine_uuid_v4(value)
                .map(|value| value.as_str().to_owned())
                .ok_or(AdmissionError::Unavailable)?
        }
        (None, Some(_)) | (Some(_), None) => return Err(AdmissionError::Unavailable),
    };
    let calibration_not_after = match calibration_not_after_header {
        None => None,
        Some(value)
            if matches!(authz, Authz::Admin { .. })
                && calibration_target.is_some()
                && calibration_request_id_header.is_some() =>
        {
            Some(parse_calibration_not_after(value)?)
        }
        Some(_) => return Err(AdmissionError::Unavailable),
    };

    Ok(PendingGeminiAdmission {
        authz,
        execution,
        calibration_request_id,
        calibration_target,
        calibration_not_after,
    })
}

pub(super) fn reserve_cost_with_prices(
    prices: metering::GeminiPrices,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
) -> i128 {
    let input_rate = [
        prices.input,
        prices.audio_input,
        prices.cached_input,
        prices.cached_audio_input,
        prices.long_input,
        prices.long_audio_input,
        prices.long_cached_input,
        prices.long_cached_audio_input,
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let output_rate = prices.output.max(prices.long_output);
    let image_output_rate = prices.image_output;
    let grounding = if grounding_enabled {
        match prices.search {
            // Reserve a deliberately conservative bounded query fanout. Final settlement remains
            // authoritative and the account overdraft floor bounds any larger provider fanout.
            metering::GeminiSearchBilling::PerQuery { nano } => nano.saturating_mul(32),
            metering::GeminiSearchBilling::PerGroundedPrompt { nano } => nano,
        }
    } else {
        0
    };
    (estimated_input_tokens as i128)
        .saturating_mul(input_rate)
        .saturating_add((requested_output_tokens as i128).saturating_mul(output_rate))
        .saturating_add((image_output_tokens as i128).saturating_mul(image_output_rate))
        .saturating_add(grounding)
}

#[cfg(test)]
fn reserve_cost(
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
) -> i128 {
    let prices = metering::gemini_prices_at(&model.id, pool::now()).unwrap_or(model.prices);
    reserve_cost_with_prices(
        prices,
        estimated_input_tokens,
        requested_output_tokens,
        image_output_tokens,
        grounding_enabled,
    )
}

/// Fit the requested output ceiling into the customer's currently available balance. Text routes
/// can write a lower `generationConfig.maxOutputTokens` before upstream. Image generation has a
/// fixed media-output component and is admitted only when the full request fits, because silently
/// lowering image size would change the requested product.
#[cfg(test)]
fn reservation_for_budget(
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
    allow_output_cap: bool,
    mult_bp: i64,
    available_nano: i64,
) -> Option<(u64, i64)> {
    let prices = metering::gemini_prices_at(&model.id, pool::now()).unwrap_or(model.prices);
    reservation_for_budget_with_prices(
        prices,
        estimated_input_tokens,
        requested_output_tokens,
        image_output_tokens,
        grounding_enabled,
        allow_output_cap,
        mult_bp,
        available_nano,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn reservation_for_budget_with_prices(
    prices: metering::GeminiPrices,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
    allow_output_cap: bool,
    mult_bp: i64,
    available_nano: i64,
) -> Option<(u64, i64)> {
    let requested = requested_output_tokens.max(1);
    if mult_bp <= 0 {
        return Some((requested, 0));
    }
    let budget = available_nano.max(0) as i128;
    if budget == 0 {
        return None;
    }
    let cost = |output_tokens| {
        metering::apply_multiplier(
            reserve_cost_with_prices(
                prices,
                estimated_input_tokens,
                output_tokens,
                image_output_tokens,
                grounding_enabled,
            ),
            mult_bp,
        )
        .clamp(1, i64::MAX as i128)
    };
    if cost(1) > budget {
        return None;
    }
    if !allow_output_cap {
        let hold = cost(requested);
        return (hold <= budget).then_some((requested, hold as i64));
    }
    let mut low = 1u64;
    let mut high = requested;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if cost(middle) <= budget {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let hold = cost(low) as i64;
    Some((low, hold))
}

#[allow(clippy::too_many_arguments)]
async fn reserve_gemini_legacy(
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
    allow_output_cap: bool,
    mult_bp: i64,
    available_nano: i64,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
    billable_fact: Option<(GeminiRequestFactSeed, GeminiBillableRequestSpec)>,
) -> Result<GeminiReserveResult, AdmissionError> {
    reserve_gemini_legacy_at(
        billing,
        account_id,
        key,
        model,
        estimated_input_tokens,
        requested_output_tokens,
        image_output_tokens,
        grounding_enabled,
        allow_output_cap,
        mult_bp,
        available_nano,
        request_id,
        execution,
        pool::now(),
        billable_fact,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn reserve_gemini_legacy_at(
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
    allow_output_cap: bool,
    mult_bp: i64,
    available_nano: i64,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
    priced_ts: i64,
    billable_fact: Option<(GeminiRequestFactSeed, GeminiBillableRequestSpec)>,
) -> Result<GeminiReserveResult, AdmissionError> {
    // Hot tariff override: the matched catalog family resolves against the process-wide book; an
    // override replaces only the base vector (the conservative maximum/long-context selection
    // stays code-applied on top) and pins `<family>/v<version>` for settlement.
    let compiled = metering::gemini_prices_at(&model.id, priced_ts).unwrap_or(model.prices);
    let resolved = match metering::gemini_matched_tariff_at(&model.id, priced_ts) {
        Some((family, _)) => tariff_book::reserve_base(
            &tariff_book::snapshot(),
            family,
            priced_ts,
            compiled,
            tariff_book::as_gemini,
        ),
        None => tariff_book::ReserveBase {
            prices: compiled,
            pin: None,
        },
    };
    let Some((effective_output_tokens, hold)) = reservation_for_budget_with_prices(
        resolved.prices,
        estimated_input_tokens,
        requested_output_tokens,
        image_output_tokens,
        grounding_enabled,
        allow_output_cap,
        mult_bp,
        available_nano,
    ) else {
        return Err(AdmissionError::LowBalance);
    };
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
                    registry::PROVIDER_GOOGLE,
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
                    registry::PROVIDER_GOOGLE,
                    mult_bp,
                )
                .await
        }
    };
    match reserved {
        Ok(Some(_)) => Ok((
            effective_output_tokens,
            hold,
            mult_bp,
            priced_ts,
            resolved.pin,
            request_fact_context,
        )),
        Ok(None) => Err(AdmissionError::LowBalance),
        Err(error) => {
            elog::error(
                "gemini-billing",
                format!("Gemini billing reservation failed: {error:#}"),
            );
            Err(AdmissionError::Unavailable)
        }
    }
}

#[cfg(test)]
fn settled_charge(
    model: &GeminiModel,
    usage: &metering::GeminiUsage,
    hold: i64,
    mult_bp: i64,
    now: i64,
) -> (i64, Option<registry::UsageEventInput>) {
    let prices = metering::gemini_prices_at(&model.id, now).unwrap_or(model.prices);
    settled_charge_with_prices(model, usage, hold, mult_bp, now, prices)
}

/// `settled_charge` under one explicit rate card.
#[allow(clippy::too_many_arguments)]
pub(crate) fn settled_charge_with_prices(
    model: &GeminiModel,
    usage: &metering::GeminiUsage,
    _hold: i64,
    mult_bp: i64,
    now: i64,
    prices: metering::GeminiPrices,
) -> (i64, Option<registry::UsageEventInput>) {
    let real = metering::gemini::cost_nanodollars(usage, &prices);
    let computed = metering::apply_multiplier(real, mult_bp);
    let charge = computed.clamp(0, i64::MAX as i128) as i64;
    if real <= 0 {
        return (charge, None);
    }

    let prompt_tokens = usage
        .input_tokens
        .saturating_add(usage.audio_input_tokens)
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cached_audio_input_tokens);
    let long = prompt_tokens > prices.long_context_threshold;
    let (input_rate, audio_rate, cached_rate, cached_audio_rate, output_rate) = if long {
        (
            prices.long_input,
            prices.long_audio_input,
            prices.long_cached_input,
            prices.long_cached_audio_input,
            prices.long_output,
        )
    } else {
        (
            prices.input,
            prices.audio_input,
            prices.cached_input,
            prices.cached_audio_input,
            prices.output,
        )
    };
    let input_nano = (usage.input_tokens as i128)
        .saturating_mul(input_rate)
        .saturating_add((usage.audio_input_tokens as i128).saturating_mul(audio_rate));
    let cache_nano = (usage.cached_input_tokens as i128)
        .saturating_mul(cached_rate)
        .saturating_add(
            (usage.cached_audio_input_tokens as i128).saturating_mul(cached_audio_rate),
        );
    let output_nano = (usage.output_tokens as i128)
        .saturating_mul(output_rate)
        .saturating_add((usage.image_output_tokens as i128).saturating_mul(prices.image_output));
    let (search_requests, search_nano) = match prices.search {
        metering::GeminiSearchBilling::PerQuery { nano } => (
            usage.search_queries,
            (usage.search_queries as i128).saturating_mul(nano),
        ),
        metering::GeminiSearchBilling::PerGroundedPrompt { nano } => (
            usage.grounded_search_prompts,
            (usage.grounded_search_prompts as i128).saturating_mul(nano),
        ),
    };
    let clamp = |value: i128| value.clamp(0, i64::MAX as i128) as i64;
    let event = registry::UsageEventInput {
        model: model.id.clone(),
        provider: registry::PROVIDER_GOOGLE.to_string(),
        input_tokens: usage
            .input_tokens
            .saturating_add(usage.audio_input_tokens)
            .min(i64::MAX as u64) as i64,
        output_tokens: usage
            .output_tokens
            .saturating_add(usage.image_output_tokens)
            .min(i64::MAX as u64) as i64,
        cache_read_tokens: usage
            .cached_input_tokens
            .saturating_add(usage.cached_audio_input_tokens)
            .min(i64::MAX as u64) as i64,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        web_search_requests: search_requests.min(i64::MAX as u64) as i64,
        real_nano: clamp(real),
        charge_basis_nano: clamp(real),
        speed: "standard".to_string(),
        inference_geo: String::new(),
        input_nano: clamp(input_nano),
        output_nano: clamp(output_nano),
        cache_read_nano: clamp(cache_nano),
        cache_write_5m_nano: 0,
        cache_write_1h_nano: 0,
        web_search_nano: clamp(search_nano),
        priced_ts: now,
    };
    (charge, Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AffinityStore, Breaker, Clients, ProviderMode, ProxyConfig};
    use pool::{Pool, Reserve};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn billable_terminal_evidence_keeps_exact_attempts_and_delivery_classes() {
        for (attempts, provider, delivery) in [
            (
                0,
                ProviderTerminalClass::Transport,
                DeliveryState::NotStarted,
            ),
            (
                1,
                ProviderTerminalClass::ClientError,
                DeliveryState::Interrupted,
            ),
            (
                2,
                ProviderTerminalClass::UpstreamError,
                DeliveryState::Interrupted,
            ),
            (
                3,
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
            ),
            (4, ProviderTerminalClass::Success, DeliveryState::Completed),
        ] {
            let observer = super::super::ActualSendObserver::default();
            for _ in 0..attempts {
                observer.record().await;
            }
            let context = GeminiBillableFactContext {
                admitted_at: pool::now(),
                lifecycle_clock: RequestLifecycleClock::default(),
                actual_sends: observer,
                downstream_disconnect: Arc::new(AtomicBool::new(false)),
            };
            let evidence = context.terminal_evidence(
                Some(if provider == ProviderTerminalClass::Success {
                    200
                } else {
                    503
                }),
                provider,
                delivery,
                None,
                true,
                Some(provider == ProviderTerminalClass::Success),
            );
            assert_eq!(evidence.internal_attempt_count, Some(attempts));
            assert_eq!(evidence.provider_terminal_class, provider);
            assert_eq!(evidence.delivery_state, delivery);
        }
    }

    #[test]
    fn cancellation_terminal_evidence_keeps_attempt_count_unknown() {
        let context = GeminiBillableFactContext {
            admitted_at: pool::now(),
            lifecycle_clock: RequestLifecycleClock::default(),
            actual_sends: super::super::ActualSendObserver::default(),
            downstream_disconnect: Arc::new(AtomicBool::new(false)),
        };
        let evidence = context.terminal_evidence(
            None,
            ProviderTerminalClass::Unknown,
            DeliveryState::Unknown,
            None,
            false,
            None,
        );
        assert_eq!(evidence.internal_attempt_count, None);
        assert_eq!(evidence.http_status_code, None);
    }

    #[tokio::test]
    async fn sqlite_money_reservation_never_owns_a_generation_fact() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gemini-generation-fact-sqlite-{}-{unique}.sqlite",
            std::process::id()
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing =
            Arc::new(crate::billing::AsyncBilling::start(path_string.clone(), 1).unwrap());
        billing
            .create_account("gemini-fact-sqlite-account", None, 10_000)
            .await
            .unwrap();
        billing
            .topup("gemini-fact-sqlite-account", 1_000_000, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key(
                "gemini-fact-sqlite-key",
                "gemini-fact-sqlite-account",
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let app = app_state(Arc::clone(&billing), &path_string);
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            crate::execution::LOGICAL_REQUEST_ID_HEADER,
            "123e4567-e89b-42d3-a456-426614174084".parse().unwrap(),
        );
        let logical = crate::execution::admit_logical_request_id(&mut headers).unwrap();
        let lifecycle = RequestLifecycleClock::default();
        let pending = PendingGeminiAdmission {
            authz: Authz::Metered {
                account_id: "gemini-fact-sqlite-account".to_owned(),
                key_id: "key_123e4567e89b42d3a456426614174084".to_owned(),
                key: "gemini-fact-sqlite-key".to_owned(),
                available_nano: 1_000_000,
                mult_bp: 10_000,
                provider_mult_bp: Vec::new(),
            },
            execution: registry::ExecutionAttempt::direct(),
            calibration_request_id: "123e4567-e89b-42d3-a456-426614174085".to_owned(),
            calibration_target: None,
            calibration_not_after: None,
        };
        let seed = pending
            .request_fact_seed(Some(&logical), None, Some(&lifecycle), pool::now())
            .unwrap();
        let spec = GeminiBillableRequestSpec::native(
            Some("gemini-2.5-flash".to_owned()),
            Some("gemini-2.5-flash".to_owned()),
            false,
            RequestClassification::default(),
        );
        let (admission, _) = pending
            .reserve(&app, &model(), 1, 1, 0, false, true, Some((seed, spec)))
            .await
            .unwrap();
        assert!(admission.actual_send_observer().is_none());
        let _ = admission.settle(&model(), None, "profile-sqlite");
        billing.flush().await.unwrap();
        let connection = registry::open(&path_string).unwrap();
        assert!(!connection
            .prepare("SELECT count(*) FROM request_facts")
            .is_ok());
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn admin_and_missing_context_do_not_create_billable_fact_seeds() {
        let admin = PendingGeminiAdmission {
            authz: Authz::Admin {
                affinity_scope: "admin".to_owned(),
            },
            execution: registry::ExecutionAttempt::direct(),
            calibration_request_id: "admin-request".to_owned(),
            calibration_target: None,
            calibration_not_after: None,
        };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            crate::execution::LOGICAL_REQUEST_ID_HEADER,
            "123e4567-e89b-42d3-a456-426614174079".parse().unwrap(),
        );
        let logical = crate::execution::admit_logical_request_id(&mut headers).unwrap();
        let lifecycle = RequestLifecycleClock::default();
        assert!(admin
            .request_fact_seed(Some(&logical), None, Some(&lifecycle), pool::now())
            .is_none());
        let metered = PendingGeminiAdmission {
            authz: Authz::Metered {
                account_id: "gemini-fact-test-account".to_owned(),
                key_id: "key_123e4567e89b42d3a456426614174079".to_owned(),
                key: "gemini-fact-test-key".to_owned(),
                available_nano: 1_000_000,
                mult_bp: 10_000,
                provider_mult_bp: Vec::new(),
            },
            execution: registry::ExecutionAttempt::direct(),
            calibration_request_id: "metered-request".to_owned(),
            calibration_target: None,
            calibration_not_after: None,
        };
        assert!(metered
            .request_fact_seed(None, None, Some(&lifecycle), pool::now())
            .is_none());
        assert!(metered
            .request_fact_seed(Some(&logical), None, None, pool::now())
            .is_none());
        let seed = metered
            .request_fact_seed(Some(&logical), None, Some(&lifecycle), pool::now())
            .expect("client attribution is optional and normalizes to unknown");
        assert_eq!(
            seed.client_attribution.kind(),
            registry::request_facts::ClientKind::Unknown
        );
    }

    fn proxy_config() -> Arc<ProxyConfig> {
        Arc::new(ProxyConfig {
            api_keys: Vec::new(),
            control_keys: Vec::new(),
            panel_keys: Vec::new(),
            default_mult_bp: 10_000,
            trust_loopback: false,
            upstream: "http://127.0.0.1:1".to_string(),
            claudestore_fallback: None,
            max_tries: 1,
            util_cap: 1.0,
            cool_secs: 1,
            smooth_wait_ms: 0,
            poll: false,
            inject_identity: false,
            identity: String::new(),
            inject_billing: false,
            cc_version: String::new(),
            cc_entrypoint: String::new(),
            default_beta: String::new(),
            user_agent: "gemini-strict-test".to_string(),
            user_agents: Vec::new(),
            anthropic_version: String::new(),
            connect_timeout: 1,
            read_timeout: 120,
            nonstream_read_timeout: 1800,
            x_app: String::new(),
            stainless_lang: String::new(),
            stainless_runtime: String::new(),
            stainless_runtime_version: String::new(),
            stainless_package_version: String::new(),
            stainless_os: String::new(),
            stainless_arch: String::new(),
        })
    }

    fn app_state(billing: Arc<crate::billing::AsyncBilling>, path: &str) -> AppState {
        let cfg = proxy_config();
        AppState {
            provider: ProviderMode::Gemini,
            authority: Arc::new(registry::authority::AuthorityConfig::new(
                path.to_string(),
                None,
            )),
            data_db_path: Arc::new(path.to_string()),
            pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
            affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
            clients: Arc::new(Clients::new(&cfg)),
            body_storage: None,
            codex: None,
            gemini: None,
            gemini_batch: None,
            gemini_batch_runtime: None,
            kimi: None,
            glm: None,
            tripo3d: None,
            suno: None,
            billing: Some(billing),
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(Breaker::new(1)),
            metrics: Arc::new(Metrics::new()),
            probe_poke: None,
            admin_changes: tokio::sync::broadcast::channel(16).0,
            cfg,
        }
    }

    fn model() -> GeminiModel {
        let spec = metering::gemini_catalog_at(0)
            .into_iter()
            .find(|spec| spec.id == "gemini-2.5-flash")
            .unwrap();
        GeminiModel {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            created: 1,
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        }
    }

    fn image_model() -> GeminiModel {
        let spec = metering::gemini_catalog_at(0)
            .into_iter()
            .find(|spec| spec.id == "gemini-3.1-flash-image")
            .unwrap();
        GeminiModel {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            created: 1,
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        }
    }

    fn gemini_36_model_at(at: i64) -> GeminiModel {
        let spec = metering::gemini_catalog_at(at)
            .into_iter()
            .find(|spec| spec.id == "gemini-3.6-flash")
            .unwrap();
        GeminiModel {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            created: 1,
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        }
    }

    #[tokio::test]
    async fn dormant_release_keeps_the_gemini_zero_balance_gate() {
        const ACCOUNT: &str = "gemini-zero-balance";
        const KEY: &str = "gemini-zero-balance-key";
        const REQUEST_ID: &str = "428f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-zero-balance-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing =
            Arc::new(crate::billing::AsyncBilling::start(path_string.clone(), 1).unwrap());
        billing.create_account(ACCOUNT, None, 10_000).await.unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        let app = app_state(Arc::clone(&billing), &path_string);

        let result = reserve_gemini_metered(
            &billing,
            ACCOUNT,
            KEY,
            &model(),
            100,
            10,
            0,
            false,
            true,
            10_000,
            0,
            REQUEST_ID,
            &registry::ExecutionAttempt::direct(),
            None,
        )
        .await;
        assert!(matches!(result, Err(AdmissionError::LowBalance)));
        let account = billing.account(ACCOUNT).await.unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (0, 0));
        let connection = registry::open(&path_string).unwrap();
        let reservations: i64 = connection
            .query_row("SELECT COUNT(*) FROM billing_reservations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(reservations, 0);

        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn deadline_calibration_requires_a_canonical_pair_without_breaking_legacy_exact() {
        const ADMIN_KEY: &str = "gemini-calibration-admin";
        const REQUEST_ID: &str = "123e4567-e89b-42d3-a456-426614174000";
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-calibration-request-id-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing =
            Arc::new(crate::billing::AsyncBilling::start(path_string.clone(), 1).unwrap());
        let mut app = app_state(Arc::clone(&billing), &path_string);
        Arc::make_mut(&mut app.cfg).api_keys = vec![ADMIN_KEY.to_owned()];
        let peer = "198.51.100.10:12345".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-goog-api-key", ADMIN_KEY.parse().unwrap());
        headers.insert(CALIBRATION_PROFILE_HEADER, "profile_a".parse().unwrap());
        headers.insert(CALIBRATION_REQUEST_ID_HEADER, REQUEST_ID.parse().unwrap());
        let not_after = (pool::now() + 60) as u64;
        headers.insert(
            CALIBRATION_NOT_AFTER_HEADER,
            not_after.to_string().parse().unwrap(),
        );

        let pending = begin_admission(&app, &headers, &peer).await.unwrap();
        assert_eq!(pending.calibration_target(), Some("profile_a"));
        assert_eq!(pending.calibration_request_id, REQUEST_ID);
        assert_eq!(pending.calibration_not_after(), Some(not_after));

        let mut legacy = HeaderMap::new();
        legacy.insert("x-goog-api-key", ADMIN_KEY.parse().unwrap());
        legacy.insert(CALIBRATION_PROFILE_HEADER, "profile_a".parse().unwrap());
        let pending = begin_admission(&app, &legacy, &peer).await.unwrap();
        assert_eq!(pending.calibration_target(), Some("profile_a"));
        assert!(
            EnginePricingRequestId::from_engine_uuid_v4(&pending.calibration_request_id).is_some()
        );
        assert_eq!(pending.calibration_not_after(), None);

        let mut request_without_profile = HeaderMap::new();
        request_without_profile.insert("x-goog-api-key", ADMIN_KEY.parse().unwrap());
        request_without_profile.insert(CALIBRATION_REQUEST_ID_HEADER, REQUEST_ID.parse().unwrap());
        assert!(matches!(
            begin_admission(&app, &request_without_profile, &peer).await,
            Err(AdmissionError::Unavailable)
        ));

        headers.insert(
            CALIBRATION_REQUEST_ID_HEADER,
            "123e4567-e89b-12d3-a456-426614174000".parse().unwrap(),
        );
        assert!(matches!(
            begin_admission(&app, &headers, &peer).await,
            Err(AdmissionError::Unavailable)
        ));

        headers.insert(CALIBRATION_REQUEST_ID_HEADER, REQUEST_ID.parse().unwrap());
        for invalid in ["0", "01", "+1", "9007199254741"] {
            headers.insert(CALIBRATION_NOT_AFTER_HEADER, invalid.parse().unwrap());
            assert!(matches!(
                begin_admission(&app, &headers, &peer).await,
                Err(AdmissionError::Unavailable)
            ));
        }

        headers.insert(
            CALIBRATION_NOT_AFTER_HEADER,
            (pool::now() + 60).to_string().parse().unwrap(),
        );
        for missing in [CALIBRATION_PROFILE_HEADER, CALIBRATION_REQUEST_ID_HEADER] {
            let mut incomplete = headers.clone();
            incomplete.remove(missing);
            assert!(matches!(
                begin_admission(&app, &incomplete, &peer).await,
                Err(AdmissionError::Unavailable)
            ));
        }

        let mut duplicate = headers.clone();
        duplicate.append(CALIBRATION_NOT_AFTER_HEADER, "123".parse().unwrap());
        assert!(matches!(
            begin_admission(&app, &duplicate, &peer).await,
            Err(AdmissionError::Unavailable)
        ));

        const METERED_ACCOUNT: &str = "gemini-calibration-metered";
        const METERED_KEY: &str = "gemini-calibration-metered-key";
        billing
            .create_account(METERED_ACCOUNT, None, 10_000)
            .await
            .unwrap();
        billing
            .topup(METERED_ACCOUNT, 1_000_000, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key(METERED_KEY, METERED_ACCOUNT, None, None, None)
            .await
            .unwrap();
        let mut metered_headers = headers.clone();
        metered_headers.insert("x-goog-api-key", METERED_KEY.parse().unwrap());
        assert!(matches!(
            begin_admission(&app, &metered_headers, &peer).await,
            Err(AdmissionError::Unavailable)
        ));

        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settlement_maps_google_usage_without_losing_audio_cache_thought_or_search() {
        let usage = metering::GeminiUsage {
            input_tokens: 100,
            tool_prompt_tokens: 5,
            audio_input_tokens: 20,
            cached_input_tokens: 50,
            cached_audio_input_tokens: 10,
            output_tokens: 30,
            thinking_output_tokens: 10,
            image_output_tokens: 0,
            search_queries: 7,
            grounded_search_prompts: 1,
        };
        let (charge, event) = settled_charge(&model(), &usage, i64::MAX, 10_000, 123);
        let event = event.unwrap();
        assert_eq!(event.provider, registry::PROVIDER_GOOGLE);
        assert_eq!(event.input_tokens, 120);
        assert_eq!(event.cache_read_tokens, 60);
        assert_eq!(event.output_tokens, 30);
        assert_eq!(event.web_search_requests, 1);
        assert_eq!(event.input_nano, 100 * 300 + 20 * 1_000);
        assert_eq!(event.cache_read_nano, 50 * 30 + 10 * 100);
        assert_eq!(event.output_nano, 30 * 2_500);
        assert_eq!(event.web_search_nano, 35_000_000);
        assert_eq!(event.real_nano, charge);
        assert_eq!(event.priced_ts, 123);
    }

    #[test]
    fn calibration_event_preserves_every_gemini_token_and_cost_leg() {
        let usage = metering::GeminiUsage {
            input_tokens: 100,
            tool_prompt_tokens: 5,
            audio_input_tokens: 20,
            cached_input_tokens: 50,
            cached_audio_input_tokens: 10,
            output_tokens: 30,
            thinking_output_tokens: 10,
            image_output_tokens: 0,
            search_queries: 7,
            grounded_search_prompts: 1,
        };
        let event =
            gemini_calibration_event("request-1", "opaque-profile", &model(), &usage, 123).unwrap();

        assert_eq!(event.provider, registry::PROVIDER_GOOGLE);
        assert_eq!(event.subject_id, "opaque-profile");
        assert_eq!(event.input_tokens, 100);
        assert_eq!(event.audio_input_tokens, 20);
        assert_eq!(event.cache_read_tokens, 60);
        assert_eq!(event.cached_audio_input_tokens, 10);
        assert_eq!(event.output_tokens, 30);
        assert_eq!(event.thinking_output_tokens, 10);
        assert_eq!(event.tool_prompt_tokens, 5);
        assert_eq!(event.search_queries, 7);
        assert_eq!(event.grounded_search_prompts, 1);
        assert_eq!(event.api_input_nanousd, 100 * 300);
        assert_eq!(event.api_audio_input_nanousd, 20 * 1_000);
        assert_eq!(event.api_cache_read_nanousd, 50 * 30);
        assert_eq!(event.api_cached_audio_input_nanousd, 10 * 100);
        assert_eq!(event.api_output_nanousd, 30 * 2_500);
        assert_eq!(event.api_search_nanousd, 35_000_000);
        assert_eq!(
            event.api_total_nanousd,
            event.api_input_nanousd
                + event.api_audio_input_nanousd
                + event.api_cache_read_nanousd
                + event.api_cached_audio_input_nanousd
                + event.api_output_nanousd
                + event.api_search_nanousd
        );
    }

    #[test]
    fn reserve_uses_the_most_expensive_input_bucket_and_full_requested_output() {
        let model = model();
        let cost = reserve_cost(&model, 1_000, 100, 0, false);
        assert_eq!(cost, 1_000 * 1_000 + 100 * 2_500);
        assert!(reserve_cost(&model, 1_000, 100, 0, true) > cost);
    }

    #[test]
    fn reservation_caps_output_before_upstream_without_exceeding_available_balance() {
        let model = model();
        let available =
            metering::apply_multiplier(reserve_cost(&model, 1_000, 7, 0, false), 10_000) as i64;
        let (output, hold) =
            reservation_for_budget(&model, 1_000, 100, 0, false, true, 10_000, available).unwrap();
        assert_eq!(output, 7);
        assert_eq!(hold, available);

        let cannot_afford_one =
            metering::apply_multiplier(reserve_cost(&model, 1_000, 1, 0, false), 10_000) as i64 - 1;
        assert!(reservation_for_budget(
            &model,
            1_000,
            100,
            0,
            false,
            true,
            10_000,
            cannot_afford_one,
        )
        .is_none());

        assert_eq!(
            reservation_for_budget(&model, 1_000, 100, 0, false, true, 0, 0),
            Some((100, 0)),
            "a zero-multiplier service request is metered without reserving balance",
        );
    }

    #[test]
    fn image_reserve_and_settlement_use_the_official_media_output_sku() {
        let model = image_model();
        let full = reserve_cost(&model, 100, 32_768, 2_520, false);
        assert_eq!(full, 100 * 500 + 32_768 * 3_000 + 2_520 * 60_000);
        assert!(reservation_for_budget(
            &model,
            100,
            32_768,
            2_520,
            false,
            false,
            10_000,
            (full - 1) as i64,
        )
        .is_none());
        let admitted = reservation_for_budget(
            &model,
            100,
            32_768,
            2_520,
            false,
            false,
            10_000,
            full as i64,
        )
        .unwrap();
        assert_eq!(admitted, (32_768, full as i64));

        let usage = metering::GeminiUsage {
            input_tokens: 100,
            output_tokens: 20,
            image_output_tokens: 1_120,
            ..metering::GeminiUsage::default()
        };
        let (charge, event) = settled_charge(&model, &usage, i64::MAX, 10_000, 123);
        let expected = 100 * 500 + 20 * 3_000 + 1_120 * 60_000;
        let event = event.unwrap();
        assert_eq!(charge as i128, expected);
        assert_eq!(event.output_tokens, 1_140);
        assert_eq!(event.output_nano as i128, 20 * 3_000 + 1_120 * 60_000);
        assert_eq!(event.real_nano as i128, expected);
    }

    #[tokio::test]
    async fn compiled_tariff_stays_on_the_admission_epoch_across_the_cutoff() {
        const CUTOFF: i64 = 1_798_761_600;
        const ACCOUNT: &str = "gemini-epoch-boundary";
        const KEY: &str = "gemini-epoch-boundary-key";
        const REQUEST_ID: &str = "gemini-epoch-boundary-request";
        const TOPUP: i64 = 20_000_000;
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-epoch-boundary-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            crate::billing::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap(),
        );
        billing.create_account(ACCOUNT, None, 10_000).await.unwrap();
        billing.topup(ACCOUNT, TOPUP, Some("seed")).await.unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        let model = gemini_36_model_at(CUTOFF - 1);
        let usage = metering::GeminiUsage {
            input_tokens: 1_000,
            output_tokens: 100,
            ..metering::GeminiUsage::default()
        };
        let priced_ts = CUTOFF - 1;
        let completion_ts = CUTOFF;
        let (_, hold, mult_bp, reserved_priced_ts, pinned_tariff, request_fact) =
            reserve_gemini_legacy_at(
                &billing,
                ACCOUNT,
                KEY,
                &model,
                usage.input_tokens,
                usage.output_tokens,
                0,
                false,
                false,
                10_000,
                TOPUP,
                REQUEST_ID,
                &registry::ExecutionAttempt::direct(),
                priced_ts,
                None,
            )
            .await
            .unwrap();
        let promo_hold = 1_000 * 750 + 100 * 3_750;
        assert_eq!(hold, promo_hold);
        assert_eq!(mult_bp, 10_000);
        assert_eq!(reserved_priced_ts, priced_ts);
        assert!(pinned_tariff.is_none());
        assert!(request_fact.is_none());
        let admission = GeminiAdmission {
            reservation: Some(Reservation {
                billing: Arc::clone(&billing),
                account_id: ACCOUNT.to_owned(),
                key: KEY.to_owned(),
                mult_bp,
                hold,
                tariff_priced_ts: reserved_priced_ts,
                pinned_tariff,
                request_id: REQUEST_ID.to_owned(),
                request_fact: None,
                guard: HoldGuard::new(
                    Some(Arc::clone(&billing)),
                    ACCOUNT.to_owned(),
                    KEY.to_owned(),
                    hold,
                    REQUEST_ID.to_owned(),
                ),
            }),
            calibration_request_id: REQUEST_ID.to_owned(),
            exact_calibration_target: false,
            calibration_not_after: None,
        };
        let calibration_event = admission
            .settle_at(&model, Some(&usage), "profile-1", completion_ts, None)
            .unwrap();
        billing.flush().await.unwrap();
        let account = billing.account(ACCOUNT).await.unwrap().unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);

        let promo_cost = promo_hold;
        assert_eq!(account.spent_nano, promo_cost);
        assert_eq!(account.reserved_nano, 0);
        assert_eq!(calibration_event.priced_ts, priced_ts);
        assert_eq!(calibration_event.completed_at, completion_ts);
        assert_eq!(calibration_event.api_total_nanousd, promo_cost);
        assert_ne!(promo_cost, 1_000 * 1_500 + 100 * 7_500);
    }

    /// Settlement replays the exact override version pinned at admission, never the compiled
    /// card.
    #[tokio::test]
    async fn settlement_replays_the_pinned_override_version() {
        const ACCOUNT: &str = "gemini-pinned-override";
        const KEY: &str = "gemini-pinned-override-key";
        const REQUEST_ID: &str = "gemini-pinned-override-request";
        const TOPUP: i64 = 1_000_000_000;
        const HOLD: i64 = 400_000_000;
        let _lock = tariff_book::GLOBAL_BOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let compiled = model().prices;
        // effective_from = i64::MAX keeps the row invisible to every timestamped resolve — and so
        // to every concurrently running test; only the exact pinned-version lookup sees it.
        let payload = serde_json::json!({
            "input": (compiled.input * 2).to_string(),
            "audio_input": (compiled.audio_input * 2).to_string(),
            "cached_input": (compiled.cached_input * 2).to_string(),
            "cached_audio_input": (compiled.cached_audio_input * 2).to_string(),
            "output": (compiled.output * 2).to_string(),
            "image_output": (compiled.image_output * 2).to_string(),
            "long_context_threshold": compiled.long_context_threshold,
            "long_input": (compiled.long_input * 2).to_string(),
            "long_audio_input": (compiled.long_audio_input * 2).to_string(),
            "long_cached_input": (compiled.long_cached_input * 2).to_string(),
            "long_cached_audio_input": (compiled.long_cached_audio_input * 2).to_string(),
            "long_output": (compiled.long_output * 2).to_string(),
            "search": {"kind": "per_grounded_prompt", "nano": "35000000"},
        });
        tariff_book::install_global_rows_for_test(vec![tariff_book::test_row(
            "google/gemini/gemini-2.5-flash",
            2,
            i64::MAX,
            payload,
        )]);

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-pinned-override-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            crate::billing::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap(),
        );
        billing.create_account(ACCOUNT, None, 10_000).await.unwrap();
        billing.topup(ACCOUNT, TOPUP, Some("seed")).await.unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        billing
            .reserve_request(REQUEST_ID, ACCOUNT, KEY, HOLD)
            .await
            .unwrap();

        let admission = GeminiAdmission {
            reservation: Some(Reservation {
                billing: Arc::clone(&billing),
                account_id: ACCOUNT.to_owned(),
                key: KEY.to_owned(),
                mult_bp: 10_000,
                hold: HOLD,
                tariff_priced_ts: 0,
                pinned_tariff: Some(tariff_book::PinnedTariff {
                    family: "google/gemini/gemini-2.5-flash".to_owned(),
                    version: 2,
                    schedule_id: "google/gemini/gemini-2.5-flash/v2".to_owned(),
                }),
                request_id: REQUEST_ID.to_owned(),
                request_fact: None,
                guard: HoldGuard::new(
                    Some(Arc::clone(&billing)),
                    ACCOUNT.to_owned(),
                    KEY.to_owned(),
                    HOLD,
                    REQUEST_ID.to_owned(),
                ),
            }),
            calibration_request_id: REQUEST_ID.to_owned(),
            exact_calibration_target: false,
            calibration_not_after: None,
        };
        let usage = metering::GeminiUsage {
            input_tokens: 1_000,
            output_tokens: 100,
            ..metering::GeminiUsage::default()
        };
        admission.settle(&model(), Some(&usage), "profile-1");
        billing.flush().await.unwrap();
        let account = billing.account(ACCOUNT).await.unwrap().unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);
        tariff_book::clear_global_book_for_test();
        let expected = 1_000 * (compiled.input * 2) + 100 * (compiled.output * 2);
        assert_eq!(account.spent_nano, i64::try_from(expected).unwrap());
        assert_eq!(account.reserved_nano, 0);
    }

    /// A missing append-only override version is an integrity error. In particular, it must not
    /// create a compiled-price calibration event after leaving customer money reserved for the
    /// durable reconciler.
    #[tokio::test]
    async fn missing_pinned_override_publishes_neither_charge_nor_calibration() {
        const ACCOUNT: &str = "gemini-missing-pinned-override";
        const KEY: &str = "gemini-missing-pinned-override-key";
        const REQUEST_ID: &str = "gemini-missing-pinned-override-request";
        const TOPUP: i64 = 1_000_000_000;
        const HOLD: i64 = 400_000_000;
        let _lock = tariff_book::GLOBAL_BOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        tariff_book::clear_global_book_for_test();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-missing-pinned-override-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            crate::billing::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap(),
        );
        billing.create_account(ACCOUNT, None, 10_000).await.unwrap();
        billing.topup(ACCOUNT, TOPUP, Some("seed")).await.unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        billing
            .reserve_request(REQUEST_ID, ACCOUNT, KEY, HOLD)
            .await
            .unwrap();

        let admission = GeminiAdmission {
            reservation: Some(Reservation {
                billing: Arc::clone(&billing),
                account_id: ACCOUNT.to_owned(),
                key: KEY.to_owned(),
                mult_bp: 10_000,
                hold: HOLD,
                tariff_priced_ts: 0,
                pinned_tariff: Some(tariff_book::PinnedTariff {
                    family: "google/gemini/gemini-2.5-flash".to_owned(),
                    version: 7,
                    schedule_id: "google/gemini/gemini-2.5-flash/v7".to_owned(),
                }),
                request_id: REQUEST_ID.to_owned(),
                request_fact: None,
                guard: HoldGuard::new(
                    Some(Arc::clone(&billing)),
                    ACCOUNT.to_owned(),
                    KEY.to_owned(),
                    HOLD,
                    REQUEST_ID.to_owned(),
                ),
            }),
            calibration_request_id: REQUEST_ID.to_owned(),
            exact_calibration_target: false,
            calibration_not_after: None,
        };
        let usage = metering::GeminiUsage {
            input_tokens: 1_000,
            output_tokens: 100,
            ..metering::GeminiUsage::default()
        };
        let event = admission.settle_at(&model(), Some(&usage), "profile-1", 1, None);
        billing.flush().await.unwrap();
        let account = billing.account(ACCOUNT).await.unwrap().unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);
        tariff_book::clear_global_book_for_test();

        assert!(event.is_none());
        assert_eq!(account.spent_nano, 0);
        assert_eq!(account.reserved_nano, HOLD);
    }

    #[test]
    fn settlement_retains_full_actual_beyond_the_admission_hold() {
        let usage = metering::GeminiUsage {
            input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            ..metering::GeminiUsage::default()
        };
        let (charge, _) = settled_charge(&model(), &usage, 17, 10_000, 0);
        assert_eq!(charge, i64::MAX);
        assert!(charge as i128 > 17 + metering::OVERDRAFT_NANO);
    }

    /// An unmeasured turn is not billed at the admission ceiling. The hold covers a
    /// byte-conservative input estimate plus the model's entire output limit, so charging it made
    /// customers pay a double-digit multiple of the turn; it is a reservation, not a price. Both
    /// shapes of "no usage" — absent and all-zero — settle the same way, and neither invents a usage
    /// event.
    #[test]
    fn an_unmeasured_turn_is_not_billed_at_the_admission_ceiling() {
        let zero = metering::GeminiUsage::default();
        for usage in [None, Some(&zero)] {
            let (charge, event) = settled_charge_or_hold(&model(), usage, 123_456, 10_000, 0);
            assert_eq!(charge, 0);
            assert!(event.is_none());
        }

        // The conservative fallback survives as an operator switch for a provider that stops
        // reporting usage altogether.
        crate::settlement_policy::set_charge_hold_on_unknown_usage(true);
        let (charge, event) = settled_charge_or_hold(&model(), None, 123_456, 10_000, 0);
        crate::settlement_policy::set_charge_hold_on_unknown_usage(false);
        assert_eq!(charge, 123_456);
        assert!(event.is_none());
    }
}
