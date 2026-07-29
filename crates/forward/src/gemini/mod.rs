//! Native Gemini-compatible surface backed by encrypted paid Code Assist OAuth profiles.

mod api;
mod billing;
mod config;
mod pool;

pub use api::api as gemini_api;
pub use config::{GeminiConfig, GeminiModel, GeminiPrices, GeminiProfileSpec, GeminiProfilesFile};
pub use pool::{GeminiGateway, GeminiOperationalStatus, GeminiProfileStatus};
