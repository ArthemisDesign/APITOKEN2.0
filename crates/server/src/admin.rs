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
use registry::pricing::{
    validate_account_policy_binding, validate_account_policy_shape, validate_active_expectation,
    validate_policy_active_expectation, validate_pricing_catalog, validate_provider_switches,
    AccountPolicyActivationSpec, AccountPolicyBindingSpec, AccountPolicySpec, ActiveExpectation,
    PolicyActiveExpectation, PricingCatalogSpec, PricingMutation, PricingRejection,
    ProviderSwitchSpec,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogActivationReq {
    catalog: PricingCatalogSpec,
    expectation: ActiveExpectation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSwitchActivationReq {
    switches: ProviderSwitchSpec,
    expectation: ActiveExpectation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyActivationReq {
    policy: AccountPolicySpec,
    binding: AccountPolicyBindingSpec,
    expectation: PolicyActiveExpectation,
}

fn pricing_mutation_response(mutation: PricingMutation, identity: Value) -> Response {
    match mutation {
        PricingMutation::Stored => Json(json!({
            "result": "stored",
            "identity": identity,
        }))
        .into_response(),
        PricingMutation::Applied => Json(json!({
            "result": "applied",
            "identity": identity,
        }))
        .into_response(),
        PricingMutation::Unchanged => Json(json!({
            "result": "unchanged",
            "identity": identity,
        }))
        .into_response(),
        PricingMutation::Rejected(rejection) => {
            let (status, code) = match &rejection {
                PricingRejection::Invalid { .. } => (StatusCode::BAD_REQUEST, "invalid"),
                PricingRejection::MissingDependency { .. } => {
                    (StatusCode::CONFLICT, "missing_dependency")
                }
                PricingRejection::Stale { .. } => (StatusCode::CONFLICT, "stale"),
                PricingRejection::VersionConflict => (StatusCode::CONFLICT, "version_conflict"),
                PricingRejection::CasMismatch { .. } => (StatusCode::CONFLICT, "cas_mismatch"),
                PricingRejection::PolicyCasMismatch { .. } => {
                    (StatusCode::CONFLICT, "policy_cas_mismatch")
                }
                PricingRejection::Locked => (StatusCode::LOCKED, "locked"),
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

fn invalid_pricing_request(reason: impl Into<String>, identity: Value) -> Response {
    pricing_mutation_response(
        PricingMutation::Rejected(PricingRejection::Invalid {
            reason: reason.into(),
        }),
        identity,
    )
}

fn invalid_pricing_path(message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
}

fn valid_pricing_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// POST /admin/pricing/catalog/prepare — store one immutable product catalog generation.
pub async fn prepare_pricing_catalog(
    State(app): State<AppState>,
    Json(spec): Json<PricingCatalogSpec>,
) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    let identity = json!({"catalog": spec.clone()});
    match b.prepare_pricing_catalog(spec).await {
        Ok(mutation) => pricing_mutation_response(mutation, identity),
        Err(error) => authority_unavailable("pricing catalog prepare", error),
    }
}

/// GET /admin/pricing/catalog/{product_id}/active — read the durable active head and full version.
pub async fn active_pricing_catalog(
    State(app): State<AppState>,
    Path(product_id): Path<String>,
) -> Response {
    if !valid_pricing_id(&product_id) {
        return invalid_pricing_path("invalid product_id");
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.active_pricing_catalog(&product_id).await {
        Ok(Some(catalog)) => Json(json!({"catalog": catalog})).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "no active catalog"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("active pricing catalog read", error),
    }
}

/// GET /admin/pricing/catalog/{product_id}/version/{generation} — immutable version lookup.
pub async fn pricing_catalog_version(
    State(app): State<AppState>,
    Path((product_id, generation)): Path<(String, i64)>,
) -> Response {
    if !valid_pricing_id(&product_id) || generation <= 0 {
        return invalid_pricing_path("invalid product_id or generation");
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b
        .pricing_catalog_by_generation(&product_id, generation)
        .await
    {
        Ok(Some(catalog)) => Json(json!({"catalog": catalog})).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown catalog"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("pricing catalog version read", error),
    }
}

/// POST /admin/pricing/catalog/{product_id}/activate — monotonic CAS of the product head.
pub async fn activate_pricing_catalog(
    State(app): State<AppState>,
    Path(product_id): Path<String>,
    Json(req): Json<CatalogActivationReq>,
) -> Response {
    if !valid_pricing_id(&product_id) {
        return invalid_pricing_path("invalid product_id");
    }
    let CatalogActivationReq {
        catalog,
        expectation,
    } = req;
    let identity = json!({
        "catalog": catalog.clone(),
        "expectation": expectation.clone(),
    });
    if catalog.product_id != product_id {
        return invalid_pricing_request(
            "catalog product_id does not match the activation URL",
            identity,
        );
    }
    if let Err(error) =
        validate_pricing_catalog(&catalog).and_then(|()| validate_active_expectation(&expectation))
    {
        return invalid_pricing_request(error.to_string(), identity);
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b
        .pricing_catalog_by_generation(&product_id, catalog.generation)
        .await
    {
        Ok(Some(prepared)) if prepared == catalog => {}
        Ok(Some(_)) => {
            return pricing_mutation_response(
                PricingMutation::Rejected(PricingRejection::VersionConflict),
                identity,
            )
        }
        Ok(None) => {
            return pricing_mutation_response(
                PricingMutation::Rejected(PricingRejection::MissingDependency {
                    dependency: format!(
                        "prepared catalog {product_id} generation {}",
                        catalog.generation
                    ),
                }),
                identity,
            )
        }
        Err(error) => return authority_unavailable("pricing catalog activation preflight", error),
    }
    let target = catalog.target();
    match b
        .activate_pricing_catalog(&product_id, target, expectation)
        .await
    {
        Ok(mutation) => pricing_mutation_response(mutation, identity),
        Err(error) => authority_unavailable("pricing catalog activation", error),
    }
}

/// POST /admin/pricing/switches/prepare — store one immutable global switch generation.
pub async fn prepare_provider_switches(
    State(app): State<AppState>,
    Json(spec): Json<ProviderSwitchSpec>,
) -> Response {
    let identity = json!({"switches": spec.clone()});
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.prepare_provider_switches(spec).await {
        Ok(mutation) => pricing_mutation_response(mutation, identity),
        Err(error) => authority_unavailable("provider switches prepare", error),
    }
}

pub async fn active_provider_switches(State(app): State<AppState>) -> Response {
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.active_provider_switches().await {
        Ok(Some(switches)) => Json(json!({"switches": switches})).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "no active provider switches"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("active provider switches read", error),
    }
}

pub async fn provider_switches_version(
    State(app): State<AppState>,
    Path(generation): Path<i64>,
) -> Response {
    if generation <= 0 {
        return invalid_pricing_path("invalid generation");
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.provider_switches_by_generation(generation).await {
        Ok(Some(switches)) => Json(json!({"switches": switches})).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown provider switches"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("provider switches version read", error),
    }
}

pub async fn activate_provider_switches(
    State(app): State<AppState>,
    Json(req): Json<ProviderSwitchActivationReq>,
) -> Response {
    let ProviderSwitchActivationReq {
        switches,
        expectation,
    } = req;
    let identity = json!({
        "switches": switches.clone(),
        "expectation": expectation.clone(),
    });
    if let Err(error) = validate_provider_switches(&switches)
        .and_then(|()| validate_active_expectation(&expectation))
    {
        return invalid_pricing_request(error.to_string(), identity);
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.provider_switches_by_generation(switches.generation).await {
        Ok(Some(prepared)) if prepared == switches => {}
        Ok(Some(_)) => {
            return pricing_mutation_response(
                PricingMutation::Rejected(PricingRejection::VersionConflict),
                identity,
            )
        }
        Ok(None) => {
            return pricing_mutation_response(
                PricingMutation::Rejected(PricingRejection::MissingDependency {
                    dependency: format!(
                        "prepared provider switches generation {}",
                        switches.generation
                    ),
                }),
                identity,
            )
        }
        Err(error) => {
            return authority_unavailable("provider switches activation preflight", error)
        }
    }
    let target = switches.target();
    match b.activate_provider_switches(target, expectation).await {
        Ok(mutation) => pricing_mutation_response(mutation, identity),
        Err(error) => authority_unavailable("provider switches activation", error),
    }
}

/// POST /admin/pricing/policy/prepare — store one immutable full account-policy version.
pub async fn prepare_account_policy(
    State(app): State<AppState>,
    Json(spec): Json<AccountPolicySpec>,
) -> Response {
    let identity = json!({"policy": spec.clone()});
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.prepare_account_policy(spec).await {
        Ok(mutation) => pricing_mutation_response(mutation, identity),
        Err(error) => authority_unavailable("account pricing policy prepare", error),
    }
}

pub async fn active_account_policy(
    State(app): State<AppState>,
    Path(account_id): Path<String>,
) -> Response {
    if !valid_pricing_id(&account_id) {
        return invalid_pricing_path("invalid account_id");
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.active_account_policy(&account_id).await {
        Ok(Some(policy)) => Json(json!({"active": policy})).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "no active account policy"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("active account pricing policy read", error),
    }
}

/// GET /admin/pricing/policy/{account_id}/state — one coherent policy and dual-lineage snapshot.
pub async fn account_policy_state(
    State(app): State<AppState>,
    Path(account_id): Path<String>,
) -> Response {
    if !valid_pricing_id(&account_id) {
        return invalid_pricing_path("invalid account_id");
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b.pricing_read_bundle(&account_id).await {
        Ok(bundle) => Json(json!({"state": bundle})).into_response(),
        Err(error) => match b.account(&account_id).await {
            Ok(None) => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "unknown account"})),
            )
                .into_response(),
            Ok(Some(_)) | Err(_) => {
                authority_unavailable("account pricing policy state read", error)
            }
        },
    }
}

pub async fn account_policy_version(
    State(app): State<AppState>,
    Path((account_id, effective_version)): Path<(String, i64)>,
) -> Response {
    if !valid_pricing_id(&account_id) || effective_version <= 0 {
        return invalid_pricing_path("invalid account_id or effective_version");
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b
        .account_policy_by_version(&account_id, effective_version)
        .await
    {
        Ok(Some(policy)) => Json(json!({"policy": policy})).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "unknown account policy"})),
        )
            .into_response(),
        Err(error) => authority_unavailable("account pricing policy version read", error),
    }
}

pub async fn activate_account_policy(
    State(app): State<AppState>,
    Path(account_id): Path<String>,
    Json(req): Json<PolicyActivationReq>,
) -> Response {
    if !valid_pricing_id(&account_id) {
        return invalid_pricing_path("invalid account_id");
    }
    let PolicyActivationReq {
        policy,
        binding,
        expectation,
    } = req;
    let activation = AccountPolicyActivationSpec {
        account_id: account_id.clone(),
        effective_version: policy.effective_version,
        content_digest: policy.content_digest.clone(),
        binding: binding.clone(),
    };
    let identity = json!({
        "policy": policy.clone(),
        "activation": activation.clone(),
        "expectation": expectation.clone(),
    });
    if policy.account_id != account_id {
        return invalid_pricing_request(
            "policy account_id does not match the activation URL",
            identity,
        );
    }
    if let Err(error) = validate_account_policy_shape(&policy)
        .and_then(|()| validate_account_policy_binding(&binding))
        .and_then(|()| validate_policy_active_expectation(&expectation))
    {
        return invalid_pricing_request(error.to_string(), identity);
    }
    let b = match billing(&app) {
        Ok(b) => b,
        Err(response) => return response,
    };
    match b
        .account_policy_by_version(&account_id, policy.effective_version)
        .await
    {
        Ok(Some(prepared)) if prepared == policy => {}
        Ok(Some(_)) => {
            return pricing_mutation_response(
                PricingMutation::Rejected(PricingRejection::VersionConflict),
                identity,
            )
        }
        Ok(None) => {
            return pricing_mutation_response(
                PricingMutation::Rejected(PricingRejection::MissingDependency {
                    dependency: format!(
                        "prepared account policy {account_id} effective version {}",
                        policy.effective_version
                    ),
                }),
                identity,
            )
        }
        Err(error) => {
            return authority_unavailable("account pricing policy activation preflight", error)
        }
    }
    match b.activate_account_policy(activation, expectation).await {
        Ok(mutation) => pricing_mutation_response(mutation, identity),
        Err(error) => authority_unavailable("account pricing policy activation", error),
    }
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
