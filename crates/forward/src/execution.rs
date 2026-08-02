use axum::http::{HeaderMap, HeaderName, HeaderValue};

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
