//! Control-плоскость движка: `/admin/*` — управление аккаунтами/ключами/балансом за ОТДЕЛЬНЫМ
//! служебным токеном (`CLAUDE_API_CONTROL_KEY`). Это контракт, которым будущая КОММЕРЦИЯ
//! (отдельный сервис) управляет движком, не имея прав неметеренного форвардинга. Движок остаётся
//! авторитетом ЖИВОГО баланса — коммерция лишь создаёт аккаунты/ключи и кредитует (идемпотентно).
//!
//! Все записи идут через тот же single-writer актор биллинга (`AsyncBilling`) — дисциплина единого
//! писателя, никаких гонок с reserve/settle горячего пути.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use forward::AppState;
use serde::Deserialize;
use serde_json::{json, Value};

/// Биллинг обязателен для control-операций (аккаунты/деньги живут в нём). Нет → 503.
/// (Err-вариант — axum `Response`, намеренно «большой»: это ранний ответ ошибки, не горячий путь.)
#[allow(clippy::result_large_err)]
fn billing(app: &AppState) -> Result<&std::sync::Arc<forward::AsyncBilling>, Response> {
    app.billing.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "billing disabled"})),
        )
            .into_response()
    })
}

fn authority_unavailable(context: &str, error: anyhow::Error) -> Response {
    eprintln!("billing authority {context} failed: {error:#}");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "billing authority unavailable"})),
    )
        .into_response()
}

fn is_control_conflict(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    message.contains("idempotency reference")
        || message.contains("unique constraint")
        || message.contains("constraint failed")
        || message.contains("duplicate key")
        || message.contains("already exists")
}

#[derive(Deserialize)]
pub struct CreateAccountReq {
    handle: Option<String>,
    mult_bp: Option<i64>,
}

/// POST /admin/account — создать аккаунт. Тело: {handle?, mult_bp?}. → {account, mult_bp, handle}.
pub async fn create_account(
    State(app): State<AppState>,
    Json(req): Json<CreateAccountReq>,
) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    // A commercial registration retry must not orphan duplicate engine accounts. Handles are unique,
    // so returning the existing account makes provisioning idempotent for a stable user handle.
    if let Some(handle) = req.handle.as_deref() {
        match b.account_by_handle(handle).await {
            Ok(Some(existing)) => {
                return Json(json!({
                    "account": existing.id, "mult_bp": existing.mult_bp, "handle": existing.handle,
                }))
                .into_response()
            }
            Ok(None) => {}
            Err(error) => return authority_unavailable("account lookup", error),
        }
    }
    let id = match crate::gen_account_id() {
        Ok(i) => i,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };
    let mult = req.mult_bp.unwrap_or(app.cfg.default_mult_bp);
    match b.create_account(&id, req.handle.as_deref(), mult).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"account": id, "mult_bp": mult, "handle": req.handle})),
        )
            .into_response(),
        Err(error) if is_control_conflict(&error) => (
            StatusCode::CONFLICT,
            Json(json!({"error": "account already exists"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("account creation", error),
    }
}

/// GET /admin/account/{id} — состояние аккаунта (баланс/резерв/spent/статус). → 404 если нет.
pub async fn get_account(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b.account(&id).await {
        Ok(Some(a)) => Json(json!({
            "account": a.id,
            "balance_nano": a.balance_nano,
            "spent_nano": a.spent_nano,
            "reserved_nano": a.reserved_nano,
            "balance": metering::nano_to_usd_string(a.balance_nano as i128),
            "mult_bp": a.mult_bp,
            "status": a.status,
            "handle": a.handle,
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown account"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("account lookup", error),
    }
}

/// Маска ключа для листинга (полный ключ виден лишь единожды при выпуске): sk-pool-abcd…wxyz.
fn mask(k: &str) -> String {
    if k.len() <= 16 {
        return format!("{}…", &k[..k.len().min(6)]);
    }
    format!("{}…{}", &k[..12], &k[k.len() - 4..])
}

/// GET /admin/account/{id}/keys — ключи аккаунта (маскированные) + метаданные для дашборда.
pub async fn list_keys(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b.account(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "unknown account"})),
            )
                .into_response()
        }
        Err(error) => return authority_unavailable("account lookup", error),
    }
    let key_rows = match b.keys_by_account(&id).await {
        Ok(rows) => rows,
        Err(error) => return authority_unavailable("key listing", error),
    };
    let keys: Vec<_> = key_rows
        .into_iter()
        .map(|k| {
            json!({
                "key_id": k.key_id,
                "key_masked": mask(&k.key),
                "label": k.label,
                "status": k.status,
                "spent_nano": k.spent_nano,
                "spent": metering::nano_to_usd_string(k.spent_nano as i128),
                "reserved_nano": k.reserved_nano,
                "spend_limit_nano": k.spend_limit_nano,
                "expires_ts": k.expires_ts,
                "created_ts": k.created_ts,
                "last_used_ts": k.last_used_ts,
            })
        })
        .collect();
    Json(json!({"account": id, "keys": keys})).into_response()
}

#[derive(Deserialize)]
pub struct LedgerQuery {
    limit: Option<i64>,
    after_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerAckReq {
    last_id: String,
}

/// The pricing worker durably acknowledges a consumed ledger cursor. Retention removes old charge
/// detail only below this watermark, so a lagging/restarted worker cannot lose events.
pub async fn ack_ledger(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<LedgerAckReq>,
) -> Response {
    let billing = match billing(&app) {
        Ok(billing) => billing,
        Err(response) => return response,
    };
    let Ok(last_id) = req.last_id.parse::<i64>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "last_id must be a nonnegative integer"})),
        )
            .into_response();
    };
    if last_id < 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "last_id must be a nonnegative integer"})),
        )
            .into_response();
    }
    match billing.ledger_ack("pricing", &id, last_id).await {
        Ok(_) => Json(json!({
            "account": id, "consumer": "pricing", "last_id": last_id.to_string(),
        }))
        .into_response(),
        Err(error) => authority_unavailable("ledger checkpoint", error),
    }
}

/// GET /admin/account/{id}/ledger?limit=N — история движений баланса (свежие сверху).
/// kind: topup (пополнение) | charge (списание) | adjust (коррекция). Для дашборда «расход/платежи».
pub async fn list_ledger(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<LedgerQuery>,
) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b.account(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "unknown account"})),
            )
                .into_response()
        }
        Err(error) => return authority_unavailable("account lookup", error),
    }
    let limit = q.limit.unwrap_or(50);
    let ledger = match q.after_id {
        Some(after) => b.ledger_after(&id, after, limit).await,
        None => b.ledger(&id, limit).await,
    };
    let ledger = match ledger {
        Ok(rows) => rows,
        Err(error) => return authority_unavailable("ledger listing", error),
    };
    let rows: Vec<_> = ledger
        .into_iter()
        .map(|e| {
            json!({
                "id": e.id,
                "kind": e.kind,
                "amount_nano": e.amount_nano,
                "amount": metering::nano_to_usd_string(e.amount_nano as i128),
                "key_masked": e.key.as_deref().map(mask),
                "ref": e.reference,
                "balance_after_nano": e.balance_after_nano,
                "ts": e.ts,
                "model": e.model,
            })
        })
        .collect();
    Json(json!({"account": id, "entries": rows})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditReq {
    amount_nano: Option<String>,
    r#ref: Option<String>,
}

fn qualified_topup_ref(reference: &str) -> bool {
    reference
        .split_once(':')
        .is_some_and(|(provider, transaction)| {
            !provider.is_empty()
                && !transaction.is_empty()
                && !provider.chars().any(char::is_whitespace)
                && !transaction.chars().any(char::is_whitespace)
        })
}

/// POST /admin/account/{id}/credit — зачислить средства. Тело: {amount_nano: "...", ref?}.
/// Идемпотентно по `ref` (повторный вебхук платежа НЕ задвоит). → {account, balance_nano}.
pub async fn credit_account(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreditReq>,
) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let nano = match req.amount_nano.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => match value.parse::<i64>() {
            Ok(n) => n,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "amount_nano must be an i64 decimal string"})),
                )
                    .into_response()
            }
        },
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "amount_nano string required"})),
            )
                .into_response()
        }
    };
    // Идемпотентность пополнения держится ТОЛЬКО на UNIQUE(ref) для kind=topup И ref IS NOT NULL.
    // Поэтому ПОЛОЖИТЕЛЬНОЕ зачисление (платёж) ОБЯЗАНО нести provider-qualified ref — иначе
    // независимые провайдеры с одинаковым transaction id столкнутся в глобальном UNIQUE-индексе.
    let ref_trimmed = req
        .r#ref
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if nano > 0 {
        match ref_trimmed {
            Some(reference) if qualified_topup_ref(reference) => {}
            _ => return (StatusCode::BAD_REQUEST,
                Json(json!({"error": "ref must be provider-qualified as <provider>:<transaction-id>"}))).into_response(),
        }
    }
    match b.topup(&id, nano, ref_trimmed).await {
        Ok(Some(bal)) => Json(json!({
                "account": id, "balance_nano": bal, "balance": metering::nano_to_usd_string(bal as i128),
            })).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "unknown account"}))).into_response(),
        Err(error) if is_control_conflict(&error) => (StatusCode::CONFLICT,
            Json(json!({"error": "ref already used by a different payment"}))).into_response(),
        Err(error) => authority_unavailable("account credit", error),
    }
}

#[derive(Deserialize)]
pub struct StatusReq {
    status: String,
}

#[derive(Deserialize)]
pub struct LabelReq {
    label: String,
}

fn valid_status(s: &str) -> bool {
    matches!(s, "active" | "disabled")
}

/// POST /admin/account/{id}/status — active|disabled. → {updated}.
pub async fn account_status(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<StatusReq>,
) -> Response {
    if !valid_status(&req.status) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "status must be active|disabled"})),
        )
            .into_response();
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let n = match b.account_status(&id, &req.status).await {
        Ok(n) => n,
        Err(error) => return authority_unavailable("account status update", error),
    };
    if n == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown account"})),
        )
            .into_response();
    }
    Json(json!({"account": id, "status": req.status, "updated": n})).into_response()
}

#[derive(Deserialize)]
pub struct PricingReq {
    mult_bp: i64,
}

/// POST /admin/account/{id}/pricing — change the multiplier used for future charges.
pub async fn account_pricing(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PricingReq>,
) -> Response {
    if !(0..=10_000).contains(&req.mult_bp) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "mult_bp must be 0..10000"})),
        )
            .into_response();
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let n = match b.account_multiplier(&id, req.mult_bp).await {
        Ok(n) => n,
        Err(error) => return authority_unavailable("account pricing update", error),
    };
    if n == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown account"})),
        )
            .into_response();
    }
    Json(json!({"account": id, "mult_bp": req.mult_bp, "updated": n})).into_response()
}

#[derive(Deserialize)]
pub struct IssueKeyReq {
    account_id: String,
    label: Option<String>,
    spend_limit_nano: Option<String>,
    expires_ts: Option<i64>,
}

/// POST /admin/key — выпустить ключ доступа к аккаунту. Тело: {account_id, label?}. → {key, account}.
/// Аккаунт обязан существовать (иначе висячий ключ). Сам ключ показываем ЕДИНСТВЕННЫЙ раз.
pub async fn issue_key(State(app): State<AppState>, Json(req): Json<IssueKeyReq>) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b.account(&req.account_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "unknown account"})),
            )
                .into_response()
        }
        Err(error) => return authority_unavailable("account lookup", error),
    }
    let label = req.label.map(|value| value.trim().to_owned());
    if label
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.chars().count() > 64)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "label must be 1..64 characters after trimming"})),
        )
            .into_response();
    }
    let spend_limit_nano = match req.spend_limit_nano {
        Some(value) => match value.parse::<i64>() {
            Ok(parsed) if parsed > 0 => Some(parsed),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "spend_limit_nano must be a positive integer string"})),
                )
                    .into_response()
            }
        },
        None => None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    if req.expires_ts.is_some_and(|expires| expires <= now) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "expires_ts must be in the future"})),
        )
            .into_response();
    }
    let key = match crate::gen_key() {
        Ok(k) => k,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };
    match b
        .issue_key(
            &key,
            &req.account_id,
            label.as_deref(),
            spend_limit_nano,
            req.expires_ts,
        )
        .await
    {
        Ok(()) => match b.get(&key).await {
            Ok(Some(row)) => (
                StatusCode::OK,
                Json(json!({
                    "key": key, "key_id": row.key_id, "account": req.account_id, "label": label,
                    "spend_limit_nano": row.spend_limit_nano, "expires_ts": row.expires_ts,
                })),
            )
                .into_response(),
            Ok(None) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "issued key could not be read"})),
            )
                .into_response(),
            Err(error) => authority_unavailable("issued key lookup", error),
        },
        Err(error) if is_control_conflict(&error) => (
            StatusCode::CONFLICT,
            Json(json!({"error": "key already exists"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("key issuance", error),
    }
}

/// POST /admin/key-id/{key_id}/status — revoke/enable through a stable non-secret identifier.
pub async fn key_status_by_id(
    State(app): State<AppState>,
    Path(key_id): Path<String>,
    Json(req): Json<StatusReq>,
) -> Response {
    if !valid_status(&req.status) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "status must be active|disabled"})),
        )
            .into_response();
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let n = match b.key_status_by_id(&key_id, &req.status).await {
        Ok(n) => n,
        Err(error) => return authority_unavailable("key status update", error),
    };
    if n == 0 {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response();
    }
    Json(json!({"key_id": key_id, "status": req.status, "updated": n})).into_response()
}

/// POST /admin/key-id/{key_id}/label — rename through a stable non-secret identifier.
pub async fn key_label_by_id(
    State(app): State<AppState>,
    Path(key_id): Path<String>,
    Json(req): Json<LabelReq>,
) -> Response {
    let label = req.label.trim().to_owned();
    if label.is_empty() || label.chars().count() > 64 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "label must be 1..64 characters after trimming"})),
        )
            .into_response();
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let n = match b.key_label_by_id(&key_id, &label).await {
        Ok(n) => n,
        Err(error) => return authority_unavailable("key label update", error),
    };
    if n == 0 {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response();
    }
    Json(json!({"key_id": key_id, "label": label, "updated": n})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyPolicyReq {
    // Values deliberately use `Value`: unlike `Option<T>`, this keeps JSON null distinct from a
    // missing field, so replacement requests must explicitly choose keep/remove for both controls.
    spend_limit_nano: Value,
    expires_ts: Value,
}

/// POST /admin/account/{account_id}/key-id/{key_id}/policy — replace both mutable guardrails.
/// Null removes a guardrail. A limit below settled + reserved usage is rejected atomically.
pub async fn key_policy_by_id(
    State(app): State<AppState>,
    Path((account_id, key_id)): Path<(String, String)>,
    Json(req): Json<KeyPolicyReq>,
) -> Response {
    let spend_limit_nano = match req.spend_limit_nano {
        Value::Null => None,
        Value::String(value) => match value.parse::<i64>() {
            Ok(parsed) if parsed > 0 => Some(parsed),
            _ => return (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({"error": "spend_limit_nano must be null or a positive integer string"}),
                ),
            )
                .into_response(),
        },
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({"error": "spend_limit_nano must be null or a positive integer string"}),
                ),
            )
                .into_response()
        }
    };
    let expires_ts =
        match req.expires_ts {
            Value::Null => None,
            Value::Number(value) => match value.as_i64() {
                Some(parsed) => Some(parsed),
                None => return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "expires_ts must be null or a future integer timestamp"})),
                )
                    .into_response(),
            },
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "expires_ts must be null or a future integer timestamp"})),
                )
                    .into_response()
            }
        };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    if expires_ts.is_some_and(|expires| expires <= now) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "expires_ts must be null or in the future"})),
        )
            .into_response();
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b
        .key_policy_by_id(&account_id, &key_id, spend_limit_nano, expires_ts)
        .await
    {
        Ok(registry::KeyPolicyUpdate::Updated) => Json(json!({
            "key_id": key_id,
            "spend_limit_nano": spend_limit_nano,
            "expires_ts": expires_ts,
            "updated": 1,
        }))
        .into_response(),
        Ok(registry::KeyPolicyUpdate::NotFound) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response()
        }
        Ok(registry::KeyPolicyUpdate::LimitBelowUsage) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "spend limit is below settled and reserved usage",
                "code": "limit_below_committed",
            })),
        )
            .into_response(),
        Ok(registry::KeyPolicyUpdate::ExpiryNotFuture) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "expires_ts must be null or in the future"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("key policy update", error),
    }
}

#[derive(Deserialize)]
pub struct UsageQuery {
    window: Option<String>,
}

/// Окно вида "30d"/"7d"/"24h"/"90d"/"all" → нижняя граница ts (unix-сек). Дефолт — 30 дней.
fn window_since(window: &str) -> (String, i64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let w = window.trim();
    if w.eq_ignore_ascii_case("all") {
        return ("all".into(), 0);
    }
    let (num, unit_secs) = if let Some(n) = w.strip_suffix('h') {
        (n.parse::<i64>().ok(), 3_600)
    } else if let Some(n) = w.strip_suffix('d') {
        (n.parse::<i64>().ok(), 86_400)
    } else {
        (None, 0)
    };
    match num {
        Some(n) if n > 0 => (w.to_string(), now - n * unit_secs),
        _ => ("30d".into(), now - 30 * 86_400), // дефолт при пустом/битом окне
    }
}

/// GET /admin/account/{id}/usage?window=30d — разбивка расхода по токенам/моделям для дашборда.
/// Долларовый эквивалент по корзинам считаем здесь (per-model суммы токенов × официальные ставки).
pub async fn list_usage(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<UsageQuery>,
) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b.account(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "unknown account"})),
            )
                .into_response()
        }
        Err(error) => return authority_unavailable("account lookup", error),
    }
    let (window, since) = window_since(q.window.as_deref().unwrap_or("30d"));
    let aggs = match b.usage_by_model(&id, since).await {
        Ok(aggs) => aggs,
        Err(error) => return authority_unavailable("usage aggregation", error),
    };

    // Sum immutable monetary components captured at settlement. Legacy rows created before
    // migration 0004 retain their exact real_nano total but cannot be truthfully split across
    // buckets; report that amount as unattributed instead of repricing history at today's tariff.
    let (mut in_n, mut out_n, mut cr_n, mut cw_n, mut ws_n): (i128, i128, i128, i128, i128) =
        (0, 0, 0, 0, 0);
    let mut unattributed_n: i128 = 0;
    let (mut in_t, mut out_t, mut cr_t, mut cw_t, mut ws_r): (i64, i64, i64, i64, i64) =
        (0, 0, 0, 0, 0);
    let (mut total_official, mut total_charged, mut total_requests): (i128, i128, i64) = (0, 0, 0);
    let models: Vec<_> = aggs
        .iter()
        .map(|m| {
            let stored_components = m.input_nano as i128
                + m.output_nano as i128
                + m.cache_read_nano as i128
                + m.cache_write_5m_nano as i128
                + m.cache_write_1h_nano as i128
                + m.web_search_nano as i128;
            if stored_components == 0 && m.real_nano > 0 {
                unattributed_n += m.real_nano as i128;
            } else {
                in_n += m.input_nano as i128;
                out_n += m.output_nano as i128;
                cr_n += m.cache_read_nano as i128;
                cw_n += m.cache_write_5m_nano as i128 + m.cache_write_1h_nano as i128;
                ws_n += m.web_search_nano as i128;
            }
            in_t += m.input_tokens;
            out_t += m.output_tokens;
            cr_t += m.cache_read_tokens;
            cw_t += m.cache_write_5m_tokens + m.cache_write_1h_tokens;
            ws_r += m.web_search_requests;
            total_official += m.real_nano as i128;
            total_charged += m.charge_nano as i128;
            total_requests += m.requests;
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
        })
        .collect();

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
            "unattributed_legacy": { "official_nano": unattributed_n.to_string() },
        },
        "models": models,
    }))
    .into_response()
}
