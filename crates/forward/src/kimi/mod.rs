//! KIMI (Kimi Code) subscription plane.
//!
//! Contract: `docs/engine/KIMI_PROVIDER.md`. The plane is deliberately smaller than the Gemini
//! one: the subscription serves an Anthropic-compatible endpoint, so the engine's native protocol
//! is forwarded without a translation layer.
//!
pub mod client;
pub mod config;
mod gateway;
pub mod pool;
pub mod queue;
pub mod roster;
pub mod selection;
pub mod transport;

pub use gateway::{
    bounded_plan_label, KimiGateway, KimiOperationalStatus, KimiProfileStatus,
    KimiQuotaWindowStatus,
};
pub(crate) use gateway::{parse_kimi_calibration_headers, KimiBillingInput, KimiRequest};
