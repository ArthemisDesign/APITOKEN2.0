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
use base64::Engine as _;
use forward::AppState;
use registry::pricing::{
    parse_tariff_override_payload, validate_tariff_family, TariffOverrideInsert,
    TariffOverrideInsertOutcome, TariffOverrideRejection, TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::tariff_admin::{compiled_tariff_catalog_at, ensure_seed_safe, family_head_version};

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
    elog::error(
        "server-admin",
        format!("billing authority {context} failed: {error:#}"),
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "billing authority unavailable"})),
    )
        .into_response()
}

const REQUEST_FACT_CURSOR_VERSION: u8 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestFactsQuery {
    from: Option<i64>,
    to: Option<i64>,
    account_id: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

fn request_fact_window(
    from: Option<i64>,
    to: Option<i64>,
) -> Result<registry::request_facts::RequestFactReadWindow, Response> {
    let (Some(from), Some(to)) = (from, to) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"from and to are required"})),
        )
            .into_response());
    };
    let window = registry::request_facts::RequestFactReadWindow { from, to };
    if window.validate().is_err() || to > pool::now().saturating_add(1) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid request-fact window"})),
        )
            .into_response());
    }
    Ok(window)
}

fn validate_account_filter(account_id: Option<String>) -> Result<Option<String>, Response> {
    if account_id.as_deref().is_some_and(|id| {
        id.is_empty()
            || id.len() > registry::request_facts::MAX_REQUEST_FACT_ACCOUNT_ID_LEN
            || !id.is_ascii()
            || id.bytes().any(|byte| byte.is_ascii_control())
    }) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid account_id"})),
        )
            .into_response());
    }
    Ok(account_id)
}

fn encode_request_fact_cursor(cursor: registry::request_facts::RequestFactCursor) -> String {
    let mut bytes = [0u8; 17];
    bytes[0] = REQUEST_FACT_CURSOR_VERSION;
    bytes[1..9].copy_from_slice(&cursor.admitted_at.to_be_bytes());
    bytes[9..17].copy_from_slice(&cursor.fact_id.to_be_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_request_fact_cursor(
    value: Option<&str>,
) -> Result<Option<registry::request_facts::RequestFactCursor>, Response> {
    let Some(value) = value else { return Ok(None) };
    if value.len() > 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid cursor"})),
        )
            .into_response());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid cursor"})),
            )
                .into_response()
        })?;
    if bytes.len() != 17 || bytes[0] != REQUEST_FACT_CURSOR_VERSION {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid cursor"})),
        )
            .into_response());
    }
    let admitted_at = i64::from_be_bytes(bytes[1..9].try_into().expect("fixed cursor slice"));
    let fact_id = i64::from_be_bytes(bytes[9..17].try_into().expect("fixed cursor slice"));
    if admitted_at < 0 || fact_id <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid cursor"})),
        )
            .into_response());
    }
    Ok(Some(registry::request_facts::RequestFactCursor {
        admitted_at,
        fact_id,
    }))
}

fn runtime_request_fact_health(app: &AppState, stuck_nonterminal: Option<u64>) -> Value {
    let Some(billing) = app.billing.as_ref() else {
        return Value::Null;
    };
    let snapshot = billing.request_fact_delivery_snapshot();
    json!({
        "observed_at": pool::now(),
        "process_started_at": snapshot.process_started_at,
        "continuity": if snapshot.process_started_at.is_some() { "process_local" } else { "unknown" },
        "queue_capacity": forward::TERMINAL_REQUEST_FACT_QUEUE_CAPACITY,
        "queue_depth": snapshot.queue_depth,
        "accepted_total": snapshot.accepted,
        "persisted_total": snapshot.persisted,
        "deduplicated_total": snapshot.deduplicated,
        "dropped_invalid_total": snapshot.dropped_invalid,
        "dropped_full_total": snapshot.dropped_full,
        "dropped_closed_total": snapshot.dropped_closed,
        "dropped_unsupported_total": snapshot.dropped_unsupported,
        "persistence_failed_total": snapshot.persistence_failed,
        "persistence_health": match snapshot.persistence_health {
            forward::RequestFactPersistenceHealth::Unknown => "unknown",
            forward::RequestFactPersistenceHealth::Healthy => "healthy",
            forward::RequestFactPersistenceHealth::Failed => "failed",
        },
        "stuck_nonterminal_count": stuck_nonterminal,
    })
}

fn request_fact_coverage(
    window: registry::request_facts::RequestFactReadWindow,
    totals: &registry::request_facts::RequestFactSummaryTotals,
) -> Value {
    json!({
        "scope_version": registry::request_facts::REQUEST_FACT_SCOPE_VERSION,
        "from": window.from,
        "to": window.to,
        "persisted_facts": totals.persisted,
        "terminal_facts": totals.terminal,
        "nonterminal_facts": totals.nonterminal,
        "required_evidence_unknown_facts": totals.required_evidence_unknown,
        "drops": {"value": Value::Null, "reason":"no_durable_window_attribution"},
        "persistence_failures": {"value": Value::Null, "reason":"no_durable_window_attribution"},
        "admitted_denominator": Value::Null,
        "coverage_percentage": Value::Null,
        "status": "unknown",
    })
}

pub async fn request_facts_summary(
    State(app): State<AppState>,
    Query(query): Query<RequestFactsQuery>,
) -> Response {
    let window = match request_fact_window(query.from, query.to) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let account_id = match validate_account_filter(query.account_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if query.cursor.is_some() || query.limit.is_some() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"cursor and limit are not valid for summary"})),
        )
            .into_response();
    }
    let billing = match billing(&app) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let summary = match billing.request_facts_summary(window, account_id).await {
        Ok(value) => value,
        Err(error) => return authority_unavailable("request-fact summary", error),
    };
    let stuck = match billing.request_facts_stuck_count(pool::now()).await {
        Ok(value) => value,
        Err(error) => return authority_unavailable("request-fact health", error),
    };
    Json(json!({
        "scope_version": registry::request_facts::REQUEST_FACT_SCOPE_VERSION,
        "from": window.from,
        "to": window.to,
        "summary": summary,
        "coverage": request_fact_coverage(window, &summary.totals),
        "runtime": runtime_request_fact_health(&app, stuck),
    }))
    .into_response()
}

pub async fn request_facts_page(
    State(app): State<AppState>,
    Query(query): Query<RequestFactsQuery>,
) -> Response {
    let window = match request_fact_window(query.from, query.to) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let account_id = match validate_account_filter(query.account_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let limit = query
        .limit
        .unwrap_or(registry::request_facts::MAX_REQUEST_FACT_READ_LIMIT);
    if !(1..=registry::request_facts::MAX_REQUEST_FACT_READ_LIMIT).contains(&limit) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"limit must be between 1 and 200"})),
        )
            .into_response();
    }
    let cursor = match decode_request_fact_cursor(query.cursor.as_deref()) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let billing = match billing(&app) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let page = match billing
        .request_facts_page(window, account_id.clone(), cursor, limit)
        .await
    {
        Ok(value) => value,
        Err(error) => return authority_unavailable("request-fact page", error),
    };
    let summary = match billing.request_facts_summary(window, account_id).await {
        Ok(value) => value,
        Err(error) => return authority_unavailable("request-fact coverage", error),
    };
    let stuck = match billing.request_facts_stuck_count(pool::now()).await {
        Ok(value) => value,
        Err(error) => return authority_unavailable("request-fact health", error),
    };
    Json(json!({
        "scope_version": registry::request_facts::REQUEST_FACT_SCOPE_VERSION,
        "from": window.from,
        "to": window.to,
        "rows": page.rows,
        "next_cursor": page.next.map(encode_request_fact_cursor),
        "coverage": request_fact_coverage(window, &summary.totals),
        "runtime": runtime_request_fact_health(&app, stuck),
    }))
    .into_response()
}

pub async fn request_facts_logical(
    State(app): State<AppState>,
    Path(logical_request_id): Path<String>,
) -> Response {
    if !registry::request_facts::is_canonical_uuid_v4(&logical_request_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid logical request ID"})),
        )
            .into_response();
    }
    let billing = match billing(&app) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let result = match billing
        .request_facts_logical(logical_request_id.clone())
        .await
    {
        Ok(value) => value,
        Err(error) => return authority_unavailable("request-fact logical lookup", error),
    };
    let stuck = match billing.request_facts_stuck_count(pool::now()).await {
        Ok(value) => value,
        Err(error) => return authority_unavailable("request-fact health", error),
    };
    Json(json!({
        "scope_version": registry::request_facts::REQUEST_FACT_SCOPE_VERSION,
        "logical_request_id": logical_request_id,
        "rows": result.rows,
        "truncated": result.truncated,
        "runtime": runtime_request_fact_health(&app, stuck),
    }))
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
            elog::error("server-admin", format!("account id generation failed: {e}"));
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountsQueryReq {
    account_ids: Vec<String>,
}

/// POST /admin/accounts/query — bounded batch account snapshot for commerce/admin reads.
pub async fn query_accounts(
    State(app): State<AppState>,
    Json(req): Json<AccountsQueryReq>,
) -> Response {
    if req.account_ids.is_empty()
        || req.account_ids.len() > 500
        || req
            .account_ids
            .iter()
            .any(|id| !id.starts_with("acct_") || id.len() > 200)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "account_ids must contain 1 to 500 valid account IDs"})),
        )
            .into_response();
    }
    let requested: HashSet<String> = req.account_ids.into_iter().collect();
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b.accounts().await {
        Ok(rows) => {
            let accounts: Vec<_> = rows
                .into_iter()
                .filter(|account| requested.contains(&account.id))
                .map(|account| {
                    json!({
                        "account": account.id,
                        "balance_nano": account.balance_nano,
                        "spent_nano": account.spent_nano,
                        "reserved_nano": account.reserved_nano,
                        "balance": metering::nano_to_usd_string(account.balance_nano as i128),
                        "mult_bp": account.mult_bp,
                        "status": account.status,
                        "handle": account.handle,
                    })
                })
                .collect();
            Json(json!({"accounts": accounts})).into_response()
        }
        Err(error) => authority_unavailable("batch account lookup", error),
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
                "request_id": e.request_id,
                "amount_nano": e.amount_nano,
                "amount": metering::nano_to_usd_string(e.amount_nano as i128),
                "key_masked": e.key.as_deref().map(mask),
                "ref": e.reference,
                "balance_after_nano": e.balance_after_nano,
                "ts": e.ts,
                "model": e.model,
                "provider": e.provider,
                "official_nano": e.official_nano,
                "uncollected_nano": e.uncollected_nano,
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
#[serde(deny_unknown_fields)]
pub struct ProviderDiscountReq {
    provider_id: String,
    /// Payable multiplier in basis points, or `null` to remove the override so the account falls
    /// back to its default discount.
    mult_bp: Option<i64>,
}

/// GET /admin/account/{id}/discounts — the account default plus every provider override.
pub async fn account_discounts(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let account = match b.account(&id).await {
        Ok(Some(account)) => account,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "unknown account"})),
            )
                .into_response()
        }
        Err(error) => return authority_unavailable("account discount read", error),
    };
    let overrides = match b.account_provider_discounts(&id).await {
        Ok(rows) => rows,
        Err(error) => return authority_unavailable("account discount read", error),
    };
    let providers: serde_json::Map<String, serde_json::Value> = overrides
        .into_iter()
        .map(|(provider_id, mult_bp)| (provider_id, json!(mult_bp)))
        .collect();
    Json(json!({
        "account": account.id,
        "mult_bp": account.mult_bp,
        "providers": providers,
    }))
    .into_response()
}

/// POST /admin/account/{id}/discounts — set or clear one provider override.
///
/// This is the whole B2B pricing surface: a customer keeps one default discount and, where their
/// terms differ, one row per provider. A write is live on the next request — there is no version
/// to activate, no snapshot to keep in step and nothing that can disagree with the balance.
pub async fn set_account_discount(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ProviderDiscountReq>,
) -> Response {
    if let Some(mult_bp) = req.mult_bp {
        if let Err(error) = registry::ensure_valid_provider_discount(&req.provider_id, mult_bp) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string()})),
            )
                .into_response();
        }
    } else if !registry::DISCOUNT_PROVIDER_IDS.contains(&req.provider_id.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unknown provider id"})),
        )
            .into_response();
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match b
        .account_provider_discount(&id, &req.provider_id, req.mult_bp)
        .await
    {
        Ok(changed) => Json(json!({
            "account": id,
            "provider_id": req.provider_id,
            "mult_bp": req.mult_bp,
            "changed": changed,
        }))
        .into_response(),
        Err(error) if error.to_string().contains("unknown account") => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown account"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("account discount update", error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
            elog::error("server-admin", format!("key generation failed: {e}"));
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
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
            Ok(None) => {
                elog::error("server-admin", "issued key could not be read back");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "issued key could not be read"})),
                )
                    .into_response();
            }
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyStatusReq {
    status: String,
}

/// POST /admin/key-id/{key_id}/status — revoke/enable through a stable non-secret identifier.
pub async fn key_status_by_id(
    State(app): State<AppState>,
    Path(key_id): Path<String>,
    Json(req): Json<KeyStatusReq>,
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

/// Окно вида "30d"/"7d"/"24h"/"90d"/"all" → фиксированный полуинтервал unix-секунд.
/// Верхняя граница фиксирует отчёт: параллельный settle не может попасть только в один из срезов.
fn window_bounds(window: &str) -> (String, i64, i64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    window_bounds_at(window, now)
}

fn window_bounds_at(window: &str, now: i64) -> (String, i64, i64) {
    let w = window.trim();
    if w.eq_ignore_ascii_case("all") {
        return ("all".into(), 0, now);
    }
    let (num, unit_secs) = if let Some(n) = w.strip_suffix('h') {
        (n.parse::<i64>().ok(), 3_600)
    } else if let Some(n) = w.strip_suffix('d') {
        (n.parse::<i64>().ok(), 86_400)
    } else {
        (None, 0)
    };
    match num.and_then(|n| n.checked_mul(unit_secs).filter(|_| n > 0)) {
        Some(duration) => (w.to_string(), now.saturating_sub(duration), now),
        None => ("30d".into(), now.saturating_sub(30 * 86_400), now),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct UsageSummary {
    input_nano: i128,
    output_nano: i128,
    cache_read_nano: i128,
    cache_write_nano: i128,
    web_search_nano: i128,
    unattributed_nano: i128,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    web_search_requests: i64,
    total_official_nano: i128,
    total_charged_nano: i128,
    billed_events: i64,
}

fn summarize_usage(aggs: &[registry::UsageModelAgg]) -> UsageSummary {
    let mut summary = UsageSummary::default();
    for model in aggs {
        let stored_components = model.input_nano as i128
            + model.output_nano as i128
            + model.cache_read_nano as i128
            + model.cache_write_5m_nano as i128
            + model.cache_write_1h_nano as i128
            + model.web_search_nano as i128;
        if stored_components == 0 && model.real_nano > 0 {
            summary.unattributed_nano += model.real_nano as i128;
        } else {
            summary.input_nano += model.input_nano as i128;
            summary.output_nano += model.output_nano as i128;
            summary.cache_read_nano += model.cache_read_nano as i128;
            summary.cache_write_nano +=
                model.cache_write_5m_nano as i128 + model.cache_write_1h_nano as i128;
            summary.web_search_nano += model.web_search_nano as i128;
        }
        summary.input_tokens += model.input_tokens;
        summary.output_tokens += model.output_tokens;
        summary.cache_read_tokens += model.cache_read_tokens;
        summary.cache_write_tokens += model.cache_write_5m_tokens + model.cache_write_1h_tokens;
        summary.web_search_requests += model.web_search_requests;
        summary.total_official_nano += model.real_nano as i128;
        summary.total_charged_nano += model.charge_nano as i128;
        summary.billed_events += model.requests;
    }
    summary
}

/// GET /admin/account/{id}/usage?window=30d — разбивка расхода по токенам/моделям для дашборда.
/// Dollar-значения — сохранённые immutable компоненты settlement, а не пересчёт по текущему прайсу.
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
    let (window, since, until) = window_bounds(q.window.as_deref().unwrap_or("30d"));
    let report = match b.usage_report(&id, since, until).await {
        Ok(report) => report,
        Err(error) => return authority_unavailable("usage aggregation", error),
    };
    let registry::UsageReport {
        models: aggs,
        daily,
        daily_providers,
        keys,
    } = report;

    // Legacy rows retain their exact total but cannot be truthfully split across token buckets.
    let summary = summarize_usage(&aggs);
    let models: Vec<_> = aggs
        .iter()
        .map(|m| {
            json!({
                "model": m.model,
                "provider": m.provider,
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
    let daily: Vec<_> = daily
        .into_iter()
        .map(|day| {
            json!({
                "day_ts": day.day_ts,
                "requests": day.requests,
                "official_nano": day.real_nano.to_string(),
                "charged_nano": day.charge_nano.to_string(),
            })
        })
        .collect();
    let daily_providers: Vec<_> = daily_providers
        .into_iter()
        .map(|day| {
            json!({
                "day_ts": day.day_ts,
                "provider": day.provider,
                "requests": day.requests,
                "official_nano": day.real_nano.to_string(),
                "charged_nano": day.charge_nano.to_string(),
            })
        })
        .collect();
    let keys: Vec<_> = keys
        .into_iter()
        .map(|key| {
            json!({
                "key_masked": key.key.as_deref().map(mask),
                "requests": key.requests,
                "official_nano": key.real_nano.to_string(),
                "charged_nano": key.charge_nano.to_string(),
            })
        })
        .collect();

    Json(json!({
        "account": id,
        "window": window,
        "since_ts": since,
        "until_ts": until,
        "requests": summary.billed_events,
        "total_official_nano": summary.total_official_nano.to_string(),
        "total_charged_nano": summary.total_charged_nano.to_string(),
        "buckets": {
            "input": { "tokens": summary.input_tokens, "official_nano": summary.input_nano.to_string() },
            "output": { "tokens": summary.output_tokens, "official_nano": summary.output_nano.to_string() },
            "cache_read": { "tokens": summary.cache_read_tokens, "official_nano": summary.cache_read_nano.to_string() },
            "cache_write": { "tokens": summary.cache_write_tokens, "official_nano": summary.cache_write_nano.to_string() },
            "web_search": { "requests": summary.web_search_requests, "official_nano": summary.web_search_nano.to_string() },
            "unattributed_legacy": { "official_nano": summary.unattributed_nano.to_string() },
        },
        "models": models,
        "daily": daily,
        "daily_providers": daily_providers,
        "keys": keys,
    }))
    .into_response()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn invalid_pricing_request(reason: impl Into<String>, identity: Value) -> Response {
    let _ = identity;
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"result": "rejected", "code": "invalid", "reason": reason.into()})),
    )
        .into_response()
}

fn valid_attribution(value: &str) -> bool {
    !value.is_empty() && value.trim() == value
}

fn tariff_override_outcome_response(
    outcome: TariffOverrideInsertOutcome,
    identity: Value,
) -> Response {
    match outcome {
        TariffOverrideInsertOutcome::Inserted(row) => Json(json!({
            "result": "inserted",
            "identity": identity,
            "override": row,
        }))
        .into_response(),
        TariffOverrideInsertOutcome::Unchanged(row) => Json(json!({
            "result": "unchanged",
            "identity": identity,
            "override": row,
        }))
        .into_response(),
        TariffOverrideInsertOutcome::Rejected(rejection) => {
            let (status, code) = match &rejection {
                TariffOverrideRejection::Invalid { .. } => (StatusCode::BAD_REQUEST, "invalid"),
                TariffOverrideRejection::Conflict { .. } => (StatusCode::CONFLICT, "conflict"),
                TariffOverrideRejection::SequenceViolation { .. } => {
                    (StatusCode::CONFLICT, "sequence_violation")
                }
            };
            (
                status,
                Json(json!({
                    "result": "rejected",
                    "code": code,
                    "identity": identity,
                    "rejection": rejection,
                })),
            )
                .into_response()
        }
    }
}

/// GET /admin/pricing/tariffs — every override row, ordered by (family, version).
pub async fn list_tariff_overrides(State(app): State<AppState>) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.list_tariff_overrides().await {
        Ok(overrides) => Json(json!({"overrides": overrides})).into_response(),
        Err(error) => authority_unavailable("tariff override list", error),
    }
}

/// GET /admin/pricing/tariffs/compiled — the compiled catalog dump in the exact canonical
/// payload shape the override table stores, so an auditor can diff DB rows against the code.
/// Read-only and authority-free: the answer is built from `metering` alone.
pub async fn compiled_tariff_catalog(State(app): State<AppState>) -> Response {
    let _ = &app;
    let now = now_unix();
    let families: Vec<Value> = compiled_tariff_catalog_at(now)
        .iter()
        .map(|entry| {
            json!({
                "tariff_family": entry.tariff_family,
                "payload": entry.payload,
                "has_future_epoch": entry.has_future_epoch,
                "seed_safe": entry.seed_safe,
            })
        })
        .collect();
    Json(json!({"compiled_ts": now, "families": families})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TariffOverrideReq {
    pub tariff_family: String,
    pub effective_from: i64,
    pub payload: Value,
    pub created_by: String,
    pub reason: String,
}

/// POST /admin/pricing/tariffs/override — publish the next version of one family. The request
/// carries no version: the server computes `head + 1` (2 when the family has no rows) and
/// retries exactly once on a sequence race with the authority-returned `expected_next`.
pub async fn publish_tariff_override(
    State(app): State<AppState>,
    Json(req): Json<TariffOverrideReq>,
) -> Response {
    let identity = json!({
        "tariff_family": req.tariff_family,
        "effective_from": req.effective_from,
        "payload": req.payload,
        "created_by": req.created_by,
        "reason": req.reason,
    });
    if let Err(error) = validate_tariff_family(&req.tariff_family) {
        return invalid_pricing_request(format!("{error:#}"), identity);
    }
    if !valid_attribution(&req.created_by) || !valid_attribution(&req.reason) {
        return invalid_pricing_request(
            "created_by and reason must be non-empty without surrounding whitespace",
            identity,
        );
    }
    // Parse once here so a malformed payload is a clean 400 instead of an authority round trip.
    if let Err(error) = parse_tariff_override_payload(&req.tariff_family, &req.payload) {
        return invalid_pricing_request(format!("{error:#}"), identity);
    }
    let now = now_unix();
    if req.effective_from < now - TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS {
        return invalid_pricing_request(
            format!(
                "effective_from must be >= now - {TARIFF_OVERRIDE_CLOCK_SKEW_GRACE_SECS}s \
                 (the determinism rule for non-seed overrides)"
            ),
            identity,
        );
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    let rows = match b.list_tariff_overrides().await {
        Ok(rows) => rows,
        Err(error) => return authority_unavailable("tariff override list", error),
    };
    let mut version = family_head_version(&rows, &req.tariff_family) + 1;
    for attempt in 0..2 {
        let insert = TariffOverrideInsert {
            tariff_family: req.tariff_family.clone(),
            version,
            effective_from: req.effective_from,
            payload: req.payload.clone(),
            created_by: req.created_by.clone(),
            reason: req.reason.clone(),
        };
        match b.insert_tariff_override(insert).await {
            Ok(TariffOverrideInsertOutcome::Rejected(
                TariffOverrideRejection::SequenceViolation { expected_next },
            )) if attempt == 0 => {
                // A concurrent publisher appended the version we computed; retry exactly once
                // with the head the authority actually expects.
                version = expected_next;
            }
            Ok(outcome) => return tariff_override_outcome_response(outcome, identity),
            Err(error) => return authority_unavailable("tariff override insert", error),
        }
    }
    unreachable!("the retry loop returns on every outcome after at most one retry")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TariffSeedReq {
    pub created_by: String,
    pub reason: String,
    pub tariff_family: Option<String>,
}

/// POST /admin/pricing/tariffs/seed — bridge one compiled family (or every compiled family)
/// into the table as version 2 with `effective_from = 0`, built only from the compiled
/// `metering` constants. A selected multi-epoch family makes the whole request fail with 400
/// before authority access because one zero-time row cannot preserve its schedule. Exact replay
/// is `unchanged`, so re-seeding is idempotent. A family whose head already advanced past version
/// 2 is refused: seeding never overwrites operator versions. For seed-safe targets, per-family
/// outcomes are reported; any refused/rejected family makes the overall status 409 while the
/// remaining seed-safe families still seed.
pub async fn seed_tariff_overrides(
    State(app): State<AppState>,
    Json(req): Json<TariffSeedReq>,
) -> Response {
    let identity = json!({
        "created_by": req.created_by,
        "reason": req.reason,
        "tariff_family": req.tariff_family,
    });
    if !valid_attribution(&req.created_by) || !valid_attribution(&req.reason) {
        return invalid_pricing_request(
            "created_by and reason must be non-empty without surrounding whitespace",
            identity,
        );
    }
    let catalog = compiled_tariff_catalog_at(now_unix());
    let targets: Vec<&crate::tariff_admin::CompiledTariff> = match req.tariff_family.as_deref() {
        Some(family) => match catalog.iter().find(|entry| entry.tariff_family == family) {
            Some(entry) => vec![entry],
            None => {
                return invalid_pricing_request(
                    format!("unknown compiled tariff family {family:?}"),
                    identity,
                )
            }
        },
        None => catalog.iter().collect(),
    };
    if let Err(error) = ensure_seed_safe(&targets) {
        return invalid_pricing_request(error, identity);
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    let rows = match b.list_tariff_overrides().await {
        Ok(rows) => rows,
        Err(error) => return authority_unavailable("tariff override list", error),
    };
    let mut outcomes = Vec::with_capacity(targets.len());
    let mut conflict = false;
    for target in targets {
        let head = family_head_version(&rows, target.tariff_family);
        if head > 2 {
            // The family already carries operator versions; seeding is only the bridge from the
            // compiled constants to data, never an overwrite.
            conflict = true;
            outcomes.push(json!({
                "tariff_family": target.tariff_family,
                "result": "refused",
                "code": "seed_refused",
                "head_version": head,
            }));
            continue;
        }
        let insert = TariffOverrideInsert {
            tariff_family: target.tariff_family.to_owned(),
            version: 2,
            effective_from: 0,
            payload: target.payload.clone(),
            created_by: req.created_by.clone(),
            reason: req.reason.clone(),
        };
        match b.insert_tariff_override(insert).await {
            Ok(TariffOverrideInsertOutcome::Inserted(row)) => outcomes.push(json!({
                "tariff_family": target.tariff_family,
                "result": "inserted",
                "override": row,
            })),
            Ok(TariffOverrideInsertOutcome::Unchanged(row)) => outcomes.push(json!({
                "tariff_family": target.tariff_family,
                "result": "unchanged",
                "override": row,
            })),
            Ok(TariffOverrideInsertOutcome::Rejected(rejection)) => {
                // Conflict: version 2 was republished with different content. SequenceViolation:
                // version 3+ appeared between the list and this insert. Invalid on a compiled
                // payload would mean converter/registry drift. None of these may be advanced by
                // a seed, so the family is reported and the overall answer is 409.
                conflict = true;
                outcomes.push(json!({
                    "tariff_family": target.tariff_family,
                    "result": "rejected",
                    "rejection": rejection,
                }));
            }
            Err(error) => return authority_unavailable("tariff override seed insert", error),
        }
    }
    let status = if conflict {
        StatusCode::CONFLICT
    } else {
        StatusCode::OK
    };
    (status, Json(json!({"outcomes": outcomes}))).into_response()
}

#[cfg(test)]
mod usage_tests {
    use super::{summarize_usage, window_bounds_at, UsageSummary};
    use registry::UsageModelAgg;

    #[test]
    fn usage_windows_are_fixed_half_open_intervals() {
        let now = 2_000_000_000;
        assert_eq!(
            window_bounds_at("30d", now),
            ("30d".into(), now - 30 * 86_400, now)
        );
        assert_eq!(
            window_bounds_at("24h", now),
            ("24h".into(), now - 24 * 3_600, now)
        );
        assert_eq!(window_bounds_at("all", now), ("all".into(), 0, now));
    }

    #[test]
    fn invalid_or_overflowing_usage_windows_fall_back_safely() {
        let now = 2_000_000_000;
        let expected = ("30d".into(), now - 30 * 86_400, now);
        assert_eq!(window_bounds_at("broken", now), expected);
        assert_eq!(window_bounds_at("999999999999999999d", now), expected);
    }

    #[test]
    fn stored_buckets_and_legacy_value_reconcile_exactly_without_overflow() {
        let modern = UsageModelAgg {
            requests: 2,
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 30,
            cache_write_5m_tokens: 4,
            cache_write_1h_tokens: 5,
            web_search_requests: 6,
            real_nano: 21,
            charge_nano: 8,
            input_nano: 1,
            output_nano: 2,
            cache_read_nano: 3,
            cache_write_5m_nano: 4,
            cache_write_1h_nano: 5,
            web_search_nano: 6,
            ..Default::default()
        };
        let legacy = UsageModelAgg {
            requests: 3,
            input_tokens: 7,
            real_nano: i64::MAX,
            charge_nano: i64::MAX,
            ..Default::default()
        };

        let summary = summarize_usage(&[modern, legacy]);
        assert_eq!(
            summary,
            UsageSummary {
                input_nano: 1,
                output_nano: 2,
                cache_read_nano: 3,
                cache_write_nano: 9,
                web_search_nano: 6,
                unattributed_nano: i64::MAX as i128,
                input_tokens: 17,
                output_tokens: 20,
                cache_read_tokens: 30,
                cache_write_tokens: 9,
                web_search_requests: 6,
                total_official_nano: i64::MAX as i128 + 21,
                total_charged_nano: i64::MAX as i128 + 8,
                billed_events: 5,
            }
        );
        assert_eq!(
            summary.input_nano
                + summary.output_nano
                + summary.cache_read_nano
                + summary.cache_write_nano
                + summary.web_search_nano
                + summary.unattributed_nano,
            summary.total_official_nano
        );
    }
}
