//! KIMI (Kimi Code) subscription plane.
//!
//! Contract: `docs/engine/KIMI_PROVIDER.md`. The plane is deliberately smaller than the Gemini
//! one: the subscription serves an Anthropic-compatible endpoint, so the engine's native protocol
//! is forwarded without a translation layer.
//!
//! Dormant: `server` validates the default-off operator config, but the generation gateway is not
//! wired yet, so no roster profile routes traffic merely because the switch is enabled.

pub mod client;
pub mod config;
pub mod pool;
pub mod queue;
pub mod roster;
pub mod selection;
pub mod transport;
