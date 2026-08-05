//! Персистентное состояние бота в SQLite (переживает рестарт — в отличие от JSON/памяти
//! старого бота). Пользователи, офферы, отклики, и МАШИНА создания оффера (admin_state).
//!
//! Доступ из конкурентных задач — через Mutex<Connection>. Операции синхронные и короткие,
//! `.await` под локом не держим.

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

pub struct Store {
    c: Mutex<Connection>,
}

#[derive(Clone, Debug, Default)]
pub struct UserRow {
    pub chat_id: i64,
    pub uid: i64,
    pub username: String,
    pub status: String,    // new | pending | approved | rejected | pending_admin
    pub role: String,      // "" | admin
    pub address: String,   // BEP-20
    pub want: String,      // ожидаемый ввод (reg_address | ho_* | cx_* | gm_* | km_* | glm_*)
    pub hproxy: String,    // прокси аккаунта при передаче доступа (handover)
    pub hproxy_order: i64, // IPRoyal order id за handover-прокси (0 = ручной/внешний)
    pub hregion: String,   // площадка GLM-аккаунта текущей сделки ("" = int, "cn" = bigmodel.cn)
}

#[derive(Clone, Debug)]
pub struct Offer {
    pub id: i64,
    pub product: String,
    pub price: String,
    pub created_by: i64,
    pub seller_chat: i64,     // адресат оффера (0 = не задан)
    pub proxy_source: String, // buyer | seller | legacy
    pub buyer_proxy: String,  // прокси покупателя, если proxy_source=buyer
}

#[derive(Clone, Debug, Default)]
pub struct AdminState {
    pub chat_id: i64,
    pub step: String,
    pub product: String,
    pub seller_chat: i64,
    pub mode: String, // single | batch
    pub quantity: i64,
    pub unit_price: String,
    pub proxy_source: String, // buyer | seller
    pub draft_proxies: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PurchaseBatch {
    pub id: i64,
    pub product: String,
    pub unit_price: String,
    pub quantity: i64,
    pub total_price: String,
    pub created_by: i64,
    pub seller_chat: i64,
    pub proxy_source: String, // buyer | seller
    pub status: String, // offered | accepted | paying | paid | processing | paused | completed | rejected | cancelled
    pub payment_tx: String,
    pub current_item: i64, // 1-based; 0 until payment
}

#[derive(Clone, Debug)]
pub struct BatchOverview {
    pub batch: PurchaseBatch,
    pub completed: i64,
    pub remaining: i64,
}

#[derive(Clone, Debug)]
pub struct BatchItem {
    pub id: i64,
    pub batch_id: i64,
    pub item_no: i64, // 1-based position in the batch
    pub product: String,
    pub price: String,
    pub proxy: String,
    pub status: String, // pending | processing | completed
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchCompletion {
    pub batch_id: i64,
    pub item_no: i64,
    pub total: i64,
    pub completed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SellerJobRef {
    pub kind: String, // offer | batch
    pub offer_id: i64,
    pub batch_id: i64,
    pub item_no: i64,
    pub token: String, // unique activation generation; prevents stale-callback ABA
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SellerJob {
    pub seller_chat: i64,
    pub reference: SellerJobRef,
    pub product: String,
    pub phase: String, // accepted | paying | processing
    pub total: i64,
}

impl SellerJob {
    pub fn job_ref(&self) -> SellerJobRef {
        self.reference.clone()
    }
}

#[derive(Clone, Debug)]
pub struct GeminiOAuthSession {
    pub state: String,
    pub chat_id: i64,
    pub sealed_payload: String,
    pub expires_ts: i64,
    pub job: Option<SellerJobRef>,
}

/// An account waiting for Google's own account verification. Everything identifying — tokens,
/// subject, email, project, tier and proxy — lives only inside `sealed_payload`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiPendingVerification {
    pub chat_id: i64,
    pub sealed_payload: String,
    pub expires_ts: i64,
    /// Includes the attempt being claimed right now.
    pub attempts: i64,
    /// Earliest moment the background sweep may run the next acceptance attempt.
    pub next_probe_ts: i64,
    /// Moment automatic probing stops. The envelope itself survives until `expires_ts`, so the
    /// credential stays recorded even when the account never passed.
    pub probe_deadline_ts: i64,
    pub deadline_notified: bool,
    pub job: Option<SellerJobRef>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyAuthorityStatus {
    Local,
    Unknown,
}

impl ProxyAuthorityStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Unknown => "unknown",
        }
    }

    fn from_db(value: &str) -> rusqlite::Result<Self> {
        match value {
            "local" => Ok(Self::Local),
            "unknown" => Ok(Self::Unknown),
            _ => Err(rusqlite::Error::InvalidColumnType(
                0,
                "authority_status".into(),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

/// Durable link created only after a credential has been published or reconciled. This is a
/// private database projection: only `inventory_id` may be copied into a public API response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyBinding {
    pub inventory_id: String,
    pub provider: String,
    pub local_id: String,
    pub order_id: i64,
    /// Canonical provider allocation address. `None` exists only for unresolved legacy rows.
    pub allocation_ip: Option<IpAddr>,
    pub issued_at: i64,
    pub authority_status: ProxyAuthorityStatus,
    pub updated_at: i64,
}

/// Exact immutable renewal target. Multiple selections may share an order, but neither an inventory
/// allocation nor the exact `(order_id, allocation_ip)` pair may appear twice.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RenewalSelection {
    pub inventory_id: String,
    pub order_id: i64,
    pub allocation_ip: IpAddr,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenewalRequestState {
    Pending,
    InProgress,
    Completed,
    Failed,
    Indeterminate,
}

impl RenewalRequestState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Indeterminate => "indeterminate",
        }
    }

    fn from_db(value: &str) -> rusqlite::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "indeterminate" => Ok(Self::Indeterminate),
            _ => Err(rusqlite::Error::InvalidColumnType(
                0,
                "state".into(),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenewalEventOutcome {
    Renewed,
    Unchanged,
    NotFound,
    Rejected,
    ProviderUnavailable,
    Indeterminate,
}

impl RenewalEventOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Renewed => "renewed",
            Self::Unchanged => "unchanged",
            Self::NotFound => "not_found",
            Self::Rejected => "rejected",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::Indeterminate => "indeterminate",
        }
    }

    fn from_db(value: &str) -> rusqlite::Result<Self> {
        match value {
            "renewed" => Ok(Self::Renewed),
            "unchanged" => Ok(Self::Unchanged),
            "not_found" => Ok(Self::NotFound),
            "rejected" => Ok(Self::Rejected),
            "provider_unavailable" => Ok(Self::ProviderUnavailable),
            "indeterminate" => Ok(Self::Indeterminate),
            _ => Err(rusqlite::Error::InvalidColumnType(
                0,
                "outcome".into(),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenewalRequest {
    pub id: i64,
    pub idempotency_key: String,
    /// Compatibility projection for the public lifecycle handler.
    pub inventory_ids: Vec<String>,
    /// Compatibility projection for provider calls. Repeated order IDs are valid.
    pub order_ids: Vec<i64>,
    /// Private validated actor that originally created the request.
    pub requested_by: String,
    pub state: RenewalRequestState,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenewalEvent {
    pub id: i64,
    pub request_id: i64,
    pub order_id: i64,
    pub outcome: RenewalEventOutcome,
    pub observed_at: i64,
    pub new_expiry_at: Option<i64>,
}

/// Exact durable event identity used by replay. Retrieval fails closed for migrated legacy events
/// that predate allocation-level snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactRenewalEvent {
    pub event: RenewalEvent,
    pub inventory_id: String,
    pub allocation_ip: IpAddr,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyLifecycleConflict {
    BindingOrderChanged,
    OrderAlreadyBound,
    IdempotencyKeyReused,
    RenewalEventChanged,
}

impl std::fmt::Display for ProxyLifecycleConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::BindingOrderChanged => {
                "proxy binding already has a different order or allocation"
            }
            Self::OrderAlreadyBound => {
                "proxy order and allocation already belong to another binding"
            }
            Self::IdempotencyKeyReused => {
                "renewal idempotency key already has a different exact selection"
            }
            Self::RenewalEventChanged => "renewal inventory already has a different result",
        };
        f.write_str(message)
    }
}

impl std::error::Error for ProxyLifecycleConflict {}

const INVENTORY_ID_PREFIX: &str = "inv_";
const INVENTORY_RANDOM_BYTES: usize = 24;
const INVENTORY_ID_LEN: usize = INVENTORY_ID_PREFIX.len() + 32;
const INVENTORY_ID_MAX_LEN: usize = 160;
const INVENTORY_ID_ATTEMPTS: usize = 16;
const REQUESTED_BY_MAX_LEN: usize = 128;
const LEGACY_REQUESTED_BY: &str = "legacy";

fn valid_requested_by(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= REQUESTED_BY_MAX_LEN
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'@' | b'.' | b'_' | b':' | b'/' | b'+' | b'-')
        })
}

fn new_inventory_id() -> Result<String> {
    let mut random = [0u8; INVENTORY_RANDOM_BYTES];
    getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("CSPRNG unavailable"))?;
    Ok(format!(
        "{INVENTORY_ID_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(random)
    ))
}

fn valid_inventory_id(value: &str) -> bool {
    value.len() == INVENTORY_ID_LEN
        && value.len() <= INVENTORY_ID_MAX_LEN
        && value.starts_with(INVENTORY_ID_PREFIX)
        && value[INVENTORY_ID_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn parse_db_ip(value: Option<String>, column: usize) -> rusqlite::Result<Option<IpAddr>> {
    value
        .map(|value| {
            value.parse().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn proxy_binding_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProxyBinding> {
    Ok(ProxyBinding {
        inventory_id: row.get(0)?,
        provider: row.get(1)?,
        local_id: row.get(2)?,
        order_id: row.get(3)?,
        allocation_ip: parse_db_ip(row.get(4)?, 4)?,
        issued_at: row.get(5)?,
        authority_status: ProxyAuthorityStatus::from_db(&row.get::<_, String>(6)?)?,
        updated_at: row.get(7)?,
    })
}

fn canonical_selections(
    selections: &[RenewalSelection],
) -> Result<(
    Vec<RenewalSelection>,
    Vec<String>,
    Vec<i64>,
    String,
    String,
    String,
)> {
    if selections.is_empty() {
        bail!("renewal request must contain at least one inventory id");
    }
    let mut canonical = selections.to_vec();
    if canonical
        .iter()
        .any(|selection| !valid_inventory_id(&selection.inventory_id) || selection.order_id <= 0)
    {
        bail!("renewal selections contain an invalid inventory or order id");
    }
    canonical.sort_by(|left, right| left.inventory_id.cmp(&right.inventory_id));
    if canonical
        .windows(2)
        .any(|pair| pair[0].inventory_id == pair[1].inventory_id)
    {
        bail!("renewal inventory ids must be unique");
    }
    let mut allocations = HashSet::with_capacity(canonical.len());
    if canonical
        .iter()
        .any(|selection| !allocations.insert((selection.order_id, selection.allocation_ip)))
    {
        bail!("renewal order and allocation pairs must be unique");
    }
    let inventory_ids = canonical
        .iter()
        .map(|selection| selection.inventory_id.clone())
        .collect::<Vec<_>>();
    let order_ids = canonical
        .iter()
        .map(|selection| selection.order_id)
        .collect::<Vec<_>>();
    let encoded_selections = serde_json::to_string(&canonical)?;
    let encoded_inventory_ids = serde_json::to_string(&inventory_ids)?;
    let encoded_order_ids = serde_json::to_string(&order_ids)?;
    if encoded_selections.len() > 32768
        || encoded_inventory_ids.len() > 16384
        || encoded_order_ids.len() > 8192
    {
        bail!("renewal selections exceed storage limits");
    }
    Ok((
        canonical,
        inventory_ids,
        order_ids,
        encoded_selections,
        encoded_inventory_ids,
        encoded_order_ids,
    ))
}

fn from_sql_json_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn decode_order_ids(encoded: &str) -> rusqlite::Result<Vec<i64>> {
    if encoded.starts_with('[') {
        return serde_json::from_str(encoded).map_err(from_sql_json_error);
    }
    encoded
        .split(',')
        .map(|value| {
            value.parse::<i64>().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .collect()
}

fn renewal_request_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RenewalRequest> {
    let requested_by = row.get::<_, String>(5)?;
    if !valid_requested_by(&requested_by) {
        return Err(rusqlite::Error::InvalidColumnType(
            5,
            "requested_by".into(),
            rusqlite::types::Type::Text,
        ));
    }
    let state = RenewalRequestState::from_db(&row.get::<_, String>(6)?)?;
    let selections = match row.get::<_, Option<String>>(2)? {
        Some(value) if !value.is_empty() => {
            serde_json::from_str::<Vec<RenewalSelection>>(&value).map_err(from_sql_json_error)?
        }
        _ if matches!(
            state,
            RenewalRequestState::Completed
                | RenewalRequestState::Failed
                | RenewalRequestState::Indeterminate
        ) =>
        {
            Vec::new()
        }
        _ => {
            return Err(rusqlite::Error::InvalidColumnType(
                2,
                "selections".into(),
                rusqlite::types::Type::Null,
            ));
        }
    };
    let (inventory_ids, order_ids) = if selections.is_empty() {
        let inventory_ids = row
            .get::<_, Option<String>>(3)?
            .filter(|value| !value.is_empty())
            .map(|value| serde_json::from_str(&value).map_err(from_sql_json_error))
            .transpose()?
            .unwrap_or_default();
        let order_ids = decode_order_ids(&row.get::<_, String>(4)?)?;
        (inventory_ids, order_ids)
    } else {
        let (_, inventory_ids, order_ids, _, _, _) =
            canonical_selections(&selections).map_err(|_| {
                rusqlite::Error::InvalidColumnType(
                    2,
                    "selections".into(),
                    rusqlite::types::Type::Text,
                )
            })?;
        (inventory_ids, order_ids)
    };
    Ok(RenewalRequest {
        id: row.get(0)?,
        idempotency_key: row.get(1)?,
        inventory_ids,
        order_ids,
        requested_by,
        state,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn request_selections(c: &Connection, request_id: i64) -> Result<Vec<RenewalSelection>> {
    let encoded = c
        .query_row(
            "SELECT selections FROM proxy_renewal_requests WHERE id=?1",
            rusqlite::params![request_id],
            |row| row.get::<_, Option<String>>(0),
        )?
        .context("renewal request has no exact selection snapshot")?;
    let selections: Vec<RenewalSelection> = serde_json::from_str(&encoded)?;
    Ok(canonical_selections(&selections)?.0)
}

fn renewal_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RenewalEvent> {
    let _: Option<String> = row.get(2)?;
    let _: Option<IpAddr> = parse_db_ip(row.get(4)?, 4)?;
    Ok(RenewalEvent {
        id: row.get(0)?,
        request_id: row.get(1)?,
        order_id: row.get(3)?,
        outcome: RenewalEventOutcome::from_db(&row.get::<_, String>(5)?)?,
        observed_at: row.get(6)?,
        new_expiry_at: row.get(7)?,
    })
}

fn exact_renewal_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExactRenewalEvent> {
    let event = renewal_event_from_row(row)?;
    let allocation_ip = parse_db_ip(row.get(4)?, 4)?.ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(4, "allocation_ip".into(), rusqlite::types::Type::Null)
    })?;
    Ok(ExactRenewalEvent {
        event,
        inventory_id: row.get(2)?,
        allocation_ip,
    })
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn table_has_column(c: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = c.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_proxy_lifecycle(c: &mut Connection) -> Result<()> {
    // Probe the OS CSPRNG before taking the migration lock. Public IDs never fall back to order,
    // local identity, timestamps, or SQLite's non-contractual random functions.
    let _ = new_inventory_id()?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let binding_has_inventory = table_has_column(&tx, "proxy_bindings", "inventory_id")?;
    let binding_has_allocation = table_has_column(&tx, "proxy_bindings", "allocation_ip")?;
    let binding_inventory = if binding_has_inventory {
        "inventory_id"
    } else {
        "NULL"
    };
    let binding_allocation = if binding_has_allocation {
        "allocation_ip"
    } else {
        "NULL"
    };
    let binding_rows = {
        let sql = format!(
            "SELECT {binding_inventory},provider,local_id,order_id,{binding_allocation},\
                    issued_at,authority_status,updated_at FROM proxy_bindings ORDER BY rowid"
        );
        let mut statement = tx.prepare(&sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut assigned = HashSet::with_capacity(binding_rows.len());
    let mut bindings = Vec::with_capacity(binding_rows.len());
    for (current, provider, local_id, order_id, allocation, issued_at, authority, updated_at) in
        binding_rows
    {
        let inventory_id = if current
            .as_deref()
            .is_some_and(|value| valid_inventory_id(value) && assigned.insert(value.to_owned()))
        {
            current.unwrap()
        } else {
            let mut replacement = None;
            for _ in 0..INVENTORY_ID_ATTEMPTS {
                let candidate = new_inventory_id()?;
                if assigned.insert(candidate.clone()) {
                    replacement = Some(candidate);
                    break;
                }
            }
            replacement.context("could not allocate unique proxy inventory id")?
        };
        // An old row has no allocation evidence. A malformed value is equally unresolved; neither
        // case is guessed from local identifiers or current provider inventory.
        let allocation = allocation.and_then(|value| value.parse::<IpAddr>().ok());
        bindings.push((
            inventory_id,
            provider,
            local_id,
            order_id,
            allocation,
            issued_at,
            authority,
            updated_at,
        ));
    }

    let request_has_selections = table_has_column(&tx, "proxy_renewal_requests", "selections")?;
    let request_has_inventory = table_has_column(&tx, "proxy_renewal_requests", "inventory_ids")?;
    let request_has_actor = table_has_column(&tx, "proxy_renewal_requests", "requested_by")?;
    let request_selections = if request_has_selections {
        "selections"
    } else {
        "NULL"
    };
    let request_inventory = if request_has_inventory {
        "inventory_ids"
    } else {
        "NULL"
    };
    let request_actor = if request_has_actor {
        "requested_by"
    } else {
        "NULL"
    };
    let requests = {
        let sql = format!(
            "SELECT id,idempotency_key,{request_selections},{request_inventory},order_ids,\
                    {request_actor},state,created_at,updated_at \
             FROM proxy_renewal_requests ORDER BY id"
        );
        let mut statement = tx.prepare(&sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let event_has_inventory = table_has_column(&tx, "proxy_renewal_events", "inventory_id")?;
    let event_has_allocation = table_has_column(&tx, "proxy_renewal_events", "allocation_ip")?;
    let event_inventory = if event_has_inventory {
        "inventory_id"
    } else {
        "NULL"
    };
    let event_allocation = if event_has_allocation {
        "allocation_ip"
    } else {
        "NULL"
    };
    let events = {
        let sql = format!(
            "SELECT id,request_id,{event_inventory},order_id,{event_allocation},outcome,\
                    observed_at,new_expiry_at FROM proxy_renewal_events ORDER BY id"
        );
        let mut statement = tx.prepare(&sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    tx.execute_batch(
        "DROP INDEX IF EXISTS proxy_bindings_authority_idx;
         DROP INDEX IF EXISTS proxy_bindings_inventory_id_idx;
         DROP INDEX IF EXISTS proxy_renewal_requests_state_idx;
         DROP INDEX IF EXISTS proxy_renewal_events_request_idx;
         ALTER TABLE proxy_bindings RENAME TO proxy_bindings_legacy;
         ALTER TABLE proxy_renewal_requests RENAME TO proxy_renewal_requests_legacy;
         ALTER TABLE proxy_renewal_events RENAME TO proxy_renewal_events_legacy;
         CREATE TABLE proxy_bindings(
            inventory_id TEXT NOT NULL UNIQUE CHECK(length(inventory_id) BETWEEN 1 AND 160),
            provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 64),
            local_id TEXT NOT NULL CHECK(length(local_id) BETWEEN 1 AND 255),
            order_id INTEGER NOT NULL CHECK(order_id > 0), allocation_ip TEXT,
            issued_at INTEGER NOT NULL CHECK(issued_at > 0),
            authority_status TEXT NOT NULL CHECK(authority_status IN ('local','unknown')),
            updated_at INTEGER NOT NULL CHECK(updated_at > 0),
            PRIMARY KEY(provider,local_id), UNIQUE(order_id,allocation_ip));
         CREATE TABLE proxy_renewal_requests(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            idempotency_key TEXT NOT NULL UNIQUE CHECK(length(idempotency_key) BETWEEN 1 AND 255),
            selections TEXT CHECK(length(selections) BETWEEN 1 AND 32768),
            inventory_ids TEXT CHECK(length(inventory_ids) BETWEEN 1 AND 16384),
            order_ids TEXT NOT NULL CHECK(length(order_ids) BETWEEN 1 AND 8192),
            requested_by TEXT NOT NULL CHECK(length(requested_by) BETWEEN 1 AND 128),
            state TEXT NOT NULL CHECK(state IN
                ('pending','in_progress','completed','failed','indeterminate')),
            created_at INTEGER NOT NULL CHECK(created_at > 0),
            updated_at INTEGER NOT NULL CHECK(updated_at > 0));
         CREATE TABLE proxy_renewal_events(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id INTEGER NOT NULL REFERENCES proxy_renewal_requests(id),
            inventory_id TEXT, order_id INTEGER NOT NULL CHECK(order_id > 0), allocation_ip TEXT,
            outcome TEXT NOT NULL CHECK(outcome IN
                ('renewed','unchanged','not_found','rejected','provider_unavailable','indeterminate')),
            observed_at INTEGER NOT NULL CHECK(observed_at > 0),
            new_expiry_at INTEGER CHECK(new_expiry_at IS NULL OR new_expiry_at > 0),
            UNIQUE(request_id,inventory_id));"
    )?;
    for (
        inventory_id,
        provider,
        local_id,
        order_id,
        allocation,
        issued_at,
        authority,
        updated_at,
    ) in bindings
    {
        tx.execute(
            "INSERT INTO proxy_bindings VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                inventory_id,
                provider,
                local_id,
                order_id,
                allocation.map(|ip| ip.to_string()),
                issued_at,
                authority,
                updated_at
            ],
        )?;
    }
    for (id, key, selections, inventory_ids, order_ids, actor, state, created_at, updated_at) in
        requests
    {
        let exact = selections
            .as_deref()
            .filter(|value| !value.is_empty())
            .and_then(|value| serde_json::from_str::<Vec<RenewalSelection>>(value).ok())
            .and_then(|value| canonical_selections(&value).ok())
            .is_some();
        let selections = exact.then_some(selections).flatten();
        let state = if state == "in_progress" || (!exact && state == "pending") {
            "indeterminate"
        } else {
            state.as_str()
        };
        let updated_at = if state == "indeterminate" {
            now().max(1)
        } else {
            updated_at
        };
        let actor = actor
            .filter(|value| valid_requested_by(value))
            .unwrap_or_else(|| LEGACY_REQUESTED_BY.to_string());
        tx.execute(
            "INSERT INTO proxy_renewal_requests VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                id,
                key,
                selections,
                inventory_ids,
                order_ids,
                actor,
                state,
                created_at,
                updated_at
            ],
        )?;
    }
    for (id, request_id, inventory_id, order_id, allocation, outcome, observed_at, expiry) in events
    {
        let allocation = allocation.and_then(|value| value.parse::<IpAddr>().ok());
        tx.execute(
            "INSERT INTO proxy_renewal_events VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                id,
                request_id,
                inventory_id.filter(|value| valid_inventory_id(value)),
                order_id,
                allocation.map(|ip| ip.to_string()),
                outcome,
                observed_at,
                expiry
            ],
        )?;
    }
    tx.execute_batch(
        "DROP TABLE proxy_renewal_events_legacy;
         DROP TABLE proxy_renewal_requests_legacy;
         DROP TABLE proxy_bindings_legacy;
         CREATE INDEX proxy_bindings_authority_idx
            ON proxy_bindings(authority_status,provider,local_id);
         CREATE INDEX proxy_renewal_requests_state_idx
            ON proxy_renewal_requests(state,created_at,id);
         CREATE INDEX proxy_renewal_events_request_idx
            ON proxy_renewal_events(request_id,id);",
    )?;
    tx.commit()?;
    Ok(())
}

fn seller_job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SellerJob> {
    Ok(SellerJob {
        seller_chat: row.get(0)?,
        reference: SellerJobRef {
            kind: row.get(1)?,
            offer_id: row.get(2)?,
            batch_id: row.get(3)?,
            item_no: row.get(4)?,
            token: row.get(5)?,
        },
        product: row.get(6)?,
        phase: row.get(7)?,
        total: row.get(8)?,
    })
}

impl Store {
    pub fn open(path: &str) -> Result<Store> {
        let path_ref = std::path::Path::new(path);
        if path != ":memory:" {
            if let Ok(metadata) = std::fs::symlink_metadata(path_ref) {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    bail!("authbot state database must be a regular non-symlink file");
                }
            }
        }
        if let Some(dir) = path_ref.parent() {
            if !dir.as_os_str().is_empty() {
                let existed = dir.exists();
                std::fs::create_dir_all(dir).context("create authbot state directory")?;
                let metadata =
                    std::fs::symlink_metadata(dir).context("stat authbot state directory")?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("authbot state directory must be a real directory");
                }
                if !existed {
                    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
                } else if metadata.permissions().mode() & 0o077 != 0 {
                    bail!("authbot state directory must not be accessible by group or others");
                }
            }
        }
        let mut c = Connection::open(path)?;
        if path != ":memory:" {
            std::fs::set_permissions(path_ref, std::fs::Permissions::from_mode(0o600))?;
        }
        c.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS users(
                chat_id INTEGER PRIMARY KEY, uid INTEGER, username TEXT DEFAULT '',
                status TEXT DEFAULT 'new', role TEXT DEFAULT '', address TEXT DEFAULT '',
                want TEXT DEFAULT '', hproxy TEXT DEFAULT '', ts INTEGER DEFAULT 0);
             CREATE TABLE IF NOT EXISTS offers(
                id INTEGER PRIMARY KEY AUTOINCREMENT, product TEXT, price TEXT,
                created_by INTEGER, ts INTEGER DEFAULT 0,
                proxy_source TEXT DEFAULT 'legacy', buyer_proxy TEXT DEFAULT '');
             CREATE TABLE IF NOT EXISTS responses(
                offer_id INTEGER, uid INTEGER, status TEXT DEFAULT '', address TEXT DEFAULT '',
                ts INTEGER DEFAULT 0, PRIMARY KEY(offer_id, uid));
             CREATE TABLE IF NOT EXISTS offer_archive_events(
                offer_id INTEGER PRIMARY KEY,
                seller_chat INTEGER NOT NULL, seller_uid INTEGER NOT NULL,
                response_status TEXT NOT NULL, job_phase TEXT NOT NULL,
                archived_by INTEGER NOT NULL, ts INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS admin_state(
                chat_id INTEGER PRIMARY KEY, step TEXT, product TEXT DEFAULT '',
                mode TEXT DEFAULT 'single', quantity INTEGER DEFAULT 1,
                unit_price TEXT DEFAULT '', proxy_source TEXT DEFAULT '',
                draft_proxies TEXT DEFAULT '');
             CREATE TABLE IF NOT EXISTS gemini_oauth_sessions(
                state TEXT PRIMARY KEY,
                chat_id INTEGER NOT NULL UNIQUE,
                sealed_payload TEXT NOT NULL,
                expires_ts INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                job_kind TEXT NOT NULL DEFAULT '',
                job_offer_id INTEGER NOT NULL DEFAULT 0,
                job_batch_id INTEGER NOT NULL DEFAULT 0,
                job_item_no INTEGER NOT NULL DEFAULT 0,
                job_token TEXT NOT NULL DEFAULT '',
                ts INTEGER NOT NULL DEFAULT 0);
             -- Token material of a Google account that Google itself holds for verification. It
             -- passed OAuth, tier and project admission but was refused the acceptance generation,
             -- so it must NOT reach the roster. Keeping it sealed here is what lets the seller
             -- press one button after verifying instead of walking both consents again; it is
             -- fenced to the exact seller-job generation and expires on its own.
             CREATE TABLE IF NOT EXISTS gemini_pending_verifications(
                chat_id INTEGER PRIMARY KEY,
                sealed_payload TEXT NOT NULL,
                expires_ts INTEGER NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                job_kind TEXT NOT NULL DEFAULT '',
                job_offer_id INTEGER NOT NULL DEFAULT 0,
                job_batch_id INTEGER NOT NULL DEFAULT 0,
                job_item_no INTEGER NOT NULL DEFAULT 0,
                job_token TEXT NOT NULL DEFAULT '',
                ts INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS purchase_batches(                id INTEGER PRIMARY KEY AUTOINCREMENT,
                product TEXT NOT NULL, unit_price TEXT NOT NULL,
                quantity INTEGER NOT NULL, total_price TEXT NOT NULL,
                created_by INTEGER NOT NULL, seller_chat INTEGER NOT NULL,
                proxy_source TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'offered',
                payment_tx TEXT DEFAULT '', current_item INTEGER NOT NULL DEFAULT 0,
                ts INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS batch_items(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                batch_id INTEGER NOT NULL, item_no INTEGER NOT NULL,
                product TEXT NOT NULL, price TEXT NOT NULL,
                proxy TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'pending',
                UNIQUE(batch_id, item_no));
             CREATE TABLE IF NOT EXISTS seller_jobs(
                seller_chat INTEGER PRIMARY KEY,
                kind TEXT NOT NULL, offer_id INTEGER NOT NULL DEFAULT 0,
                batch_id INTEGER NOT NULL DEFAULT 0, item_no INTEGER NOT NULL DEFAULT 0,
                job_token TEXT NOT NULL DEFAULT '',
                product TEXT NOT NULL, phase TEXT NOT NULL,
                ts INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS proxy_bindings(
                inventory_id TEXT NOT NULL UNIQUE CHECK(length(inventory_id) BETWEEN 1 AND 160),
                provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 64),
                local_id TEXT NOT NULL CHECK(length(local_id) BETWEEN 1 AND 255),
                order_id INTEGER NOT NULL CHECK(order_id > 0),
                allocation_ip TEXT,
                issued_at INTEGER NOT NULL CHECK(issued_at > 0),
                authority_status TEXT NOT NULL
                    CHECK(authority_status IN ('local','unknown')),
                updated_at INTEGER NOT NULL CHECK(updated_at > 0),
                PRIMARY KEY(provider,local_id), UNIQUE(order_id,allocation_ip));
             CREATE INDEX IF NOT EXISTS proxy_bindings_authority_idx
                ON proxy_bindings(authority_status,provider,local_id);
             CREATE TABLE IF NOT EXISTS proxy_renewal_requests(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                idempotency_key TEXT NOT NULL UNIQUE
                    CHECK(length(idempotency_key) BETWEEN 1 AND 255),
                selections TEXT CHECK(length(selections) BETWEEN 1 AND 32768),
                inventory_ids TEXT CHECK(length(inventory_ids) BETWEEN 1 AND 16384),
                order_ids TEXT NOT NULL CHECK(length(order_ids) BETWEEN 1 AND 8192),
                requested_by TEXT NOT NULL CHECK(length(requested_by) BETWEEN 1 AND 128),
                state TEXT NOT NULL CHECK(state IN
                    ('pending','in_progress','completed','failed','indeterminate')),
                created_at INTEGER NOT NULL CHECK(created_at > 0),
                updated_at INTEGER NOT NULL CHECK(updated_at > 0));
             CREATE INDEX IF NOT EXISTS proxy_renewal_requests_state_idx
                ON proxy_renewal_requests(state,created_at,id);
             CREATE TABLE IF NOT EXISTS proxy_renewal_events(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id INTEGER NOT NULL REFERENCES proxy_renewal_requests(id),
                inventory_id TEXT,
                order_id INTEGER NOT NULL CHECK(order_id > 0),
                allocation_ip TEXT,
                outcome TEXT NOT NULL CHECK(outcome IN
                    ('renewed','unchanged','not_found','rejected',
                     'provider_unavailable','indeterminate')),
                observed_at INTEGER NOT NULL CHECK(observed_at > 0),
                new_expiry_at INTEGER CHECK(new_expiry_at IS NULL OR new_expiry_at > 0),
                UNIQUE(request_id,inventory_id));
             CREATE INDEX IF NOT EXISTS proxy_renewal_events_request_idx
                ON proxy_renewal_events(request_id,id);",
        )?;
        let _ = c.execute("ALTER TABLE users ADD COLUMN hproxy TEXT DEFAULT ''", []); // мягкая миграция
                                                                                      // Legacy Developer-API builds added `hproject`. It is intentionally ignored: OAuth
                                                                                      // identity/project data now exists only inside the encrypted credential envelope.
        let _ = c.execute("ALTER TABLE users ADD COLUMN hproject TEXT DEFAULT ''", []);
        // IPRoyal order behind a bot-issued handover proxy, kept until Antigravity OAuth
        // seals the proxy/order pair into its one-use PKCE session.
        let _ = c.execute(
            "ALTER TABLE users ADD COLUMN hproxy_order INTEGER DEFAULT 0",
            [],
        );
        // GLM platform of the current seller job ("" = international default, "cn" =
        // bigmodel.cn). Per-deal context: prepare_glm_account resets it on entry.
        let _ = c.execute("ALTER TABLE users ADD COLUMN hregion TEXT DEFAULT ''", []);
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN sealed_payload TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN proxy_issued INTEGER DEFAULT 0",
            [],
        ); // 1 оффер = 1 прокси
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN seller_chat INTEGER DEFAULT 0",
            [],
        ); // адресный оффер: кому
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN proxy_source TEXT DEFAULT 'legacy'",
            [],
        ); // buyer | seller | legacy
        let _ = c.execute(
            "ALTER TABLE offers ADD COLUMN buyer_proxy TEXT DEFAULT ''",
            [],
        ); // секрет прокси покупателя для одиночного оффера
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN seller_chat INTEGER DEFAULT 0",
            [],
        ); // выбранный продавец
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN mode TEXT DEFAULT 'single'",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN quantity INTEGER DEFAULT 1",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN unit_price TEXT DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN proxy_source TEXT DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE admin_state ADD COLUMN draft_proxies TEXT DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_kind TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_offer_id INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_batch_id INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_item_no INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_oauth_sessions ADD COLUMN job_token TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE seller_jobs ADD COLUMN job_token TEXT NOT NULL DEFAULT ''",
            [],
        );
        // Автопроба припаркованного аккаунта. Expand-only: старые записи получают нули и будут
        // подхвачены первой же парковкой, а не воскреснут с невалидным расписанием.
        let _ = c.execute(
            "ALTER TABLE gemini_pending_verifications ADD COLUMN next_probe_ts INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_pending_verifications ADD COLUMN probe_deadline_ts INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_pending_verifications ADD COLUMN deadline_notified INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE gemini_pending_verifications ADD COLUMN last_failure TEXT NOT NULL DEFAULT ''",
            [],
        );
        c.execute(
            "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))) WHERE job_token=''",
            [],
        )?;
        migrate_proxy_lifecycle(&mut c)?;
        Ok(Store { c: Mutex::new(c) })
    }

    // ── пользователи ─────────────────────────────────────────────────────────
    pub fn register_user(&self, chat: i64, uid: i64, username: &str) -> Result<UserRow> {
        let c = self.c.lock().unwrap();
        c.execute(
            "INSERT INTO users(chat_id, uid, username, status, ts) VALUES(?1,?2,?3,'new',?4)
             ON CONFLICT(chat_id) DO UPDATE SET uid=excluded.uid, username=excluded.username",
            rusqlite::params![chat, uid, username, now()],
        )?;
        drop(c);
        Ok(self.get_user(chat)?.unwrap_or_default())
    }

    pub fn get_user(&self, chat: i64) -> Result<Option<UserRow>> {
        let c = self.c.lock().unwrap();
        let r = c.query_row(
            "SELECT chat_id,uid,username,status,role,address,want,hproxy,hproxy_order,hregion FROM users WHERE chat_id=?1",
            rusqlite::params![chat],
            |r| Ok(UserRow {
                chat_id: r.get(0)?, uid: r.get(1)?, username: r.get(2)?, status: r.get(3)?,
                role: r.get(4)?, address: r.get(5)?, want: r.get(6)?, hproxy: r.get(7)?,
                hproxy_order: r.get(8)?, hregion: r.get(9)?,
    }),
        ).optional()?;
        Ok(r)
    }

    pub fn set_status(&self, chat: i64, status: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET status=?1 WHERE chat_id=?2",
            rusqlite::params![status, chat],
        )?;
        Ok(())
    }
    pub fn set_role(&self, chat: i64, role: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET role=?1 WHERE chat_id=?2",
            rusqlite::params![role, chat],
        )?;
        Ok(())
    }
    pub fn set_address(&self, chat: i64, addr: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET address=?1 WHERE chat_id=?2",
            rusqlite::params![addr, chat],
        )?;
        Ok(())
    }
    pub fn set_want(&self, chat: i64, want: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET want=?1 WHERE chat_id=?2",
            rusqlite::params![want, chat],
        )?;
        Ok(())
    }
    pub fn set_hproxy(&self, chat: i64, hproxy: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET hproxy=?1 WHERE chat_id=?2",
            rusqlite::params![hproxy, chat],
        )?;
        Ok(())
    }
    pub fn set_hproxy_order(&self, chat: i64, order_id: i64) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET hproxy_order=?1 WHERE chat_id=?2",
            rusqlite::params![order_id, chat],
        )?;
        Ok(())
    }
    /// GLM platform of the seller's current deal. Callers gate on the active GLM job and the
    /// `glm_ready` step; `""` restores the international default.
    pub fn set_hregion(&self, chat: i64, region: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE users SET hregion=?1 WHERE chat_id=?2",
            rusqlite::params![region, chat],
        )?;
        Ok(())
    }
    /// Persist a short-lived PKCE transaction so an authbot restart does not strand a seller in
    /// the browser. This table never contains Google access/refresh tokens or account identity.
    pub fn start_gemini_oauth(
        &self,
        chat_id: i64,
        state: &str,
        sealed_payload: &str,
        expires_ts: i64,
        proxy_order_id: i64,
    ) -> Result<Option<SellerJobRef>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1 OR expires_ts<?2",
            rusqlite::params![chat_id, now()],
        )?;
        // A brand new consent supersedes any account still waiting for Google's verification: its
        // sealed tokens belong to the previous generation and must not survive into this one.
        tx.execute(
            "DELETE FROM gemini_pending_verifications WHERE chat_id=?1 OR expires_ts<?2",
            rusqlite::params![chat_id, now()],
        )?;
        let mut job = tx
            .query_row(
                "SELECT kind,offer_id,batch_id,item_no,job_token
                 FROM seller_jobs WHERE seller_chat=?1 AND phase='processing'",
                rusqlite::params![chat_id],
                |row| {
                    Ok(SellerJobRef {
                        kind: row.get(0)?,
                        offer_id: row.get(1)?,
                        batch_id: row.get(2)?,
                        item_no: row.get(3)?,
                        token: row.get(4)?,
                    })
                },
            )
            .optional()?;
        if let Some(current) = job.as_mut() {
            let changed = tx.execute(
                "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))),ts=?3
                 WHERE seller_chat=?1 AND job_token=?2 AND phase='processing'",
                rusqlite::params![chat_id, current.token, now()],
            )?;
            if changed != 1 {
                tx.rollback()?;
                bail!("active seller job changed while starting Gemini OAuth");
            }
            current.token = tx.query_row(
                "SELECT job_token FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![chat_id],
                |row| row.get(0),
            )?;
        }
        let job = job.unwrap_or(SellerJobRef {
            kind: String::new(),
            offer_id: 0,
            batch_id: 0,
            item_no: 0,
            token: String::new(),
        });
        let bound_job = (!job.kind.is_empty()).then(|| job.clone());
        if let Some(expected) = bound_job.as_ref() {
            let changed = tx.execute(
                "UPDATE users SET want='gm_wait',hproxy='',hproxy_order=?1 WHERE chat_id=?2
                   AND EXISTS (
                       SELECT 1 FROM seller_jobs
                       WHERE seller_chat=?2 AND kind=?3 AND offer_id=?4 AND batch_id=?5
                         AND item_no=?6 AND job_token=?7 AND phase='processing')",
                rusqlite::params![
                    proxy_order_id,
                    chat_id,
                    expected.kind,
                    expected.offer_id,
                    expected.batch_id,
                    expected.item_no,
                    expected.token
                ],
            )?;
            if changed != 1 {
                tx.rollback()?;
                bail!("active seller job changed while persisting Gemini OAuth");
            }
        }
        tx.execute(
            "INSERT INTO gemini_oauth_sessions(
                state,chat_id,sealed_payload,expires_ts,status,ts,
                job_kind,job_offer_id,job_batch_id,job_item_no,job_token)
             VALUES(?1,?2,?3,?4,'pending',?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                state,
                chat_id,
                sealed_payload,
                expires_ts,
                now(),
                job.kind,
                job.offer_id,
                job.batch_id,
                job.item_no,
                job.token
            ],
        )?;
        tx.commit()?;
        Ok(bound_job)
    }

    /// Atomically replace a claimed Gemini OAuth phase with the next one while rotating the exact
    /// seller-job generation. The legacy bootstrap callback cannot create an Antigravity consent
    /// for a job that was paused, cancelled or replaced while Google was processing the first code.
    pub fn advance_gemini_oauth(
        &self,
        previous: &GeminiOAuthSession,
        state: &str,
        sealed_payload: &str,
        expires_ts: i64,
        proxy_order_id: i64,
    ) -> Result<Option<SellerJobRef>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let persisted = tx
            .query_row(
                "SELECT chat_id,job_kind,job_offer_id,job_batch_id,job_item_no,job_token
                 FROM gemini_oauth_sessions
                 WHERE state=?1 AND status='processing' AND expires_ts>=?2",
                rusqlite::params![previous.state, now()],
                |row| {
                    let kind: String = row.get(1)?;
                    let offer_id: i64 = row.get(2)?;
                    let batch_id: i64 = row.get(3)?;
                    let item_no: i64 = row.get(4)?;
                    let token: String = row.get(5)?;
                    Ok((
                        row.get::<_, i64>(0)?,
                        (!kind.is_empty()).then(|| SellerJobRef {
                            kind,
                            offer_id,
                            batch_id,
                            item_no,
                            token,
                        }),
                    ))
                },
            )
            .optional()?;
        if persisted.as_ref() != Some(&(previous.chat_id, previous.job.clone())) {
            tx.rollback()?;
            bail!("claimed Gemini OAuth phase changed before transition");
        }

        let mut next_job = previous.job.clone();
        if let Some(current) = next_job.as_mut() {
            let changed = tx.execute(
                "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))),ts=?7
                 WHERE seller_chat=?1 AND kind=?2 AND offer_id=?3 AND batch_id=?4
                   AND item_no=?5 AND job_token=?6 AND phase='processing'",
                rusqlite::params![
                    previous.chat_id,
                    current.kind,
                    current.offer_id,
                    current.batch_id,
                    current.item_no,
                    current.token,
                    now()
                ],
            )?;
            if changed != 1 {
                tx.rollback()?;
                bail!("seller job changed before Gemini OAuth phase transition");
            }
            current.token = tx.query_row(
                "SELECT job_token FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![previous.chat_id],
                |row| row.get(0),
            )?;
            let changed = tx.execute(
                "UPDATE users SET want='gm_wait',hproxy='',hproxy_order=?1 WHERE chat_id=?2
                   AND EXISTS (
                       SELECT 1 FROM seller_jobs
                       WHERE seller_chat=?2 AND kind=?3 AND offer_id=?4 AND batch_id=?5
                         AND item_no=?6 AND job_token=?7 AND phase='processing')",
                rusqlite::params![
                    proxy_order_id,
                    previous.chat_id,
                    current.kind,
                    current.offer_id,
                    current.batch_id,
                    current.item_no,
                    current.token
                ],
            )?;
            if changed != 1 {
                tx.rollback()?;
                bail!("seller state changed before Gemini OAuth phase transition");
            }
        }

        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![previous.chat_id],
        )?;
        let stored_job = next_job.clone().unwrap_or(SellerJobRef {
            kind: String::new(),
            offer_id: 0,
            batch_id: 0,
            item_no: 0,
            token: String::new(),
        });
        tx.execute(
            "INSERT INTO gemini_oauth_sessions(
                state,chat_id,sealed_payload,expires_ts,status,ts,
                job_kind,job_offer_id,job_batch_id,job_item_no,job_token)
             VALUES(?1,?2,?3,?4,'pending',?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                state,
                previous.chat_id,
                sealed_payload,
                expires_ts,
                now(),
                stored_job.kind,
                stored_job.offer_id,
                stored_job.batch_id,
                stored_job.item_no,
                stored_job.token
            ],
        )?;
        tx.commit()?;
        Ok(next_job)
    }

    /// Claim an OAuth callback exactly once. A repeated callback cannot exchange the same code or
    /// race a second credential publication.
    pub fn claim_gemini_oauth(&self, state: &str) -> Result<Option<GeminiOAuthSession>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let session = tx
            .query_row(
                "SELECT state,chat_id,sealed_payload,expires_ts,
                        job_kind,job_offer_id,job_batch_id,job_item_no,job_token
                 FROM gemini_oauth_sessions
                 WHERE state=?1 AND status='pending' AND expires_ts>=?2",
                rusqlite::params![state, now()],
                |row| {
                    let kind: String = row.get(4)?;
                    let offer_id: i64 = row.get(5)?;
                    let batch_id: i64 = row.get(6)?;
                    let item_no: i64 = row.get(7)?;
                    let token: String = row.get(8)?;
                    Ok(GeminiOAuthSession {
                        state: row.get(0)?,
                        chat_id: row.get(1)?,
                        sealed_payload: row.get(2)?,
                        expires_ts: row.get(3)?,
                        job: (!kind.is_empty()).then(|| SellerJobRef {
                            kind,
                            offer_id,
                            batch_id,
                            item_no,
                            token,
                        }),
                    })
                },
            )
            .optional()?;
        if session.is_some() {
            tx.execute(
                "UPDATE gemini_oauth_sessions SET status='processing',ts=?2
                 WHERE state=?1 AND status='pending'",
                rusqlite::params![state, now()],
            )?;
        }
        tx.commit()?;
        Ok(session)
    }

    /// Незавершённая PKCE-транзакция продавца — только чтобы вернуть работу на шаг назад, не
    /// спрашивая egress заново.
    ///
    /// Намеренно НЕ видит `status='processing'`: если callback уже забрал код, откат обязан
    /// отказать, а не гонку с обменом устраивать. Секрет остаётся запечатанным — распечатывает его
    /// только `gemini_oauth`, и наружу он не выходит.
    pub fn pending_gemini_session(&self, chat_id: i64) -> Result<Option<GeminiOAuthSession>> {
        Ok(self
            .c
            .lock()
            .unwrap()
            .query_row(
                "SELECT state,chat_id,sealed_payload,expires_ts,
                        job_kind,job_offer_id,job_batch_id,job_item_no,job_token
                 FROM gemini_oauth_sessions
                 WHERE chat_id=?1 AND status='pending' AND expires_ts>=?2",
                rusqlite::params![chat_id, now()],
                |row| {
                    let kind: String = row.get(4)?;
                    let offer_id: i64 = row.get(5)?;
                    let batch_id: i64 = row.get(6)?;
                    let item_no: i64 = row.get(7)?;
                    let token: String = row.get(8)?;
                    Ok(GeminiOAuthSession {
                        state: row.get(0)?,
                        chat_id: row.get(1)?,
                        sealed_payload: row.get(2)?,
                        expires_ts: row.get(3)?,
                        job: (!kind.is_empty()).then(|| SellerJobRef {
                            kind,
                            offer_id,
                            batch_id,
                            item_no,
                            token,
                        }),
                    })
                },
            )
            .optional()?)
    }

    /// Read the one exact OAuth generation regardless of whether its callback is merely pending or
    /// already claimed. `/cancel` needs the sealed egress from a claimed generation so it can rotate
    /// the seller-job token, delete the old capability and immediately issue fresh links without
    /// asking for the proxy again.
    pub fn active_gemini_session(&self, chat_id: i64) -> Result<Option<GeminiOAuthSession>> {
        Ok(self
            .c
            .lock()
            .unwrap()
            .query_row(
                "SELECT state,chat_id,sealed_payload,expires_ts,
                        job_kind,job_offer_id,job_batch_id,job_item_no,job_token
                 FROM gemini_oauth_sessions WHERE chat_id=?1",
                rusqlite::params![chat_id],
                |row| {
                    let kind: String = row.get(4)?;
                    let offer_id: i64 = row.get(5)?;
                    let batch_id: i64 = row.get(6)?;
                    let item_no: i64 = row.get(7)?;
                    let token: String = row.get(8)?;
                    Ok(GeminiOAuthSession {
                        state: row.get(0)?,
                        chat_id: row.get(1)?,
                        sealed_payload: row.get(2)?,
                        expires_ts: row.get(3)?,
                        job: (!kind.is_empty()).then(|| SellerJobRef {
                            kind,
                            offer_id,
                            batch_id,
                            item_no,
                            token,
                        }),
                    })
                },
            )
            .optional()?)
    }

    /// Claimed callbacks cannot be resumed after a process restart: the one-use Google code may
    /// already have been exchanged. The new process lists only those generations and replaces each
    /// with a fresh state+PKCE transaction through the normal generation-rotating restart path.
    pub fn interrupted_gemini_chats(&self) -> Result<Vec<i64>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT chat_id FROM gemini_oauth_sessions
             WHERE status='processing' ORDER BY ts,chat_id",
        )?;
        let chats = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(chats)
    }

    /// Read one still-pending phase for form rendering without claiming its one-use capability.
    /// The caller receives only the sealed payload and opens it inside `gemini_oauth`.
    pub fn pending_gemini_session_by_state(
        &self,
        state: &str,
    ) -> Result<Option<GeminiOAuthSession>> {
        Ok(self
            .c
            .lock()
            .unwrap()
            .query_row(
                "SELECT state,chat_id,sealed_payload,expires_ts,
                        job_kind,job_offer_id,job_batch_id,job_item_no,job_token
                 FROM gemini_oauth_sessions
                 WHERE state=?1 AND status='pending' AND expires_ts>=?2",
                rusqlite::params![state, now()],
                |row| {
                    let kind: String = row.get(4)?;
                    let offer_id: i64 = row.get(5)?;
                    let batch_id: i64 = row.get(6)?;
                    let item_no: i64 = row.get(7)?;
                    let token: String = row.get(8)?;
                    Ok(GeminiOAuthSession {
                        state: row.get(0)?,
                        chat_id: row.get(1)?,
                        sealed_payload: row.get(2)?,
                        expires_ts: row.get(3)?,
                        job: (!kind.is_empty()).then(|| SellerJobRef {
                            kind,
                            offer_id,
                            batch_id,
                            item_no,
                            token,
                        }),
                    })
                },
            )
            .optional()?)
    }

    /// Идёт ли прямо сейчас обмен одноразового кода этого продавца.
    ///
    /// Пока идёт, шаг назад обязан отказать: код уже отдан Google и второй раз не сработает, а
    /// публикация может завершиться в любой момент. Отказать честно дешевле, чем оставить
    /// продавца с ощущением, что он вернулся, и разъехавшимся состоянием.
    pub fn gemini_oauth_in_flight(&self, chat_id: i64) -> Result<bool> {
        Ok(self.c.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM gemini_oauth_sessions
             WHERE chat_id=?1 AND status='processing')",
            rusqlite::params![chat_id],
            |row| row.get::<_, i64>(0),
        )? == 1)
    }

    /// Seal the token material of an account whose consent succeeded but whose acceptance
    /// generation has not passed yet. Exactly one such account may wait per seller chat, fenced to
    /// the seller-job generation that produced it, so a later deal can never publish an earlier
    /// deal's account. `probe_deadline_ts` bounds automatic retries; `expires_ts` bounds how long
    /// the credential itself stays recorded, and it is always the later of the two.
    pub fn park_gemini_verification(
        &self,
        chat_id: i64,
        sealed_payload: &str,
        expires_ts: i64,
        probe_deadline_ts: i64,
        next_probe_ts: i64,
        job: Option<&SellerJobRef>,
    ) -> Result<()> {
        let stored = job.cloned().unwrap_or(SellerJobRef {
            kind: String::new(),
            offer_id: 0,
            batch_id: 0,
            item_no: 0,
            token: String::new(),
        });
        self.c.lock().unwrap().execute(
            "INSERT INTO gemini_pending_verifications(
                chat_id,sealed_payload,expires_ts,attempts,ts,
                job_kind,job_offer_id,job_batch_id,job_item_no,job_token,
                next_probe_ts,probe_deadline_ts,deadline_notified,last_failure)
             VALUES(?1,?2,?3,0,?4,?5,?6,?7,?8,?9,?10,?11,0,'')
             ON CONFLICT(chat_id) DO UPDATE SET
                sealed_payload=excluded.sealed_payload,expires_ts=excluded.expires_ts,
                attempts=0,ts=excluded.ts,job_kind=excluded.job_kind,
                job_offer_id=excluded.job_offer_id,job_batch_id=excluded.job_batch_id,
                job_item_no=excluded.job_item_no,job_token=excluded.job_token,
                next_probe_ts=excluded.next_probe_ts,
                probe_deadline_ts=excluded.probe_deadline_ts,
                deadline_notified=0,last_failure=''",
            rusqlite::params![
                chat_id,
                sealed_payload,
                expires_ts,
                now(),
                stored.kind,
                stored.offer_id,
                stored.batch_id,
                stored.item_no,
                stored.token,
                next_probe_ts,
                probe_deadline_ts
            ],
        )?;
        Ok(())
    }

    /// Read the parked account and count this attempt in one transaction, so a seller hammering
    /// the button — or the background sweep racing that press — cannot start several paid
    /// acceptance generations from one parked record. Claiming also pushes `next_probe_ts` forward
    /// by one interval, which is what serializes the sweep against the button.
    pub fn claim_gemini_verification(
        &self,
        chat_id: i64,
        next_probe_ts: i64,
    ) -> Result<Option<GeminiPendingVerification>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM gemini_pending_verifications WHERE expires_ts<?1",
            rusqlite::params![now()],
        )?;
        let parked = tx
            .query_row(
                "SELECT sealed_payload,expires_ts,attempts,
                        job_kind,job_offer_id,job_batch_id,job_item_no,job_token,
                        next_probe_ts,probe_deadline_ts,deadline_notified
                 FROM gemini_pending_verifications WHERE chat_id=?1",
                rusqlite::params![chat_id],
                |row| {
                    let kind: String = row.get(3)?;
                    let job = (!kind.is_empty()).then(|| SellerJobRef {
                        kind,
                        offer_id: row.get(4).unwrap_or_default(),
                        batch_id: row.get(5).unwrap_or_default(),
                        item_no: row.get(6).unwrap_or_default(),
                        token: row.get(7).unwrap_or_default(),
                    });
                    Ok(GeminiPendingVerification {
                        chat_id,
                        sealed_payload: row.get(0)?,
                        expires_ts: row.get(1)?,
                        attempts: row.get::<_, i64>(2)? + 1,
                        next_probe_ts: row.get(8).unwrap_or_default(),
                        probe_deadline_ts: row.get(9).unwrap_or_default(),
                        deadline_notified: row.get::<_, i64>(10).unwrap_or_default() != 0,
                        job,
                    })
                },
            )
            .optional()?;
        if parked.is_some() {
            tx.execute(
                "UPDATE gemini_pending_verifications
                 SET attempts=attempts+1,ts=?2,next_probe_ts=?3 WHERE chat_id=?1",
                rusqlite::params![chat_id, now(), next_probe_ts],
            )?;
        }
        tx.commit()?;
        Ok(parked)
    }

    /// Sellers whose parked account is due for another automatic acceptance attempt. The sweep
    /// claims each of them separately, so this read only decides who to look at.
    pub fn due_gemini_verifications(&self) -> Result<Vec<i64>> {
        let now = now();
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT chat_id FROM gemini_pending_verifications
             WHERE expires_ts>=?1 AND probe_deadline_ts>?1 AND next_probe_ts<=?1
             ORDER BY next_probe_ts",
        )?;
        let due = statement
            .query_map(rusqlite::params![now], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<i64>, _>>()?;
        Ok(due)
    }

    /// Sellers whose automatic window has just closed and who have not been told yet. The
    /// credential stays recorded; only probing stops.
    pub fn expired_gemini_probe_windows(&self) -> Result<Vec<i64>> {
        let now = now();
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT chat_id FROM gemini_pending_verifications
             WHERE expires_ts>=?1 AND probe_deadline_ts>0 AND probe_deadline_ts<=?1
                   AND deadline_notified=0",
        )?;
        let expired = statement
            .query_map(rusqlite::params![now], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<i64>, _>>()?;
        Ok(expired)
    }

    /// Record the outcome of one automatic attempt without touching the sealed material: when to
    /// try again and the bounded failure code shown to the seller. A terminal verdict passes
    /// `next_probe_ts = 0` together with a zero deadline to stop the sweep while keeping the
    /// credential on record.
    pub fn schedule_gemini_probe(
        &self,
        chat_id: i64,
        next_probe_ts: i64,
        probe_deadline_ts: Option<i64>,
        last_failure: &str,
    ) -> Result<()> {
        let c = self.c.lock().unwrap();
        match probe_deadline_ts {
            Some(deadline) => c.execute(
                "UPDATE gemini_pending_verifications
                 SET next_probe_ts=?2,probe_deadline_ts=?3,last_failure=?4,ts=?5
                 WHERE chat_id=?1",
                rusqlite::params![chat_id, next_probe_ts, deadline, last_failure, now()],
            )?,
            None => c.execute(
                "UPDATE gemini_pending_verifications
                 SET next_probe_ts=?2,last_failure=?3,ts=?4 WHERE chat_id=?1",
                rusqlite::params![chat_id, next_probe_ts, last_failure, now()],
            )?,
        };
        Ok(())
    }

    /// One-shot flag: the seller and the admins have been told the automatic window closed.
    pub fn mark_gemini_probe_window_notified(&self, chat_id: i64) -> Result<bool> {
        Ok(self.c.lock().unwrap().execute(
            "UPDATE gemini_pending_verifications SET deadline_notified=1
             WHERE chat_id=?1 AND deadline_notified=0",
            rusqlite::params![chat_id],
        )? == 1)
    }

    /// Replace only the sealed material of an already parked account — a refreshed access token or
    /// a tier/project a later attempt managed to resolve. The schedule, attempt count and
    /// seller-job fence are deliberately untouched.
    pub fn reseal_gemini_verification(&self, chat_id: i64, sealed_payload: &str) -> Result<bool> {
        Ok(self.c.lock().unwrap().execute(
            "UPDATE gemini_pending_verifications SET sealed_payload=?2,ts=?3 WHERE chat_id=?1",
            rusqlite::params![chat_id, sealed_payload, now()],
        )? == 1)
    }

    /// Is an account still parked for this seller? Read-only: it must not consume an attempt,
    /// because it only decides whether the retry button is worth offering.
    pub fn gemini_verification_is_parked(&self, chat_id: i64) -> Result<bool> {
        let parked = self
            .c
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM gemini_pending_verifications WHERE chat_id=?1 AND expires_ts>=?2",
                rusqlite::params![chat_id, now()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(parked.is_some())
    }

    pub fn clear_gemini_verification(&self, chat_id: i64) -> Result<()> {
        self.c.lock().unwrap().execute(
            "DELETE FROM gemini_pending_verifications WHERE chat_id=?1",
            rusqlite::params![chat_id],
        )?;
        Ok(())
    }

    pub fn finish_gemini_oauth(&self, state: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "DELETE FROM gemini_oauth_sessions WHERE state=?1",
            rusqlite::params![state],
        )?;
        Ok(())
    }

    pub fn fail_gemini_oauth(&self, state: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "DELETE FROM gemini_oauth_sessions WHERE state=?1",
            rusqlite::params![state],
        )?;
        Ok(())
    }

    /// A Claude OAuth child cannot survive a bot restart. Keep the proxy, but ask for email again
    /// so the seller receives a fresh authorization session instead of getting stuck at ho_code.
    pub fn recover_interrupted_handoffs(&self) -> Result<usize> {
        Ok(self
            .c
            .lock()
            .unwrap()
            .execute("UPDATE users SET want='ho_email' WHERE want='ho_code'", [])?)
    }

    /// The Codex device-flow child cannot survive a restart either, and its one-time code expires
    /// unattended. Return the seller to the email step so a fresh device flow can be issued instead
    /// of leaving them waiting for a confirmation nothing is polling for any more.
    pub fn recover_interrupted_codex_handoffs(&self) -> Result<usize> {
        Ok(self
            .c
            .lock()
            .unwrap()
            .execute("UPDATE users SET want='cx_email' WHERE want='cx_wait'", [])?)
    }

    /// Key validation lives only in memory and the key itself is never persisted, so a restart
    /// mid-validation loses it. Return the seller to the readiness step: they press the button
    /// again and resend the key into a fresh validation. The proxy and the platform selection
    /// survive, so the retry runs on the same egress and platform.
    pub fn recover_interrupted_glm_handoffs(&self) -> Result<usize> {
        Ok(self.c.lock().unwrap().execute(
            "UPDATE users SET want='glm_ready' WHERE want='glm_wait'",
            [],
        )?)
    }

    /// Normalize every removed Gemini custom-client wizard state to the single official-CLI proxy
    /// step. A retained bot-issued proxy lets the bot show account preparation without asking for
    /// the proxy again; authorization starts only after the seller confirms that the account is ready.
    pub fn recover_legacy_gemini_handoffs(&self) -> Result<usize> {
        Ok(self.c.lock().unwrap().execute(
            "UPDATE users SET want='gm_gproxy' WHERE want IN ('gm_proxy','gm_auth','gm_gid','gm_gsecret')",
            [],
        )?)
    }

    /// chat_id одобренных продавцов (для рассылки офферов).
    pub fn approved_sellers(&self) -> Result<Vec<i64>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare("SELECT chat_id FROM users WHERE status='approved'")?;
        let rows = s.query_map([], |r| r.get::<_, i64>(0))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    pub fn by_status(&self, status: &str) -> Result<Vec<UserRow>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare("SELECT chat_id,uid,username,status,role,address,want,hproxy,hproxy_order,hregion FROM users WHERE status=?1")?;
        let rows = s.query_map(rusqlite::params![status], |r| {
            Ok(UserRow {
                chat_id: r.get(0)?,
                uid: r.get(1)?,
                username: r.get(2)?,
                status: r.get(3)?,
                role: r.get(4)?,
                address: r.get(5)?,
                want: r.get(6)?,
                hproxy: r.get(7)?,
                hproxy_order: r.get(8)?,
                hregion: r.get(9)?,
            })
        })?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// Есть ли рантайм-админ с таким uid/username (role='admin').
    pub fn is_persisted_admin(&self, uid: i64, username: &str) -> Result<bool> {
        let c = self.c.lock().unwrap();
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM users WHERE role='admin' AND (uid=?1 OR (username<>'' AND lower(username)=lower(?2)))",
            rusqlite::params![uid, username], |r| r.get(0))?;
        Ok(n > 0)
    }

    // ── офферы ───────────────────────────────────────────────────────────────
    pub fn create_offer(
        &self,
        product: &str,
        price: &str,
        by: i64,
        seller_chat: i64,
    ) -> Result<i64> {
        self.create_offer_with_proxy(product, price, by, seller_chat, "legacy", "")
    }

    pub fn create_offer_with_proxy(
        &self,
        product: &str,
        price: &str,
        by: i64,
        seller_chat: i64,
        proxy_source: &str,
        buyer_proxy: &str,
    ) -> Result<i64> {
        let c = self.c.lock().unwrap();
        c.execute(
            "INSERT INTO offers(product,price,created_by,seller_chat,proxy_source,buyer_proxy,ts)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                product,
                price,
                by,
                seller_chat,
                proxy_source,
                buyer_proxy,
                now()
            ],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn get_offer(&self, id: i64) -> Result<Option<Offer>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,price,created_by,COALESCE(seller_chat,0),
                    COALESCE(proxy_source,'legacy'),COALESCE(buyer_proxy,'')
             FROM offers WHERE id=?1",
            rusqlite::params![id],
            |r| {
                Ok(Offer {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    price: r.get(2)?,
                    created_by: r.get(3)?,
                    seller_chat: r.get(4)?,
                    proxy_source: r.get(5)?,
                    buyer_proxy: r.get(6)?,
                })
            },
        )
        .optional()?)
    }

    pub fn set_response(&self, offer_id: i64, uid: i64, status: &str) -> Result<()> {
        self.c.lock().unwrap().execute(
            "INSERT INTO responses(offer_id,uid,status,ts) VALUES(?1,?2,?3,?4)
             ON CONFLICT(offer_id,uid) DO UPDATE SET status=excluded.status",
            rusqlite::params![offer_id, uid, status, now()],
        )?;
        Ok(())
    }

    pub fn decide_offer(&self, offer_id: i64, uid: i64, status: &str) -> Result<bool> {
        if !matches!(status, "accepted" | "rejected") {
            bail!("offer decision must be accepted or rejected");
        }
        let changed = self.c.lock().unwrap().execute(
            "INSERT OR IGNORE INTO responses(offer_id,uid,status,ts) VALUES(?1,?2,?3,?4)",
            rusqlite::params![offer_id, uid, status, now()],
        )?;
        Ok(changed == 1)
    }

    /// Accept and reserve this seller in one transaction. Waiting for an address/payment is part
    /// of the deal lifecycle, so a second single or batch cannot be accepted in the meantime.
    pub fn accept_offer(&self, offer_id: i64, seller_chat: i64, seller_uid: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let product = tx
            .query_row(
                "SELECT product FROM offers
                 WHERE id=?1 AND (COALESCE(seller_chat,0)=0 OR seller_chat=?2)",
                rusqlite::params![offer_id, seller_chat],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(product) = product else {
            tx.rollback()?;
            return Ok(false);
        };
        let job_changed = tx.execute(
            "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'offer',?2,0,0,lower(hex(randomblob(16))),?3,'accepted',?4
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1 AND status IN ('accepted','paying','paid','processing'))
               AND NOT EXISTS (
                   SELECT 1 FROM responses r
                   JOIN offers o ON o.id=r.offer_id
                   JOIN users u ON u.uid=r.uid
                   WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
            rusqlite::params![seller_chat, offer_id, product, now()],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let response_changed = tx.execute(
            "INSERT OR IGNORE INTO responses(offer_id,uid,status,ts)
             VALUES(?1,?2,'accepted',?3)",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        if response_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Правило «1 оффер = 1 прокси»: пометить, что по офферу прокси уже выпущен.
    pub fn mark_offer_proxy_issued(&self, offer_id: i64) -> Result<()> {
        self.c.lock().unwrap().execute(
            "UPDATE offers SET proxy_issued=1 WHERE id=?1",
            rusqlite::params![offer_id],
        )?;
        Ok(())
    }
    /// Выпускался ли уже прокси по этому офферу.
    pub fn offer_proxy_issued(&self, offer_id: i64) -> Result<bool> {
        let c = self.c.lock().unwrap();
        let n: i64 = c
            .query_row(
                "SELECT COALESCE(proxy_issued,0) FROM offers WHERE id=?1",
                rusqlite::params![offer_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(n > 0)
    }

    pub fn response_status(&self, offer_id: i64, uid: i64) -> Result<Option<String>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT status FROM responses WHERE offer_id=?1 AND uid=?2",
            rusqlite::params![offer_id, uid],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
    }

    pub fn accepted_offers_for_seller(&self, seller_chat: i64) -> Result<Vec<Offer>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT o.id,o.product,o.price,o.created_by,COALESCE(o.seller_chat,0),
                    COALESCE(o.proxy_source,'legacy'),COALESCE(o.buyer_proxy,'')
             FROM offers o
             WHERE o.seller_chat=?1
               AND EXISTS (SELECT 1 FROM responses r
                           WHERE r.offer_id=o.id AND r.status='accepted')
             ORDER BY o.id",
        )?;
        let rows = s.query_map(rusqlite::params![seller_chat], |r| {
            Ok(Offer {
                id: r.get(0)?,
                product: r.get(1)?,
                price: r.get(2)?,
                created_by: r.get(3)?,
                seller_chat: r.get(4)?,
                proxy_source: r.get(5)?,
                buyer_proxy: r.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    // ── единая активная работа продавца ─────────────────────────────────────

    /// Single-offer и batch используют один persisted lock. Поэтому глобальные seller fields
    /// (`want`/`hproxy`) всегда относятся ровно к одной явно названной работе.
    pub fn active_seller_job(&self, seller_chat: i64) -> Result<Option<SellerJob>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT j.seller_chat,j.kind,j.offer_id,j.batch_id,j.item_no,j.job_token,
                    j.product,j.phase,
                    CASE WHEN j.kind='batch' THEN COALESCE(b.quantity,0) ELSE 1 END
             FROM seller_jobs j
             LEFT JOIN purchase_batches b ON b.id=j.batch_id
             WHERE j.seller_chat=?1",
            rusqlite::params![seller_chat],
            seller_job_from_row,
        )
        .optional()?)
    }

    pub fn active_seller_jobs(&self) -> Result<Vec<SellerJob>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT j.seller_chat,j.kind,j.offer_id,j.batch_id,j.item_no,j.job_token,
                    j.product,j.phase,
                    CASE WHEN j.kind='batch' THEN COALESCE(b.quantity,0) ELSE 1 END
             FROM seller_jobs j
             LEFT JOIN purchase_batches b ON b.id=j.batch_id
             ORDER BY j.ts,j.seller_chat",
        )?;
        let rows = statement.query_map([], seller_job_from_row)?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// Start a new authorization attempt for the same work. Rotating the generation makes every
    /// callback from an earlier retry stale even though source/id/item are otherwise unchanged.
    pub fn rotate_seller_job_token(
        &self,
        seller_chat: i64,
        expected: &SellerJobRef,
    ) -> Result<Option<SellerJobRef>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let changed = tx.execute(
            "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))),ts=?6
             WHERE seller_chat=?1 AND kind=?2 AND offer_id=?3 AND batch_id=?4
               AND item_no=?5 AND job_token=?7 AND phase='processing'",
            rusqlite::params![
                seller_chat,
                expected.kind,
                expected.offer_id,
                expected.batch_id,
                expected.item_no,
                now(),
                expected.token
            ],
        )?;
        if changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        let updated = tx.query_row(
            "SELECT kind,offer_id,batch_id,item_no,job_token
             FROM seller_jobs WHERE seller_chat=?1",
            rusqlite::params![seller_chat],
            |row| {
                Ok(SellerJobRef {
                    kind: row.get(0)?,
                    offer_id: row.get(1)?,
                    batch_id: row.get(2)?,
                    item_no: row.get(3)?,
                    token: row.get(4)?,
                })
            },
        )?;
        tx.commit()?;
        Ok(Some(updated))
    }

    pub fn set_want_for_seller_job(
        &self,
        seller_chat: i64,
        expected: &SellerJobRef,
        want: &str,
    ) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE users SET want=?1 WHERE chat_id=?2
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?2 AND kind=?3 AND offer_id=?4 AND batch_id=?5
                     AND item_no=?6 AND job_token=?7 AND phase='processing')",
            rusqlite::params![
                want,
                seller_chat,
                expected.kind,
                expected.offer_id,
                expected.batch_id,
                expected.item_no,
                expected.token
            ],
        )?;
        Ok(changed == 1)
    }

    pub fn set_handoff_state_for_seller_job(
        &self,
        seller_chat: i64,
        expected: &SellerJobRef,
        want: &str,
        proxy: &str,
        proxy_order_id: i64,
    ) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE users SET want=?1,hproxy=?2,hproxy_order=?3 WHERE chat_id=?4
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?4 AND kind=?5 AND offer_id=?6 AND batch_id=?7
                     AND item_no=?8 AND job_token=?9 AND phase='processing')",
            rusqlite::params![
                want,
                proxy,
                proxy_order_id,
                seller_chat,
                expected.kind,
                expected.offer_id,
                expected.batch_id,
                expected.item_no,
                expected.token
            ],
        )?;
        Ok(changed == 1)
    }

    /// Ровно один шаг назад в передаче доступа продавца.
    ///
    /// Атомарно проверяет, что продавец всё ещё стоит на `expected_want` и что работа не сменила
    /// поколение, переписывает состояние, удаляет незавершённую Gemini-сессию и выдаёт новое
    /// поколение. Возвращает новый `SellerJobRef`, либо `None`, если откатывать уже нечего.
    ///
    /// Предикат `want=?expected_want` живёт внутри того же statement, что и generation guard, —
    /// это единственная точка сериализации во всей схеме. Двойное нажатие кнопки приходит двумя
    /// параллельными задачами (`tokio::spawn` на каждый апдейт), и без такого предиката
    /// read-then-write в хендлере увёл бы продавца на два шага вместо одного.
    ///
    /// `proxy: None` — `hproxy`/`hproxy_order` не трогаются вовсе (переход внутри одной egress).
    /// `Some((proxy, order))` — переписываются оба поля, и вызывающий обязан передать ТЕКУЩИЙ
    /// `hproxy_order`, а не `0`: это единственная ручка на оплаченный IPRoyal lease.
    ///
    /// `phase='processing'` в guard'е сам по себе отказывает работе в фазе `paying`, которая
    /// остаётся неизменяемой до admin review.
    pub fn rewind_handoff_step(
        &self,
        seller_chat: i64,
        expected: &SellerJobRef,
        expected_want: &str,
        want: &str,
        proxy: Option<(&str, i64)>,
    ) -> Result<Option<SellerJobRef>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        // Два отдельных литерала вместо COALESCE: пустая строка — осмысленное значение прокси,
        // и «не трогать» от «стереть» она отличаться обязана.
        let changed = match proxy {
            Some((proxy, proxy_order_id)) => tx.execute(
                "UPDATE users SET want=?1,hproxy=?2,hproxy_order=?3 WHERE chat_id=?4 AND want=?10
                   AND EXISTS (
                       SELECT 1 FROM seller_jobs
                       WHERE seller_chat=?4 AND kind=?5 AND offer_id=?6 AND batch_id=?7
                         AND item_no=?8 AND job_token=?9 AND phase='processing')",
                rusqlite::params![
                    want,
                    proxy,
                    proxy_order_id,
                    seller_chat,
                    expected.kind,
                    expected.offer_id,
                    expected.batch_id,
                    expected.item_no,
                    expected.token,
                    expected_want
                ],
            )?,
            None => tx.execute(
                "UPDATE users SET want=?1 WHERE chat_id=?2 AND want=?8
                   AND EXISTS (
                       SELECT 1 FROM seller_jobs
                       WHERE seller_chat=?2 AND kind=?3 AND offer_id=?4 AND batch_id=?5
                         AND item_no=?6 AND job_token=?7 AND phase='processing')",
                rusqlite::params![
                    want,
                    seller_chat,
                    expected.kind,
                    expected.offer_id,
                    expected.batch_id,
                    expected.item_no,
                    expected.token,
                    expected_want
                ],
            )?,
        };
        if changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        // У продавца ровно одна активная работа, поэтому любая ожидающая PKCE-транзакция
        // принадлежит именно той capability, которую этот откат и гасит.
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        let rotated = tx.execute(
            "UPDATE seller_jobs SET job_token=lower(hex(randomblob(16))),ts=?6
             WHERE seller_chat=?1 AND kind=?2 AND offer_id=?3 AND batch_id=?4
               AND item_no=?5 AND job_token=?7 AND phase='processing'",
            rusqlite::params![
                seller_chat,
                expected.kind,
                expected.offer_id,
                expected.batch_id,
                expected.item_no,
                now(),
                expected.token
            ],
        )?;
        if rotated != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        let updated = tx.query_row(
            "SELECT kind,offer_id,batch_id,item_no,job_token
             FROM seller_jobs WHERE seller_chat=?1",
            rusqlite::params![seller_chat],
            |row| {
                Ok(SellerJobRef {
                    kind: row.get(0)?,
                    offer_id: row.get(1)?,
                    batch_id: row.get(2)?,
                    item_no: row.get(3)?,
                    token: row.get(4)?,
                })
            },
        )?;
        tx.commit()?;
        Ok(Some(updated))
    }

    /// Expand-only rollout compatibility: active batches win, then one already accepted deal per
    /// otherwise idle seller is restored. Existing locks are never overwritten.
    pub fn recover_seller_jobs(&self) -> Result<usize> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let processing = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT b.seller_chat,'batch',0,b.id,b.current_item,
                    lower(hex(randomblob(16))),i.product,'processing',?1
             FROM purchase_batches b
             JOIN batch_items i ON i.batch_id=b.id AND i.item_no=b.current_item
             WHERE b.status='processing' AND i.status='processing'",
            rusqlite::params![now()],
        )?;
        let payments = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT seller_chat,'batch',0,id,0,lower(hex(randomblob(16))),product,'paying',?1
             FROM purchase_batches
             WHERE status IN ('paying','paid')",
            rusqlite::params![now()],
        )?;
        let accepted_batches = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT seller_chat,'batch',0,id,0,lower(hex(randomblob(16))),product,'accepted',ts
             FROM purchase_batches
             WHERE status='accepted'
             ORDER BY ts,id",
            [],
        )?;
        let accepted_offers = tx.execute(
            "INSERT OR IGNORE INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT u.chat_id,'offer',o.id,0,0,lower(hex(randomblob(16))),
                    o.product,'accepted',r.ts
             FROM responses r
             JOIN users u ON u.uid=r.uid
             JOIN offers o ON o.id=r.offer_id
             WHERE r.status='accepted'
             ORDER BY r.ts,o.id",
            [],
        )?;
        tx.commit()?;
        Ok(processing + payments + accepted_batches + accepted_offers)
    }

    /// Reserve the seller before the blockchain call. The response and seller lock move together,
    /// so two callbacks cannot pay/start a single offer concurrently with a batch.
    pub fn claim_offer_payment(&self, offer_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let offer = tx
            .query_row(
                "SELECT o.product,u.uid
                 FROM offers o JOIN users u ON u.chat_id=?2
                 WHERE o.id=?1 AND (COALESCE(o.seller_chat,0)=0 OR o.seller_chat=?2)",
                rusqlite::params![offer_id, seller_chat],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((product, seller_uid)) = offer else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='paying',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='accepted'",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        if response_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let mut job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='paying',ts=?3
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2 AND phase='accepted'",
            rusqlite::params![seller_chat, offer_id, now()],
        )?;
        if job_changed == 0 {
            // Compatibility for an offer accepted before seller_jobs existed.
            job_changed = tx.execute(
                "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'offer',?2,0,0,lower(hex(randomblob(16))),?3,'paying',?4
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1
                     AND status IN ('accepted','paying','paid','processing'))",
                rusqlite::params![seller_chat, offer_id, product, now()],
            )?;
        }
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn mark_offer_paid(&self, offer_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_uid = tx
            .query_row(
                "SELECT uid FROM users WHERE chat_id=?1",
                rusqlite::params![seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_uid) = seller_uid else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='paid',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='paying'",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        let job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='processing',ts=?3
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2 AND phase='paying'",
            rusqlite::params![seller_chat, offer_id, now()],
        )?;
        if response_changed != 1 || job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.commit()?;
        Ok(true)
    }

    /// Manual retry is allowed only after an admin has checked that the uncertain transaction did
    /// not land. The accepted deal keeps reserving the seller while retry is prepared.
    pub fn reset_offer_payment(&self, offer_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_uid = tx
            .query_row(
                "SELECT uid FROM users WHERE chat_id=?1",
                rusqlite::params![seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_uid) = seller_uid else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='accepted',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='paying'",
            rusqlite::params![offer_id, seller_uid, now()],
        )?;
        let job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='accepted',ts=?3
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2 AND phase='paying'",
            rusqlite::params![seller_chat, offer_id, now()],
        )?;
        if response_changed != 1 || job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.commit()?;
        Ok(true)
    }

    /// Complete only the exact offer captured when its handoff started. A delayed callback from a
    /// different flow cannot clear or advance the seller's current work.
    pub fn finish_offer_job(
        &self,
        seller_chat: i64,
        offer_id: i64,
        job_token: &str,
    ) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_uid = tx
            .query_row(
                "SELECT uid FROM users WHERE chat_id=?1",
                rusqlite::params![seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_uid) = seller_uid else {
            tx.rollback()?;
            return Ok(false);
        };
        let response_changed = tx.execute(
            "UPDATE responses SET status='completed',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status='paid'
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?4 AND kind='offer' AND offer_id=?1
                     AND job_token=?5 AND phase='processing')",
            rusqlite::params![offer_id, seller_uid, now(), seller_chat, job_token],
        )?;
        if response_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let job_changed = tx.execute(
            "DELETE FROM seller_jobs
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2
               AND job_token=?3 AND phase='processing'",
            rusqlite::params![seller_chat, offer_id, job_token],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Remove one exact single-offer generation from the operational queue while preserving its
    /// prior payment/work phase for audit. An uncertain `paying` state is deliberately immutable
    /// until the administrator completes payment review.
    pub fn archive_offer(
        &self,
        offer_id: i64,
        seller_chat: i64,
        expected_token: &str,
        archived_by: i64,
    ) -> Result<Option<String>> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let state = transaction
            .query_row(
                "SELECT u.uid,r.status,j.phase
                 FROM seller_jobs j
                 JOIN offers o ON o.id=j.offer_id
                 JOIN users u ON u.chat_id=j.seller_chat
                 JOIN responses r ON r.offer_id=j.offer_id AND r.uid=u.uid
                 WHERE j.seller_chat=?1 AND j.kind='offer' AND j.offer_id=?2
                   AND j.job_token=?3",
                rusqlite::params![seller_chat, offer_id, expected_token],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((seller_uid, response_status, job_phase)) = state else {
            transaction.rollback()?;
            return Ok(None);
        };
        let consistent = matches!(
            (response_status.as_str(), job_phase.as_str()),
            ("accepted", "accepted") | ("paid", "processing")
        );
        if !consistent {
            transaction.rollback()?;
            return Ok(None);
        }
        let timestamp = now();
        let audit_changed = transaction.execute(
            "INSERT INTO offer_archive_events(
                offer_id,seller_chat,seller_uid,response_status,job_phase,archived_by,ts)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                offer_id,
                seller_chat,
                seller_uid,
                response_status,
                job_phase,
                archived_by,
                timestamp
            ],
        )?;
        let response_changed = transaction.execute(
            "UPDATE responses SET status='cancelled',ts=?3
             WHERE offer_id=?1 AND uid=?2 AND status=?4",
            rusqlite::params![offer_id, seller_uid, timestamp, response_status],
        )?;
        let job_changed = transaction.execute(
            "DELETE FROM seller_jobs
             WHERE seller_chat=?1 AND kind='offer' AND offer_id=?2
               AND job_token=?3 AND phase=?4",
            rusqlite::params![seller_chat, offer_id, expected_token, job_phase],
        )?;
        if audit_changed != 1 || response_changed != 1 || job_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        transaction.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.commit()?;
        Ok(Some(job_phase))
    }

    // ── машина создания оффера (persisted) ────────────────────────────────────
    pub fn get_admin_state(&self, chat: i64) -> Result<Option<(String, String, i64)>> {
        Ok(self
            .get_admin_flow(chat)?
            .map(|state| (state.step, state.product, state.seller_chat)))
    }

    pub fn get_admin_flow(&self, chat: i64) -> Result<Option<AdminState>> {
        let c = self.c.lock().unwrap();
        let state = c
            .query_row(
                "SELECT chat_id,step,product,COALESCE(seller_chat,0),
                        COALESCE(mode,'single'),COALESCE(quantity,1),COALESCE(unit_price,''),
                        COALESCE(proxy_source,''),COALESCE(draft_proxies,'')
                 FROM admin_state WHERE chat_id=?1",
                rusqlite::params![chat],
                |r| {
                    let raw_proxies: String = r.get(8)?;
                    let draft_proxies = serde_json::from_str(&raw_proxies).unwrap_or_default();
                    Ok(AdminState {
                        chat_id: r.get(0)?,
                        step: r.get(1)?,
                        product: r.get(2)?,
                        seller_chat: r.get(3)?,
                        mode: r.get(4)?,
                        quantity: r.get(5)?,
                        unit_price: r.get(6)?,
                        proxy_source: r.get(7)?,
                        draft_proxies,
                    })
                },
            )
            .optional()?;
        Ok(state)
    }

    pub fn set_admin_state(
        &self,
        chat: i64,
        step: &str,
        product: &str,
        seller_chat: i64,
    ) -> Result<()> {
        self.set_admin_flow(&AdminState {
            chat_id: chat,
            step: step.to_string(),
            product: product.to_string(),
            seller_chat,
            mode: "single".into(),
            quantity: 1,
            unit_price: String::new(),
            proxy_source: String::new(),
            draft_proxies: Vec::new(),
        })
    }

    pub fn set_admin_flow(&self, state: &AdminState) -> Result<()> {
        let draft_proxies = serde_json::to_string(&state.draft_proxies)?;
        self.c.lock().unwrap().execute(
            "INSERT INTO admin_state(chat_id,step,product,seller_chat,mode,quantity,unit_price,proxy_source,draft_proxies)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(chat_id) DO UPDATE SET
                step=excluded.step, product=excluded.product, seller_chat=excluded.seller_chat,
                mode=excluded.mode, quantity=excluded.quantity, unit_price=excluded.unit_price,
                proxy_source=excluded.proxy_source, draft_proxies=excluded.draft_proxies",
            rusqlite::params![
                state.chat_id,
                state.step,
                state.product,
                state.seller_chat,
                state.mode,
                state.quantity,
                state.unit_price,
                state.proxy_source,
                draft_proxies
            ],
        )?;
        Ok(())
    }
    pub fn clear_admin_state(&self, chat: i64) -> Result<bool> {
        let n = self.c.lock().unwrap().execute(
            "DELETE FROM admin_state WHERE chat_id=?1",
            rusqlite::params![chat],
        )?;
        Ok(n > 0)
    }

    // ── batch-покупки ─────────────────────────────────────────────────────────
    pub fn create_batch(
        &self,
        product: &str,
        unit_price: &str,
        quantity: i64,
        total_price: &str,
        by: i64,
        seller_chat: i64,
        proxy_source: &str,
        proxies: &[String],
    ) -> Result<i64> {
        if !(2..=100).contains(&quantity) {
            bail!("batch quantity must be between 2 and 100");
        }
        match proxy_source {
            "buyer" if proxies.len() == quantity as usize => {}
            "buyer" => bail!("buyer-proxy batch must contain one proxy per item"),
            "seller" if proxies.is_empty() => {}
            "seller" => bail!("seller-proxy batch cannot contain buyer proxies"),
            _ => bail!("unknown batch proxy source"),
        }
        if proxies.iter().any(|proxy| proxy.trim().is_empty()) {
            bail!("batch proxies must not be empty");
        }
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO purchase_batches(product,unit_price,quantity,total_price,created_by,seller_chat,proxy_source,status,ts)
             VALUES(?1,?2,?3,?4,?5,?6,?7,'offered',?8)",
            rusqlite::params![
                product,
                unit_price,
                quantity,
                total_price,
                by,
                seller_chat,
                proxy_source,
                now()
            ],
        )?;
        let batch_id = tx.last_insert_rowid();
        for item_no in 1..=quantity {
            let proxy = proxies
                .get((item_no - 1) as usize)
                .map(String::as_str)
                .unwrap_or("");
            tx.execute(
                "INSERT INTO batch_items(batch_id,item_no,product,price,proxy,status)
                 VALUES(?1,?2,?3,?4,?5,'pending')",
                rusqlite::params![batch_id, item_no, product, unit_price, proxy],
            )?;
        }
        tx.commit()?;
        Ok(batch_id)
    }

    pub fn get_batch(&self, id: i64) -> Result<Option<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE id=?1",
            rusqlite::params![id],
            |r| {
                Ok(PurchaseBatch {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    unit_price: r.get(2)?,
                    quantity: r.get(3)?,
                    total_price: r.get(4)?,
                    created_by: r.get(5)?,
                    seller_chat: r.get(6)?,
                    proxy_source: r.get(7)?,
                    status: r.get(8)?,
                    payment_tx: r.get(9)?,
                    current_item: r.get(10)?,
                })
            },
        )
        .optional()?)
    }

    pub fn get_batch_item(&self, batch_id: i64, item_no: i64) -> Result<Option<BatchItem>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,batch_id,item_no,product,price,COALESCE(proxy,''),status
             FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, item_no],
            |r| {
                Ok(BatchItem {
                    id: r.get(0)?,
                    batch_id: r.get(1)?,
                    item_no: r.get(2)?,
                    product: r.get(3)?,
                    price: r.get(4)?,
                    proxy: r.get(5)?,
                    status: r.get(6)?,
                })
            },
        )
        .optional()?)
    }

    pub fn batch_items(&self, batch_id: i64) -> Result<Vec<BatchItem>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT id,batch_id,item_no,product,price,COALESCE(proxy,''),status
             FROM batch_items WHERE batch_id=?1 ORDER BY item_no",
        )?;
        let rows = s.query_map(rusqlite::params![batch_id], |r| {
            Ok(BatchItem {
                id: r.get(0)?,
                batch_id: r.get(1)?,
                item_no: r.get(2)?,
                product: r.get(3)?,
                price: r.get(4)?,
                proxy: r.get(5)?,
                status: r.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// Open batches for `/jobs`. Completed/rejected/cancelled history stays in SQLite but does not
    /// clutter the operational queue. `seller_chat=0` returns the admin-wide view.
    pub fn open_batch_overviews(&self, seller_chat: i64) -> Result<Vec<BatchOverview>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT b.id,b.product,b.unit_price,b.quantity,b.total_price,b.created_by,
                    b.seller_chat,b.proxy_source,b.status,COALESCE(b.payment_tx,''),
                    COALESCE(b.current_item,0),
                    SUM(CASE WHEN i.status='completed' THEN 1 ELSE 0 END)
             FROM purchase_batches b
             JOIN batch_items i ON i.batch_id=b.id
             WHERE b.status IN ('offered','accepted','paying','paid','processing','paused')
               AND (?1=0 OR b.seller_chat=?1)
             GROUP BY b.id
             ORDER BY CASE b.status WHEN 'processing' THEN 0 WHEN 'paused' THEN 1 ELSE 2 END,
                      b.ts,b.id",
        )?;
        let rows = statement.query_map(rusqlite::params![seller_chat], |row| {
            let quantity = row.get::<_, i64>(3)?;
            let completed = row.get::<_, i64>(11)?;
            Ok(BatchOverview {
                batch: PurchaseBatch {
                    id: row.get(0)?,
                    product: row.get(1)?,
                    unit_price: row.get(2)?,
                    quantity,
                    total_price: row.get(4)?,
                    created_by: row.get(5)?,
                    seller_chat: row.get(6)?,
                    proxy_source: row.get(7)?,
                    status: row.get(8)?,
                    payment_tx: row.get(9)?,
                    current_item: row.get(10)?,
                },
                completed,
                remaining: quantity - completed,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn paused_batch_for_seller(&self, seller_chat: i64) -> Result<Option<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                        proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
                 FROM purchase_batches
                 WHERE seller_chat=?1 AND status='paused' ORDER BY id LIMIT 1",
            rusqlite::params![seller_chat],
            |row| {
                Ok(PurchaseBatch {
                    id: row.get(0)?,
                    product: row.get(1)?,
                    unit_price: row.get(2)?,
                    quantity: row.get(3)?,
                    total_price: row.get(4)?,
                    created_by: row.get(5)?,
                    seller_chat: row.get(6)?,
                    proxy_source: row.get(7)?,
                    status: row.get(8)?,
                    payment_tx: row.get(9)?,
                    current_item: row.get(10)?,
                })
            },
        )
        .optional()?)
    }

    pub fn accept_batch(&self, batch_id: i64, seller_chat: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let product = tx
            .query_row(
                "SELECT product FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='offered'",
                rusqlite::params![batch_id, seller_chat],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(product) = product else {
            tx.rollback()?;
            return Ok(false);
        };
        let job_changed = tx.execute(
            "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'batch',0,?2,0,lower(hex(randomblob(16))),?3,'accepted',?4
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1 AND id<>?2
                     AND status IN ('accepted','paying','paid','processing','paused'))
               AND NOT EXISTS (
                   SELECT 1 FROM responses r
                   JOIN users u ON u.uid=r.uid
                   WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
            rusqlite::params![seller_chat, batch_id, product, now()],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET status='accepted',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='offered'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        if batch_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn reject_batch(&self, batch_id: i64, seller_chat: i64) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE purchase_batches SET status='rejected',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='offered'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        Ok(changed == 1)
    }

    /// Claim payment before calling the blockchain. Double-clicks and concurrent callbacks can
    /// therefore never send two payments or overlap this batch with a single offer.
    pub fn claim_batch_payment(&self, batch_id: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let batch = tx
            .query_row(
                "SELECT seller_chat,product FROM purchase_batches
                 WHERE id=?1 AND status='accepted'",
                rusqlite::params![batch_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((seller_chat, product)) = batch else {
            tx.rollback()?;
            return Ok(false);
        };
        let mut job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='paying',ts=?3
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=0 AND phase='accepted'",
            rusqlite::params![seller_chat, batch_id, now()],
        )?;
        if job_changed == 0 {
            // Compatibility for a batch accepted before seller_jobs existed.
            job_changed = tx.execute(
                "INSERT INTO seller_jobs(
                    seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
                 SELECT ?1,'batch',0,?2,0,lower(hex(randomblob(16))),?3,'paying',?4
                 WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
                   AND NOT EXISTS (
                       SELECT 1 FROM purchase_batches
                       WHERE seller_chat=?1 AND id<>?2
                         AND status IN ('accepted','paying','paid','processing','paused'))
                   AND NOT EXISTS (
                       SELECT 1 FROM responses r
                       JOIN users u ON u.uid=r.uid
                       WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
                rusqlite::params![seller_chat, batch_id, product, now()],
            )?;
        }
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET status='paying',ts=?2
             WHERE id=?1 AND status='accepted'",
            rusqlite::params![batch_id, now()],
        )?;
        if batch_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn mark_batch_paid(&self, batch_id: i64, tx_hash: &str) -> Result<bool> {
        let changed = self.c.lock().unwrap().execute(
            "UPDATE purchase_batches SET status='paid',payment_tx=?1,ts=?3
             WHERE id=?2 AND status='paying'",
            rusqlite::params![tx_hash, batch_id, now()],
        )?;
        Ok(changed == 1)
    }

    pub fn reset_batch_payment(&self, batch_id: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_chat = tx
            .query_row(
                "SELECT seller_chat FROM purchase_batches WHERE id=?1 AND status='paying'",
                rusqlite::params![batch_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_chat) = seller_chat else {
            tx.rollback()?;
            return Ok(false);
        };
        let changed = tx.execute(
            "UPDATE purchase_batches SET status='accepted',ts=?2
             WHERE id=?1 AND status='paying'",
            rusqlite::params![batch_id, now()],
        )?;
        let job_changed = tx.execute(
            "UPDATE seller_jobs SET phase='accepted',ts=?3
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=0 AND phase='paying'",
            rusqlite::params![seller_chat, batch_id, now()],
        )?;
        if changed != 1 || job_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.commit()?;
        Ok(true)
    }

    /// A process can stop after claiming a payment but before the subprocess returns. Keep the
    /// claim locked until an admin explicitly verifies the chain and releases it for retry; this
    /// avoids silently turning an uncertain blockchain operation into a duplicate payment.
    pub fn batches_needing_payment_review(&self) -> Result<Vec<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE status='paying' ORDER BY id",
        )?;
        let rows = s.query_map([], |r| {
            Ok(PurchaseBatch {
                id: r.get(0)?,
                product: r.get(1)?,
                unit_price: r.get(2)?,
                quantity: r.get(3)?,
                total_price: r.get(4)?,
                created_by: r.get(5)?,
                seller_chat: r.get(6)?,
                proxy_source: r.get(7)?,
                status: r.get(8)?,
                payment_tx: r.get(9)?,
                current_item: r.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn start_batch_item(&self, batch_id: i64, item_no: i64) -> Result<bool> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let batch = tx
            .query_row(
                "SELECT seller_chat,product,status,current_item
                 FROM purchase_batches WHERE id=?1",
                rusqlite::params![batch_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((seller_chat, product, batch_status, current_item)) = batch else {
            tx.rollback()?;
            return Ok(false);
        };
        let item_status = tx
            .query_row(
                "SELECT status FROM batch_items WHERE batch_id=?1 AND item_no=?2",
                rusqlite::params![batch_id, item_no],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let initial = batch_status == "paid"
            && current_item == 0
            && item_no == 1
            && item_status.as_deref() == Some("pending");
        let resumed = batch_status == "processing"
            && current_item == item_no
            && matches!(item_status.as_deref(), Some("pending" | "processing"));
        if !initial && !resumed {
            tx.rollback()?;
            return Ok(false);
        }
        let existing_job = tx
            .query_row(
                "SELECT kind,batch_id,item_no,job_token
                 FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![seller_chat],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        if let Some((kind, active_batch, active_item, _)) = existing_job.as_ref() {
            if kind != "batch"
                || *active_batch != batch_id
                || (*active_item != 0 && *active_item != item_no)
            {
                tx.rollback()?;
                return Ok(false);
            }
        }
        if initial || item_status.as_deref() == Some("pending") {
            let batch_changed = tx.execute(
                "UPDATE purchase_batches
                 SET status='processing',current_item=?2,ts=?3
                 WHERE id=?1 AND (
                    (status='paid' AND current_item=0 AND ?2=1)
                    OR (status='processing' AND current_item=?2))",
                rusqlite::params![batch_id, item_no, now()],
            )?;
            let item_changed = tx.execute(
                "UPDATE batch_items SET status='processing'
                 WHERE batch_id=?1 AND item_no=?2 AND status='pending'",
                rusqlite::params![batch_id, item_no],
            )?;
            if batch_changed != 1 || item_changed != 1 {
                tx.rollback()?;
                return Ok(false);
            }
        }
        if existing_job.is_some() {
            let changed = tx.execute(
                "UPDATE seller_jobs
                 SET job_token=CASE
                        WHEN item_no=?3 AND job_token<>'' THEN job_token
                        ELSE lower(hex(randomblob(16)))
                     END,
                     item_no=?3,product=?4,phase='processing',ts=?5
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
                   AND item_no IN (0,?3)",
                rusqlite::params![seller_chat, batch_id, item_no, product, now()],
            )?;
            if changed != 1 {
                tx.rollback()?;
                return Ok(false);
            }
        } else {
            tx.execute(
                "INSERT INTO seller_jobs(
                    seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
                 VALUES(?1,'batch',0,?2,?3,lower(hex(randomblob(16))),?4,'processing',?5)",
                rusqlite::params![seller_chat, batch_id, item_no, product, now()],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Finish one item and atomically move the cursor to the next one. The next item is started
    /// by the bot after the successful handoff, so Telegram/network work stays outside SQLite.
    pub fn finish_batch_item(
        &self,
        batch_id: i64,
        item_no: i64,
        job_token: &str,
    ) -> Result<Option<BatchCompletion>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let seller_chat = tx
            .query_row(
                "SELECT seller_chat FROM purchase_batches WHERE id=?1",
                rusqlite::params![batch_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(seller_chat) = seller_chat else {
            tx.rollback()?;
            return Ok(None);
        };
        let changed = tx.execute(
            "UPDATE batch_items SET status='completed'
             WHERE batch_id=?1 AND item_no=?2 AND status='processing'
               AND EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE id=?1 AND status='processing' AND current_item=?2
               )
               AND EXISTS (
                   SELECT 1 FROM seller_jobs
                   WHERE seller_chat=?3 AND kind='batch' AND batch_id=?1
                     AND item_no=?2 AND job_token=?4 AND phase='processing'
               )",
            rusqlite::params![batch_id, item_no, seller_chat, job_token],
        )?;
        if changed != 1 {
            tx.commit()?;
            return Ok(None);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        let (total, current): (i64, i64) = tx.query_row(
            "SELECT quantity,current_item FROM purchase_batches WHERE id=?1",
            rusqlite::params![batch_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if current >= total {
            let batch_changed = tx.execute(
                "UPDATE purchase_batches
                 SET status='completed',current_item=?2,ts=?3
                 WHERE id=?1 AND status='processing' AND current_item=?4",
                rusqlite::params![batch_id, total + 1, now(), item_no],
            )?;
            if batch_changed != 1 {
                tx.rollback()?;
                return Ok(None);
            }
            let job_changed = tx.execute(
                "DELETE FROM seller_jobs
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
                   AND item_no=?3 AND job_token=?4 AND phase='processing'",
                rusqlite::params![seller_chat, batch_id, item_no, job_token],
            )?;
            if job_changed != 1 {
                tx.rollback()?;
                return Ok(None);
            }
            tx.commit()?;
            return Ok(Some(BatchCompletion {
                batch_id,
                item_no,
                total,
                completed: true,
            }));
        }
        let next_item = current + 1;
        let next_product = tx.query_row(
            "SELECT product FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, next_item],
            |row| row.get::<_, String>(0),
        )?;
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET current_item=?2,ts=?3
             WHERE id=?1 AND status='processing' AND current_item=?4",
            rusqlite::params![batch_id, next_item, now(), item_no],
        )?;
        if batch_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        // Keep the seller reserved continuously between positions. The old implementation
        // deleted this row and recreated it when the next Telegram prompt was sent, leaving a
        // small window where another single/batch callback could occupy the seller.
        let job_changed = tx.execute(
            "UPDATE seller_jobs
             SET item_no=?5,job_token=lower(hex(randomblob(16))),product=?6,
                 phase='processing',ts=?7
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=?3 AND job_token=?4 AND phase='processing'",
            rusqlite::params![
                seller_chat,
                batch_id,
                item_no,
                job_token,
                next_item,
                next_product,
                now()
            ],
        )?;
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(BatchCompletion {
            batch_id,
            item_no,
            total,
            completed: false,
        }))
    }

    /// Pause an in-progress batch immediately. Completed items stay completed; the current
    /// unfinished item goes back to `pending`, and the seller lock is released for a single job.
    /// Every in-flight callback is invalidated by deleting its exact seller job generation.
    pub fn pause_batch(&self, batch_id: i64, seller_chat: i64) -> Result<Option<i64>> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let current_item = transaction
            .query_row(
                "SELECT current_item FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='processing' AND current_item>0",
                rusqlite::params![batch_id, seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(current_item) = current_item else {
            transaction.rollback()?;
            return Ok(None);
        };
        let item_status = transaction
            .query_row(
                "SELECT status FROM batch_items WHERE batch_id=?1 AND item_no=?2",
                rusqlite::params![batch_id, current_item],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if !matches!(item_status.as_deref(), Some("pending" | "processing")) {
            transaction.rollback()?;
            return Ok(None);
        }
        let job_changed = transaction.execute(
            "DELETE FROM seller_jobs
             WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2
               AND item_no=?3 AND phase='processing'",
            rusqlite::params![seller_chat, batch_id, current_item],
        )?;
        if job_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        transaction.execute(
            "UPDATE batch_items SET status='pending'
             WHERE batch_id=?1 AND item_no=?2 AND status='processing'",
            rusqlite::params![batch_id, current_item],
        )?;
        let batch_changed = transaction.execute(
            "UPDATE purchase_batches SET status='paused',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='processing'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        if batch_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        transaction.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.commit()?;
        Ok(Some(current_item))
    }

    /// Resume the exact pending position only when the seller is currently free. The new seller
    /// job receives a fresh generation before any Telegram instruction is sent.
    pub fn resume_paused_batch(&self, batch_id: i64, seller_chat: i64) -> Result<Option<i64>> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let batch = transaction
            .query_row(
                "SELECT current_item,product FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='paused' AND current_item>0",
                rusqlite::params![batch_id, seller_chat],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((current_item, product)) = batch else {
            transaction.rollback()?;
            return Ok(None);
        };
        let item_pending = transaction.query_row(
            "SELECT status='pending' FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, current_item],
            |row| row.get::<_, bool>(0),
        )?;
        if !item_pending {
            transaction.rollback()?;
            return Ok(None);
        }
        let job_changed = transaction.execute(
            "INSERT INTO seller_jobs(
                seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
             SELECT ?1,'batch',0,?2,?3,lower(hex(randomblob(16))),?4,'processing',?5
             WHERE NOT EXISTS (SELECT 1 FROM seller_jobs WHERE seller_chat=?1)
               AND NOT EXISTS (
                   SELECT 1 FROM purchase_batches
                   WHERE seller_chat=?1 AND id<>?2
                     AND status IN ('accepted','paying','paid','processing'))
               AND NOT EXISTS (
                   SELECT 1 FROM responses r
                   JOIN users u ON u.uid=r.uid
                   WHERE u.chat_id=?1 AND r.status IN ('accepted','paying'))",
            rusqlite::params![seller_chat, batch_id, current_item, product, now()],
        )?;
        if job_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        let batch_changed = transaction.execute(
            "UPDATE purchase_batches SET status='processing',ts=?3
             WHERE id=?1 AND seller_chat=?2 AND status='paused'",
            rusqlite::params![batch_id, seller_chat, now()],
        )?;
        if batch_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        transaction.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        transaction.commit()?;
        Ok(Some(current_item))
    }

    /// Remove a batch from the operational queue without destroying payment/audit history.
    /// Returns whether this action released the seller's active batch job.
    pub fn archive_batch(&self, batch_id: i64) -> Result<Option<bool>> {
        let mut connection = self.c.lock().unwrap();
        let transaction = connection.transaction()?;
        let batch = transaction
            .query_row(
                "SELECT seller_chat,status,current_item FROM purchase_batches
                 WHERE id=?1 AND status IN
                    ('offered','accepted','paid','processing','paused','rejected')",
                rusqlite::params![batch_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((seller_chat, status, current_item)) = batch else {
            transaction.rollback()?;
            return Ok(None);
        };
        let active_job = transaction
            .query_row(
                "SELECT kind,batch_id FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![seller_chat],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let releases_job = active_job
            .as_ref()
            .is_some_and(|(kind, active_batch)| kind == "batch" && *active_batch == batch_id);
        if active_job.is_some() && !releases_job && status != "paused" && status != "offered" {
            transaction.rollback()?;
            return Ok(None);
        }
        if current_item > 0 {
            transaction.execute(
                "UPDATE batch_items SET status='pending'
                 WHERE batch_id=?1 AND item_no=?2 AND status='processing'",
                rusqlite::params![batch_id, current_item],
            )?;
        }
        let batch_changed = transaction.execute(
            "UPDATE purchase_batches SET status='cancelled',ts=?2
             WHERE id=?1 AND status=?3",
            rusqlite::params![batch_id, now(), status],
        )?;
        if batch_changed != 1 {
            transaction.rollback()?;
            return Ok(None);
        }
        if releases_job {
            let job_changed = transaction.execute(
                "DELETE FROM seller_jobs
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2",
                rusqlite::params![seller_chat, batch_id],
            )?;
            if job_changed != 1 {
                transaction.rollback()?;
                return Ok(None);
            }
            transaction.execute(
                "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
                rusqlite::params![seller_chat],
            )?;
            transaction.execute(
                "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
                rusqlite::params![seller_chat],
            )?;
        }
        transaction.commit()?;
        Ok(Some(releases_job))
    }

    /// Admin-only recovery primitive for a position that an older bot version marked complete
    /// from an unrelated single-offer callback. It rewinds exactly one step and invalidates every
    /// in-flight input/OAuth capability for the later item. The paid batch itself stays paid.
    pub fn rewind_batch_to_previous(&self, batch_id: i64, seller_chat: i64) -> Result<Option<i64>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction()?;
        let current = tx
            .query_row(
                "SELECT current_item FROM purchase_batches
                 WHERE id=?1 AND seller_chat=?2 AND status='processing' AND current_item>1",
                rusqlite::params![batch_id, seller_chat],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(current) = current else {
            tx.rollback()?;
            return Ok(None);
        };
        let active_job = tx
            .query_row(
                "SELECT kind,batch_id,item_no FROM seller_jobs WHERE seller_chat=?1",
                rusqlite::params![seller_chat],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((kind, active_batch, active_item)) = active_job.as_ref() {
            if kind != "batch" || *active_batch != batch_id || *active_item != current {
                tx.rollback()?;
                return Ok(None);
            }
        }
        let previous = current - 1;
        let current_changed = tx.execute(
            "UPDATE batch_items SET status='pending'
             WHERE batch_id=?1 AND item_no=?2 AND status='processing'",
            rusqlite::params![batch_id, current],
        )?;
        let previous_changed = tx.execute(
            "UPDATE batch_items SET status='pending'
             WHERE batch_id=?1 AND item_no=?2 AND status='completed'",
            rusqlite::params![batch_id, previous],
        )?;
        let batch_changed = tx.execute(
            "UPDATE purchase_batches SET current_item=?2,ts=?3
             WHERE id=?1 AND seller_chat=?4 AND status='processing' AND current_item=?5",
            rusqlite::params![batch_id, previous, now(), seller_chat, current],
        )?;
        if current_changed != 1 || previous_changed != 1 || batch_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        let product = tx.query_row(
            "SELECT product FROM batch_items WHERE batch_id=?1 AND item_no=?2",
            rusqlite::params![batch_id, previous],
            |row| row.get::<_, String>(0),
        )?;
        let job_changed = if active_job.is_some() {
            tx.execute(
                "UPDATE seller_jobs
                 SET item_no=?4,job_token=lower(hex(randomblob(16))),product=?5,
                     phase='processing',ts=?6
                 WHERE seller_chat=?1 AND kind='batch' AND batch_id=?2 AND item_no=?3",
                rusqlite::params![seller_chat, batch_id, current, previous, product, now()],
            )?
        } else {
            tx.execute(
                "INSERT INTO seller_jobs(
                    seller_chat,kind,offer_id,batch_id,item_no,job_token,product,phase,ts)
                 VALUES(?1,'batch',0,?2,?3,lower(hex(randomblob(16))),?4,'processing',?5)",
                rusqlite::params![seller_chat, batch_id, previous, product, now()],
            )?
        };
        if job_changed != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "DELETE FROM gemini_oauth_sessions WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.execute(
            "UPDATE users SET want='',hproxy='',hproxy_order=0 WHERE chat_id=?1",
            rusqlite::params![seller_chat],
        )?;
        tx.commit()?;
        Ok(Some(previous))
    }

    pub fn active_batch_for_seller(&self, seller_chat: i64) -> Result<Option<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE seller_chat=?1 AND status='processing'
             ORDER BY id LIMIT 1",
            rusqlite::params![seller_chat],
            |r| {
                Ok(PurchaseBatch {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    unit_price: r.get(2)?,
                    quantity: r.get(3)?,
                    total_price: r.get(4)?,
                    created_by: r.get(5)?,
                    seller_chat: r.get(6)?,
                    proxy_source: r.get(7)?,
                    status: r.get(8)?,
                    payment_tx: r.get(9)?,
                    current_item: r.get(10)?,
                })
            },
        )
        .optional()?)
    }

    pub fn accepted_batches_for_seller(&self, seller_chat: i64) -> Result<Vec<PurchaseBatch>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT id,product,unit_price,quantity,total_price,created_by,seller_chat,
                    proxy_source,status,COALESCE(payment_tx,''),COALESCE(current_item,0)
             FROM purchase_batches WHERE seller_chat=?1 AND status='accepted' ORDER BY id",
        )?;
        let rows = s.query_map(rusqlite::params![seller_chat], |r| {
            Ok(PurchaseBatch {
                id: r.get(0)?,
                product: r.get(1)?,
                unit_price: r.get(2)?,
                quantity: r.get(3)?,
                total_price: r.get(4)?,
                created_by: r.get(5)?,
                seller_chat: r.get(6)?,
                proxy_source: r.get(7)?,
                status: r.get(8)?,
                payment_tx: r.get(9)?,
                current_item: r.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// Batches that were paid or were moving to the next item when the process stopped. The
    /// caller decides whether to resend an instruction based on the seller's persisted `want`.
    pub fn batches_needing_resume(&self) -> Result<Vec<(PurchaseBatch, BatchItem)>> {
        let c = self.c.lock().unwrap();
        let mut s = c.prepare(
            "SELECT b.id,b.product,b.unit_price,b.quantity,b.total_price,b.created_by,b.seller_chat,
                    b.proxy_source,b.status,COALESCE(b.payment_tx,''),COALESCE(b.current_item,0),
                    i.id,i.batch_id,i.item_no,i.product,i.price,COALESCE(i.proxy,''),i.status
             FROM purchase_batches b
             JOIN batch_items i ON i.batch_id=b.id
                AND i.item_no=CASE WHEN b.current_item=0 THEN 1 ELSE b.current_item END
             WHERE b.status IN ('paid','processing')
               AND i.status IN ('pending','processing')
             ORDER BY b.id",
        )?;
        let rows = s.query_map([], |r| {
            Ok((
                PurchaseBatch {
                    id: r.get(0)?,
                    product: r.get(1)?,
                    unit_price: r.get(2)?,
                    quantity: r.get(3)?,
                    total_price: r.get(4)?,
                    created_by: r.get(5)?,
                    seller_chat: r.get(6)?,
                    proxy_source: r.get(7)?,
                    status: r.get(8)?,
                    payment_tx: r.get(9)?,
                    current_item: r.get(10)?,
                },
                BatchItem {
                    id: r.get(11)?,
                    batch_id: r.get(12)?,
                    item_no: r.get(13)?,
                    product: r.get(14)?,
                    price: r.get(15)?,
                    proxy: r.get(16)?,
                    status: r.get(17)?,
                },
            ))
        })?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// Compatibility path for callers that have not integrated allocation evidence yet. New rows
    /// remain unresolved and therefore cannot be selected for an exact renewal.
    pub fn upsert_proxy_binding(
        &self,
        provider: &str,
        local_id: &str,
        order_id: i64,
        issued_at: i64,
        authority_status: ProxyAuthorityStatus,
    ) -> Result<ProxyBinding> {
        self.upsert_proxy_binding_inner(
            provider,
            local_id,
            order_id,
            None,
            issued_at,
            authority_status,
        )
    }

    /// Allocation-level upsert for integration. The IP is parsed through `IpAddr` and stored in its
    /// canonical text form. Replaying or exactly backfilling a legacy unresolved row preserves its
    /// opaque inventory ID and original issuance timestamp.
    pub fn upsert_proxy_binding_allocation(
        &self,
        provider: &str,
        local_id: &str,
        order_id: i64,
        allocation_ip: &str,
        issued_at: i64,
        authority_status: ProxyAuthorityStatus,
    ) -> Result<ProxyBinding> {
        let allocation_ip = allocation_ip
            .parse::<IpAddr>()
            .context("proxy allocation IP is invalid")?;
        self.upsert_proxy_binding_inner(
            provider,
            local_id,
            order_id,
            Some(allocation_ip),
            issued_at,
            authority_status,
        )
    }

    fn upsert_proxy_binding_inner(
        &self,
        provider: &str,
        local_id: &str,
        order_id: i64,
        allocation_ip: Option<IpAddr>,
        issued_at: i64,
        authority_status: ProxyAuthorityStatus,
    ) -> Result<ProxyBinding> {
        if provider.is_empty() || provider.len() > 64 || local_id.is_empty() || local_id.len() > 255
        {
            bail!("proxy binding identifiers are empty or exceed storage limits");
        }
        if order_id <= 0 || issued_at <= 0 {
            bail!("proxy binding order and issuance time must be positive");
        }
        let allocation = allocation_ip.map(|ip| ip.to_string());
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                "SELECT inventory_id,provider,local_id,order_id,allocation_ip,issued_at,
                        authority_status,updated_at
                 FROM proxy_bindings WHERE provider=?1 AND local_id=?2",
                rusqlite::params![provider, local_id],
                proxy_binding_from_row,
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing.order_id != order_id
                || (existing.allocation_ip.is_some() && existing.allocation_ip != allocation_ip)
            {
                return Err(ProxyLifecycleConflict::BindingOrderChanged.into());
            }
            tx.execute(
                "UPDATE proxy_bindings
                 SET allocation_ip=COALESCE(allocation_ip,?3),authority_status=?4,updated_at=?5
                 WHERE provider=?1 AND local_id=?2",
                rusqlite::params![
                    provider,
                    local_id,
                    allocation,
                    authority_status.as_str(),
                    now()
                ],
            )?;
            let binding = tx.query_row(
                "SELECT inventory_id,provider,local_id,order_id,allocation_ip,issued_at,
                        authority_status,updated_at
                 FROM proxy_bindings WHERE provider=?1 AND local_id=?2",
                rusqlite::params![provider, local_id],
                proxy_binding_from_row,
            )?;
            tx.commit()?;
            return Ok(binding);
        }
        if allocation.is_some()
            && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM proxy_bindings
                 WHERE order_id=?1 AND allocation_ip=?2)",
                rusqlite::params![order_id, allocation],
                |row| row.get::<_, i64>(0),
            )? == 1
        {
            return Err(ProxyLifecycleConflict::OrderAlreadyBound.into());
        }
        let timestamp = now();
        let mut inserted = false;
        for _ in 0..INVENTORY_ID_ATTEMPTS {
            let inventory_id = new_inventory_id()?;
            match tx.execute(
                "INSERT INTO proxy_bindings(inventory_id,provider,local_id,order_id,allocation_ip,
                                             issued_at,authority_status,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![
                    inventory_id,
                    provider,
                    local_id,
                    order_id,
                    allocation,
                    issued_at,
                    authority_status.as_str(),
                    timestamp
                ],
            ) {
                Ok(_) => {
                    inserted = true;
                    break;
                }
                Err(error)
                    if error.sqlite_error_code()
                        == Some(rusqlite::ErrorCode::ConstraintViolation)
                        && tx.query_row(
                            "SELECT EXISTS(SELECT 1 FROM proxy_bindings WHERE inventory_id=?1)",
                            rusqlite::params![inventory_id],
                            |row| row.get::<_, i64>(0),
                        )? == 1 => {}
                Err(error) => return Err(error.into()),
            }
        }
        if !inserted {
            bail!("could not allocate unique proxy inventory id");
        }
        let binding = tx.query_row(
            "SELECT inventory_id,provider,local_id,order_id,allocation_ip,issued_at,
                    authority_status,updated_at
             FROM proxy_bindings WHERE provider=?1 AND local_id=?2",
            rusqlite::params![provider, local_id],
            proxy_binding_from_row,
        )?;
        tx.commit()?;
        Ok(binding)
    }

    pub fn get_proxy_binding_by_inventory_id(
        &self,
        inventory_id: &str,
    ) -> Result<Option<ProxyBinding>> {
        if !valid_inventory_id(inventory_id) {
            bail!("invalid proxy inventory id");
        }
        Ok(self
            .c
            .lock()
            .unwrap()
            .query_row(
                "SELECT inventory_id,provider,local_id,order_id,allocation_ip,issued_at,
                        authority_status,updated_at
                 FROM proxy_bindings WHERE inventory_id=?1",
                rusqlite::params![inventory_id],
                proxy_binding_from_row,
            )
            .optional()?)
    }

    pub fn list_proxy_bindings(&self) -> Result<Vec<ProxyBinding>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT inventory_id,provider,local_id,order_id,allocation_ip,issued_at,
                    authority_status,updated_at
             FROM proxy_bindings ORDER BY provider,local_id",
        )?;
        let rows = statement
            .query_map([], proxy_binding_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Compatibility adapter for existing callers. It resolves each opaque inventory ID back to the
    /// durable binding and refuses unresolved legacy allocations rather than inventing a snapshot.
    pub fn create_or_get_renewal_request(
        &self,
        idempotency_key: &str,
        selections: &[(String, i64)],
        requested_by: &str,
    ) -> Result<RenewalRequest> {
        let exact = selections
            .iter()
            .map(|(inventory_id, order_id)| {
                let binding = self
                    .get_proxy_binding_by_inventory_id(inventory_id)?
                    .context("renewal selection contains an unknown inventory id")?;
                if binding.order_id != *order_id {
                    bail!("renewal selection order does not match durable binding");
                }
                Ok(RenewalSelection {
                    inventory_id: inventory_id.clone(),
                    order_id: *order_id,
                    allocation_ip: binding
                        .allocation_ip
                        .context("renewal selection allocation is unresolved")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.create_or_get_renewal_request_exact(idempotency_key, &exact, requested_by)
    }

    pub fn create_or_get_renewal_request_exact(
        &self,
        idempotency_key: &str,
        selections: &[RenewalSelection],
        requested_by: &str,
    ) -> Result<RenewalRequest> {
        if idempotency_key.is_empty() || idempotency_key.len() > 255 {
            bail!("renewal idempotency key is empty or exceeds storage limit");
        }
        if !valid_requested_by(requested_by) {
            bail!("renewal request actor is invalid");
        }
        let (
            selections,
            _inventory_ids,
            _order_ids,
            encoded_selections,
            encoded_inventory_ids,
            encoded_order_ids,
        ) = canonical_selections(selections)?;
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,
                        created_at,updated_at
                 FROM proxy_renewal_requests WHERE idempotency_key=?1",
                rusqlite::params![idempotency_key],
                renewal_request_from_row,
            )
            .optional()?;
        if let Some(existing) = existing {
            if request_selections(&tx, existing.id)? != selections {
                return Err(ProxyLifecycleConflict::IdempotencyKeyReused.into());
            }
            tx.commit()?;
            return Ok(existing);
        }
        let timestamp = now();
        tx.execute(
            "INSERT INTO proxy_renewal_requests(idempotency_key,selections,inventory_ids,order_ids,
                                                requested_by,state,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,'pending',?6,?6)",
            rusqlite::params![
                idempotency_key,
                encoded_selections,
                encoded_inventory_ids,
                encoded_order_ids,
                requested_by,
                timestamp
            ],
        )?;
        let request = tx.query_row(
            "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,
                    created_at,updated_at
             FROM proxy_renewal_requests WHERE id=last_insert_rowid()",
            [],
            renewal_request_from_row,
        )?;
        tx.commit()?;
        Ok(request)
    }

    pub fn get_renewal_request_by_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<RenewalRequest>> {
        if idempotency_key.is_empty() || idempotency_key.len() > 255 {
            bail!("renewal idempotency key is empty or exceeds storage limit");
        }
        Ok(self
            .c
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,created_at,updated_at
                 FROM proxy_renewal_requests WHERE idempotency_key=?1",
                rusqlite::params![idempotency_key],
                renewal_request_from_row,
            )
            .optional()?)
    }

    pub fn claim_renewal_request(&self, request_id: i64) -> Result<Option<RenewalRequest>> {
        if request_id <= 0 {
            bail!("renewal request id must be positive");
        }
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE proxy_renewal_requests SET state='in_progress',updated_at=?2
             WHERE id=?1 AND state='pending'",
            rusqlite::params![request_id, now()],
        )?;
        let request = if changed == 1 {
            tx.query_row(
                "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,created_at,updated_at
                 FROM proxy_renewal_requests WHERE id=?1",
                rusqlite::params![request_id],
                renewal_request_from_row,
            )
            .optional()?
        } else {
            None
        };
        tx.commit()?;
        Ok(request)
    }

    pub fn claim_next_renewal_request(&self) -> Result<Option<RenewalRequest>> {
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request_id = tx
            .query_row(
                "SELECT id FROM proxy_renewal_requests
                 WHERE state='pending' ORDER BY created_at,id LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let request = if let Some(request_id) = request_id {
            let changed = tx.execute(
                "UPDATE proxy_renewal_requests SET state='in_progress',updated_at=?2
                 WHERE id=?1 AND state='pending'",
                rusqlite::params![request_id, now()],
            )?;
            if changed != 1 {
                bail!("pending renewal request changed while claiming");
            }
            tx.query_row(
                "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,created_at,updated_at
                 FROM proxy_renewal_requests WHERE id=?1",
                rusqlite::params![request_id],
                renewal_request_from_row,
            )
            .optional()?
        } else {
            None
        };
        tx.commit()?;
        Ok(request)
    }

    /// Compatibility adapter for order-oriented callers. It is accepted only when that order names
    /// exactly one request item; duplicate-order requests must use inventory identity explicitly.
    pub fn record_renewal_event(
        &self,
        request_id: i64,
        order_id: i64,
        outcome: RenewalEventOutcome,
        observed_at: i64,
        new_expiry_at: Option<i64>,
    ) -> Result<RenewalEvent> {
        let c = self.c.lock().unwrap();
        let selections = request_selections(&c, request_id)?;
        drop(c);
        let matches = selections
            .iter()
            .filter(|selection| selection.order_id == order_id)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            bail!("renewal order does not identify exactly one selected allocation");
        }
        self.record_renewal_event_for_inventory(
            request_id,
            &matches[0].inventory_id,
            outcome,
            observed_at,
            new_expiry_at,
        )
    }

    pub fn record_renewal_event_for_inventory(
        &self,
        request_id: i64,
        inventory_id: &str,
        outcome: RenewalEventOutcome,
        observed_at: i64,
        new_expiry_at: Option<i64>,
    ) -> Result<RenewalEvent> {
        if request_id <= 0 || !valid_inventory_id(inventory_id) || observed_at <= 0 {
            bail!("renewal event identity and timestamp are invalid");
        }
        if new_expiry_at.is_some_and(|expiry| expiry <= 0) {
            bail!("renewal event expiry must be positive");
        }
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request = tx
            .query_row(
                "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,
                        created_at,updated_at
                 FROM proxy_renewal_requests WHERE id=?1",
                rusqlite::params![request_id],
                renewal_request_from_row,
            )
            .optional()?
            .context("renewal request not found")?;
        let selection = request_selections(&tx, request_id)?
            .into_iter()
            .find(|selection| selection.inventory_id == inventory_id)
            .context("renewal event inventory does not belong to exact request")?;
        let existing = tx
            .query_row(
                "SELECT id,request_id,inventory_id,order_id,allocation_ip,outcome,observed_at,
                        new_expiry_at
                 FROM proxy_renewal_events WHERE request_id=?1 AND inventory_id=?2",
                rusqlite::params![request_id, inventory_id],
                |row| {
                    let event = renewal_event_from_row(row)?;
                    Ok((
                        event,
                        row.get::<_, Option<String>>(2)?,
                        parse_db_ip(row.get(4)?, 4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((existing, existing_inventory, existing_allocation)) = existing {
            if existing_inventory.as_deref() != Some(inventory_id)
                || existing.order_id != selection.order_id
                || existing_allocation != Some(selection.allocation_ip)
                || existing.outcome != outcome
                || existing.observed_at != observed_at
                || existing.new_expiry_at != new_expiry_at
            {
                return Err(ProxyLifecycleConflict::RenewalEventChanged.into());
            }
            tx.commit()?;
            return Ok(existing);
        }
        if request.state != RenewalRequestState::InProgress {
            bail!("new renewal events require an in-progress request");
        }
        tx.execute(
            "INSERT INTO proxy_renewal_events(request_id,inventory_id,order_id,allocation_ip,outcome,
                                               observed_at,new_expiry_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                request_id,
                inventory_id,
                selection.order_id,
                selection.allocation_ip.to_string(),
                outcome.as_str(),
                observed_at,
                new_expiry_at
            ],
        )?;
        let event = tx.query_row(
            "SELECT id,request_id,inventory_id,order_id,allocation_ip,outcome,observed_at,
                    new_expiry_at
             FROM proxy_renewal_events WHERE id=last_insert_rowid()",
            [],
            renewal_event_from_row,
        )?;
        tx.commit()?;
        Ok(event)
    }

    pub fn complete_renewal_request(&self, request_id: i64) -> Result<RenewalRequest> {
        self.finish_renewal_request(request_id, RenewalRequestState::Completed)
    }

    pub fn fail_renewal_request(&self, request_id: i64) -> Result<RenewalRequest> {
        self.finish_renewal_request(request_id, RenewalRequestState::Failed)
    }

    pub fn indeterminate_renewal_request(&self, request_id: i64) -> Result<RenewalRequest> {
        self.finish_renewal_request(request_id, RenewalRequestState::Indeterminate)
    }

    fn finish_renewal_request(
        &self,
        request_id: i64,
        terminal: RenewalRequestState,
    ) -> Result<RenewalRequest> {
        if request_id <= 0 {
            bail!("renewal request id must be positive");
        }
        if !matches!(
            terminal,
            RenewalRequestState::Completed
                | RenewalRequestState::Failed
                | RenewalRequestState::Indeterminate
        ) {
            bail!("renewal request target state must be terminal");
        }
        let mut c = self.c.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let request = tx
            .query_row(
                "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,created_at,updated_at
                 FROM proxy_renewal_requests WHERE id=?1",
                rusqlite::params![request_id],
                renewal_request_from_row,
            )
            .optional()?
            .context("renewal request not found")?;
        if request.state == terminal {
            tx.commit()?;
            return Ok(request);
        }
        if request.state != RenewalRequestState::InProgress {
            bail!("renewal request is not in progress");
        }
        if terminal == RenewalRequestState::Completed {
            let event_ids = {
                let mut statement = tx.prepare(
                    "SELECT inventory_id FROM proxy_renewal_events
                     WHERE request_id=?1 AND inventory_id IS NOT NULL ORDER BY inventory_id",
                )?;
                let rows = statement
                    .query_map(rusqlite::params![request_id], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            let selected_ids = request_selections(&tx, request_id)?
                .into_iter()
                .map(|selection| selection.inventory_id)
                .collect::<Vec<_>>();
            if event_ids != selected_ids {
                bail!("completed renewal request requires one event per selected inventory");
            }
        }
        tx.execute(
            "UPDATE proxy_renewal_requests SET state=?2,updated_at=?3
             WHERE id=?1 AND state='in_progress'",
            rusqlite::params![request_id, terminal.as_str(), now()],
        )?;
        let request = tx.query_row(
            "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,created_at,updated_at
             FROM proxy_renewal_requests WHERE id=?1",
            rusqlite::params![request_id],
            renewal_request_from_row,
        )?;
        tx.commit()?;
        Ok(request)
    }

    pub fn get_renewal_request(&self, request_id: i64) -> Result<Option<RenewalRequest>> {
        Ok(self
            .c
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,created_at,updated_at
                 FROM proxy_renewal_requests WHERE id=?1",
                rusqlite::params![request_id],
                renewal_request_from_row,
            )
            .optional()?)
    }

    /// Exact durable targets for processing/replay integration. Unresolved legacy requests return an
    /// error and therefore cannot be renewed through an order-only guess.
    pub fn get_renewal_selections(&self, request_id: i64) -> Result<Vec<RenewalSelection>> {
        if request_id <= 0 {
            bail!("renewal request id must be positive");
        }
        request_selections(&self.c.lock().unwrap(), request_id)
    }

    pub fn list_renewal_requests(&self) -> Result<Vec<RenewalRequest>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,state,created_at,updated_at
             FROM proxy_renewal_requests ORDER BY created_at,id",
        )?;
        let rows = statement
            .query_map([], renewal_request_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_renewal_events(&self, request_id: i64) -> Result<Vec<RenewalEvent>> {
        Ok(self
            .get_exact_renewal_events(request_id)?
            .into_iter()
            .map(|event| event.event)
            .collect())
    }

    pub fn get_exact_renewal_events(&self, request_id: i64) -> Result<Vec<ExactRenewalEvent>> {
        if request_id <= 0 {
            bail!("renewal request id must be positive");
        }
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT id,request_id,inventory_id,order_id,allocation_ip,outcome,observed_at,
                    new_expiry_at
             FROM proxy_renewal_events WHERE request_id=?1 ORDER BY inventory_id,id",
        )?;
        let rows = statement
            .query_map(rusqlite::params![request_id], exact_renewal_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_renewal_events(&self) -> Result<Vec<RenewalEvent>> {
        let c = self.c.lock().unwrap();
        let mut statement = c.prepare(
            "SELECT id,request_id,inventory_id,order_id,allocation_ip,outcome,observed_at,
                    new_expiry_at
             FROM proxy_renewal_events ORDER BY request_id,inventory_id,id",
        )?;
        let rows = statement
            .query_map([], renewal_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn counts(&self) -> (i64, i64) {
        let c = self.c.lock().unwrap();
        let u = c
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap_or(0);
        let o = c
            .query_row("SELECT COUNT(*) FROM offers", [], |r| r.get(0))
            .unwrap_or(0);
        (u, o)
    }
}

#[cfg(test)]
mod tests;
