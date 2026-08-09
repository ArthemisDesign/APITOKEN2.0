//! Pricing helpers shared by the provider admission paths.
//!
//! Pricing is deliberately small: an account carries one discount, admission caps the reserve to
//! the balance, and settlement replays the tariff the reserve pinned. The per-account policy
//! resolver, the shadow-evaluation pipeline and the release-v2 machinery are gone — they made a
//! funded account unable to spend its own balance whenever the two funding representations
//! disagreed, which is exactly the failure this module no longer permits.
//!
//! `bridge` keeps only the engine-owned request identity and the small validation helpers the
//! provider quote builders still use. `tariff_book` is the process-wide hot tariff override book,
//! refreshed from the billing reader actor and read on the reserve/settlement hot paths.
//! Contract — `crates/forward/CLAUDE.md`.

mod bridge;
pub mod tariff_book;

pub(crate) use bridge::{
    snapshot_identity_is_oversized, EnginePricingRequestId, PricingBridgePrepare,
};
pub use bridge::PricingBridgeFallbackReason;
