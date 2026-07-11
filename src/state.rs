//! Общее состояние сервера, клонируется в каждый axum-хендлер.

use crate::config::Config;
use crate::pool::Pool;
use crate::upstream::Clients;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub pool: Arc<Pool>,
    pub clients: Arc<Clients>,
}
