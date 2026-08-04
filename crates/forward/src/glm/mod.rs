//! GLM (Zhipu AI / Z.ai) Coding Plan subscription plane.
//!
//! Contract: `docs/engine/GLM_PROVIDER.md`. Like KIMI the plane is deliberately smaller than
//! the Gemini one: the subscription serves an Anthropic-compatible endpoint, so the engine's
//! native protocol is forwarded without a translation layer. Unlike KIMI the credential is a
//! static API key — there is no OAuth refresh family anywhere in this plane.
//!
//! This is the runtime-primitives layer: configuration, per-profile transport and error
//! classification, roster loading, the quota wire, selection, the attempt loop and the
//! turn-evidence queue. The gateway arrives as a separate step and extends this surface.
//!
//! The plane is backend-only and **off by default** (`docs/engine/GLM_PROVIDER.md` §0).

pub mod client;
pub mod config;
pub mod pool;
pub mod queue;
pub mod roster;
pub mod selection;
pub mod transport;
