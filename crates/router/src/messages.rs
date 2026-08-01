//! Universal `POST /v1/messages` and `/v1/messages/count_tokens` entry.
//! Shared model dispatch and optional serial fallback live in `routing.rs`;
//! synthetic errors keep the Anthropic Messages envelope.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::response::Response;

use crate::routing::{self, Surface};
use crate::AppState;

pub async fn proxy_messages(State(state): State<Arc<AppState>>, req: Request) -> Response {
    routing::proxy_universal(state, req, Surface::Messages).await
}
