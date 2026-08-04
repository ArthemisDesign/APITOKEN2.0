//! GLM (Zhipu AI / Z.ai) Coding Plan subscription plane.
//!
//! Contract: `docs/engine/GLM_PROVIDER.md`. Like KIMI the plane is deliberately smaller than
//! the Gemini one: the subscription serves an Anthropic-compatible endpoint, so the engine's
//! native protocol is forwarded without a translation layer. Unlike KIMI the credential is a
//! static API key — there is no OAuth refresh family anywhere in this plane.
//!
//! This is the runtime layer: configuration, per-profile transport and error classification,
//! roster loading, the quota wire, selection, the attempt loop, the turn-evidence queue and
//! the live gateway that ties them to the Anthropic Messages plane.
//!
//! The plane is backend-only and **off by default** (`docs/engine/GLM_PROVIDER.md` §0).

pub mod client;
pub mod config;
mod gateway;
pub mod pool;
pub mod queue;
pub mod roster;
pub mod selection;
pub mod transport;

pub use gateway::{
    bounded_plan_label, GlmGateway, GlmOperationalStatus, GlmProfileStatus, GlmQuotaWindowStatus,
};
pub(crate) use gateway::{GlmBillingInput, GlmRequest};
