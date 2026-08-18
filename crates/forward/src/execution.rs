use axum::http::{Extensions, HeaderMap, HeaderName, HeaderValue};
use registry::request_facts::{ClientKind, ClientSource};
use std::fmt;
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};

/// Private router-to-plane capability carrying one logical customer request identity.
pub const LOGICAL_REQUEST_ID_HEADER: HeaderName =
    HeaderName::from_static("x-apitoken-logical-request-id");

/// Optional public attribution hint. Admission consumes every value before provider dispatch.
pub const CLIENT_ATTRIBUTION_HEADER: HeaderName = HeaderName::from_static("x-apitoken-client");

/// Canonical logical identity admitted once by the provider process.
///
/// The inner value stays private so later instrumentation cannot construct an unchecked identity.
#[derive(Clone, Eq, PartialEq)]
pub struct LogicalRequestId(String);

impl LogicalRequestId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LogicalRequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // This capability is never request-log material. Keep accidental derived diagnostics redacted.
        formatter.write_str("LogicalRequestId(<redacted>)")
    }
}

/// Shared once-only lifecycle observation for the first successful public response bytes.
///
/// The timestamp cannot be supplied by a caller: the server's outer response-body seam asks this
/// carrier to observe the process clock only when it is returning a non-empty DATA frame. Clones
/// share the same atomic state through synthesized provider-leaf requests. Terminal producers can
/// seal an unobserved clock so a later body poll cannot add evidence after terminalization.
#[derive(Clone, Default)]
pub struct RequestLifecycleClock(Arc<AtomicI64>);

const LIFECYCLE_CLOCK_SEALED_WITHOUT_BYTE: i64 = -1;

impl RequestLifecycleClock {
    /// Observe `pool::now()` once when it is a positive Unix epoch second.
    pub fn observe_first_public_byte(&self) {
        if self.0.load(Ordering::Acquire) == 0 {
            self.observe_first_public_byte_at(pool::now());
        }
    }

    /// Return the first positive observed Unix epoch second, or `None` while unobserved or sealed.
    pub fn first_public_byte_at(&self) -> Option<i64> {
        match self.0.load(Ordering::Acquire) {
            timestamp if timestamp > 0 => Some(timestamp),
            _ => None,
        }
    }

    /// Atomically seal this clock at terminalization and return safely ordered first-byte evidence.
    ///
    /// Sealing is nonwaiting and linearizes with the outer body's first-byte observation. An open
    /// clock is sealed even when the supplied lifecycle bounds are invalid. Existing evidence is
    /// returned only when it lies inside the inclusive admission-to-terminal interval.
    pub(crate) fn seal_first_public_byte_for_terminal(
        &self,
        admitted_at: i64,
        terminal_at: i64,
    ) -> Option<i64> {
        let observed = match self.0.compare_exchange(
            0,
            LIFECYCLE_CLOCK_SEALED_WITHOUT_BYTE,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => None,
            Err(timestamp) if timestamp > 0 => Some(timestamp),
            Err(_) => None,
        };

        if admitted_at < 0 || terminal_at < admitted_at {
            return None;
        }
        observed.filter(|timestamp| (admitted_at..=terminal_at).contains(timestamp))
    }

    fn observe_first_public_byte_at(&self, timestamp: i64) {
        if timestamp > 0 {
            let _ = self
                .0
                .compare_exchange(0, timestamp, Ordering::AcqRel, Ordering::Acquire);
        }
    }
}

impl fmt::Debug for RequestLifecycleClock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RequestLifecycleClock(<redacted>)")
    }
}

/// Privacy-bounded client attribution admitted once at the provider boundary.
///
/// The raw header is never retained. Producer values are the closed v1 set even though registry
/// preserves its wider deployed vocabulary for compatibility with already-persisted rows.
#[derive(Clone, Eq, PartialEq)]
pub struct ClientAttribution {
    kind: ClientKind,
    source: ClientSource,
    version: Option<String>,
}

impl ClientAttribution {
    pub fn kind(&self) -> ClientKind {
        self.kind
    }

    pub fn source(&self) -> ClientSource {
        self.source
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub(crate) fn unknown_for_internal_use() -> Self {
        Self {
            kind: ClientKind::Unknown,
            source: ClientSource::Unknown,
            version: None,
        }
    }
}

impl fmt::Debug for ClientAttribution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClientAttribution(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalRequestIdError {
    Duplicate,
    Invalid,
}

/// Consume and normalize the optional public client-attribution hint.
///
/// Malformed, duplicated, unsupported, or absent evidence fails open to typed `unknown`. A present
/// malformed value is terminal for classification and never falls through to heuristics. Heuristic
/// v1 has no reviewed positive OpenCode or Claude Code signatures, so absence is also `unknown`.
pub fn admit_client_attribution(headers: &mut HeaderMap) -> ClientAttribution {
    let parsed = {
        let mut values = headers.get_all(&CLIENT_ATTRIBUTION_HEADER).iter();
        match (values.next(), values.next()) {
            (None, None) => None,
            (Some(value), None) => Some(parse_explicit_client_attribution(value)),
            (Some(_), Some(_)) => Some(None),
            (None, Some(_)) => unreachable!("HeaderMap iterator cannot skip its first value"),
        }
    };
    headers.remove(&CLIENT_ATTRIBUTION_HEADER);

    match parsed {
        Some(Some(attribution)) => attribution,
        // A malformed explicit value never falls through. Heuristic v1 also deliberately has no
        // reviewed positive signatures, so both missing and invalid evidence stay unknown.
        Some(None) | None => ClientAttribution::unknown_for_internal_use(),
    }
}

fn parse_explicit_client_attribution(value: &HeaderValue) -> Option<ClientAttribution> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 80
        || bytes.contains(&b',')
        || !bytes.is_ascii()
        || bytes.iter().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    let value = std::str::from_utf8(bytes).ok()?;
    let mut segments = value.split('/');
    let kind = match segments.next()? {
        "opencode" => ClientKind::OpenCode,
        "claude_code" => ClientKind::ClaudeCode,
        _ => return None,
    };
    let version = segments.next();
    if segments.next().is_some() {
        return None;
    }
    let version = match version {
        None => None,
        Some(version)
            if (1..=64).contains(&version.len())
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte)) =>
        {
            Some(version.to_owned())
        }
        Some(_) => return None,
    };
    Some(ClientAttribution {
        kind,
        source: ClientSource::Explicit,
        version,
    })
}

/// Consume the reserved logical-ID capability at the provider-process boundary.
///
/// Absence is direct ingress and creates one fresh CSPRNG UUIDv4. A trusted router value must be
/// exactly one canonical lowercase UUIDv4. The header is removed before either return path, so no
/// caller can accidentally forward even a malformed value to an internal adapter or upstream.
pub fn admit_logical_request_id(
    headers: &mut HeaderMap,
) -> Result<LogicalRequestId, LogicalRequestIdError> {
    let parsed = {
        let mut values = headers.get_all(&LOGICAL_REQUEST_ID_HEADER).iter();
        match (values.next(), values.next()) {
            (None, None) => Ok(None),
            (Some(value), None) => value
                .to_str()
                .map(|value| Some(value.to_owned()))
                .map_err(|_| LogicalRequestIdError::Invalid),
            (Some(_), Some(_)) => Err(LogicalRequestIdError::Duplicate),
            (None, Some(_)) => unreachable!("HeaderMap iterator cannot skip its first value"),
        }
    };
    headers.remove(&LOGICAL_REQUEST_ID_HEADER);

    match parsed? {
        None => Ok(LogicalRequestId(crate::fresh_request_id())),
        Some(value) if registry::request_facts::is_canonical_uuid_v4(&value) => {
            Ok(LogicalRequestId(value))
        }
        Some(_) => Err(LogicalRequestIdError::Invalid),
    }
}

/// Preserve already-admitted typed context when a universal adapter synthesizes its leaf request.
/// Wire headers deliberately remain absent.
pub(crate) fn inherit_request_context(source: &Extensions, target: &mut Extensions) {
    if let Some(logical_request_id) = source.get::<LogicalRequestId>() {
        target.insert(logical_request_id.clone());
    }
    if let Some(client_attribution) = source.get::<ClientAttribution>() {
        target.insert(client_attribution.clone());
    }
    if let Some(lifecycle_clock) = source.get::<RequestLifecycleClock>() {
        target.insert(lifecycle_clock.clone());
    }
}

#[cfg(test)]
mod request_lifecycle_clock_tests {
    use super::*;

    #[test]
    fn first_byte_wins_before_terminal_seal() {
        let clock = RequestLifecycleClock::default();

        clock.observe_first_public_byte_at(101);

        assert_eq!(
            clock.seal_first_public_byte_for_terminal(100, 102),
            Some(101)
        );
        clock.observe_first_public_byte_at(202);
        assert_eq!(clock.first_public_byte_at(), Some(101));
    }

    #[test]
    fn terminal_seal_wins_before_later_first_byte() {
        let clock = RequestLifecycleClock::default();

        assert_eq!(clock.seal_first_public_byte_for_terminal(100, 102), None);
        clock.observe_first_public_byte_at(101);

        assert_eq!(clock.first_public_byte_at(), None);
        assert_eq!(clock.seal_first_public_byte_for_terminal(100, 102), None);
    }

    #[test]
    fn clones_share_observation_and_terminal_seal() {
        let observed = RequestLifecycleClock::default();
        let observed_clone = observed.clone();
        observed_clone.observe_first_public_byte_at(101);
        assert_eq!(
            observed.seal_first_public_byte_for_terminal(100, 102),
            Some(101)
        );

        let sealed = RequestLifecycleClock::default();
        let sealed_clone = sealed.clone();
        assert_eq!(
            sealed_clone.seal_first_public_byte_for_terminal(100, 102),
            None
        );
        sealed.observe_first_public_byte_at(101);
        assert_eq!(sealed.first_public_byte_at(), None);
    }

    #[test]
    fn repeated_terminal_seal_is_idempotent() {
        let without_byte = RequestLifecycleClock::default();
        assert_eq!(
            without_byte.seal_first_public_byte_for_terminal(100, 102),
            None
        );
        assert_eq!(
            without_byte.seal_first_public_byte_for_terminal(100, 102),
            None
        );

        let observed = RequestLifecycleClock::default();
        observed.observe_first_public_byte_at(101);
        assert_eq!(
            observed.seal_first_public_byte_for_terminal(100, 102),
            Some(101)
        );
        assert_eq!(
            observed.seal_first_public_byte_for_terminal(100, 102),
            Some(101)
        );
    }

    #[test]
    fn inclusive_lifecycle_endpoints_are_valid() {
        let admitted = RequestLifecycleClock::default();
        admitted.observe_first_public_byte_at(100);
        assert_eq!(
            admitted.seal_first_public_byte_for_terminal(100, 102),
            Some(100)
        );

        let terminal = RequestLifecycleClock::default();
        terminal.observe_first_public_byte_at(102);
        assert_eq!(
            terminal.seal_first_public_byte_for_terminal(100, 102),
            Some(102)
        );
    }

    #[test]
    fn invalid_bounds_seal_open_clock() {
        for (admitted_at, terminal_at) in [(-1, 102), (103, 102), (-1, -1)] {
            let clock = RequestLifecycleClock::default();

            assert_eq!(
                clock.seal_first_public_byte_for_terminal(admitted_at, terminal_at),
                None
            );
            clock.observe_first_public_byte_at(101);
            assert_eq!(clock.first_public_byte_at(), None);
        }
    }

    #[test]
    fn invalid_observed_evidence_stays_observed_but_is_not_returned() {
        for observed_at in [99, 103] {
            let clock = RequestLifecycleClock::default();
            clock.observe_first_public_byte_at(observed_at);

            assert_eq!(clock.seal_first_public_byte_for_terminal(100, 102), None);
            clock.observe_first_public_byte_at(101);
            assert_eq!(clock.first_public_byte_at(), Some(observed_at));
        }
    }

    #[test]
    fn non_positive_observations_leave_clock_open_until_terminal_seal() {
        let clock = RequestLifecycleClock::default();
        clock.observe_first_public_byte_at(0);
        clock.observe_first_public_byte_at(-1);

        assert_eq!(clock.first_public_byte_at(), None);
        assert_eq!(clock.seal_first_public_byte_for_terminal(100, 102), None);
        clock.observe_first_public_byte_at(101);
        assert_eq!(clock.first_public_byte_at(), None);
    }

    #[test]
    fn debug_output_redacts_open_observed_and_sealed_state() {
        let open = RequestLifecycleClock::default();
        let observed = RequestLifecycleClock::default();
        observed.observe_first_public_byte_at(101);
        let sealed = RequestLifecycleClock::default();
        sealed.seal_first_public_byte_for_terminal(100, 102);

        for clock in [open, observed, sealed] {
            assert_eq!(format!("{clock:?}"), "RequestLifecycleClock(<redacted>)");
        }
    }
}

#[cfg(test)]
mod client_attribution_tests {
    use super::*;

    fn classify(values: &[HeaderValue]) -> (ClientAttribution, HeaderMap) {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(&CLIENT_ATTRIBUTION_HEADER, value.clone());
        }
        let attribution = admit_client_attribution(&mut headers);
        (attribution, headers)
    }

    fn assert_unknown(values: &[HeaderValue]) {
        let (attribution, headers) = classify(values);
        assert_eq!(attribution.kind(), ClientKind::Unknown);
        assert_eq!(attribution.source(), ClientSource::Unknown);
        assert_eq!(attribution.version(), None);
        assert_eq!(format!("{attribution:?}"), "ClientAttribution(<redacted>)");
        assert!(!headers.contains_key(&CLIENT_ATTRIBUTION_HEADER));
    }

    #[test]
    fn accepts_the_closed_explicit_v1_grammar_and_bounds() {
        for (value, kind, version) in [
            ("opencode", ClientKind::OpenCode, None),
            ("claude_code", ClientKind::ClaudeCode, None),
            ("opencode/1", ClientKind::OpenCode, Some("1")),
            (
                "claude_code/Az09._+-",
                ClientKind::ClaudeCode,
                Some("Az09._+-"),
            ),
        ] {
            let (attribution, headers) = classify(&[HeaderValue::from_str(value).unwrap()]);
            assert_eq!(attribution.kind(), kind, "value={value:?}");
            assert_eq!(attribution.source(), ClientSource::Explicit);
            assert_eq!(attribution.version(), version);
            assert!(!headers.contains_key(&CLIENT_ATTRIBUTION_HEADER));
        }

        let max_version = "V".repeat(64);
        let max_total = format!("opencode/{max_version}");
        assert_eq!(max_total.len(), 73);
        let (attribution, _) = classify(&[HeaderValue::from_str(&max_total).unwrap()]);
        assert_eq!(attribution.version(), Some(max_version.as_str()));

        // Header-wide 80-byte admission remains independent of the stricter 64-byte version bound.
        assert_unknown(&[HeaderValue::from_str(&format!("opencode/{}", "v".repeat(65))).unwrap()]);
        assert_unknown(&[HeaderValue::from_str(&"x".repeat(80)).unwrap()]);
        assert_unknown(&[HeaderValue::from_str(&"x".repeat(81)).unwrap()]);
    }

    #[test]
    fn malformed_unsupported_case_variants_and_missing_are_unknown_and_removed() {
        assert_unknown(&[]);
        for value in [
            "",
            "OpenCode",
            "CLAUDE_CODE",
            "opencode/",
            "claude_code/",
            "opencode/1/2",
            "opencode//1",
            "opencode,claude_code",
            "opencode/1,claude_code/2",
            "cursor",
            "codex_cli/1",
            "sdk",
            "opencode/1 2",
            "opencode/1/",
            " opencode",
            "opencode ",
        ] {
            assert_unknown(&[HeaderValue::from_str(value).unwrap()]);
        }

        // HeaderValue admits opaque obs-text, which the classifier must reject as non-ASCII.
        assert_unknown(&[HeaderValue::from_bytes(&[0x80]).unwrap()]);

        // HTTP itself rejects wire controls before a HeaderMap can exist. Keep the classifier's
        // explicit `is_ascii_control` guard for any future header representation.
        assert!(HeaderValue::from_bytes(&[0x7f]).is_err());
        assert_unknown(&[HeaderValue::from_bytes(b"opencode/1	2").unwrap()]);
    }

    #[test]
    fn duplicate_field_lines_are_unknown_even_when_identical_or_individually_valid() {
        for second in ["opencode", "claude_code/2"] {
            assert_unknown(&[
                HeaderValue::from_static("opencode"),
                HeaderValue::from_str(second).unwrap(),
            ]);
        }
    }
}

pub(crate) const EXECUTION_GROUP_HEADER: HeaderName =
    HeaderName::from_static("x-apitoken-execution-group");
pub(crate) const EXECUTION_ATTEMPT_HEADER: HeaderName =
    HeaderName::from_static("x-apitoken-attempt");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionIdentityError {
    Incomplete,
    Duplicate,
    InvalidGroup,
    InvalidAttempt,
}

impl ExecutionIdentityError {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Incomplete => "incomplete",
            Self::Duplicate => "duplicate",
            Self::InvalidGroup => "invalid_group",
            Self::InvalidAttempt => "invalid_attempt",
        }
    }
}

fn exactly_one<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<Option<&'a HeaderValue>, ExecutionIdentityError> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
        (None, None) => Ok(None),
        (Some(value), None) => Ok(Some(value)),
        (Some(_), Some(_)) => Err(ExecutionIdentityError::Duplicate),
        (None, Some(_)) => unreachable!("HeaderMap iterator cannot skip its first value"),
    }
}

/// Parse the router-owned execution identity once at plane admission. Public ingress strips both
/// headers, so an absent pair is an ordinary direct execution. Partial, duplicated, malformed or
/// non-canonical pairs fail closed before any money mutation.
pub(crate) fn parse_execution_attempt(
    headers: &HeaderMap,
) -> Result<registry::ExecutionAttempt, ExecutionIdentityError> {
    let group = exactly_one(headers, &EXECUTION_GROUP_HEADER)?;
    let attempt = exactly_one(headers, &EXECUTION_ATTEMPT_HEADER)?;
    let (group, attempt) = match (group, attempt) {
        (None, None) => return Ok(registry::ExecutionAttempt::direct()),
        (Some(group), Some(attempt)) => (group, attempt),
        _ => return Err(ExecutionIdentityError::Incomplete),
    };

    let group = group
        .to_str()
        .map_err(|_| ExecutionIdentityError::InvalidGroup)?;
    let attempt = attempt
        .to_str()
        .map_err(|_| ExecutionIdentityError::InvalidAttempt)?;
    if attempt.is_empty()
        || attempt.starts_with('0')
        || !attempt.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ExecutionIdentityError::InvalidAttempt);
    }
    let attempt = attempt
        .parse::<i32>()
        .ok()
        .filter(|attempt| *attempt > 0)
        .ok_or(ExecutionIdentityError::InvalidAttempt)?;
    registry::ExecutionAttempt::grouped(group, attempt)
        .map_err(|_| ExecutionIdentityError::InvalidGroup)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP: &str = "018f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";

    #[test]
    fn absent_pair_is_direct() {
        assert_eq!(
            parse_execution_attempt(&HeaderMap::new()).unwrap(),
            registry::ExecutionAttempt::direct()
        );
    }

    #[test]
    fn canonical_pair_is_grouped() {
        let mut headers = HeaderMap::new();
        headers.insert(&EXECUTION_GROUP_HEADER, HeaderValue::from_static(GROUP));
        headers.insert(&EXECUTION_ATTEMPT_HEADER, HeaderValue::from_static("12"));
        let parsed = parse_execution_attempt(&headers).unwrap();
        assert_eq!(parsed.group_id(), Some(GROUP));
        assert_eq!(parsed.attempt(), 12);
    }

    #[test]
    fn malformed_partial_noncanonical_and_duplicate_pairs_fail_closed() {
        for (group, attempt, expected) in [
            (Some(GROUP), None, ExecutionIdentityError::Incomplete),
            (None, Some("1"), ExecutionIdentityError::Incomplete),
            (
                Some("018F47A2-9B2D-4DC4-8F11-4D43B7D8B62A"),
                Some("1"),
                ExecutionIdentityError::InvalidGroup,
            ),
            (
                Some("018f47a2-9b2d-3dc4-8f11-4d43b7d8b62a"),
                Some("1"),
                ExecutionIdentityError::InvalidGroup,
            ),
            (
                Some(GROUP),
                Some("01"),
                ExecutionIdentityError::InvalidAttempt,
            ),
            (
                Some(GROUP),
                Some("0"),
                ExecutionIdentityError::InvalidAttempt,
            ),
            (
                Some(GROUP),
                Some("2147483648"),
                ExecutionIdentityError::InvalidAttempt,
            ),
        ] {
            let mut headers = HeaderMap::new();
            if let Some(group) = group {
                headers.insert(
                    &EXECUTION_GROUP_HEADER,
                    HeaderValue::from_str(group).unwrap(),
                );
            }
            if let Some(attempt) = attempt {
                headers.insert(
                    &EXECUTION_ATTEMPT_HEADER,
                    HeaderValue::from_str(attempt).unwrap(),
                );
            }
            assert_eq!(parse_execution_attempt(&headers), Err(expected));
        }

        let mut duplicate = HeaderMap::new();
        duplicate.append(&EXECUTION_GROUP_HEADER, HeaderValue::from_static(GROUP));
        duplicate.append(&EXECUTION_GROUP_HEADER, HeaderValue::from_static(GROUP));
        duplicate.insert(&EXECUTION_ATTEMPT_HEADER, HeaderValue::from_static("1"));
        assert_eq!(
            parse_execution_attempt(&duplicate),
            Err(ExecutionIdentityError::Duplicate)
        );
    }
}

#[cfg(test)]
mod logical_request_id_tests {
    use super::*;

    const CANONICAL: &str = "018f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";

    fn one(value: HeaderValue) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(&LOGICAL_REQUEST_ID_HEADER, value);
        headers
    }

    #[test]
    fn missing_generates_fresh_canonical_distinct_values_and_removes_header() {
        let mut first_headers = HeaderMap::new();
        let first = admit_logical_request_id(&mut first_headers).unwrap();
        let mut second_headers = HeaderMap::new();
        let second = admit_logical_request_id(&mut second_headers).unwrap();

        assert!(registry::request_facts::is_canonical_uuid_v4(
            first.as_str()
        ));
        assert!(registry::request_facts::is_canonical_uuid_v4(
            second.as_str()
        ));
        assert_ne!(first, second);
        assert!(!first_headers.contains_key(&LOGICAL_REQUEST_ID_HEADER));
        assert!(!second_headers.contains_key(&LOGICAL_REQUEST_ID_HEADER));
    }

    #[test]
    fn exactly_one_canonical_value_is_accepted_and_removed() {
        let mut headers = one(HeaderValue::from_static(CANONICAL));
        let parsed = admit_logical_request_id(&mut headers).unwrap();
        assert_eq!(parsed.as_str(), CANONICAL);
        assert_eq!(format!("{parsed:?}"), "LogicalRequestId(<redacted>)");
        assert!(!headers.contains_key(&LOGICAL_REQUEST_ID_HEADER));
    }

    #[test]
    fn duplicate_identical_and_different_values_fail_closed_and_are_removed() {
        for second in [CANONICAL, "aaaaaaaa-aaaa-4aaa-9aaa-aaaaaaaaaaaa"] {
            let mut headers = one(HeaderValue::from_static(CANONICAL));
            headers.append(
                &LOGICAL_REQUEST_ID_HEADER,
                HeaderValue::from_str(second).unwrap(),
            );
            assert_eq!(
                admit_logical_request_id(&mut headers),
                Err(LogicalRequestIdError::Duplicate)
            );
            assert!(!headers.contains_key(&LOGICAL_REQUEST_ID_HEADER));
        }
    }

    #[test]
    fn every_noncanonical_single_value_fails_closed_and_is_removed() {
        for value in [
            "018f47a2-9b2d-4dc4-8f11-4d43b7d8b62a,aaaaaaaa-aaaa-4aaa-9aaa-aaaaaaaaaaaa",
            " 018f47a2-9b2d-4dc4-8f11-4d43b7d8b62a",
            "018f47a2-9b2d-4dc4-8f11-4d43b7d8b62a ",
            "018f47a2-9b2d-4dc4-8f11-4d43b7d8b 62a",
            "018F47A2-9B2D-4DC4-8F11-4D43B7D8B62A",
            "018f47a2-9b2d-3dc4-8f11-4d43b7d8b62a",
            "018f47a2-9b2d-4dc4-7f11-4d43b7d8b62a",
            "018f47a2-9b2d-4dc4-cf11-4d43b7d8b62a",
            "018f47a2-9b2d-4dc4-8f11-4d43b7d8b62",
            "not-a-uuid",
            "",
        ] {
            let mut headers = one(HeaderValue::from_str(value).unwrap());
            assert_eq!(
                admit_logical_request_id(&mut headers),
                Err(LogicalRequestIdError::Invalid),
                "value={value:?}"
            );
            assert!(!headers.contains_key(&LOGICAL_REQUEST_ID_HEADER));
        }

        let mut non_utf8 = one(HeaderValue::from_bytes(&[0x80]).unwrap());
        assert_eq!(
            admit_logical_request_id(&mut non_utf8),
            Err(LogicalRequestIdError::Invalid)
        );
        assert!(!non_utf8.contains_key(&LOGICAL_REQUEST_ID_HEADER));
    }

    #[test]
    fn extension_inheritance_copies_typed_context_only_not_wire_headers() {
        let mut headers = one(HeaderValue::from_static(CANONICAL));
        headers.insert(
            &CLIENT_ATTRIBUTION_HEADER,
            HeaderValue::from_static("opencode/1.2.3"),
        );
        let logical = admit_logical_request_id(&mut headers).unwrap();
        let attribution = admit_client_attribution(&mut headers);
        let lifecycle_clock = RequestLifecycleClock::default();
        let mut source = Extensions::new();
        source.insert(logical);
        source.insert(attribution);
        source.insert(lifecycle_clock.clone());
        let mut target = Extensions::new();
        inherit_request_context(&source, &mut target);

        assert_eq!(
            target.get::<LogicalRequestId>().unwrap().as_str(),
            CANONICAL
        );
        let attribution = target.get::<ClientAttribution>().unwrap();
        assert_eq!(attribution.kind(), ClientKind::OpenCode);
        assert_eq!(attribution.source(), ClientSource::Explicit);
        assert_eq!(attribution.version(), Some("1.2.3"));
        let inherited_clock = target.get::<RequestLifecycleClock>().unwrap();
        assert!(inherited_clock.first_public_byte_at().is_none());
        inherited_clock.observe_first_public_byte_at(123);
        assert_eq!(lifecycle_clock.first_public_byte_at(), Some(123));
        assert_eq!(
            format!("{inherited_clock:?}"),
            "RequestLifecycleClock(<redacted>)"
        );
        assert!(!headers.contains_key(&LOGICAL_REQUEST_ID_HEADER));
        assert!(!headers.contains_key(&CLIENT_ATTRIBUTION_HEADER));
    }
}
