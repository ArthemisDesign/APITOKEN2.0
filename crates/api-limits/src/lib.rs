//! Checked payload-limit contract shared by the public router and provider planes.
//!
//! This leaf crate deliberately has no HTTP, environment, network, provider, or async runtime
//! dependencies. Environment ownership remains in `crates/router/src/config.rs` and
//! `crates/server/src/config.rs`.

use core::fmt;

pub const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ByteLimit(u64);

impl ByteLimit {
    pub const fn from_bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    pub const fn from_mib(mib: u64) -> Option<Self> {
        match mib.checked_mul(MIB) {
            Some(bytes) => Some(Self(bytes)),
            None => None,
        }
    }

    pub const fn bytes(self) -> u64 {
        self.0
    }

    pub fn as_usize(self) -> Result<usize, LimitError> {
        usize::try_from(self.0).map_err(|_| LimitError::PlatformOverflow)
    }

    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(bytes) => Some(Self(bytes)),
            None => None,
        }
    }
}

impl fmt::Display for ByteLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 % MIB == 0 {
            write!(formatter, "{} MiB", self.0 / MIB)
        } else {
            write!(formatter, "{} bytes", self.0)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AdmissionUnits(u32);

impl AdmissionUnits {
    pub const fn new(units: u32) -> Option<Self> {
        if units == 0 {
            None
        } else {
            Some(Self(units))
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn for_bytes(bytes: ByteLimit, unit: ByteLimit) -> Result<Self, LimitError> {
        if unit.bytes() == 0 {
            return Err(LimitError::Zero);
        }
        let rounded = bytes
            .bytes()
            .checked_add(unit.bytes() - 1)
            .ok_or(LimitError::ArithmeticOverflow)?
            / unit.bytes();
        let units = u32::try_from(rounded).map_err(|_| LimitError::ArithmeticOverflow)?;
        Self::new(units).ok_or(LimitError::Zero)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteClass {
    UniversalText,
    AnthropicText,
    OpenAiText,
    GeminiText,
    GeminiMedia,
    ImageGeneration,
    ImageEdit,
    ControlPlane,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitError {
    Zero,
    InvalidDecimal,
    ArithmeticOverflow,
    PlatformOverflow,
    RequestExceedsHardCeiling,
    RequestExceedsSpool,
    SpoolExceedsHardCeiling,
    MemoryBudgetExceedsHardCeiling,
    MemoryThresholdExceedsRequest,
    ResponseExceedsHardCeiling,
    ResponseExceedsProcessBudget,
    FrameEnvelopeTooSmall,
}

impl fmt::Display for LimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Zero => "value must be greater than zero",
            Self::InvalidDecimal => {
                "expected an unsigned base-10 integer without whitespace or signs"
            }
            Self::ArithmeticOverflow => "value exceeds the checked arithmetic range",
            Self::PlatformOverflow => "value does not fit this platform's address space",
            Self::RequestExceedsHardCeiling => {
                "per-request limit exceeds the compile-time hard ceiling"
            }
            Self::RequestExceedsSpool => "per-request limit exceeds the spool budget",
            Self::SpoolExceedsHardCeiling => "spool budget exceeds the compile-time hard ceiling",
            Self::MemoryBudgetExceedsHardCeiling => {
                "memory budget exceeds the compile-time hard ceiling"
            }
            Self::MemoryThresholdExceedsRequest => "memory threshold exceeds the per-request limit",
            Self::ResponseExceedsHardCeiling => {
                "response limit exceeds the compile-time hard ceiling"
            }
            Self::ResponseExceedsProcessBudget => {
                "response limit exceeds the process memory budget"
            }
            Self::FrameEnvelopeTooSmall => {
                "binary frame envelope is smaller than body plus framing overhead"
            }
        })
    }
}

impl std::error::Error for LimitError {}

pub fn parse_decimal_mib(value: &str) -> Result<ByteLimit, LimitError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LimitError::InvalidDecimal);
    }
    let mib = value
        .parse::<u64>()
        .map_err(|_| LimitError::ArithmeticOverflow)?;
    if mib == 0 {
        return Err(LimitError::Zero);
    }
    ByteLimit::from_mib(mib).ok_or(LimitError::ArithmeticOverflow)
}

pub fn parse_decimal_seconds(value: &str) -> Result<u64, LimitError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LimitError::InvalidDecimal);
    }
    let seconds = value
        .parse::<u64>()
        .map_err(|_| LimitError::ArithmeticOverflow)?;
    if seconds == 0 {
        return Err(LimitError::Zero);
    }
    Ok(seconds)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BodyLimits {
    pub request: ByteLimit,
    pub memory_budget: ByteLimit,
    pub spool_budget: ByteLimit,
    pub memory_threshold: ByteLimit,
    pub response: ByteLimit,
}

impl BodyLimits {
    pub fn validate(self, spool_hard_ceiling: ByteLimit) -> Result<Self, LimitError> {
        if self.request.bytes() == 0
            || self.memory_budget.bytes() == 0
            || self.spool_budget.bytes() == 0
            || self.memory_threshold.bytes() == 0
            || self.response.bytes() == 0
        {
            return Err(LimitError::Zero);
        }
        if self.request > hard::REQUEST {
            return Err(LimitError::RequestExceedsHardCeiling);
        }
        if self.request > self.spool_budget {
            return Err(LimitError::RequestExceedsSpool);
        }
        if self.spool_budget > spool_hard_ceiling {
            return Err(LimitError::SpoolExceedsHardCeiling);
        }
        if self.memory_budget > hard::MEMORY_BUDGET {
            return Err(LimitError::MemoryBudgetExceedsHardCeiling);
        }
        if self.memory_threshold > self.request {
            return Err(LimitError::MemoryThresholdExceedsRequest);
        }
        if self.response > hard::RESPONSE {
            return Err(LimitError::ResponseExceedsHardCeiling);
        }
        if self.response > self.memory_budget {
            return Err(LimitError::ResponseExceedsProcessBudget);
        }
        Ok(self)
    }
}

pub fn validate_frame_envelope(
    body: ByteLimit,
    framing_overhead: ByteLimit,
    envelope: ByteLimit,
) -> Result<(), LimitError> {
    let required = body
        .checked_add(framing_overhead)
        .ok_or(LimitError::ArithmeticOverflow)?;
    if envelope < required {
        return Err(LimitError::FrameEnvelopeTooSmall);
    }
    Ok(())
}

pub mod hard {
    use super::ByteLimit;

    pub const REQUEST: ByteLimit = ByteLimit::from_bytes(256 * super::MIB);
    pub const RESPONSE: ByteLimit = ByteLimit::from_bytes(256 * super::MIB);
    pub const SPOOL: ByteLimit = ByteLimit::from_bytes(16 * 1024 * super::MIB);
    pub const MEMORY_BUDGET: ByteLimit = ByteLimit::from_bytes(8 * 1024 * super::MIB);
    pub const GEMINI_FRAME: ByteLimit = ByteLimit::from_bytes(257 * super::MIB);
}

pub mod current {
    use super::{BodyLimits, ByteLimit, MIB};

    pub const ROUTER_REQUEST: ByteLimit = ByteLimit::from_bytes(64 * MIB);
    pub const ROUTER_MEMORY_BUDGET: ByteLimit = ByteLimit::from_bytes(512 * MIB);
    pub const ROUTER_SPOOL_BUDGET: ByteLimit = ByteLimit::from_bytes(512 * MIB);
    pub const ROUTER_MEMORY_THRESHOLD: ByteLimit = ROUTER_REQUEST;
    pub const ROUTER_RESPONSE: ByteLimit = ByteLimit::from_bytes(32 * MIB);
    pub const ROUTER_BODY_IDLE_SECS: u64 = 60;
    pub const ROUTER_BODY_MAX_SECS: u64 = 5 * 60;

    pub const PROVIDER_TEXT_REQUEST: ByteLimit = ByteLimit::from_bytes(32 * MIB);
    pub const ANTHROPIC_TEXT_REQUEST: ByteLimit = ByteLimit::from_bytes(32 * MIB);
    pub const OPENAI_TEXT_REQUEST: ByteLimit = ByteLimit::from_bytes(8 * MIB);
    pub const GEMINI_TEXT_REQUEST: ByteLimit = ByteLimit::from_bytes(32 * MIB);
    pub const GEMINI_MEDIA_REQUEST: ByteLimit = ByteLimit::from_bytes(20 * MIB);
    pub const TRANSLATED_NONSTREAM_RESPONSE: ByteLimit = ByteLimit::from_bytes(32 * MIB);
    pub const GEMINI_NATIVE_RESPONSE: ByteLimit = ByteLimit::from_bytes(64 * MIB);
    pub const PROVIDER_MEMORY_BUDGET: ByteLimit = ByteLimit::from_bytes(2 * 1024 * MIB);
    pub const PROVIDER_SPOOL_BUDGET: ByteLimit = ByteLimit::from_bytes(2 * 1024 * MIB);
    pub const PROVIDER_MEMORY_THRESHOLD: ByteLimit = PROVIDER_TEXT_REQUEST;
    pub const PROVIDER_NONSTREAM_RESPONSE: ByteLimit = GEMINI_NATIVE_RESPONSE;

    pub const ROUTER: BodyLimits = BodyLimits {
        request: ROUTER_REQUEST,
        memory_budget: ROUTER_MEMORY_BUDGET,
        spool_budget: ROUTER_SPOOL_BUDGET,
        memory_threshold: ROUTER_MEMORY_THRESHOLD,
        response: ROUTER_RESPONSE,
    };
    pub const PROVIDER: BodyLimits = BodyLimits {
        request: PROVIDER_TEXT_REQUEST,
        memory_budget: PROVIDER_MEMORY_BUDGET,
        spool_budget: PROVIDER_SPOOL_BUDGET,
        memory_threshold: PROVIDER_MEMORY_THRESHOLD,
        response: PROVIDER_NONSTREAM_RESPONSE,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_parsers_are_strict_and_checked() {
        assert_eq!(
            parse_decimal_mib("32").unwrap(),
            ByteLimit::from_bytes(32 * MIB)
        );
        for invalid in ["", "0", " 32", "32 ", "+32", "-1", "1.5", "０"] {
            assert!(parse_decimal_mib(invalid).is_err(), "accepted {invalid:?}");
        }
        assert_eq!(parse_decimal_seconds("120"), Ok(120));
        assert!(parse_decimal_seconds("0").is_err());
    }

    #[test]
    fn admission_rounds_up_without_unchecked_math() {
        let unit = ByteLimit::from_bytes(MIB);
        assert_eq!(
            AdmissionUnits::for_bytes(ByteLimit::from_bytes(1), unit)
                .unwrap()
                .get(),
            1
        );
        assert_eq!(AdmissionUnits::for_bytes(unit, unit).unwrap().get(), 1);
        assert_eq!(
            AdmissionUnits::for_bytes(ByteLimit::from_bytes(MIB + 1), unit)
                .unwrap()
                .get(),
            2
        );
        assert!(AdmissionUnits::for_bytes(ByteLimit::from_bytes(u64::MAX), unit).is_err());
    }

    #[test]
    fn production_defaults_obey_compile_ceiling_relationships() {
        assert_eq!(current::ROUTER.validate(hard::SPOOL), Ok(current::ROUTER));
        assert_eq!(
            current::PROVIDER.validate(hard::SPOOL),
            Ok(current::PROVIDER)
        );
        assert!(current::ROUTER.request <= hard::REQUEST);
        assert!(current::PROVIDER.response <= hard::RESPONSE);
        assert!(current::PROVIDER.memory_budget <= hard::MEMORY_BUDGET);
        assert!(current::ANTHROPIC_TEXT_REQUEST <= current::PROVIDER_TEXT_REQUEST);
        assert!(current::OPENAI_TEXT_REQUEST <= current::PROVIDER_TEXT_REQUEST);
        assert!(current::GEMINI_TEXT_REQUEST <= current::PROVIDER_TEXT_REQUEST);
        assert!(current::GEMINI_MEDIA_REQUEST <= current::GEMINI_TEXT_REQUEST);
        assert!(current::TRANSLATED_NONSTREAM_RESPONSE <= current::PROVIDER_NONSTREAM_RESPONSE);
    }

    #[test]
    fn relationship_validation_fails_closed() {
        let invalid = BodyLimits {
            request: ByteLimit::from_bytes(2 * MIB),
            memory_budget: ByteLimit::from_bytes(MIB),
            spool_budget: ByteLimit::from_bytes(MIB),
            memory_threshold: ByteLimit::from_bytes(MIB),
            response: ByteLimit::from_bytes(MIB),
        };
        assert_eq!(
            invalid.validate(hard::SPOOL),
            Err(LimitError::RequestExceedsSpool)
        );
        for (invalid, expected) in [
            (
                BodyLimits {
                    request: ByteLimit::from_bytes(257 * MIB),
                    memory_budget: hard::MEMORY_BUDGET,
                    spool_budget: hard::SPOOL,
                    memory_threshold: ByteLimit::from_bytes(MIB),
                    response: ByteLimit::from_bytes(MIB),
                },
                LimitError::RequestExceedsHardCeiling,
            ),
            (
                BodyLimits {
                    request: ByteLimit::from_bytes(MIB),
                    memory_budget: ByteLimit::from_bytes(hard::MEMORY_BUDGET.bytes() + MIB),
                    spool_budget: hard::SPOOL,
                    memory_threshold: ByteLimit::from_bytes(MIB),
                    response: ByteLimit::from_bytes(MIB),
                },
                LimitError::MemoryBudgetExceedsHardCeiling,
            ),
            (
                BodyLimits {
                    request: ByteLimit::from_bytes(MIB),
                    memory_budget: hard::MEMORY_BUDGET,
                    spool_budget: hard::SPOOL,
                    memory_threshold: ByteLimit::from_bytes(MIB),
                    response: ByteLimit::from_bytes(257 * MIB),
                },
                LimitError::ResponseExceedsHardCeiling,
            ),
        ] {
            assert_eq!(invalid.validate(hard::SPOOL), Err(expected));
        }
        assert_eq!(
            validate_frame_envelope(
                ByteLimit::from_bytes(256 * MIB),
                ByteLimit::from_bytes(MIB),
                ByteLimit::from_bytes(256 * MIB),
            ),
            Err(LimitError::FrameEnvelopeTooSmall)
        );
    }

    #[test]
    fn formatter_is_stable_for_mib_and_raw_bytes() {
        assert_eq!(ByteLimit::from_bytes(32 * MIB).to_string(), "32 MiB");
        assert_eq!(ByteLimit::from_bytes(7).to_string(), "7 bytes");
    }
}
