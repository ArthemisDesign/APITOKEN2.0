//! KIMI (Kimi Code) subscription plane.
//!
//! Contract: `docs/engine/KIMI_PROVIDER.md`. The plane is deliberately smaller than the Gemini
//! one: the subscription serves an Anthropic-compatible endpoint, so the engine's native protocol
//! is forwarded without a translation layer.
//!
//! Dormant: the plane is not wired into `server` yet, so nothing routes here.

pub mod client;
pub mod pool;
pub mod queue;
pub mod roster;
pub mod selection;
pub mod transport;
