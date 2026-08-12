//! Tripo3D (VAST / Holymolly) prepaid API plane.
//!
//! Contract: `docs/engine/TRIPO3D_PROVIDER.md`. Unlike KIMI/GLM this plane is NOT an
//! Anthropic-compatible chat provider: Tripo3D is a task-based media API, so the request
//! lifecycle is create task → poll → download the artifact into our own storage → settle
//! exactly from the provider-reported `consumed_credit`. There is no streaming and no
//! first-public-byte boundary; the money boundary is a successful upstream task creation.
//!
//! This is the runtime layer: configuration, per-profile transport and error classification,
//! roster loading, the balance/task wire, selection, the attempt loop, the turn-evidence queue
//! and the task-lifecycle gateway that ties them to the plane's own REST endpoints.
//!
//! The plane is backend-only and **off by default** (`docs/engine/TRIPO3D_PROVIDER.md` §0):
//! pooling/resale needs written consent from Holymolly, so until that exists the plane is
//! internal capacity and calibration only, with no public catalog, router namespace or
//! storefront.

// Dormant until the task-lifecycle gateway lands (docs/engine/TRIPO3D_PROVIDER.md §8): these
// modules are consumed only by their own tests so far. The allowance is module-wide rather
// than per-item so the compiler does not force a piecemeal gateway.
#![allow(dead_code)]

pub mod client;
pub mod config;
pub mod pool;
pub mod queue;
pub mod roster;
pub mod selection;
pub mod transport;
