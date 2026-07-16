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
        forward::Metrics::inc(&app.metrics.auth_failures); // спайк = скан/брутфорс control-ключа
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
    // A commercial registration retry must not orphan duplicate engine accounts. Handles are unique,
    // so returning the existing account makes provisioning idempotent for a stable user handle.
    if let Some(handle) = req.handle.as_deref() {
        if let Some(existing) = b.account_by_handle(handle).await {
            return Json(json!({
                "account": existing.id, "mult_bp": existing.mult_bp, "handle": existing.handle,
            })).into_response();
        }
    }
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
    if b.account(&id).await.is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response();
    }
    let keys: Vec<_> = b.keys_by_account(&id).await.into_iter().map(|k| json!({
        "key_id": k.key_id,
        "key_masked": mask(&k.key),
        "label": k.label,
        "status": k.status,
        "spent_nano": k.spent_nano,
        "spent": metering::nano_to_usd_string(k.spent_nano as i128),
    })).collect();
    Json(json!({"account": id, "keys": keys})).into_response()
}

#[derive(Deserialize)]
pub struct LedgerQuery { limit: Option<i64>, after_id: Option<i64> }

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
    if b.account(&id).await.is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response();
    }
    let limit = q.limit.unwrap_or(50);
    let ledger = match q.after_id {
        Some(after) => b.ledger_after(&id, after, limit).await,
        None => b.ledger(&id, limit).await,
    };
    let rows: Vec<_> = ledger.into_iter().map(|e| json!({
        "id": e.id,
        "kind": e.kind,
        "amount_nano": e.amount_nano,
        "amount": metering::nano_to_usd_string(e.amount_nano as i128),
        "key_masked": e.key.as_deref().map(mask),
        "ref": e.reference,
        "balance_after_nano": e.balance_after_nano,
        "ts": e.ts,
        "model": e.model,
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
    // Идемпотентность пополнения держится ТОЛЬКО на UNIQUE(ref) для kind=topup И ref IS NOT NULL.
    // Поэтому ПОЛОЖИТЕЛЬНОЕ зачисление (платёж) ОБЯЗАНО нести ref = id транзакции — иначе ретрай вебхука
    // задвоил бы баланс. Отрицательная коррекция (adjust, ручная) ref не требует. Тримим ref.
    let ref_trimmed = req.r#ref.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if nano > 0 && ref_trimmed.is_none() {
        return (StatusCode::BAD_REQUEST,
            Json(json!({"error": "ref required for credit (idempotency); use payment/transaction id"}))).into_response();
    }
    match b.topup(&id, nano, ref_trimmed).await {
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
pub struct PricingReq { mult_bp: i64 }

/// POST /admin/account/{id}/pricing — change the multiplier used for future charges.
pub async fn account_pricing(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PricingReq>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    if !(0..=10_000).contains(&req.mult_bp) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "mult_bp must be 0..10000"}))).into_response();
    }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let n = b.account_multiplier(&id, req.mult_bp).await;
    if n == 0 { return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response(); }
    Json(json!({"account": id, "mult_bp": req.mult_bp, "updated": n})).into_response()
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
        match b.get(&key).await {
            Some(row) => (StatusCode::OK, Json(json!({
                "key": key, "key_id": row.key_id, "account": req.account_id, "label": req.label,
            }))).into_response(),
            None => (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "issued key could not be read"}))).into_response(),
        }
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

/// POST /admin/key-id/{key_id}/status — revoke/enable through a stable non-secret identifier.
pub async fn key_status_by_id(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(key_id): Path<String>,
    Json(req): Json<StatusReq>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    if !valid_status(&req.status) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "status must be active|disabled"}))).into_response();
    }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    let n = b.key_status_by_id(&key_id, &req.status).await;
    if n == 0 { return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response(); }
    Json(json!({"key_id": key_id, "status": req.status, "updated": n})).into_response()
}

#[derive(Deserialize)]
pub struct UsageQuery { window: Option<String> }

/// Окно вида "30d"/"7d"/"24h"/"90d"/"all" → нижняя граница ts (unix-сек). Дефолт — 30 дней.
fn window_since(window: &str) -> (String, i64) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64).unwrap_or(0);
    let w = window.trim();
    if w.eq_ignore_ascii_case("all") { return ("all".into(), 0); }
    let (num, unit_secs) = if let Some(n) = w.strip_suffix('h') { (n.parse::<i64>().ok(), 3_600) }
        else if let Some(n) = w.strip_suffix('d') { (n.parse::<i64>().ok(), 86_400) }
        else { (None, 0) };
    match num {
        Some(n) if n > 0 => (w.to_string(), now - n * unit_secs),
        _ => ("30d".into(), now - 30 * 86_400), // дефолт при пустом/битом окне
    }
}

/// GET /admin/account/{id}/usage?window=30d — разбивка расхода по токенам/моделям для дашборда.
/// Долларовый эквивалент по корзинам считаем здесь (per-model суммы токенов × официальные ставки).
pub async fn list_usage(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<UsageQuery>,
) -> Response {
    if let Some(r) = deny(&app, &headers, &peer) { return r; }
    let b = match billing(&app) { Ok(b) => b, Err(r) => return r };
    if b.account(&id).await.is_none() {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response();
    }
    let (window, since) = window_since(q.window.as_deref().unwrap_or("30d"));
    let aggs = b.usage_by_model(&id, since).await;

    // Пер-корзинный официальный $ (×1.0): суммируем токены×ставку модели по всем моделям.
    let (mut in_n, mut out_n, mut cr_n, mut cw_n, mut ws_n): (i128, i128, i128, i128, i128) = (0, 0, 0, 0, 0);
    let (mut in_t, mut out_t, mut cr_t, mut cw_t, mut ws_r): (i64, i64, i64, i64, i64) = (0, 0, 0, 0, 0);
    let (mut total_official, mut total_charged, mut total_requests): (i128, i128, i64) = (0, 0, 0);
    let models: Vec<_> = aggs.iter().map(|m| {
        let p = metering::model_prices(&m.model);
        in_n += m.input_tokens as i128 * p.input;
        out_n += m.output_tokens as i128 * p.output;
        cr_n += m.cache_read_tokens as i128 * p.cache_read;
        cw_n += m.cache_write_5m_tokens as i128 * p.cache_write_5m + m.cache_write_1h_tokens as i128 * p.cache_write_1h;
        ws_n += m.web_search_requests as i128 * metering::WEB_SEARCH_NANO;
        in_t += m.input_tokens; out_t += m.output_tokens; cr_t += m.cache_read_tokens;
        cw_t += m.cache_write_5m_tokens + m.cache_write_1h_tokens; ws_r += m.web_search_requests;
        total_official += m.real_nano as i128; total_charged += m.charge_nano as i128; total_requests += m.requests;
        json!({
            "model": m.model,
            "requests": m.requests,
            "input_tokens": m.input_tokens,
            "output_tokens": m.output_tokens,
            "cache_read_tokens": m.cache_read_tokens,
            "cache_write_5m_tokens": m.cache_write_5m_tokens,
            "cache_write_1h_tokens": m.cache_write_1h_tokens,
            "web_search_requests": m.web_search_requests,
            "official_nano": m.real_nano.to_string(),
            "charged_nano": m.charge_nano.to_string(),
        })
    }).collect();

    Json(json!({
        "account": id,
        "window": window,
        "requests": total_requests,
        "total_official_nano": total_official.to_string(),
        "total_charged_nano": total_charged.to_string(),
        "buckets": {
            "input": { "tokens": in_t, "official_nano": in_n.to_string() },
            "output": { "tokens": out_t, "official_nano": out_n.to_string() },
            "cache_read": { "tokens": cr_t, "official_nano": cr_n.to_string() },
            "cache_write": { "tokens": cw_t, "official_nano": cw_n.to_string() },
            "web_search": { "requests": ws_r, "official_nano": ws_n.to_string() },
        },
        "models": models,
    })).into_response()
}
