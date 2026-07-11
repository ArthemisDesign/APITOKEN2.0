//! Общее состояние форвардинга, клонируется в каждый axum-хендлер.

use crate::config::ProxyConfig;
use crate::upstream::Clients;
use pool::Pool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<ProxyConfig>,
    pub pool: Arc<Pool>,
    pub clients: Arc<Clients>,
}
