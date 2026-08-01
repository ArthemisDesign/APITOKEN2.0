//! Native Gemini-compatible surface backed by encrypted paid Antigravity OAuth profiles.

mod api;
mod billing;
mod calibration;
mod chat;
mod config;
mod pool;
mod responses;
mod transport;

pub use api::api as gemini_api;
pub use chat::gemini_chat_completions;
pub use responses::gemini_responses;
pub use calibration::WindowCalibration;
pub(crate) use calibration::{apply_observation_with_history, ESTIMATOR_VERSION};
pub use config::{
    subscription_model_supported, GeminiConfig, GeminiModel, GeminiPrices, GeminiProfileSpec,
    GeminiProfilesFile, GEMINI_NODE_EXPECTED_JA3, GEMINI_NODE_EXPECTED_JA4,
    GEMINI_NODE_FETCH_EXPECTED_JA3, GEMINI_NODE_FETCH_EXPECTED_JA4,
    GEMINI_NODE_FETCH_TRANSPORT_PROFILE, GEMINI_NODE_TRANSPORT_PROFILE,
};
pub use pool::{
    GeminiGateway, GeminiModelStatus, GeminiOperationalStatus, GeminiProfileStatus,
    GeminiWindowCapacityReport,
};
