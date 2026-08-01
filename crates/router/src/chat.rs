//! Universal `POST /v1/chat/completions` entry. Shared model dispatch and
//! optional serial fallback live in `routing.rs`; plane adapters own protocol
//! translation and the response remains streaming.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::response::Response;

use crate::routing::{self, Surface};
use crate::AppState;

pub async fn proxy_chat(State(state): State<Arc<AppState>>, req: Request) -> Response {
    routing::proxy_universal(state, req, Surface::Chat).await
}
