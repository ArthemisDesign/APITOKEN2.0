//! Control-плоскость движка: `/admin/*` — управление аккаунтами/ключами/балансом за ОТДЕЛЬНЫМ
//! служебным токеном (`CLAUDE_API_CONTROL_KEY`). Это контракт, которым будущая КОММЕРЦИЯ
//! (отдельный сервис) управляет движком, не имея прав неметеренного форвардинга. Движок остаётся
//! авторитетом ЖИВОГО баланса — коммерция лишь создаёт аккаунты/ключи и кредитует (идемпотентно).
//!
//! Все записи идут через тот же single-writer актор биллинга (`AsyncBilling`) — дисциплина единого
//! писателя, никаких гонок с reserve/settle горячего пути.

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use forward::{control_authed, AppState};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;

/// Гейт control-плоскости: admin/control-ключ (или loopback-dev). Возвращает 401-ответ, если нет прав.
fn deny(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> Option<Response> {
    if control_authed(app, headers, peer) {
        None
    } else {
        Some((StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response())
    }
}

/// Биллинг обязателен для control-операций (аккаунты/деньги живут в нём). Нет → 503.
/// (Err-вариант — axum `Response`, намеренно «большой»: это ранний ответ ошибки, не горячий путь.)
#[allow(clippy::result_large_err)]
fn billing(app: &AppState) -> Result<&std::sync::Arc<forward::AsyncBilling>, Response> {
    app.billing.as_ref().ok_or_else(||
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "billing disabled"}))).into_response())
}

fn usd_to_nano(usd: f64) -> i64 { (usd * 1e9).round() as i64 }

#[derive(Deserialize)]
pub struct CreateAccountReq {
    handle: Option<String>,
    mult_bp: Option<i64>,
}

/// POST /admin/account — создать аккаунт. Тело: {handle?, mult_bp?}. → {account, mult_bp, handle}.
pub async fn create_account(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<CreateAccountReq>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let id = match crate::gen_account_id() {
        Ok(i) => i,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };
    let mult = req.mult_bp.unwrap_or(app.cfg.default_mult_bp);
    if b.create_account(&id, req.handle.as_deref(), mult).await {
        (StatusCode::OK, Json(json!({"account": id, "mult_bp": mult, "handle": req.handle}))).into_response()
    } else {
        (StatusCode::CONFLICT, Json(json!({"error": "create failed"}))).into_response()
    }
}

/// GET /admin/account/{id} — состояние аккаунта (баланс/резерв/spent/статус). → 404 если нет.
pub async fn get_account(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    match b.account(&id).await {
        Some(a) => Json(json!({
            "account": a.id,
            "balance_nano": a.balance_nano,
            "spent_nano": a.spent_nano,
            "reserved_nano": a.reserved_nano,
            "balance": metering::nano_to_usd_string(a.balance_nano as i128),
            "mult_bp": a.mult_bp,
            "status": a.status,
            "handle": a.handle,
        })).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response(),
    }
}

/// Маска ключа для листинга (полный ключ виден лишь единожды при выпуске): sk-pool-abcd…wxyz.
fn mask(k: &str) -> String {
    if k.len() <= 16 { return format!("{}…", &k[..k.len().min(6)]); }
    format!("{}…{}", &k[..12], &k[k.len() - 4..])
}

/// GET /admin/account/{id}/keys — ключи аккаунта (маскированные) + метаданные для дашборда.
pub async fn list_keys(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let keys: Vec<_> = b.keys_by_account(&id).await.into_iter().map(|k| json!({
        "key_masked": mask(&k.key),
        "label": k.label,
        "status": k.status,
        "spent_nano": k.spent_nano,
        "spent": metering::nano_to_usd_string(k.spent_nano as i128),
    })).collect();
    Json(json!({"account": id, "keys": keys})).into_response()
}

#[derive(Deserialize)]
pub struct LedgerQuery { limit: Option<i64> }

/// GET /admin/account/{id}/ledger?limit=N — история движений баланса (свежие сверху).
/// kind: topup (пополнение) | charge (списание) | adjust (коррекция). Для дашборда «расход/платежи».
pub async fn list_ledger(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<LedgerQuery>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let rows: Vec<_> = b.ledger(&id, q.limit.unwrap_or(50)).await.into_iter().map(|e| json!({
        "id": e.id,
        "kind": e.kind,
        "amount_nano": e.amount_nano,
        "amount": metering::nano_to_usd_string(e.amount_nano as i128),
        "key_masked": e.key.as_deref().map(mask),
        "ref": e.reference,
        "balance_after_nano": e.balance_after_nano,
        "ts": e.ts,
    })).collect();
    Json(json!({"account": id, "entries": rows})).into_response()
}

#[derive(Deserialize)]
pub struct CreditReq {
    usd: Option<f64>,
    amount_nano: Option<i64>,
    r#ref: Option<String>,
}

/// POST /admin/account/{id}/credit — зачислить средства. Тело: {usd? | amount_nano?, ref?}.
/// Идемпотентно по `ref` (повторный вебхук платежа НЕ задвоит). → {account, balance_nano}.
pub async fn credit_account(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<CreditReq>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let nano = match req.amount_nano.or_else(|| req.usd.map(usd_to_nano)) {
        Some(n) => n,
        None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "need usd or amount_nano"}))).into_response(),
    };
    match b.topup(&id, nano, req.r#ref.as_deref()).await {
        Some(bal) => Json(json!({
            "account": id, "balance_nano": bal, "balance": metering::nano_to_usd_string(bal as i128),
        })).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response(),
    }
}

#[derive(Deserialize)]
pub struct StatusReq { status: String }

fn valid_status(s: &str) -> bool { matches!(s, "active" | "disabled") }

/// POST /admin/account/{id}/status — active|disabled. → {updated}.
pub async fn account_status(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<StatusReq>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    if !valid_status(&req.status) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "status must be active|disabled"}))).into_response();
    }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let n = b.account_status(&id, &req.status).await;
    if n == 0 { return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response(); }
    Json(json!({"account": id, "status": req.status, "updated": n})).into_response()
}

#[derive(Deserialize)]
pub struct IssueKeyReq {
    account_id: String,
    label: Option<String>,
}

/// POST /admin/key — выпустить ключ доступа к аккаунту. Тело: {account_id, label?}. → {key, account}.
/// Аккаунт обязан существовать (иначе висячий ключ). Сам ключ показываем ЕДИНСТВЕННЫЙ раз.
pub async fn issue_key(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<IssueKeyReq>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    if b.account(&req.account_id).await.is_none() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "unknown account"}))).into_response();
    }
    let key = match crate::gen_key() {
        Ok(k) => k,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };
    if b.issue_key(&key, &req.account_id, req.label.as_deref()).await {
        (StatusCode::OK, Json(json!({"key": key, "account": req.account_id, "label": req.label}))).into_response()
    } else {
        (StatusCode::CONFLICT, Json(json!({"error": "issue failed"}))).into_response()
    }
}

/// POST /admin/key/{key}/status — active|disabled (отзыв ключа). → {updated}.
pub async fn key_status(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Json(req): Json<StatusReq>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    if !valid_status(&req.status) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "status must be active|disabled"}))).into_response();
    }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let n = b.key_status(&key, &req.status).await;
    if n == 0 { return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response(); }
    Json(json!({"status": req.status, "updated": n})).into_response()
}
