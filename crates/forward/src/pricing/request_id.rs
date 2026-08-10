//! Engine-owned request identity for the provider quote builders.
//!
//! The pricing bridge that once sampled requests into a shadow evaluation is gone; what survives
//! is the one thing the money path actually needs — proof that a request id was minted by this
//! engine's CSPRNG and not taken from a client or an upstream header.

/// An engine-owned CSPRNG UUIDv4. Keeping this constructor crate-private prevents a public adapter
/// from accidentally deriving money identity from a client/upstream identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EnginePricingRequestId(String);

impl EnginePricingRequestId {
    pub(crate) fn from_engine_uuid_v4(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 36
            || bytes[8] != b'-'
            || bytes[13] != b'-'
            || bytes[18] != b'-'
            || bytes[23] != b'-'
            || bytes[14] != b'4'
            || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        {
            return None;
        }
        if bytes.iter().enumerate().any(|(index, byte)| {
            !matches!(index, 8 | 13 | 18 | 23) && !matches!(*byte, b'0'..=b'9' | b'a'..=b'f')
        }) {
            return None;
        }
        Some(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_request_id_accepts_only_canonical_lowercase_uuid_v4() {
        assert!(EnginePricingRequestId::from_engine_uuid_v4(
            "123e4567-e89b-42d3-a456-426614174000"
        )
        .is_some());
        for invalid in [
            "123e4567-e89b-12d3-a456-426614174000",
            "123e4567-e89b-42d3-7456-426614174000",
            "123E4567-E89B-42D3-A456-426614174000",
            "123e4567e89b42d3a456426614174000",
            "client-request",
        ] {
            assert!(EnginePricingRequestId::from_engine_uuid_v4(invalid).is_none());
        }
    }
}
