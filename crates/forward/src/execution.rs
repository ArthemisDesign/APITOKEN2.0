use axum::http::{Extensions, HeaderMap, HeaderName, HeaderValue};
use std::fmt;

/// Private router-to-plane capability carrying one logical customer request identity.
pub const LOGICAL_REQUEST_ID_HEADER: HeaderName =
    HeaderName::from_static("x-apitoken-logical-request-id");

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalRequestIdError {
    Duplicate,
    Invalid,
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

/// Preserve the already-admitted typed context when a universal adapter synthesizes its leaf
/// request. The wire header deliberately remains absent.
pub(crate) fn inherit_logical_request_id(source: &Extensions, target: &mut Extensions) {
    if let Some(logical_request_id) = source.get::<LogicalRequestId>() {
        target.insert(logical_request_id.clone());
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
    fn extension_inheritance_copies_only_the_typed_context_not_a_wire_header() {
        let mut headers = one(HeaderValue::from_static(CANONICAL));
        let logical = admit_logical_request_id(&mut headers).unwrap();
        let mut source = Extensions::new();
        source.insert(logical);
        let mut target = Extensions::new();
        inherit_logical_request_id(&source, &mut target);

        assert_eq!(
            target.get::<LogicalRequestId>().unwrap().as_str(),
            CANONICAL
        );
        assert!(!headers.contains_key(&LOGICAL_REQUEST_ID_HEADER));
    }
}
