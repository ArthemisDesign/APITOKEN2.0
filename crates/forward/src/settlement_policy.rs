//! What a customer owes for a turn whose real cost was never measured.
//!
//! The preflight hold is an admission device: a byte-conservative input estimate plus the model's
//! entire output limit, reserved so a request cannot outspend its balance. It was never a price. On
//! this fleet it runs 19x-250x the measured cost of a turn, so the moment it was used as a
//! settlement — a stream that ended without usage, a delivery marker that could not be written, an
//! owner that died mid-answer — the customer paid a double-digit multiple for their answer.
//!
//! Charging it is now off everywhere and stays a switch rather than a deletion: an operator facing
//! a provider that stops reporting usage entirely needs a way back to the conservative behaviour
//! without a deploy. Reserving is untouched — that is the admission job, and removing it would let
//! parallel requests outrun a balance.
//!
//! The flag is process-global because it is a single fleet-wide policy read on the settlement path
//! of every plane, and threading one boolean through five provider configs would buy nothing but
//! churn. `crates/server/src/config.rs` remains the only place that reads the environment; it calls
//! the setter once at startup.

use std::sync::atomic::{AtomicBool, Ordering};

static CHARGE_HOLD_ON_UNKNOWN_USAGE: AtomicBool = AtomicBool::new(false);

/// Install the fleet-wide policy. Called once by the composition layer from its parsed config.
pub fn set_charge_hold_on_unknown_usage(enabled: bool) {
    CHARGE_HOLD_ON_UNKNOWN_USAGE.store(enabled, Ordering::Release);
}

pub fn charge_hold_on_unknown_usage() -> bool {
    CHARGE_HOLD_ON_UNKNOWN_USAGE.load(Ordering::Acquire)
}

/// Settlement for a turn whose usage never arrived: nothing, unless the operator has deliberately
/// re-armed the conservative fallback.
///
/// Zero cannot be gamed into free inference. A customer cannot cause this branch: it needs the
/// provider to withhold usage or our own process to die, and neither is reachable from a request.
/// What a customer *can* do — disconnect mid-answer — is settled from measured usage on every
/// plane, not from here.
pub fn unknown_usage_charge(hold: i64) -> i64 {
    if charge_hold_on_unknown_usage() {
        hold.max(0)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default matters more than the switch: a fresh process must never bill a ceiling.
    #[test]
    fn an_unmeasured_turn_costs_nothing_unless_the_fallback_is_re_armed() {
        assert!(!charge_hold_on_unknown_usage());
        assert_eq!(unknown_usage_charge(3_400_000_000), 0);
        assert_eq!(unknown_usage_charge(-7), 0);

        set_charge_hold_on_unknown_usage(true);
        assert_eq!(unknown_usage_charge(3_400_000_000), 3_400_000_000);
        // Even re-armed, a negative hold is not a charge.
        assert_eq!(unknown_usage_charge(-7), 0);
        set_charge_hold_on_unknown_usage(false);
        assert_eq!(unknown_usage_charge(3_400_000_000), 0);
    }
}
