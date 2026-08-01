//! Universal `POST /v1/responses` entry. Shared model dispatch and optional
//! serial fallback live in `routing.rs`. Stored Responses endpoints remain
//! native OpenAI routes in `main.rs` and never use this handler.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::response::Response;

use crate::routing::{self, Surface};
use crate::AppState;

pub async fn proxy_responses(State(state): State<Arc<AppState>>, req: Request) -> Response {
    routing::proxy_universal(state, req, Surface::Responses).await
}
