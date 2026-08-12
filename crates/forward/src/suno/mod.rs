//! Suno (suno.com) subscription session-pool plane.
//!
//! Contract: `docs/engine/SUNO_PROVIDER.md`. Unlike KIMI/GLM this plane is NOT an
//! Anthropic-compatible chat provider: Suno is a task-based media API, so the request lifecycle
//! is create generation → poll (feed/clip) → download the audio into our own storage → settle
//! from the attributed credit delta (or the documented conservative reserve when attribution is
//! ambiguous). There is no streaming and no first-public-byte boundary; the money boundary is a
//! successful upstream generation creation.
//!
//! The credential is a subscription web session (the Clerk `__client` cookie), not an API key:
//! short-lived JWTs are minted on demand through the profile's pinned egress, and a mint
//! response may rotate the underlying Clerk token via `set-cookie`, so the runtime holds a
//! per-profile single-flight from mint through envelope re-seal (manifest §2, the KIMI
//! rotating-family discipline). There is no official public API: every wire fact is
//! `oss-hypothesis` and fails closed until a controlled live run proves it.
//!
//! This is the runtime layer: configuration, per-profile transport and error classification,
//! roster loading, the session/quota wire, selection, the attempt loop, the turn-evidence queue
//! and the generation gateway that ties them to the plane's own REST endpoints.
//!
//! The plane is backend-only and **off by default** (`docs/engine/SUNO_PROVIDER.md` §0): the
//! ToS prohibits resale and there is no partner agreement, so until that changes the plane is
//! internal capacity and calibration only, with no public catalog, router namespace or
//! storefront.
//!
//! Dormant until the gateway commit lands: consumed only by its own tests so far.
#![allow(dead_code)]

pub mod client;
pub mod config;
pub mod pool;
pub mod queue;
pub mod roster;
pub mod selection;
pub mod transport;
