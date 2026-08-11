//! # registry — реестр подписок (пункт 1)
//!
//! Источник истины пула: engine-owned PostgreSQL. SQLite remains a migration/rollback source. Для форвардинг-прокси подписке нужны
//! только OAuth-токен + прокси (+ статус/флот). Токен берётся из колонки `token` (inline)
//! либо из файла `token_file`. Совместим с исторической subscriptions.db (мягкая миграция).
//!
//! **Границы крейта:** только хранение/чтение подписок. НИКАКОЙ HTTP/логики пула.
//! Ниже по стеку зависеть не от кого.

pub mod authority;
mod glm_calibration;
mod kimi_calibration;
pub mod pg;
pub mod pricing;
mod provider_calibration;

pub use glm_calibration::*;
pub use kimi_calibration::*;

/// Column order shared by every KIMI calibration read. One list keeps SELECT and the row mapper
/// from drifting apart, which is the classic source of silently shifted columns.
pub const KIMI_CALIBRATION_COLUMNS: &str = "subject_id,plan,window_duration_secs,window_name,\
resets_at,anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano,\
used_fraction_units,measurement_resolution_fraction_units,observed_at,native_limit_units,\
native_used_units,observed_fraction_units,observed_spend_nano,samples,\
unattributed_fraction_units,current_capacity_nano,current_low_nano,current_high_nano,\
current_confidence_bp,last_measured_at,estimator_version,version,updated_ts";

/// Validate a KIMI calibration row read back from the authority.
///
/// A stored row that violates its own invariants is refused rather than served: publishing a
/// capacity built on an impossible row would be worse than publishing nothing.
pub fn validate_kimi_calibration_row(row: &KimiCalibrationRow) -> Result<()> {
    if row.subject_id.is_empty() || row.plan.is_empty() {
        bail!("KIMI calibration row has no identity");
    }
    if row.window_duration_secs <= 0 {
        bail!("KIMI calibration row has an invalid window duration");
    }
    if row.native_limit_units <= 0 || row.native_used_units > row.native_limit_units {
        bail!("KIMI calibration row has an invalid quota window");
    }
    if !(0..=KIMI_FRACTION_SCALE).contains(&row.used_fraction_units) {
        bail!("KIMI calibration row fraction is out of range");
    }
    if !(1..=KIMI_FRACTION_SCALE).contains(&row.measurement_resolution_fraction_units) {
        bail!("KIMI calibration row resolution is out of range");
    }
    match (
        row.current_low_nano,
        row.current_high_nano,
        row.current_capacity_nano,
    ) {
        (Some(_), _, None) | (_, Some(_), None) => {
            bail!("KIMI calibration row has bounds without a capacity")
        }
        (Some(low), Some(high), _) if low > high => {
            bail!("KIMI calibration row bounds are inverted")
        }
        _ => {}
    }
    Ok(())
}

/// Validate that a state row and the observation about to advance it describe the same window.
pub fn validate_kimi_calibration_pair(
    state: &KimiCalibrationRow,
    observation: &KimiWindowObservation,
) -> Result<()> {
    validate_kimi_calibration_row(state)?;
    if state.subject_id != observation.subject_id
        || state.plan != observation.plan
        || state.window_duration_secs != observation.window_duration_secs
    {
        bail!("KIMI calibration state and observation describe different windows");
    }
    Ok(())
}

/// Column order shared by every GLM calibration read. One list keeps SELECT and the row mapper
/// from drifting apart, which is the classic source of silently shifted columns.
pub const GLM_CALIBRATION_COLUMNS: &str = "subject_id,plan,window_duration_secs,reset_at,\
anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_api_nanousd,\
anchor_spend_native_microcredits,used_fraction_units,measurement_resolution_fraction_units,\
observed_at,native_limit_microcredits,native_used_microcredits,observed_fraction_units,\
observed_spend_api_nanousd,observed_spend_native_microcredits,samples,\
unattributed_fraction_units,current_capacity_nanousd,current_low_nanousd,current_high_nanousd,\
current_confidence_bp,last_measured_at,estimator_version,version,updated_ts";

/// Validate a GLM calibration row read back from the authority.
///
/// A stored row that violates its own invariants is refused rather than served: publishing a
/// capacity built on an impossible row would be worse than publishing nothing. Fraction legs
/// and the native window halves are nullable because the quota endpoint's units are unproven —
/// but a present half must be sane, and the fraction/resolution pair must move together.
pub fn validate_glm_calibration_row(row: &GlmCalibrationRow) -> Result<()> {
    if row.subject_id.is_empty() || row.plan.is_empty() {
        bail!("GLM calibration row has no identity");
    }
    if row.window_duration_secs <= 0 {
        bail!("GLM calibration row has an invalid window duration");
    }
    if row
        .native_limit_microcredits
        .is_some_and(|limit| limit <= 0)
        || row.native_used_microcredits.is_some_and(|used| used < 0)
        || match (row.native_used_microcredits, row.native_limit_microcredits) {
            (Some(used), Some(limit)) => used > limit,
            _ => false,
        }
    {
        bail!("GLM calibration row has an invalid quota window");
    }
    if let Some(fraction) = row.used_fraction_units {
        if !(0..=GLM_FRACTION_SCALE).contains(&fraction) {
            bail!("GLM calibration row fraction is out of range");
        }
    }
    if let Some(resolution) = row.measurement_resolution_fraction_units {
        if !(1..=GLM_FRACTION_SCALE).contains(&resolution) {
            bail!("GLM calibration row resolution is out of range");
        }
    }
    if row.used_fraction_units.is_some() != row.measurement_resolution_fraction_units.is_some() {
        bail!("GLM calibration row fraction and resolution must move together");
    }
    if row.anchor_used_fraction_units.is_some() != row.anchor_resolution_fraction_units.is_some() {
        bail!("GLM calibration row anchor fraction and resolution must move together");
    }
    match (
        row.current_low_nanousd,
        row.current_high_nanousd,
        row.current_capacity_nanousd,
    ) {
        (Some(_), _, None) | (_, Some(_), None) => {
            bail!("GLM calibration row has bounds without a capacity")
        }
        (Some(low), Some(high), _) if low > high => {
            bail!("GLM calibration row bounds are inverted")
        }
        _ => {}
    }
    Ok(())
}

/// Validate that a state row and the observation about to advance it describe the same window.
pub fn validate_glm_calibration_pair(
    state: &GlmCalibrationRow,
    observation: &GlmWindowObservation,
) -> Result<()> {
    validate_glm_calibration_row(state)?;
    if state.subject_id != observation.subject_id
        || state.plan != observation.plan
        || state.window_duration_secs != observation.window_duration_secs
    {
        bail!("GLM calibration state and observation describe different windows");
    }
    Ok(())
}
pub use provider_calibration::*;

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static EXECUTION_GROUP_DOUBLE_WINNER_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Validated identity of one provider-plane execution. Direct requests deliberately keep a
/// nullable group so old and new writers share the same durable representation: the reservation
/// request ID is the effective group when `group_id` is absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionAttempt {
    group_id: Option<String>,
    attempt: i32,
}

impl ExecutionAttempt {
    pub fn direct() -> Self {
        Self {
            group_id: None,
            attempt: 1,
        }
    }

    pub fn grouped(group_id: impl Into<String>, attempt: i32) -> Result<Self> {
        let group_id = group_id.into();
        if !is_canonical_uuid_v4(&group_id) {
            bail!("execution group must be a canonical lowercase UUIDv4");
        }
        if attempt <= 0 {
            bail!("execution attempt must be positive");
        }
        Ok(Self {
            group_id: Some(group_id),
            attempt,
        })
    }

    pub fn group_id(&self) -> Option<&str> {
        self.group_id.as_deref()
    }

    pub fn attempt(&self) -> i32 {
        self.attempt
    }

    pub fn effective_group_id<'a>(&'a self, request_id: &'a str) -> &'a str {
        self.group_id.as_deref().unwrap_or(request_id)
    }
}

impl Default for ExecutionAttempt {
    fn default() -> Self {
        Self::direct()
    }
}

fn is_canonical_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || bytes[8] != b'-'
        || bytes[13] != b'-'
        || bytes[18] != b'-'
        || bytes[23] != b'-'
        || bytes[14] != b'4'
        || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
    {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 8 | 13 | 18 | 23) || matches!(byte, b'0'..=b'9' | b'a'..=b'f')
    })
}

/// Process-global monotonic counter exported by every engine slot. The winner-table primary key is
/// the correctness mechanism; this counter is only an always-zero operational tripwire.
pub fn execution_group_double_winner_total() -> u64 {
    EXECUTION_GROUP_DOUBLE_WINNER_TOTAL.load(Ordering::Relaxed)
}

pub(crate) fn record_execution_group_loser(
    group_id: &str,
    winner_request_id: &str,
    loser_request_id: &str,
    attempt: i32,
) {
    fn bounded(value: &str) -> String {
        value
            .chars()
            .take(128)
            .map(|character| match character {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | ':' => character,
                _ => '?',
            })
            .collect()
    }

    EXECUTION_GROUP_DOUBLE_WINNER_TOTAL.fetch_add(1, Ordering::Relaxed);
    elog::error("registry", format!("event=execution_group_double_winner group_id={} winner_request_id={} loser_request_id={} attempt={}", bounded(group_id),
        bounded(winner_request_id),
        bounded(loser_request_id),
        attempt,));
}

/// Рантайм-запись подписки с УЖЕ разрешённым токеном (inline или из файла).
#[derive(Clone, Debug)]
pub struct Sub {
    pub email: String,
    pub token: String, // OAuth Bearer подписки (секрет)
    pub proxy: String, // http://user:pass@ip:port ("" = без прокси)
    pub fleet: String,
    pub plan: String, // pro|max5|max20 (детект) — для per-sub прайора ёмкости в pool ("" = неизвестно)
}

const COLS: &[(&str, &str)] = &[
    ("email", "TEXT PRIMARY KEY"),
    ("token", "TEXT"),
    ("token_file", "TEXT"),
    ("proxy", "TEXT"),
    ("plan", "TEXT"),
    ("status", "TEXT"),
    ("fleet", "TEXT"),
    ("added_ts", "INTEGER"),
    ("added", "TEXT"),
    // Метаданные прокси (заполняет authbot — владелец жизненного цикла; движок лишь читает/показывает):
    ("proxy_expire", "TEXT"), // дата истечения прокси из IPRoyal (ISO), "" = неизвестно
    ("proxy_checked_ts", "INTEGER"), // ts последней health-проверки прокси (fingerprint-free)
    ("proxy_ok", "INTEGER"),  // 1=жив / 0=мёртв на последней проверке (NULL=не проверялся)
    // Durable auth-health (движок пишет из коррелированных probe; переживает рестарт). Зеркало
    // engine PostgreSQL migration 0003. Токен-fingerprint даёт авто-ревайв при замене токена.
    ("auth_state", "TEXT"), // 'healthy' | 'suspect' | 'dead'
    ("auth_fail_streak", "INTEGER"),
    ("first_auth_fail_ts", "INTEGER"),
    ("last_auth_fail_ts", "INTEGER"),
    ("last_auth_http", "INTEGER"),
    ("dead_since_ts", "INTEGER"),
    ("dead_reason", "TEXT"),
    ("auth_token_fp", "TEXT"),
];


const SQLITE_ATTRIBUTION_COLUMNS: &[(&str, &str)] = &[
    ("attribution_schema_version", "INTEGER"),
    ("snapshot_kind", "TEXT"),
    ("product_id", "TEXT"),
    ("account_class", "TEXT"),
    ("requested_model_id", "TEXT"),
    ("canonical_model_id", "TEXT"),
    ("served_model_id", "TEXT"),
    ("served_canonical_model_id", "TEXT"),
    ("billing_invariant_code", "TEXT"),
    ("alias_generation", "INTEGER"),
    ("rule_id", "TEXT"),
    ("rule_digest", "TEXT"),
    ("rule_scope", "TEXT"),
    ("pricing_mode", "TEXT"),
    ("rule_origin", "TEXT"),
    ("discount_bps", "INTEGER"),
    ("payable_multiplier_bp", "INTEGER"),
    ("policy_id", "TEXT"),
    ("policy_version", "INTEGER"),
    ("effective_policy_version", "INTEGER"),
    ("policy_digest", "TEXT"),
    ("catalog_generation", "INTEGER"),
    ("switch_generation", "INTEGER"),
    ("tariff_schedule_id", "TEXT"),
    ("tariff_priced_ts", "INTEGER"),
    ("official_cost_json", "TEXT"),
    ("paid_funded_nano", "INTEGER"),
    ("bonus_funded_nano", "INTEGER"),
    ("other_funded_nano", "INTEGER"),
    ("funding_allocation_json", "TEXT"),
    ("track_eligible", "INTEGER"),
    ("retention_eligible", "INTEGER"),
    ("commission_eligible", "INTEGER"),
    ("snapshot_digest", "TEXT"),
    ("source_policy_digest", "TEXT"),
    ("admission_catalog_generation", "INTEGER"),
    ("admission_catalog_digest", "TEXT"),
    ("admission_switch_generation", "INTEGER"),
    ("admission_switch_digest", "TEXT"),
    ("runtime_manifest_generation", "INTEGER"),
    ("runtime_manifest_digest", "TEXT"),
];

pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            let _ = fs::create_dir_all(dir);
        }
    }
    let c = Connection::open(path).with_context(|| format!("открыть БД {path}"))?;
    // AUDIT(C38): this database is authoritative for balances and ledger entries. In WAL mode,
    // synchronous=FULL makes an acknowledged commit durable across OS crashes and power loss.
    // Performance-sensitive nonfinancial state should move to a separate database if needed.
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; \
                     PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;",
    )?;
    c.execute(
        "CREATE TABLE IF NOT EXISTS subs(email TEXT PRIMARY KEY, token TEXT, token_file TEXT, \
         proxy TEXT, plan TEXT DEFAULT '', status TEXT DEFAULT 'active', fleet TEXT DEFAULT 'prod', \
         added_ts INTEGER, added TEXT)",
        [],
    )?;
    // мягкая миграция: доливаем недостающие колонки в существующую (историческую) таблицу
    for (name, ty) in COLS {
        let _ = c.execute(&format!("ALTER TABLE subs ADD COLUMN {name} {ty}"), []);
    }
    // Биллинг: ключи клиентов с балансом в нанодолларах (1 USD = 1e9 нано). i64 INTEGER
    // вмещает до ~$9.2 млрд — с запасом. balance может уйти в минус (тогда ключ блокируется).
    c.execute(
        "CREATE TABLE IF NOT EXISTS api_keys(key TEXT PRIMARY KEY, balance_nano INTEGER NOT NULL DEFAULT 0, \
         spent_nano INTEGER NOT NULL DEFAULT 0, mult_bp INTEGER NOT NULL DEFAULT 900, \
         status TEXT NOT NULL DEFAULT 'active', created_ts INTEGER, created TEXT, \
         reserved_nano INTEGER NOT NULL DEFAULT 0)",
        [],
    )?;
    // Мягкая миграция: колонка учёта незакрытых резервов (леджер крах-безопасности).
    let _ = c.execute(
        "ALTER TABLE api_keys ADD COLUMN reserved_nano INTEGER NOT NULL DEFAULT 0",
        [],
    );

    // АККАУНТЫ клиентов: ЕДИНЫЙ баланс на профиль; ключи (api_keys) — доступы к нему (1:N).
    // Баланс/резерв/наценка живут ЗДЕСЬ, не на ключе. Ключ теперь несёт account_id + label +
    // per-key spent (атрибуция расхода по ключу без разделения баланса).
    c.execute(
        "CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY, handle TEXT, \
         balance_nano INTEGER NOT NULL DEFAULT 0, spent_nano INTEGER NOT NULL DEFAULT 0, \
         reserved_nano INTEGER NOT NULL DEFAULT 0, mult_bp INTEGER NOT NULL DEFAULT 2000, \
         status TEXT NOT NULL DEFAULT 'active', created_ts INTEGER, created TEXT)",
        [],
    )?;
    // Per-provider discount override. Absent row = the account default `mult_bp`. Mirrors the
    // PostgreSQL authority (migration 0043) so the SQLite lane keeps API parity.
    c.execute(
        "CREATE TABLE IF NOT EXISTS account_provider_discounts(\
         account_id TEXT NOT NULL, \
         provider_id TEXT NOT NULL CHECK(provider_id IN ('anthropic','openai','google','kimi','glm')), \
         mult_bp INTEGER NOT NULL CHECK(mult_bp BETWEEN 0 AND 10000), \
         updated_ts INTEGER NOT NULL, PRIMARY KEY(account_id, provider_id))",
        [],
    )?;
    // handle (внешняя идентичность: TG id / email) уникален, когда задан.
    let _ = c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS accounts_handle ON accounts(handle) WHERE handle IS NOT NULL", []);
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN account_id TEXT", []);
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN label TEXT", []);
    // Stable public identifier for control-plane key management. The usable `key` remains secret;
    // dashboards and the commercial backend can revoke by `key_id` without persisting that secret.
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN key_id TEXT", []);
    let _ = c.execute(
        "ALTER TABLE api_keys ADD COLUMN spend_limit_nano INTEGER",
        [],
    );
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN expires_ts INTEGER", []);
    let _ = c.execute(
        "UPDATE api_keys SET key_id = 'key_' || lower(hex(randomblob(16))) WHERE key_id IS NULL",
        [],
    );
    let _ = c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS api_keys_key_id ON api_keys(key_id) WHERE key_id IS NOT NULL", []);
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS api_keys_account ON api_keys(account_id)",
        [],
    );

    // ЛЕДЖЕР: append-only история движений баланса (пополнения/списания/возвраты) — для точного
    // учёта, споров и дашбордов. Текущий баланс = accounts.balance_nano; ledger — журнал КАК он менялся.
    c.execute(
        "CREATE TABLE IF NOT EXISTS ledger(id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, \
         key TEXT, kind TEXT NOT NULL, amount_nano INTEGER NOT NULL, ref TEXT, \
         balance_after_nano INTEGER, ts INTEGER, model TEXT)",
        [],
    )?;
    // Атрибуция charge-строк к Claude-модели (для точного per-model дневного графика). Модель известна
    // в момент settle (тот же запрос, что и usage_event). topup/adjust модели не имеют → NULL. Идемпотентно.
    let _ = c.execute("ALTER TABLE ledger ADD COLUMN model TEXT", []);
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS ledger_acct ON ledger(account_id, id)",
        [],
    );
    // AUDIT(C2): correctness-critical idempotency indexes must fail closed. A legacy database with
    // duplicate references must not open for billing traffic without explicit operator repair.
    c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_topup_ref ON ledger(ref) \
         WHERE kind='topup' AND ref IS NOT NULL",
        [],
    )
    .context("create required unique top-up reference index")?;
    // AUDIT(C40): negative adjustments are retryable monetary mutations too, so their supplied
    // references share the same global idempotency namespace as top-ups.
    c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_money_ref ON ledger(ref) \
         WHERE kind IN ('topup','adjust') AND ref IS NOT NULL",
        [],
    )
    .context("create required unique monetary reference index")?;
    c.execute(
        "CREATE TABLE IF NOT EXISTS ledger_consumer_checkpoints(consumer TEXT NOT NULL, \
         account_id TEXT NOT NULL, last_ledger_id INTEGER NOT NULL, updated_ts INTEGER NOT NULL, \
         PRIMARY KEY(consumer,account_id))",
        [],
    )?;
    // AUDIT-TODO(C2): move schema upgrades into versioned transactions with explicit duplicate repair.
    // Retained for the future checkpoint-aware pruning path; current timestamp-only prune is disabled.
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS ledger_charge_ts ON ledger(ts) WHERE kind='charge'",
        [],
    );

    // Миграция старой модели (key=кошелёк): ключам без account_id заводим аккаунт и переносим баланс.
    migrate_legacy_keys(&c)?;
    // Персист волатильного состояния пула (переживание рестарта): cooling (бан на дни не должен
    // забываться при деплое) + калибровка ёмкости (дорого переучивать) + spent/util/reset.
    c.execute(
        "CREATE TABLE IF NOT EXISTS pool_state(email TEXT PRIMARY KEY, cooling_until INTEGER, \
         cap5h REAL, cap7d REAL, spent_total REAL, util5 REAL, util7 REAL, \
         reset5 INTEGER, reset7 INTEGER, calib_n INTEGER, updated_ts INTEGER)",
        [],
    )?;
    // Provider turn evidence is shared by Claude and Gemini. It preserves exact disjoint token and
    // API nanoUSD legs while the subject ledger supplies the cumulative spend paired with quota
    // snapshots. The Claude window authority is plan-scoped and has no nominal/prior/EMA state.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_turn_calibration_events( \
           provider TEXT NOT NULL CHECK(provider IN ('anthropic','google')), \
           request_id TEXT NOT NULL CHECK(request_id <> ''), \
           subject_id TEXT NOT NULL CHECK(subject_id <> ''), \
           model_id TEXT NOT NULL CHECK(model_id <> ''), \
           service_tier TEXT NOT NULL CHECK(service_tier IN ('standard','fast')), \
           inference_geo TEXT NOT NULL CHECK(inference_geo IN ('global','us')), \
           tariff_schedule_id TEXT NOT NULL CHECK(tariff_schedule_id <> ''), \
           priced_ts INTEGER NOT NULL CHECK(priced_ts > 0), \
           completed_at INTEGER NOT NULL CHECK(completed_at > 0), \
           input_tokens INTEGER NOT NULL CHECK(input_tokens >= 0), \
           audio_input_tokens INTEGER NOT NULL CHECK(audio_input_tokens >= 0), \
           cache_read_tokens INTEGER NOT NULL CHECK(cache_read_tokens >= 0), \
           cached_audio_input_tokens INTEGER NOT NULL CHECK(cached_audio_input_tokens >= 0), \
           cache_write_5m_tokens INTEGER NOT NULL CHECK(cache_write_5m_tokens >= 0), \
           cache_write_1h_tokens INTEGER NOT NULL CHECK(cache_write_1h_tokens >= 0), \
           output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0), \
           thinking_output_tokens INTEGER NOT NULL CHECK(thinking_output_tokens >= 0), \
           image_output_tokens INTEGER NOT NULL CHECK(image_output_tokens >= 0), \
           tool_prompt_tokens INTEGER NOT NULL CHECK(tool_prompt_tokens >= 0), \
           search_queries INTEGER NOT NULL CHECK(search_queries >= 0), \
           grounded_search_prompts INTEGER NOT NULL CHECK(grounded_search_prompts >= 0), \
           api_input_nanousd INTEGER NOT NULL CHECK(api_input_nanousd >= 0), \
           api_audio_input_nanousd INTEGER NOT NULL CHECK(api_audio_input_nanousd >= 0), \
           api_cache_read_nanousd INTEGER NOT NULL CHECK(api_cache_read_nanousd >= 0), \
           api_cached_audio_input_nanousd INTEGER NOT NULL \
             CHECK(api_cached_audio_input_nanousd >= 0), \
           api_cache_write_5m_nanousd INTEGER NOT NULL CHECK(api_cache_write_5m_nanousd >= 0), \
           api_cache_write_1h_nanousd INTEGER NOT NULL CHECK(api_cache_write_1h_nanousd >= 0), \
           api_output_nanousd INTEGER NOT NULL CHECK(api_output_nanousd >= 0), \
           api_image_output_nanousd INTEGER NOT NULL CHECK(api_image_output_nanousd >= 0), \
           api_search_nanousd INTEGER NOT NULL CHECK(api_search_nanousd >= 0), \
           api_total_nanousd INTEGER NOT NULL CHECK(api_total_nanousd > 0), \
           PRIMARY KEY(provider,request_id), \
           CHECK(cached_audio_input_tokens <= cache_read_tokens), \
           CHECK(thinking_output_tokens <= output_tokens), \
           CHECK(tool_prompt_tokens <= input_tokens), \
           CHECK(input_tokens > 0 OR audio_input_tokens > 0 OR cache_read_tokens > 0 \
             OR cache_write_5m_tokens > 0 OR cache_write_1h_tokens > 0 OR output_tokens > 0 \
             OR image_output_tokens > 0 OR search_queries > 0 \
             OR grounded_search_prompts > 0), \
           CHECK(api_total_nanousd = api_input_nanousd + api_audio_input_nanousd \
             + api_cache_read_nanousd + api_cached_audio_input_nanousd \
             + api_cache_write_5m_nanousd + api_cache_write_1h_nanousd \
             + api_output_nanousd + api_image_output_nanousd + api_search_nanousd)); \
         CREATE INDEX IF NOT EXISTS provider_turn_calibration_subject_time \
           ON provider_turn_calibration_events(provider,subject_id,completed_at DESC); \
         CREATE INDEX IF NOT EXISTS provider_turn_calibration_model_time \
           ON provider_turn_calibration_events(provider,model_id,completed_at DESC); \
         CREATE INDEX IF NOT EXISTS provider_turn_calibration_time \
           ON provider_turn_calibration_events(provider,completed_at DESC); \
         CREATE TABLE IF NOT EXISTS provider_calibration_subject_spend( \
           provider TEXT NOT NULL CHECK(provider IN ('anthropic','google')), \
           subject_id TEXT NOT NULL CHECK(subject_id <> ''), \
           spent_nano INTEGER NOT NULL DEFAULT 0 CHECK(spent_nano >= 0), \
           tracking_started_ts INTEGER NOT NULL CHECK(tracking_started_ts > 0), \
           updated_ts INTEGER NOT NULL CHECK(updated_ts > 0), \
           PRIMARY KEY(provider,subject_id)); \
         CREATE TABLE IF NOT EXISTS anthropic_window_calibrations( \
           subject_id TEXT NOT NULL CHECK(subject_id <> ''), \
           plan TEXT NOT NULL CHECK(plan <> ''), \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','7d')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           anchor_used_fraction_units INTEGER NOT NULL \
             CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000), \
           anchor_resolution_fraction_units INTEGER NOT NULL \
             CHECK(anchor_resolution_fraction_units BETWEEN 1 AND 100000000), \
           anchor_spend_nano INTEGER NOT NULL CHECK(anchor_spend_nano >= 0), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           measurement_resolution_fraction_units INTEGER NOT NULL \
             CHECK(measurement_resolution_fraction_units BETWEEN 1 AND 100000000), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           observed_fraction_units INTEGER NOT NULL DEFAULT 0 \
             CHECK(observed_fraction_units >= 0), \
           observed_spend_nano INTEGER NOT NULL DEFAULT 0 CHECK(observed_spend_nano >= 0), \
           samples INTEGER NOT NULL DEFAULT 0 CHECK(samples >= 0), \
           unattributed_fraction_units INTEGER NOT NULL DEFAULT 0 \
             CHECK(unattributed_fraction_units >= 0), \
           current_capacity_nano INTEGER \
             CHECK(current_capacity_nano IS NULL OR current_capacity_nano >= 0), \
           current_low_nano INTEGER CHECK(current_low_nano IS NULL OR current_low_nano >= 0), \
           current_high_nano INTEGER CHECK(current_high_nano IS NULL OR current_high_nano >= 0), \
           current_confidence_bp INTEGER NOT NULL DEFAULT 0 \
             CHECK(current_confidence_bp BETWEEN 0 AND 10000), \
           last_measured_at INTEGER CHECK(last_measured_at IS NULL OR last_measured_at > 0), \
           estimator_version INTEGER NOT NULL DEFAULT 1 CHECK(estimator_version > 0), \
           version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0), \
           updated_ts INTEGER NOT NULL CHECK(updated_ts > 0), \
           PRIMARY KEY(subject_id,plan,window_kind), \
           CHECK((window_kind='5h' AND window_duration_mins=300) \
             OR (window_kind='7d' AND window_duration_mins=10080)), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano >= current_low_nano), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano <= current_high_nano), \
           CHECK(current_low_nano IS NULL OR current_high_nano IS NULL \
             OR current_low_nano <= current_high_nano), \
           CHECK((samples=0 AND observed_fraction_units=0 AND observed_spend_nano=0 \
               AND current_capacity_nano IS NULL AND current_low_nano IS NULL \
               AND current_high_nano IS NULL AND current_confidence_bp=0 \
               AND last_measured_at IS NULL) \
             OR (samples>0 AND observed_fraction_units>0 AND observed_spend_nano>0 \
               AND current_capacity_nano IS NOT NULL AND current_low_nano IS NOT NULL \
               AND last_measured_at IS NOT NULL))); \
         CREATE INDEX IF NOT EXISTS anthropic_window_calibrations_cohort \
           ON anthropic_window_calibrations(plan,window_kind,window_duration_mins); \
         CREATE TABLE IF NOT EXISTS anthropic_window_observations( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           subject_id TEXT NOT NULL CHECK(subject_id <> ''), \
           plan TEXT NOT NULL CHECK(plan <> ''), \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','7d')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           measurement_resolution_fraction_units INTEGER NOT NULL \
             CHECK(measurement_resolution_fraction_units BETWEEN 1 AND 100000000), \
           gateway_spend_nano INTEGER NOT NULL CHECK(gateway_spend_nano >= 0), \
           observation_source TEXT NOT NULL CHECK(observation_source IN ('response','poll')), \
           source_request_id TEXT, \
           CHECK((window_kind='5h' AND window_duration_mins=300) \
             OR (window_kind='7d' AND window_duration_mins=10080)), \
           CHECK((observation_source='response' AND source_request_id IS NOT NULL \
               AND source_request_id <> '') \
             OR (observation_source='poll' AND source_request_id IS NULL)), \
           UNIQUE(subject_id,plan,window_kind,source_request_id), \
           UNIQUE(subject_id,plan,window_kind,resets_at,observed_at,used_fraction_units, \
             measurement_resolution_fraction_units,gateway_spend_nano,observation_source)); \
         CREATE INDEX IF NOT EXISTS anthropic_window_observations_window \
           ON anthropic_window_observations( \
             subject_id,plan,window_kind,resets_at,observed_at);",
    )?;
    // OpenAI/Codex calibration is based exclusively on durable, real gateway spend paired with
    // provider-reported window duration/reset snapshots. These tables intentionally contain no
    // configured capacity prior or fixed 5-hour/7-day slots.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS codex_home_spend( \
           home_id TEXT PRIMARY KEY, \
           spent_nano INTEGER NOT NULL DEFAULT 0 CHECK(spent_nano >= 0), \
           spent_nanocredits INTEGER CHECK(spent_nanocredits IS NULL OR spent_nanocredits >= 0), \
           credit_tracking_started_ts INTEGER \
             CHECK(credit_tracking_started_ts IS NULL OR credit_tracking_started_ts > 0), \
           updated_ts INTEGER NOT NULL); \
         CREATE TABLE IF NOT EXISTS codex_home_health( \
           home_id TEXT PRIMARY KEY, \
           account_state TEXT NOT NULL DEFAULT 'healthy' \
             CHECK(account_state IN ('healthy','suspect','dead')), \
           auth_fail_streak INTEGER NOT NULL DEFAULT 0 CHECK(auth_fail_streak >= 0), \
           first_auth_fail_ts INTEGER NOT NULL DEFAULT 0 CHECK(first_auth_fail_ts >= 0), \
           cooling_until INTEGER NOT NULL DEFAULT 0 CHECK(cooling_until >= 0), \
           updated_ts INTEGER NOT NULL); \
         CREATE TABLE IF NOT EXISTS codex_window_calibrations( \
           home_id TEXT NOT NULL, \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           anchor_used_percent INTEGER NOT NULL CHECK(anchor_used_percent BETWEEN 0 AND 100), \
           anchor_spend_nano INTEGER NOT NULL CHECK(anchor_spend_nano >= 0), \
           used_percent INTEGER NOT NULL CHECK(used_percent BETWEEN 0 AND 100), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           sum_used_sq INTEGER NOT NULL DEFAULT 0 CHECK(sum_used_sq >= 0), \
           sum_used_spend_nano INTEGER NOT NULL DEFAULT 0 CHECK(sum_used_spend_nano >= 0), \
           observed_points INTEGER NOT NULL DEFAULT 0 CHECK(observed_points >= 0), \
           samples INTEGER NOT NULL DEFAULT 0 CHECK(samples >= 0), \
           current_capacity_nano INTEGER CHECK(current_capacity_nano IS NULL OR current_capacity_nano >= 0), \
           current_low_nano INTEGER CHECK(current_low_nano IS NULL OR current_low_nano >= 0), \
           current_high_nano INTEGER CHECK(current_high_nano IS NULL OR current_high_nano >= 0), \
           current_confidence_bp INTEGER NOT NULL DEFAULT 0 CHECK(current_confidence_bp BETWEEN 0 AND 10000), \
           last_capacity_nano INTEGER CHECK(last_capacity_nano IS NULL OR last_capacity_nano >= 0), \
           last_low_nano INTEGER CHECK(last_low_nano IS NULL OR last_low_nano >= 0), \
           last_high_nano INTEGER CHECK(last_high_nano IS NULL OR last_high_nano >= 0), \
           last_confidence_bp INTEGER NOT NULL DEFAULT 0 CHECK(last_confidence_bp BETWEEN 0 AND 10000), \
           last_measured_at INTEGER CHECK(last_measured_at IS NULL OR last_measured_at > 0), \
           anchor_ready INTEGER NOT NULL DEFAULT 0 CHECK(anchor_ready IN (0,1)), \
           anchor_used_fraction_units INTEGER CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000), \
           used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           observed_fraction_units INTEGER CHECK(observed_fraction_units >= 0), \
           observed_spend_nano INTEGER CHECK(observed_spend_nano >= 0), \
           anchor_spend_nanocredits INTEGER \
             CHECK(anchor_spend_nanocredits IS NULL OR anchor_spend_nanocredits >= 0), \
           observed_spend_nanocredits INTEGER \
             CHECK(observed_spend_nanocredits IS NULL OR observed_spend_nanocredits >= 0), \
           current_capacity_nanocredits INTEGER \
             CHECK(current_capacity_nanocredits IS NULL OR current_capacity_nanocredits >= 0), \
           current_low_nanocredits INTEGER \
             CHECK(current_low_nanocredits IS NULL OR current_low_nanocredits >= 0), \
           current_high_nanocredits INTEGER \
             CHECK(current_high_nanocredits IS NULL OR current_high_nanocredits >= 0), \
           last_capacity_nanocredits INTEGER \
             CHECK(last_capacity_nanocredits IS NULL OR last_capacity_nanocredits >= 0), \
           last_low_nanocredits INTEGER \
             CHECK(last_low_nanocredits IS NULL OR last_low_nanocredits >= 0), \
           last_high_nanocredits INTEGER \
             CHECK(last_high_nanocredits IS NULL OR last_high_nanocredits >= 0), \
           credit_samples INTEGER CHECK(credit_samples IS NULL OR credit_samples >= 0), \
           credit_estimator_version INTEGER \
             CHECK(credit_estimator_version IS NULL OR credit_estimator_version > 0), \
           unattributed_fraction_units INTEGER \
             CHECK(unattributed_fraction_units IS NULL OR unattributed_fraction_units >= 0), \
           estimator_version INTEGER NOT NULL DEFAULT 1 CHECK(estimator_version > 0), \
           version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0), \
           updated_ts INTEGER NOT NULL, \
           PRIMARY KEY(home_id,window_duration_mins), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(last_low_nano IS NULL OR last_capacity_nano IS NOT NULL), \
           CHECK(last_high_nano IS NULL OR last_capacity_nano IS NOT NULL), \
           CHECK((current_low_nanocredits IS NULL AND current_high_nanocredits IS NULL) \
             OR current_capacity_nanocredits IS NOT NULL), \
           CHECK((last_low_nanocredits IS NULL AND last_high_nanocredits IS NULL) \
             OR last_capacity_nanocredits IS NOT NULL)); \
         CREATE TABLE IF NOT EXISTS codex_window_observations( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           home_id TEXT NOT NULL, \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           used_percent INTEGER NOT NULL CHECK(used_percent BETWEEN 0 AND 100), \
           used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           gateway_spend_nano INTEGER NOT NULL CHECK(gateway_spend_nano >= 0), \
           gateway_spend_nanocredits INTEGER \
             CHECK(gateway_spend_nanocredits IS NULL OR gateway_spend_nanocredits >= 0), \
           UNIQUE(home_id,window_duration_mins,resets_at,observed_at,used_percent,gateway_spend_nano)); \
         CREATE INDEX IF NOT EXISTS codex_window_observations_window \
           ON codex_window_observations(home_id,window_duration_mins,resets_at,observed_at); \
         CREATE TABLE IF NOT EXISTS codex_turn_calibration_events( \
           request_id TEXT PRIMARY KEY, \
           home_id TEXT NOT NULL CHECK(home_id <> ''), \
           model_id TEXT NOT NULL CHECK(model_id <> ''), \
           service_tier TEXT NOT NULL CHECK(service_tier IN ('standard','fast')), \
           provider_reported_tier TEXT, \
           api_tariff_schedule_id TEXT NOT NULL CHECK(api_tariff_schedule_id <> ''), \
           credit_schedule_id TEXT NOT NULL CHECK(credit_schedule_id <> ''), \
           completed_at INTEGER NOT NULL CHECK(completed_at > 0), \
           input_tokens INTEGER NOT NULL CHECK(input_tokens >= 0), \
           cached_input_tokens INTEGER NOT NULL CHECK(cached_input_tokens >= 0), \
           cache_write_input_tokens INTEGER NOT NULL CHECK(cache_write_input_tokens >= 0), \
           output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0), \
           reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0), \
           api_input_nanousd INTEGER NOT NULL CHECK(api_input_nanousd >= 0), \
           api_cached_input_nanousd INTEGER NOT NULL CHECK(api_cached_input_nanousd >= 0), \
           api_cache_write_nanousd INTEGER NOT NULL CHECK(api_cache_write_nanousd >= 0), \
           api_output_nanousd INTEGER NOT NULL CHECK(api_output_nanousd >= 0), \
           api_total_nanousd INTEGER NOT NULL CHECK(api_total_nanousd >= 0), \
           chatgpt_input_nanocredits INTEGER NOT NULL CHECK(chatgpt_input_nanocredits >= 0), \
           chatgpt_cached_input_nanocredits INTEGER NOT NULL \
             CHECK(chatgpt_cached_input_nanocredits >= 0), \
           chatgpt_output_nanocredits INTEGER NOT NULL CHECK(chatgpt_output_nanocredits >= 0), \
           chatgpt_total_nanocredits INTEGER NOT NULL CHECK(chatgpt_total_nanocredits >= 0), \
           CHECK(cached_input_tokens + cache_write_input_tokens <= input_tokens), \
           CHECK(reasoning_output_tokens <= output_tokens), \
           CHECK(input_tokens > 0 OR output_tokens > 0), \
           CHECK(api_total_nanousd = api_input_nanousd + api_cached_input_nanousd \
             + api_cache_write_nanousd + api_output_nanousd), \
           CHECK(chatgpt_total_nanocredits = chatgpt_input_nanocredits \
             + chatgpt_cached_input_nanocredits + chatgpt_output_nanocredits)); \
         CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_home_time \
           ON codex_turn_calibration_events(home_id,completed_at DESC); \
         CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_model_time \
           ON codex_turn_calibration_events(model_id,completed_at DESC); \
         CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_time \
           ON codex_turn_calibration_events(completed_at DESC);",
    )?;
    // Expand-only compatibility for SQLite databases created before estimator v3. The ignored
    // duplicate-column error is expected on every later open and on freshly created databases.
    let _ = c.execute(
        "ALTER TABLE codex_window_calibrations ADD COLUMN anchor_ready INTEGER NOT NULL DEFAULT 0 CHECK(anchor_ready IN (0,1))",
        [],
    );
    // SQLite parity for PostgreSQL migration 0015. Nullable columns preserve compatibility with
    // legacy databases and binaries; the v6 estimator reconstructs a missing fixed-point value
    // from the immutable whole-percent projection before writing both representations.
    for statement in [
        "ALTER TABLE codex_window_calibrations ADD COLUMN anchor_used_fraction_units INTEGER CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN observed_fraction_units INTEGER CHECK(observed_fraction_units >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN observed_spend_nano INTEGER CHECK(observed_spend_nano >= 0)",
        "ALTER TABLE codex_window_observations ADD COLUMN used_fraction_units INTEGER CHECK(used_fraction_units BETWEEN 0 AND 100000000)",
        "ALTER TABLE codex_home_spend ADD COLUMN spent_nanocredits INTEGER CHECK(spent_nanocredits IS NULL OR spent_nanocredits >= 0)",
        "ALTER TABLE codex_home_spend ADD COLUMN credit_tracking_started_ts INTEGER CHECK(credit_tracking_started_ts IS NULL OR credit_tracking_started_ts > 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN anchor_spend_nanocredits INTEGER CHECK(anchor_spend_nanocredits IS NULL OR anchor_spend_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN observed_spend_nanocredits INTEGER CHECK(observed_spend_nanocredits IS NULL OR observed_spend_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN current_capacity_nanocredits INTEGER CHECK(current_capacity_nanocredits IS NULL OR current_capacity_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN current_low_nanocredits INTEGER CHECK(current_low_nanocredits IS NULL OR current_low_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN current_high_nanocredits INTEGER CHECK(current_high_nanocredits IS NULL OR current_high_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN last_capacity_nanocredits INTEGER CHECK(last_capacity_nanocredits IS NULL OR last_capacity_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN last_low_nanocredits INTEGER CHECK(last_low_nanocredits IS NULL OR last_low_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN last_high_nanocredits INTEGER CHECK(last_high_nanocredits IS NULL OR last_high_nanocredits >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN credit_samples INTEGER CHECK(credit_samples IS NULL OR credit_samples >= 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN credit_estimator_version INTEGER CHECK(credit_estimator_version IS NULL OR credit_estimator_version > 0)",
        "ALTER TABLE codex_window_calibrations ADD COLUMN unattributed_fraction_units INTEGER CHECK(unattributed_fraction_units IS NULL OR unattributed_fraction_units >= 0)",
        "ALTER TABLE codex_window_observations ADD COLUMN gateway_spend_nanocredits INTEGER CHECK(gateway_spend_nanocredits IS NULL OR gateway_spend_nanocredits >= 0)",
    ] {
        let _ = c.execute(statement, []);
    }
    // Native Gemini calibration uses the two explicit Antigravity quota-summary windows. Keep
    // SQLite schema parity for importer/tests even though PostgreSQL remains production authority.
    // Large WLS accumulators are canonical decimal text because SQLite has no exact i128 integer
    // type; registry validates them before estimator arithmetic.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS gemini_profile_spend( \
           profile_id TEXT PRIMARY KEY, \
           spent_nano INTEGER NOT NULL DEFAULT 0 CHECK(spent_nano >= 0), \
           updated_ts INTEGER NOT NULL CHECK(updated_ts > 0)); \
         CREATE TABLE IF NOT EXISTS gemini_window_calibrations( \
           profile_id TEXT NOT NULL, \
           bucket_id TEXT NOT NULL, \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','weekly')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           anchor_used_fraction_units INTEGER NOT NULL \
             CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000), \
           anchor_spend_nano INTEGER NOT NULL CHECK(anchor_spend_nano >= 0), \
           anchor_ready INTEGER NOT NULL DEFAULT 0 CHECK(anchor_ready IN (0,1)), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           sum_used_sq TEXT NOT NULL DEFAULT '0', \
           sum_used_spend_nano TEXT NOT NULL DEFAULT '0', \
           observed_fraction_units INTEGER NOT NULL DEFAULT 0 \
             CHECK(observed_fraction_units >= 0), \
           observed_spend_nano INTEGER NOT NULL DEFAULT 0 \
             CHECK(observed_spend_nano >= 0), \
           samples INTEGER NOT NULL DEFAULT 0 CHECK(samples >= 0), \
           current_capacity_nano INTEGER \
             CHECK(current_capacity_nano IS NULL OR current_capacity_nano >= 0), \
           current_low_nano INTEGER \
             CHECK(current_low_nano IS NULL OR current_low_nano >= 0), \
           current_high_nano INTEGER \
             CHECK(current_high_nano IS NULL OR current_high_nano >= 0), \
           current_confidence_bp INTEGER NOT NULL DEFAULT 0 \
             CHECK(current_confidence_bp BETWEEN 0 AND 10000), \
           last_measured_at INTEGER CHECK(last_measured_at IS NULL OR last_measured_at > 0), \
           estimator_version INTEGER NOT NULL DEFAULT 1 CHECK(estimator_version > 0), \
           version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0), \
           updated_ts INTEGER NOT NULL CHECK(updated_ts > 0), \
           PRIMARY KEY(profile_id,bucket_id), \
           CHECK((bucket_id='gemini-5h' AND window_kind='5h' AND window_duration_mins=300) \
             OR (bucket_id='gemini-weekly' AND window_kind='weekly' \
               AND window_duration_mins=10080)), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano IS NOT NULL)); \
         CREATE TABLE IF NOT EXISTS gemini_window_observations( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           profile_id TEXT NOT NULL, \
           bucket_id TEXT NOT NULL, \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','weekly')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           gateway_spend_nano INTEGER NOT NULL CHECK(gateway_spend_nano >= 0), \
           CHECK((bucket_id='gemini-5h' AND window_kind='5h' AND window_duration_mins=300) \
             OR (bucket_id='gemini-weekly' AND window_kind='weekly' \
               AND window_duration_mins=10080)), \
           UNIQUE(profile_id,bucket_id,resets_at,observed_at,used_fraction_units,gateway_spend_nano)); \
         CREATE INDEX IF NOT EXISTS gemini_window_observations_window \
           ON gemini_window_observations(profile_id,bucket_id,resets_at,observed_at);",
    )?;
    // Expand-only compatibility for SQLite authorities opened before estimator v2. Production
    // PostgreSQL receives the same column through engine migration 0014.
    let _ = c.execute(
        "ALTER TABLE gemini_window_calibrations ADD COLUMN observed_spend_nano INTEGER NOT NULL DEFAULT 0 CHECK(observed_spend_nano >= 0)",
        [],
    );
    // Migration 0022 keeps the legacy Gemini estimator tables intact for migration-first rollout.
    // The exact authority is plan-scoped and preserves provider measurement resolution/source so
    // a later estimator can replay every accepted interval without inferred metadata.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS gemini_exact_window_calibrations( \
           profile_id TEXT NOT NULL CHECK(profile_id <> ''), \
           plan TEXT NOT NULL CHECK(plan <> ''), \
           bucket_id TEXT NOT NULL CHECK(bucket_id IN ('gemini-5h','gemini-weekly')), \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','weekly')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           anchor_used_fraction_units INTEGER NOT NULL \
             CHECK(anchor_used_fraction_units BETWEEN 0 AND 100000000), \
           anchor_resolution_fraction_units INTEGER NOT NULL \
             CHECK(anchor_resolution_fraction_units BETWEEN 1 AND 100000000), \
           anchor_spend_nano INTEGER NOT NULL CHECK(anchor_spend_nano >= 0), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           measurement_resolution_fraction_units INTEGER NOT NULL \
             CHECK(measurement_resolution_fraction_units BETWEEN 1 AND 100000000), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           observed_fraction_units INTEGER NOT NULL DEFAULT 0 \
             CHECK(observed_fraction_units >= 0), \
           observed_spend_nano INTEGER NOT NULL DEFAULT 0 CHECK(observed_spend_nano >= 0), \
           samples INTEGER NOT NULL DEFAULT 0 CHECK(samples >= 0), \
           unattributed_fraction_units INTEGER NOT NULL DEFAULT 0 \
             CHECK(unattributed_fraction_units >= 0), \
           current_capacity_nano INTEGER \
             CHECK(current_capacity_nano IS NULL OR current_capacity_nano >= 0), \
           current_low_nano INTEGER CHECK(current_low_nano IS NULL OR current_low_nano >= 0), \
           current_high_nano INTEGER CHECK(current_high_nano IS NULL OR current_high_nano >= 0), \
           current_confidence_bp INTEGER NOT NULL DEFAULT 0 \
             CHECK(current_confidence_bp BETWEEN 0 AND 10000), \
           last_measured_at INTEGER CHECK(last_measured_at IS NULL OR last_measured_at > 0), \
           estimator_version INTEGER NOT NULL DEFAULT 1 CHECK(estimator_version > 0), \
           version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0), \
           updated_ts INTEGER NOT NULL CHECK(updated_ts > 0), \
           PRIMARY KEY(profile_id,plan,bucket_id), \
           CHECK((bucket_id='gemini-5h' AND window_kind='5h' AND window_duration_mins=300) \
             OR (bucket_id='gemini-weekly' AND window_kind='weekly' \
               AND window_duration_mins=10080)), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano IS NOT NULL), \
           CHECK(current_low_nano IS NULL OR current_capacity_nano >= current_low_nano), \
           CHECK(current_high_nano IS NULL OR current_capacity_nano <= current_high_nano), \
           CHECK(current_low_nano IS NULL OR current_high_nano IS NULL \
             OR current_low_nano <= current_high_nano), \
           CHECK((samples=0 AND observed_fraction_units=0 AND observed_spend_nano=0 \
               AND current_capacity_nano IS NULL AND current_low_nano IS NULL \
               AND current_high_nano IS NULL AND current_confidence_bp=0 \
               AND last_measured_at IS NULL) \
             OR (samples>0 AND observed_fraction_units>0 AND observed_spend_nano>0 \
               AND current_capacity_nano IS NOT NULL AND current_low_nano IS NOT NULL \
               AND last_measured_at IS NOT NULL))); \
         CREATE INDEX IF NOT EXISTS gemini_exact_window_calibrations_cohort \
           ON gemini_exact_window_calibrations(plan,bucket_id,window_duration_mins); \
         CREATE TABLE IF NOT EXISTS gemini_exact_window_observations( \
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           profile_id TEXT NOT NULL CHECK(profile_id <> ''), \
           plan TEXT NOT NULL CHECK(plan <> ''), \
           bucket_id TEXT NOT NULL CHECK(bucket_id IN ('gemini-5h','gemini-weekly')), \
           window_kind TEXT NOT NULL CHECK(window_kind IN ('5h','weekly')), \
           window_duration_mins INTEGER NOT NULL CHECK(window_duration_mins > 0), \
           resets_at INTEGER NOT NULL CHECK(resets_at > 0), \
           observed_at INTEGER NOT NULL CHECK(observed_at > 0), \
           used_fraction_units INTEGER NOT NULL \
             CHECK(used_fraction_units BETWEEN 0 AND 100000000), \
           measurement_resolution_fraction_units INTEGER NOT NULL \
             CHECK(measurement_resolution_fraction_units BETWEEN 1 AND 100000000), \
           gateway_spend_nano INTEGER NOT NULL CHECK(gateway_spend_nano >= 0), \
           observation_source TEXT NOT NULL CHECK(observation_source IN ('response','poll')), \
           source_request_id TEXT, \
           CHECK((bucket_id='gemini-5h' AND window_kind='5h' AND window_duration_mins=300) \
             OR (bucket_id='gemini-weekly' AND window_kind='weekly' \
               AND window_duration_mins=10080)), \
           CHECK((observation_source='response' AND source_request_id IS NOT NULL \
               AND source_request_id <> '') \
             OR (observation_source='poll' AND source_request_id IS NULL)), \
           UNIQUE(profile_id,plan,bucket_id,source_request_id), \
           UNIQUE(profile_id,plan,bucket_id,resets_at,observed_at,used_fraction_units, \
             measurement_resolution_fraction_units,gateway_spend_nano,observation_source)); \
         CREATE INDEX IF NOT EXISTS gemini_exact_window_observations_window \
           ON gemini_exact_window_observations( \
             profile_id,plan,bucket_id,resets_at,observed_at);",
    )?;
    // Разбивка расхода по токенам/моделям для клиентских дашбордов (per-request). НЕ money-БД:
    // авторитет денег — accounts.balance_nano + ledger. Эта таблица — аналитика (что реально
    // потрачено по корзинам токенов и моделям), пишется рядом с charge, обрезается по ретенции.
    c.execute(
        "CREATE TABLE IF NOT EXISTS usage_events(id INTEGER PRIMARY KEY AUTOINCREMENT, \
         account_id TEXT NOT NULL, key TEXT, model TEXT, \
         input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0, \
         cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_5m_tokens INTEGER NOT NULL DEFAULT 0, \
         cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0, web_search_requests INTEGER NOT NULL DEFAULT 0, \
         real_nano INTEGER NOT NULL DEFAULT 0, charge_nano INTEGER NOT NULL DEFAULT 0, ref TEXT, ts INTEGER)",
        [],
    )?;
    for (name, ty) in [
        ("speed", "TEXT NOT NULL DEFAULT 'standard'"),
        ("inference_geo", "TEXT NOT NULL DEFAULT ''"),
        ("input_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("output_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_read_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_write_5m_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_write_1h_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("web_search_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("priced_ts", "INTEGER NOT NULL DEFAULT 0"),
        ("provider", "TEXT NOT NULL DEFAULT 'anthropic'"),
    ] {
        let _ = c.execute(
            &format!("ALTER TABLE usage_events ADD COLUMN {name} {ty}"),
            [],
        );
    }
    // Индекс под агрегацию по окну (account_id + время) и под фоновую обрезку по ts.
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS usage_events_acct_ts ON usage_events(account_id, ts)",
        [],
    );
    // SQLite money durability mirrors the PostgreSQL request lifecycle: every hold has an exact
    // request identity and lease, while settlement intent is committed to an outbox before the
    // balance mutation. Recovery can therefore distinguish pre-delivery cancellation from a
    // delivered request and can retry the exact settlement after process/database failures.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS billing_reservations( \
           request_id TEXT PRIMARY KEY, account_id TEXT NOT NULL, key TEXT NOT NULL, \
           hold_nano INTEGER NOT NULL, group_id TEXT CHECK(group_id IS NULL OR group_id<>''), \
           attempt INTEGER NOT NULL DEFAULT 1 CHECK(attempt>0), state TEXT NOT NULL, \
           balance_after_reserve_nano INTEGER NOT NULL, actual_nano INTEGER, \
           balance_after_settle_nano INTEGER, reference TEXT, lease_until INTEGER NOT NULL, \
           created_ts INTEGER NOT NULL, updated_ts INTEGER NOT NULL, settled_ts INTEGER); \
         CREATE INDEX IF NOT EXISTS billing_reservations_lease \
           ON billing_reservations(state,lease_until); \
         CREATE TABLE IF NOT EXISTS billing_settlement_outbox( \
           request_id TEXT PRIMARY KEY, actual_nano INTEGER NOT NULL, reference TEXT, \
           usage_json TEXT, state TEXT NOT NULL DEFAULT 'pending', attempts INTEGER NOT NULL DEFAULT 0, \
           next_attempt_ts INTEGER NOT NULL DEFAULT 0, last_error TEXT, \
           created_ts INTEGER NOT NULL, updated_ts INTEGER NOT NULL, committed_ts INTEGER); \
         CREATE INDEX IF NOT EXISTS billing_outbox_pending \
           ON billing_settlement_outbox(state,next_attempt_ts,created_ts); \
         CREATE TABLE IF NOT EXISTS execution_group_winner( \
           group_id TEXT PRIMARY KEY CHECK(group_id<>''), \
           winner_request_id TEXT NOT NULL CHECK(winner_request_id<>''), \
           decided_at INTEGER NOT NULL);"
    )?;
    ensure_sqlite_column(&c, "billing_reservations", "group_id", "TEXT")?;
    ensure_sqlite_column(
        &c,
        "billing_reservations",
        "attempt",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    migrate_pricing_policy_schema(&c)?;
    Ok(c)
}

fn ensure_sqlite_column(
    conn: &Connection,
    table: &str,
    name: &str,
    column_type: &str,
) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
        rusqlite::params![table, name],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute_batch(&format!(
            "ALTER TABLE \"{table}\" ADD COLUMN \"{name}\" {column_type}"
        ))
        .with_context(|| format!("add SQLite policy column {table}.{name}"))?;
    }
    Ok(())
}

fn migrate_pricing_policy_schema(conn: &Connection) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite attribution schema transaction")?;
    install_attribution_schema(&tx)?;
    tx.commit()
        .context("commit SQLite attribution schema transaction")
}

/// Request-scoped money attribution: the columns and unique indexes that make a charge and its
/// usage row idempotent per request. The retired multi-discount schema no longer installs here.
fn install_attribution_schema(conn: &Connection) -> Result<()> {
    ensure_sqlite_column(conn, "billing_settlement_outbox", "provider", "TEXT")?;
    ensure_sqlite_column(
        conn,
        "billing_settlement_outbox",
        "disposition",
        "TEXT NOT NULL DEFAULT 'settle'",
    )?;
    ensure_sqlite_column(conn, "ledger", "provider", "TEXT")?;
    ensure_sqlite_column(conn, "ledger", "official_nano", "INTEGER")?;
    ensure_sqlite_column(conn, "ledger", "request_id", "TEXT")?;
    ensure_sqlite_column(conn, "usage_events", "request_id", "TEXT")?;
    for table in ["billing_settlement_outbox", "usage_events", "ledger"] {
        for (name, column_type) in SQLITE_ATTRIBUTION_COLUMNS {
            ensure_sqlite_column(conn, table, name, column_type)?;
        }
    }
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_request_once \
           ON ledger(kind,request_id) WHERE request_id IS NOT NULL; \
         CREATE UNIQUE INDEX IF NOT EXISTS usage_events_request_once \
           ON usage_events(request_id) WHERE request_id IS NOT NULL;",
    )
    .context("install SQLite attribution indexes")?;
    install_sqlite_attribution_guards(conn)?;
    Ok(())
}



fn install_sqlite_attribution_guards(conn: &Connection) -> Result<()> {
    const COMMON_INVALID: &str = "
        (NEW.attribution_schema_version IS NOT NULL AND NEW.attribution_schema_version <= 0)
        OR (NEW.snapshot_kind IS NOT NULL
            AND NEW.snapshot_kind NOT IN ('policy_v1', 'legacy_scalar'))
        OR (NEW.alias_generation IS NOT NULL AND NEW.alias_generation <= 0)
        OR (NEW.served_canonical_model_id = '')
        OR (NEW.billing_invariant_code = '')
        OR (NEW.pricing_mode IS NOT NULL
            AND NEW.pricing_mode NOT IN ('track', 'discount', 'legacy_scalar'))
        OR (NEW.rule_origin IS NOT NULL AND NEW.rule_origin NOT IN ('managed', 'legacy'))
        OR (NEW.discount_bps IS NOT NULL
            AND (NEW.discount_bps < 0 OR NEW.discount_bps % 100 <> 0
                 OR (NEW.discount_bps > 9500
                     AND (NEW.discount_bps > 10000 OR NEW.account_class IS NOT 'service'))))
        OR (NEW.payable_multiplier_bp IS NOT NULL
            AND (NEW.payable_multiplier_bp < 0 OR NEW.payable_multiplier_bp > 10000))
        OR (NEW.track_eligible IS NOT NULL AND NEW.track_eligible NOT IN (0, 1))
        OR (NEW.retention_eligible IS NOT NULL AND NEW.retention_eligible NOT IN (0, 1))
        OR (NEW.commission_eligible IS NOT NULL AND NEW.commission_eligible NOT IN (0, 1))
        OR (NEW.official_cost_json IS NOT NULL AND NOT json_valid(NEW.official_cost_json))
        OR (NEW.funding_allocation_json IS NOT NULL
            AND NOT json_valid(NEW.funding_allocation_json))
        OR ((NEW.paid_funded_nano IS NULL)
            + (NEW.bonus_funded_nano IS NULL)
            + (NEW.other_funded_nano IS NULL)) NOT IN (0, 3)
    ";
    for (table, charged_column, charge_row_guard, table_invalid) in [
        ("billing_settlement_outbox", "actual_nano", "1", "0"),
        (
            "usage_events",
            "charge_nano",
            "1",
            "NEW.tariff_priced_ts IS NOT NULL AND NEW.tariff_priced_ts <> NEW.priced_ts",
        ),
        (
            "ledger",
            "amount_nano",
            "NEW.kind = 'charge'",
            "NEW.official_nano IS NOT NULL AND NEW.official_nano < 0",
        ),
    ] {
        let funding_invalid = format!(
            "(
                NEW.paid_funded_nano IS NOT NULL
                AND (
                    NOT ({charge_row_guard})
                    OR NEW.paid_funded_nano < 0
                    OR NEW.bonus_funded_nano < 0
                    OR NEW.other_funded_nano < 0
                    OR NEW.paid_funded_nano
                       + NEW.bonus_funded_nano
                       + NEW.other_funded_nano <> NEW.{charged_column}
                )
            )"
        );
        for (suffix, event) in [("insert", "INSERT"), ("update", "UPDATE")] {
            conn.execute_batch(&format!(
                "CREATE TRIGGER IF NOT EXISTS {table}_policy_attribution_{suffix}
                 BEFORE {event} ON \"{table}\"
                 FOR EACH ROW
                 WHEN ({COMMON_INVALID}) OR ({table_invalid}) OR ({funding_invalid})
                 BEGIN
                     SELECT RAISE(ABORT, 'invalid policy attribution');
                 END;"
            ))
            .with_context(|| format!("install SQLite attribution guard for {table} {event}"))?;
        }
    }
    Ok(())
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Миграция старой модели «ключ = кошелёк» → «аккаунт (баланс) + ключи (доступы)». Для каждого ключа
/// без `account_id` (легаси) атомарно заводим отдельный случайный аккаунт, переносим баланс/расход/
/// наценку и линкуем ключ. Повторный запуск пропускает уже связанные строки.
fn migrate_legacy_keys(c: &Connection) -> Result<()> {
    let legacy: Vec<(String, i64, i64, i64, String)> = {
        let mut stmt = c.prepare(
            "SELECT key, balance_nano, spent_nano, mult_bp, COALESCE(status,'active') \
             FROM api_keys WHERE account_id IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let tx = c.unchecked_transaction()?;
    for (key, bal, spent, mult, status) in legacy {
        // AUDIT(C39): never derive wallet identity from a short key suffix. Generate a full random
        // account ID and persist the key→account mapping atomically with the migrated balance.
        let ts = now();
        let acct: String = tx.query_row(
            "INSERT INTO accounts(id, balance_nano, spent_nano, mult_bp, status, created_ts, created) \
             VALUES('acct_' || lower(hex(randomblob(16))),?1,?2,?3,?4,?5,?6) RETURNING id",
            rusqlite::params![bal, spent, mult, status, ts, chrono_like(ts)],
            |r| r.get(0),
        )?;
        let updated = tx.execute(
            "UPDATE api_keys SET account_id=?1, reserved_nano=0 WHERE key=?2 AND account_id IS NULL",
            rusqlite::params![acct, key],
        )?;
        if updated != 1 {
            anyhow::bail!("legacy key migration lost its target row");
        }
    }
    tx.commit()?;
    // AUDIT-TODO(C39): detect and manually split wallets already merged by the historical suffix migration.
    Ok(())
}

fn resolve_token(inline: Option<String>, token_file: Option<String>) -> String {
    if let Some(t) = inline {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    if let Some(f) = token_file {
        if !f.trim().is_empty() {
            if let Ok(s) = fs::read_to_string(f.trim()) {
                return s.trim().to_string();
            }
        }
    }
    String::new()
}

/// Активные подписки нужного флота, у которых есть непустой токен.
pub fn load_active(conn: &Connection, fleet: Option<&str>) -> Result<Vec<Sub>> {
    let mut stmt = conn.prepare(
        "SELECT email, token, token_file, proxy, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(plan,'') FROM subs",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (email, token, token_file, proxy, status, sfleet, plan) = row?;
        if status != "active" {
            continue;
        }
        if let Some(f) = fleet {
            if f != sfleet {
                continue;
            }
        }
        let tok = resolve_token(token, token_file);
        if tok.is_empty() {
            continue;
        }
        out.push(Sub {
            email,
            token: tok,
            proxy: proxy.unwrap_or_default(),
            fleet: sfleet,
            plan,
        });
    }
    Ok(out)
}

// ── CLI-операции реестра ────────────────────────────────────────────────────
pub fn add(conn: &Connection, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token, token_file, proxy, status, fleet, added_ts, added) \
         VALUES(?1, ?2, NULL, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token=excluded.token, token_file=NULL, \
         proxy=excluded.proxy, status='active', fleet=excluded.fleet, \
         auth_state=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 'healthy' ELSE subs.auth_state END, \
         auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.auth_fail_streak END, \
         first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.first_auth_fail_ts END, \
         last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.last_auth_fail_ts END, \
         last_auth_http=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.last_auth_http END, \
         dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.dead_since_ts END, \
         dead_reason=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN '' ELSE subs.dead_reason END, \
         auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN '' ELSE subs.auth_token_fp END",
        rusqlite::params![email, token, proxy, fleet, now(), chrono_like(now())],
    )?;
    Ok(())
}

pub fn add_file(
    conn: &Connection,
    email: &str,
    token_file: &str,
    proxy: &str,
    fleet: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token, token_file, proxy, status, fleet, added_ts, added) \
         VALUES(?1, NULL, ?2, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token=NULL, token_file=excluded.token_file, \
         proxy=excluded.proxy, status='active', fleet=excluded.fleet, \
         auth_state=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 'healthy' ELSE subs.auth_state END, \
         auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.auth_fail_streak END, \
         first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.first_auth_fail_ts END, \
         last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.last_auth_fail_ts END, \
         last_auth_http=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.last_auth_http END, \
         dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.dead_since_ts END, \
         dead_reason=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN '' ELSE subs.dead_reason END, \
         auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN '' ELSE subs.auth_token_fp END",
        rusqlite::params![email, token_file, proxy, fleet, now(), chrono_like(now())],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, email: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET status=?1 WHERE email=?2",
        rusqlite::params![status, email],
    )?)
}
pub fn set_plan(conn: &Connection, email: &str, plan: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET plan=?1 WHERE email=?2",
        rusqlite::params![plan, email],
    )?)
}

/// (разрешённый токен, proxy) для одной подписки (любого статуса) — для детекта тарифа.
pub fn get_creds(conn: &Connection, email: &str) -> Result<Option<(String, String)>> {
    let row = conn.query_row(
        "SELECT token, token_file, proxy FROM subs WHERE email=?1",
        rusqlite::params![email],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    );
    match row {
        Ok((token, token_file, proxy)) => {
            let tok = resolve_token(token, token_file);
            if tok.is_empty() {
                Ok(None)
            } else {
                Ok(Some((tok, proxy.unwrap_or_default())))
            }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn set_proxy(conn: &Connection, email: &str, proxy: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET proxy=?1 WHERE email=?2",
        rusqlite::params![proxy, email],
    )?)
}
pub fn set_fleet(conn: &Connection, email: &str, fleet: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET fleet=?1 WHERE email=?2",
        rusqlite::params![fleet, email],
    )?)
}
/// Обновить прокси-метаданные (пишет authbot — владелец жизненного цикла прокси). `expire` — дата
/// истечения из IPRoyal (ISO, "" если неизвестно); `ok` — жив ли прокси на fingerprint-free проверке.
pub fn set_proxy_meta(
    conn: &Connection,
    email: &str,
    expire: &str,
    checked_ts: i64,
    ok: bool,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET proxy_expire=?1, proxy_checked_ts=?2, proxy_ok=?3 WHERE email=?4",
        rusqlite::params![expire, checked_ts, ok as i64, email],
    )?)
}
/// host:port из строки прокси (без user:pass) — для показа в панели/логах.
pub fn mask_proxy(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let no_scheme = p.split("://").last().unwrap_or(p);
    no_scheme
        .rsplit('@')
        .next()
        .unwrap_or(no_scheme)
        .to_string()
}
pub fn remove(conn: &Connection, email: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET status='deleted' WHERE email=?1",
        rusqlite::params![email],
    )?)
}

/// Disable every subscription (optionally in one fleet) without destroying active lease history.
pub fn clear(conn: &Connection, fleet: Option<&str>) -> Result<usize> {
    Ok(match fleet {
        Some(f) => conn.execute(
            "UPDATE subs SET status='deleted' WHERE COALESCE(fleet,'prod')=?1 AND status<>'deleted'",
            rusqlite::params![f],
        )?,
        None => conn.execute("UPDATE subs SET status='deleted' WHERE status<>'deleted'", [])?,
    })
}

/// Строка списка для CLI (без утечки токена — только флаг наличия).
pub struct SubRow {
    pub email: String,
    pub status: String,
    pub fleet: String,
    pub plan: String,
    pub has_token: bool,
    pub proxy: String,
}

pub fn list(conn: &Connection) -> Result<Vec<SubRow>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), COALESCE(plan,''), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,'') \
         FROM subs ORDER BY COALESCE(added_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SubRow {
            email: r.get::<_, String>(0)?,
            status: r.get::<_, String>(1)?,
            fleet: r.get::<_, String>(2)?,
            plan: r.get::<_, String>(3)?,
            has_token: r
                .get::<_, Option<String>>(4)?
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            proxy: r.get::<_, String>(5)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Internal Claude lifecycle authority projection. It intentionally has no `Serialize` or `Debug`:
/// the complete row contains both the account email and the raw credentialed proxy URL.
pub struct ClaudeLifecycleProfile {
    pub email: String,
    pub status: String,
    pub fleet: String,
    pub has_token: bool,
    pub proxy: String,
    pub plan: String,
    pub added_ts: i64,
    pub auth_state: String,
}

pub fn load_claude_lifecycle(conn: &Connection) -> Result<Vec<ClaudeLifecycleProfile>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,''), \
         COALESCE(plan,''), COALESCE(added_ts,0), COALESCE(auth_state,'healthy') \
         FROM subs ORDER BY COALESCE(added_ts,0), email",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ClaudeLifecycleProfile {
            email: row.get(0)?,
            status: row.get(1)?,
            fleet: row.get(2)?,
            has_token: row
                .get::<_, Option<String>>(3)?
                .is_some_and(|value| !value.is_empty()),
            proxy: row.get(4)?,
            plan: row.get(5)?,
            added_ts: row.get(6)?,
            auth_state: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Строка админ-обзора подписок (движок → панель): БЕЗ токена, прокси — маска host:port + метаданные.
pub struct SubAdmin {
    pub email: String,
    pub status: String,
    pub fleet: String,
    pub has_token: bool,
    pub proxy_host: String,     // host:port (без user:pass)
    pub proxy_expire: String,   // ISO из IPRoyal / ""
    pub proxy_ok: Option<bool>, // None = не проверялся (здоровье в осн. из движка/органики)
    pub added_ts: i64,          // момент добавления токена (срок жизни = added_ts + N дней)
    pub added: String,
    /// Durable auth-health (авторитетно из БД, переживает рестарт): 'healthy'|'suspect'|'dead'.
    pub auth_state: String,
    pub dead_reason: String, // '' если не dead
    pub dead_since_ts: i64,  // 0 если не dead
}

/// Durable auth-health одной подписки. Движок (поллер) пишет это из КОРРЕЛИРОВАННЫХ чистых probe:
/// один 401/403 не приговор (может быть транзиент/битый запрос), но N подряд за ≥T минут = мёртвый
/// токен/бан. Переживает рестарт и blue/green (в отличие от эфемерного in-memory `auth_dead`).
/// Примитивы-сентинелы (0/"" = «нет») — чтобы `pool` не тащил Option через слой.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubHealth {
    pub email: String,
    pub auth_state: String, // 'healthy' | 'suspect' | 'dead'
    pub auth_fail_streak: i64,
    pub first_auth_fail_ts: i64, // 0 = нет текущей серии отказов
    pub last_auth_fail_ts: i64,
    pub last_auth_http: i64,   // 0 = нет
    pub dead_since_ts: i64,    // 0 = не dead
    pub dead_reason: String,   // '' = нет
    pub auth_token_fp: String, // отпечаток токена, к которому относится вердикт (смена → авто-ревайв)
}

pub fn subs_admin(conn: &Connection) -> Result<Vec<SubAdmin>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,''), \
         COALESCE(proxy_expire,''), proxy_ok, COALESCE(added_ts,0), COALESCE(added,''), \
         COALESCE(auth_state,'healthy'), COALESCE(dead_reason,''), COALESCE(dead_since_ts,0) \
         FROM subs ORDER BY COALESCE(added_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        let proxy: String = r.get(4)?;
        Ok(SubAdmin {
            email: r.get(0)?,
            status: r.get(1)?,
            fleet: r.get(2)?,
            has_token: r
                .get::<_, Option<String>>(3)?
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            proxy_host: mask_proxy(&proxy),
            proxy_expire: r.get(5)?,
            proxy_ok: r.get::<_, Option<i64>>(6)?.map(|n| n != 0),
            added_ts: r.get(7)?,
            added: r.get(8)?,
            auth_state: r.get(9)?,
            dead_reason: r.get(10)?,
            dead_since_ts: r.get(11)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Загрузить durable auth-health всех подписок (движок сеет им in-memory состояние на старте).
pub fn load_sub_health(conn: &Connection, fleet: Option<&str>) -> Result<Vec<SubHealth>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(auth_state,'healthy'), COALESCE(auth_fail_streak,0), \
         COALESCE(first_auth_fail_ts,0), COALESCE(last_auth_fail_ts,0), COALESCE(last_auth_http,0), \
         COALESCE(dead_since_ts,0), COALESCE(dead_reason,''), COALESCE(auth_token_fp,''), \
         COALESCE(fleet,'prod') FROM subs")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            SubHealth {
                email: r.get(0)?,
                auth_state: r.get(1)?,
                auth_fail_streak: r.get(2)?,
                first_auth_fail_ts: r.get(3)?,
                last_auth_fail_ts: r.get(4)?,
                last_auth_http: r.get(5)?,
                dead_since_ts: r.get(6)?,
                dead_reason: r.get(7)?,
                auth_token_fp: r.get(8)?,
            },
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (h, sfleet) = row?;
        if let Some(f) = fleet {
            if f != sfleet {
                continue;
            }
        }
        out.push(h);
    }
    Ok(out)
}

/// Записать durable auth-health одной подписки (движок → БД). Идемпотентный upsert по email.
pub fn save_sub_health(conn: &Connection, h: &SubHealth) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET auth_state=?1, auth_fail_streak=?2, first_auth_fail_ts=?3, \
         last_auth_fail_ts=?4, last_auth_http=?5, dead_since_ts=?6, dead_reason=?7, auth_token_fp=?8 \
         WHERE email=?9",
        rusqlite::params![
            h.auth_state, h.auth_fail_streak, h.first_auth_fail_ts, h.last_auth_fail_ts,
            h.last_auth_http, h.dead_since_ts, h.dead_reason, h.auth_token_fp, h.email
        ],
    )?)
}

// ── Биллинг: ключи клиентов с USD-балансом (нанодоллары) ─────────────────────
//
// Слой хранения: только персист+CRUD баланса. САМ подсчёт стоимости (токены→нано) —
// в крейте `metering`; сюда приходит уже готовая сумма списания в нано. Границы держим:
// registry не знает про цены/токены, только про целые нанодоллары на ключе.

/// Строка ключа. Баланс — НЕ здесь (он на аккаунте); ключ = доступ + метка + атрибуция расхода.
#[derive(Clone, Debug)]
pub struct KeyRow {
    pub key: String,
    pub key_id: String,
    pub account_id: Option<String>,
    pub label: Option<String>,
    pub spent_nano: i64, // расход по ЭТОМУ ключу (атрибуция; баланс общий на аккаунте)
    pub reserved_nano: i64,
    pub spend_limit_nano: Option<i64>,
    pub expires_ts: Option<i64>,
    pub created_ts: i64,
    pub last_used_ts: Option<i64>,
    pub status: String,
}

/// Result of atomically replacing a key's mutable spending policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPolicyUpdate {
    Updated,
    NotFound,
    LimitBelowUsage,
    ExpiryNotFuture,
}



/// Выпустить ключ ПОД аккаунт (баланс — на аккаунте, ключ лишь ссылается). `label` — имя ключа.
pub fn key_issue(
    conn: &Connection,
    key: &str,
    account_id: &str,
    label: Option<&str>,
) -> Result<()> {
    key_issue_with_policy(conn, key, account_id, label, None, None)
}

pub fn key_issue_with_policy(
    conn: &Connection,
    key: &str,
    account_id: &str,
    label: Option<&str>,
    spend_limit_nano: Option<i64>,
    expires_ts: Option<i64>,
) -> Result<()> {
    if key.trim().is_empty() || account_id.trim().is_empty() {
        anyhow::bail!("key and account id must not be empty");
    }
    let changed = conn.execute(
        "INSERT INTO api_keys(key, key_id, account_id, label, spent_nano, reserved_nano, \
         spend_limit_nano,expires_ts,status,created_ts,created) \
         VALUES(?1,'key_' || lower(hex(randomblob(16))),?2,?3,0,0,?4,?5,'active',?6,?7) \
         ON CONFLICT(key) DO UPDATE SET label=excluded.label, \
         spend_limit_nano=excluded.spend_limit_nano,expires_ts=excluded.expires_ts \
         WHERE api_keys.account_id=excluded.account_id",
        rusqlite::params![
            key,
            account_id,
            label,
            spend_limit_nano,
            expires_ts,
            now(),
            chrono_like(now()),
        ],
    )?;
    if changed == 0 {
        anyhow::bail!("key is already owned by another account");
    }
    Ok(())
}


/// Backward-compatible startup entry point. Only expired request-scoped leases are touched; legacy
/// aggregate holds remain fail-closed because they still have no provable owner or age.
pub fn reconcile_reservations(conn: &Connection) -> Result<usize> {
    let report = sqlite_reconcile_expired(conn, 10_000, false)?;
    Ok(report.canceled_before_delivery + report.charged_after_delivery + report.processed_outbox)
}

// ── Аккаунты (профиль клиента: ЕДИНЫЙ баланс, N ключей-доступов) ─────────────────────

/// Строка аккаунта. Баланс/резерв/наценка — ЗДЕСЬ (не на ключе). `handle` — внешняя идентичность.
#[derive(Clone, Debug)]
pub struct AccountRow {
    pub id: String,
    pub handle: Option<String>,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub mult_bp: i64,
    pub status: String,
}




/// Резолв ключа → аккаунт (горячий путь авторизации). Активны должны быть И ключ, И аккаунт.
#[derive(Clone, Debug)]
pub struct KeyAuth {
    pub account_id: String,
    /// Account-wide payable multiplier in basis points — the customer's personal discount.
    pub mult_bp: i64,
    /// Per-provider overrides of `mult_bp`, empty when one number prices the whole account.
    /// Resolved through [`KeyAuth::mult_for`]; a provider without a row keeps the default.
    pub provider_mult_bp: Vec<(String, i64)>,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub spend_limit_nano: Option<i64>,
    pub expires_ts: Option<i64>,
    pub active: bool, // ключ активен И аккаунт активен
}

impl KeyAuth {
    /// The multiplier that prices this request: the provider override when the account has one,
    /// otherwise the account default. This is the entire pricing policy — no catalog, no rule
    /// lineage, no release generation between a discount and the request it prices.
    pub fn mult_for(&self, provider_id: &str) -> i64 {
        self.provider_mult_bp
            .iter()
            .find(|(provider, _)| provider == provider_id)
            .map(|(_, mult_bp)| *mult_bp)
            .unwrap_or(self.mult_bp)
    }

    pub fn active_at(&self, ts: i64) -> bool {
        self.active && self.expires_ts.is_none_or(|expires| expires > ts)
    }
}

/// Создать аккаунт. `handle` (TG id/email) опционален и уникален (когда задан).
pub fn account_create(
    conn: &Connection,
    id: &str,
    handle: Option<&str>,
    mult_bp: i64,
) -> Result<()> {
    if id.trim().is_empty() || handle.is_some_and(|value| value.trim().is_empty()) {
        anyhow::bail!("account id and supplied handle must not be empty");
    }
    if !(0..=10_000).contains(&mult_bp) {
        anyhow::bail!("account multiplier must be within 0..=10000 basis points");
    }
    conn.execute(
        "INSERT INTO accounts(id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, status, created_ts, created) \
         VALUES(?1, ?2, 0, 0, 0, ?3, 'active', ?4, ?5)",
        rusqlite::params![id, handle, mult_bp, now(), chrono_like(now())])?;
    Ok(())
}

pub fn account_get(conn: &Connection, id: &str) -> Result<Option<AccountRow>> {
    one_account(conn, "id", id)
}

/// Найти аккаунт по внешней идентичности (для входа юзера из TG/web).
pub fn account_by_handle(conn: &Connection, handle: &str) -> Result<Option<AccountRow>> {
    one_account(conn, "handle", handle)
}
fn one_account(conn: &Connection, col: &str, val: &str) -> Result<Option<AccountRow>> {
    let sql = format!(
        "SELECT id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, COALESCE(status,'active') \
         FROM accounts WHERE {col}=?1");
    match conn.query_row(&sql, rusqlite::params![val], |r| {
        Ok(AccountRow {
            id: r.get(0)?,
            handle: r.get(1)?,
            balance_nano: r.get(2)?,
            spent_nano: r.get(3)?,
            reserved_nano: r.get(4)?,
            mult_bp: r.get(5)?,
            status: r.get(6)?,
        })
    }) {
        Ok(a) => Ok(Some(a)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Список аккаунтов (для админ-CLI).
pub fn account_list(conn: &Connection) -> Result<Vec<AccountRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, COALESCE(status,'active') \
         FROM accounts ORDER BY COALESCE(created_ts,0)")?;
    let rows = stmt.query_map([], |r| {
        Ok(AccountRow {
            id: r.get(0)?,
            handle: r.get(1)?,
            balance_nano: r.get(2)?,
            spent_nano: r.get(3)?,
            reserved_nano: r.get(4)?,
            mult_bp: r.get(5)?,
            status: r.get(6)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn account_set_status(conn: &Connection, id: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE accounts SET status=?1 WHERE id=?2",
        rusqlite::params![status, id],
    )?)
}

/// Change the price multiplier for future requests. Existing ledger rows remain immutable.
pub fn account_set_mult_bp(conn: &Connection, id: &str, mult_bp: i64) -> Result<usize> {
    if !(0..=10_000).contains(&mult_bp) {
        anyhow::bail!("invalid account multiplier");
    }
    Ok(conn.execute(
        "UPDATE accounts SET mult_bp=?1 WHERE id=?2",
        rusqlite::params![mult_bp, id],
    )?)
}

/// Tombstone an account. Financial history and in-flight reservations remain settleable/auditable.
pub fn account_remove(conn: &Connection, id: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE accounts SET status='deleted' WHERE id=?1 AND status<>'deleted'",
        rusqlite::params![id],
    )?)
}

/// Пополнить баланс аккаунта (`amount` может быть отрицательным = коррекция) + запись в ledger.
/// Возвращает новый баланс, сохранённый исходный баланс точного idempotent replay, либо None, если
/// аккаунта нет. Повтор reference с другими параметрами — ошибка. Атомарно (UPDATE…RETURNING + ledger).
pub fn account_topup(
    conn: &Connection,
    id: &str,
    amount_nano: i64,
    reference: Option<&str>,
) -> Result<Option<i64>> {
    if matches!(reference, Some(r) if r.trim().is_empty()) {
        anyhow::bail!("monetary idempotency reference must not be empty");
    }
    let tx = conn.unchecked_transaction()?;
    // Начисляем баланс...
    let bal = match tx.query_row(
        "UPDATE accounts SET balance_nano = balance_nano + ?1 WHERE id = ?2 RETURNING balance_nano",
        rusqlite::params![amount_nano, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(b) => b,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Ok(None);
        } // нет аккаунта
        Err(e) => return Err(e.into()),
    };
    let kind = if amount_nano >= 0 { "topup" } else { "adjust" };
    // ...и пишем ledger. UNIQUE откатывает предварительный UPDATE. После конфликта считаем операцию
    // идемпотентным повтором ТОЛЬКО при точном совпадении account + amount + kind.
    match tx.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts) \
         VALUES(?1, NULL, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, kind, amount_nano, reference, bal, now()],
    ) {
        Ok(_) => {
            tx.commit()?;
            Ok(Some(bal))
        }
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            drop(tx); // ROLLBACK before inspecting the existing operation.
            let Some(reference) = reference else {
                return Err(rusqlite::Error::SqliteFailure(e, None).into());
            };
            let existing = conn.query_row(
                "SELECT account_id, kind, amount_nano, balance_after_nano FROM ledger \
                 WHERE ref=?1 AND kind IN ('topup','adjust')",
                rusqlite::params![reference],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                },
            );
            match existing {
                Ok((existing_id, existing_kind, existing_amount, Some(original_balance)))
                    if existing_id == id
                        && existing_kind == kind
                        && existing_amount == amount_nano =>
                {
                    Ok(Some(original_balance))
                }
                Ok(_) => {
                    elog::error("registry", "billing idempotency conflict: parameters differ from the stored operation");
                    // AUDIT-TODO(C42/C80): expose a typed idempotency conflict through Control API as HTTP 409.
                    anyhow::bail!(
                        "idempotency reference already belongs to a different monetary operation"
                    )
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Err(rusqlite::Error::SqliteFailure(e, None).into())
                }
                Err(query_err) => Err(query_err.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Горячая money-операция для group-commit (reserve/settle) — writer'а. Ссылки (не owned): вызывающий
/// (billing) держит команды, registry лишь применяет их SQL в ОДНОЙ транзакции.
pub enum HotOp<'a> {
    Reserve {
        account_id: &'a str,
        key: &'a str,
        hold: i64,
    },
    Settle {
        account_id: &'a str,
        key: &'a str,
        hold: i64,
        actual: i64,
        reference: Option<&'a str>,
        usage: Option<&'a UsageEventInput>,
    },
}

/// Shared post-reserve account floor for both registry backends. This deliberately mirrors
/// `metering::OVERDRAFT_NANO` without introducing an upward dependency from registry to metering.
pub const ACCOUNT_OVERDRAFT_NANO: i64 = 1_000_000_000;

/// Применить пачку reserve/settle в ОДНОЙ транзакции (group-commit): амортизирует стоимость коммита
/// под нагрузкой. Команды применяются ПОСЛЕДОВАТЕЛЬНО — атомарный reserve видит эффекты предыдущих
/// в этой же транзакции и каждый успешный post-balance остаётся не ниже общего account floor.
/// Возвращает результаты в порядке `ops` (индекс-в-индекс). Ошибка BEGIN/COMMIT → Err (вызывающий
/// откатывается на обработку по-одному). Per-op ошибки глушатся в None (как в прежнем writer'е:
/// `.ok().flatten()`).
pub fn apply_hot_batch(conn: &Connection, ops: &[HotOp]) -> Result<Vec<Option<i64>>> {
    let tx = conn.unchecked_transaction()?;
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        out.push(match op {
            HotOp::Reserve {
                account_id,
                key,
                hold,
            } => account_reserve_for_key(&tx, account_id, key, *hold)
                .ok()
                .flatten(),
            HotOp::Settle {
                account_id,
                key,
                hold,
                actual,
                reference,
                usage,
            } => account_settle_in(&tx, account_id, key, *hold, *actual, *reference, *usage)
                .ok()
                .flatten(),
        });
    }
    tx.commit()?;
    Ok(out)
}

/// АТОМАРНО зарезервировать `hold` по АККАУНТУ, если post-balance не пересекает общий overdraft
/// floor и аккаунт активен. Кошелёк — общий на профиль (все ключи юзера тратят из него).
pub fn account_reserve(conn: &Connection, id: &str, hold_nano: i64) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    match conn.query_row(
        "UPDATE accounts SET balance_nano = balance_nano - ?1, reserved_nano = reserved_nano + ?1 \
         WHERE id = ?2 AND status = 'active' AND balance_nano >= ?1 - ?3 RETURNING balance_nano",
        rusqlite::params![hold, id, ACCOUNT_OVERDRAFT_NANO],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(bal) => Ok(Some(bal)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Reserve against both the shared account balance and one key's lifetime spending policy.
pub fn account_reserve_for_key(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    conn.execute_batch("SAVEPOINT key_policy_reserve")?;
    let balance = match conn.query_row(
        "UPDATE accounts SET balance_nano=balance_nano-?1, reserved_nano=reserved_nano+?1 \
         WHERE id=?2 AND status='active' AND balance_nano>=?1-?3 RETURNING balance_nano",
        rusqlite::params![hold, id, ACCOUNT_OVERDRAFT_NANO],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve")?;
            return Ok(None);
        }
        Err(error) => {
            let _ =
                conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve");
            return Err(error.into());
        }
    };
    let updated = match conn.execute(
        "UPDATE api_keys SET reserved_nano=reserved_nano+?1 \
         WHERE key=?2 AND account_id=?3 AND COALESCE(status,'active')='active' \
           AND (expires_ts IS NULL OR expires_ts>CAST(strftime('%s','now') AS INTEGER)) \
           AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+?1<=spend_limit_nano)",
        rusqlite::params![hold, key, id],
    ) {
        Ok(value) => value,
        Err(error) => {
            let _ =
                conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve");
            return Err(error.into());
        }
    };
    if updated != 1 {
        conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve")?;
        return Ok(None);
    }
    conn.execute_batch("RELEASE key_policy_reserve")?;
    Ok(Some(balance))
}

/// Закрыть резерв аккаунта: баланс += hold − actual, spent += actual, reserved −= hold; per-key
/// `spent` += actual (атрибуция расхода по ключу); строка в ledger (charge). ВСЁ в ОДНОЙ транзакции.
pub fn account_settle(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    let tx = conn.unchecked_transaction()?;
    let bal = account_settle_in(&tx, id, key, hold_nano, actual_nano, reference, usage)?;
    tx.commit()?;
    Ok(bal)
}

/// SQL-тело settle БЕЗ BEGIN/COMMIT — для group-commit writer'а (несколько settle в одной транзакции).
/// Вызывающий обязан обернуть в транзакцию (`account_settle` — тонкая обёртка). `conn` может быть
/// `&Transaction` (Deref в `&Connection`). Семантика идентична `account_settle`.
pub fn account_settle_in(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    // Exact provider usage may exceed a conservative hold because of provider-added tokens or a
    // response-reported pricing modifier. Forward caps this at hold+$1, matching the account-level
    // overdraft floor; silently clamping here would turn delivered provider usage into lost revenue.
    let actual = actual_nano.max(0);
    // Возвращаем hold, но НЕ БОЛЬШЕ, чем реально числится в reserved_nano: MIN(hold, reserved).
    // Защита от двойного settle (перекрытие деплоя: reconcile уже вернул резерв, затем прилетел
    // settle старого инстанса) — иначе balance получил бы +hold дважды (over-credit) и reserved
    // ушёл бы в минус. MAX(0, …) держит reserved ≥ 0. В норме (reserved≥hold) поведение прежнее.
    let bal = match conn.query_row(
        "UPDATE accounts SET \
         balance_nano = balance_nano + MIN(?1, reserved_nano) - ?2, \
         spent_nano = spent_nano + ?2, \
         reserved_nano = MAX(0, reserved_nano - ?1) WHERE id = ?3 RETURNING balance_nano",
        rusqlite::params![hold, actual, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(b) => Some(b),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(b) = bal {
        conn.execute(
            "UPDATE api_keys SET spent_nano=spent_nano+?1, \
             reserved_nano=MAX(0,reserved_nano-?2) WHERE key=?3",
            rusqlite::params![actual, hold, key],
        )?;
        if actual > 0 {
            // Модель за списанием — из usage того же запроса (пустую строку не пишем → NULL).
            let model = usage.map(|u| u.model.as_str()).filter(|m| !m.is_empty());
            ledger_add(conn, id, Some(key), "charge", actual, reference, b, model)?;
            // usage_events (аналитика) — в ТОЙ ЖЕ транзакции, что и charge (экономим коммит на запрос).
            // Best-effort: ошибка вставки usage НЕ роняет money-коммит (аналитика не критична).
            if let Some(u) = usage {
                let _ = usage_event_add(conn, id, Some(key), u, actual, reference);
            }
        }
    }
    Ok(bal)
}

/// Read every per-provider discount of one account (control-plane listing).
pub fn account_provider_discounts(
    conn: &Connection,
    account_id: &str,
) -> Result<Vec<(String, i64)>> {
    let mut statement = conn.prepare(
        "SELECT provider_id, mult_bp FROM account_provider_discounts \
         WHERE account_id = ?1 ORDER BY provider_id",
    )?;
    let rows = statement.query_map(rusqlite::params![account_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut discounts = Vec::new();
    for row in rows {
        discounts.push(row?);
    }
    Ok(discounts)
}

/// Set (or replace) one provider discount. `mult_bp` is a payable multiplier in basis points:
/// 10000 is list price, 5000 is 50% off, 0 is free. Takes effect on the next request — there is
/// no version, no activation and no snapshot to keep in step.
pub fn set_account_provider_discount(
    conn: &Connection,
    account_id: &str,
    provider_id: &str,
    mult_bp: i64,
    ts: i64,
) -> Result<()> {
    ensure_valid_provider_discount(provider_id, mult_bp)?;
    let changed = conn.execute(
        "INSERT INTO account_provider_discounts(account_id, provider_id, mult_bp, updated_ts) \
         SELECT ?1, ?2, ?3, ?4 WHERE EXISTS(SELECT 1 FROM accounts WHERE id = ?1) \
         ON CONFLICT(account_id, provider_id) DO UPDATE SET \
           mult_bp = excluded.mult_bp, updated_ts = excluded.updated_ts",
        rusqlite::params![account_id, provider_id, mult_bp, ts],
    )?;
    if changed == 0 {
        bail!("unknown account");
    }
    Ok(())
}

/// Remove one provider discount; the account falls back to its default multiplier.
pub fn clear_account_provider_discount(
    conn: &Connection,
    account_id: &str,
    provider_id: &str,
) -> Result<bool> {
    let removed = conn.execute(
        "DELETE FROM account_provider_discounts WHERE account_id = ?1 AND provider_id = ?2",
        rusqlite::params![account_id, provider_id],
    )?;
    Ok(removed > 0)
}

/// Provider ids the engine actually prices, plus the bounds every discount must respect. A typo
/// here would silently never match a request, so the set is closed rather than free-form.
pub const DISCOUNT_PROVIDER_IDS: [&str; 5] = [
    PROVIDER_ANTHROPIC,
    PROVIDER_OPENAI,
    PROVIDER_GOOGLE,
    PROVIDER_KIMI,
    PROVIDER_GLM,
];

pub fn ensure_valid_provider_discount(provider_id: &str, mult_bp: i64) -> Result<()> {
    if !DISCOUNT_PROVIDER_IDS.contains(&provider_id) {
        bail!("unknown provider id");
    }
    if !(0..=10_000).contains(&mult_bp) {
        bail!("mult_bp must be between 0 and 10000");
    }
    Ok(())
}

/// Resolve the key, account and every provider override in one database snapshot. The left join
/// keeps an account with no overrides as one row, while the bounded provider table contributes at
/// most five rows; a pricing write is therefore visible atomically on the next authorization.
pub fn key_account(conn: &Connection, key: &str) -> Result<Option<KeyAuth>> {
    let mut statement = conn.prepare(
        "SELECT a.id, a.mult_bp, a.balance_nano, k.spent_nano, k.reserved_nano, \
         k.spend_limit_nano, k.expires_ts, \
         (COALESCE(k.status,'active')='active' AND COALESCE(a.status,'active')='active'), \
         d.provider_id, d.mult_bp \
         FROM api_keys k \
         JOIN accounts a ON a.id = k.account_id \
         LEFT JOIN account_provider_discounts d ON d.account_id = a.id \
         WHERE k.key = ?1 ORDER BY d.provider_id",
    )?;
    let mut rows = statement.query(rusqlite::params![key])?;
    let Some(first) = rows.next()? else {
        return Ok(None);
    };
    let account_id = first.get(0)?;
    let mult_bp = first.get(1)?;
    let balance_nano = first.get(2)?;
    let spent_nano = first.get(3)?;
    let reserved_nano = first.get(4)?;
    let spend_limit_nano = first.get(5)?;
    let expires_ts = first.get(6)?;
    let active = first.get::<_, i64>(7)? != 0;
    let mut provider_mult_bp = Vec::with_capacity(DISCOUNT_PROVIDER_IDS.len());
    match (
        first.get::<_, Option<String>>(8)?,
        first.get::<_, Option<i64>>(9)?,
    ) {
        (Some(provider), Some(multiplier)) => provider_mult_bp.push((provider, multiplier)),
        (None, None) => {}
        _ => bail!("provider discount row is structurally invalid"),
    }
    while let Some(row) = rows.next()? {
        match (
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<i64>>(9)?,
        ) {
            (Some(provider), Some(multiplier)) => provider_mult_bp.push((provider, multiplier)),
            (None, None) => {}
            _ => bail!("provider discount row is structurally invalid"),
        }
    }
    Ok(Some(KeyAuth {
        account_id,
        mult_bp,
        provider_mult_bp,
        balance_nano,
        spent_nano,
        reserved_nano,
        spend_limit_nano,
        expires_ts,
        active,
    }))
}

/// Консистентный ОНЛАЙН-бэкап всей БД в `out_path` через `VACUUM INTO` (best-practice для живого
/// SQLite): создаёт целостный снимок, безопасный при WAL и параллельной работе. НЕЛЬЗЯ просто
/// копировать `.db` — без `-wal`/`-shm` копия рассинхронизирована/битая. `out_path` должен НЕ
/// существовать (VACUUM INTO создаёт файл). Восстановление: остановить сервис, положить снимок на
/// место `subscriptions.db`, удалить `-wal`/`-shm`, запустить.
pub fn backup_to(conn: &Connection, out_path: &str) -> Result<()> {
    let esc = out_path.replace('\'', "''"); // путь наш, но экранируем кавычку
    conn.execute_batch(&format!("VACUUM INTO '{esc}'"))?;
    Ok(())
}

/// Свернуть WAL в основную БД и обрезать файл (TRUNCATE). Авто-checkpoint SQLite (порог ~1000 стр.)
/// обычно держит WAL в узде, но под НЕПРЕРЫВНОЙ записью + постоянными читателями (наш случай:
/// reserve/settle на каждом запросе + N read-соединений) чекпоинт может откладываться и WAL растёт.
/// Периодический явный TRUNCATE-чекпоинт держит файл ограниченным. PASSIVE не нужен — вызываем редко
/// из persist_loop; занятость нормальна, вернём Ok даже если часть страниц осталась (не критично).
pub fn wal_checkpoint(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

pub fn ledger_ack(
    conn: &Connection,
    consumer: &str,
    account_id: &str,
    last_ledger_id: i64,
) -> Result<usize> {
    if consumer.trim().is_empty() || last_ledger_id < 0 {
        anyhow::bail!("invalid ledger checkpoint");
    }
    Ok(conn.execute(
        "INSERT INTO ledger_consumer_checkpoints(consumer,account_id,last_ledger_id,updated_ts) \
         VALUES(?1,?2,?3,?4) ON CONFLICT(consumer,account_id) DO UPDATE SET \
         last_ledger_id=MAX(last_ledger_id,excluded.last_ledger_id),updated_ts=excluded.updated_ts",
        rusqlite::params![consumer, account_id, last_ledger_id, now()],
    )?)
}

/// Delete charge detail only after the required pricing consumer has durably acknowledged it.
/// Top-ups/adjustments remain as the long-term accounting record.
pub fn ledger_prune(conn: &Connection, older_than_ts: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM ledger WHERE id IN ( \
           SELECT l.id FROM ledger l JOIN ledger_consumer_checkpoints c \
             ON c.account_id=l.account_id AND c.consumer='pricing' \
           WHERE l.kind='charge' AND l.ts < ?1 AND l.id <= c.last_ledger_id \
           ORDER BY l.id LIMIT 5000 \
         )",
        rusqlite::params![older_than_ts],
    )?)
}

/// Добавить строку в append-only ledger (журнал движений баланса). `model` — Claude-модель за
/// charge-строкой (для точного per-model графика); у topup/adjust модели нет → None.
#[allow(clippy::too_many_arguments)]
fn ledger_add(
    conn: &Connection,
    account_id: &str,
    key: Option<&str>,
    kind: &str,
    amount_nano: i64,
    reference: Option<&str>,
    balance_after: i64,
    model: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts, model) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![account_id, key, kind, amount_nano, reference, balance_after, now(), model])?;
    Ok(())
}

/// Traffic predates provider attribution, or was queued by an engine release that only served
/// Claude. Either way the Claude fleet is the only upstream it could have used.
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
/// The OpenAI-compatible Codex home pool.
pub const PROVIDER_OPENAI: &str = "openai";
/// The isolated native Gemini-compatible subscription pool.
pub const PROVIDER_GOOGLE: &str = "google";
/// Backend-only Kimi Code subscription pool. It shares the Anthropic public wire but keeps a
/// distinct settlement attribution so Claude and KIMI economics can never be blended.
pub const PROVIDER_KIMI: &str = "kimi";
/// Backend-only GLM (Zhipu AI / Z.ai) Coding Plan subscription pool. It shares the Anthropic
/// public wire but keeps a distinct settlement attribution and dual-ledger calibration so GLM
/// economics can never be blended with any other provider.
pub const PROVIDER_GLM: &str = "glm";

/// Fleets whose membership arrives as a sealed Auth Bot roster the engine may read but never
/// write. Only these can carry an operator disable (`pool_member_disables`): Claude subscriptions
/// live in this authority already and use their own `active|paused|disabled` status, so admitting
/// them here would give one subscription two competing routability switches.
pub const ROSTER_BACKED_PROVIDERS: &[&str] =
    &[PROVIDER_GOOGLE, PROVIDER_OPENAI, PROVIDER_KIMI, PROVIDER_GLM];

/// Fail closed on anything outside the fixed roster-backed plane. The DB CHECK enforces the same
/// set, but rejecting here keeps a typo from reaching PostgreSQL as a constraint violation the
/// caller would have to interpret.
pub fn require_roster_backed_provider(provider: &str) -> Result<()> {
    if !ROSTER_BACKED_PROVIDERS.contains(&provider) {
        anyhow::bail!("pool member provider is outside the roster-backed plane: {provider}");
    }
    Ok(())
}

fn default_provider() -> String {
    PROVIDER_ANTHROPIC.to_string()
}

/// Разбивка одного оплаченного запроса по корзинам токенов + модель (owned — переживает канал
/// биллинг-актора). `real_nano` — стоимость по официальным ценам (×1.0, до наценки).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct UsageEventInput {
    pub model: String,
    /// Upstream that served the request: `anthropic` for the Claude fleet, `openai` for the
    /// Codex home pool. Defaulted on deserialization so settlement rows queued by a previous
    /// engine release stay readable across a blue-green promotion.
    #[serde(default = "default_provider")]
    pub provider: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub web_search_requests: i64,
    /// Official price of the full turn the provider actually produced.
    pub real_nano: i64,
    /// Official price of the slice the customer is billed for. Equal to `real_nano` everywhere
    /// except a turn the customer capped with `max_tokens`: the transport cannot stop generation,
    /// so the provider may overshoot, and the customer is charged only up to the ceiling it asked
    /// for — exactly what the emulated API would bill. Keeping both figures is what lets the
    /// ledger's multiplier invariant hold on the billed basis while the absorbed overage stays
    /// visible as the difference between the two.
    #[serde(default)]
    pub charge_basis_nano: i64,
    pub speed: String,
    pub inference_geo: String,
    pub input_nano: i64,
    pub output_nano: i64,
    pub cache_read_nano: i64,
    pub cache_write_5m_nano: i64,
    pub cache_write_1h_nano: i64,
    pub web_search_nano: i64,
    pub priced_ts: i64,
}

/// Create one durable SQLite reservation. Exact active replays return the original post-reserve
/// balance; a reused request ID with different parameters or a terminal request fails closed.
pub fn sqlite_reserve_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    lease_secs: i64,
) -> Result<Option<i64>> {
    sqlite_reserve_request_for_execution(
        conn,
        request_id,
        account_id,
        key,
        hold_nano,
        lease_secs,
        &ExecutionAttempt::direct(),
    )
}

/// Group-aware reservation primitive used by provider-plane runtime admissions. Existing callers
/// stay direct through `sqlite_reserve_request`, while exact replay also fences the immutable
/// execution group and one-based attempt.
#[allow(clippy::too_many_arguments)]
pub fn sqlite_maintenance_prune(
    conn: &Connection,
    older_than_ts: i64,
) -> Result<crate::pg::MaintenanceReport> {
    pricing::validate_request_lifecycle_prune_cutoff(older_than_ts, now())?;
    let tx = conn.unchecked_transaction()?;
    let outbox = tx.execute(
        "DELETE FROM billing_settlement_outbox WHERE request_id IN ( \
           SELECT request_id FROM billing_settlement_outbox WHERE state='done' AND committed_ts<?1 \
           ORDER BY committed_ts,request_id LIMIT 5000)",
        rusqlite::params![older_than_ts],
    )?;
    let reservations = tx.execute(
        "DELETE FROM billing_reservations WHERE request_id IN ( \
           SELECT request_id FROM billing_reservations
            WHERE state IN ('settled','canceled') AND settled_ts<?1 \
             AND request_id NOT IN (SELECT request_id FROM billing_settlement_outbox) \
           ORDER BY settled_ts,request_id LIMIT 5000)",
        rusqlite::params![older_than_ts],
    )?;
    tx.execute(
        "DELETE FROM execution_group_winner AS winner
          WHERE NOT EXISTS (
            SELECT 1 FROM billing_reservations reservation
             WHERE COALESCE(reservation.group_id,reservation.request_id)=winner.group_id
          )",
        [],
    )?;
    tx.commit()?;
    Ok(crate::pg::MaintenanceReport {
        outbox,
        reservations,
        ..Default::default()
    })
}

#[allow(clippy::too_many_arguments)]
fn sqlite_enqueue_settlement(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
    disposition: &str,
) -> Result<Option<i64>> {
    if !matches!(disposition, "settle" | "cancel" | "reconcile_full_hold") {
        anyhow::bail!("invalid SQLite settlement disposition");
    }
    let usage_json = usage.map(serde_json::to_string).transpose()?;
    let tx = conn.unchecked_transaction()?;
    let reservation = tx.query_row(
        "SELECT account_id,key,hold_nano,state,actual_nano,balance_after_settle_nano,reference \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        },
    );
    let (stored_account, stored_key, stored_hold, state, stored_actual, stored_balance, stored_ref) =
        match reservation {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                anyhow::bail!("settlement has no durable reservation")
            }
            Err(error) => return Err(error.into()),
        };
    if stored_account != account_id || stored_key != key || stored_hold != hold_nano {
        anyhow::bail!("settlement parameters do not match reservation");
    }
    let actual = actual_nano.max(0);
    if state == "settled" {
        let expected_actual = sqlite_terminal_expected_actual(&tx, request_id, actual)?;
        if stored_actual == Some(expected_actual) && stored_ref.as_deref() == reference {
            tx.commit()?;
            return Ok(stored_balance);
        }
        anyhow::bail!("settlement request ID was reused with different parameters");
    }

    let timestamp = now();
    let inserted = tx.execute(
        "INSERT INTO billing_settlement_outbox( \
           request_id,actual_nano,reference,usage_json,disposition,state,attempts,next_attempt_ts,
           created_ts,updated_ts) \
         VALUES(?1,?2,?3,?4,?5,'pending',0,0,?6,?6) ON CONFLICT(request_id) DO NOTHING",
        rusqlite::params![
            request_id,
            actual,
            reference,
            usage_json,
            disposition,
            timestamp
        ],
    )?;
    if inserted == 0 {
        let existing = tx.query_row(
            "SELECT actual_nano,reference,usage_json,disposition
               FROM billing_settlement_outbox WHERE request_id=?1",
            rusqlite::params![request_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        if existing
            != (
                actual,
                reference.map(str::to_owned),
                usage_json,
                disposition.to_owned(),
            )
        {
            anyhow::bail!("settlement request ID was reused with different parameters");
        }
    }
    tx.commit()?;
    Ok(None)
}

/// Apply one already-durable SQLite settlement intent atomically with its ledger/usage rows.
pub fn sqlite_process_settlement(conn: &Connection, request_id: &str) -> Result<Option<i64>> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let outbox = tx.query_row(
        "SELECT actual_nano,reference,usage_json,state,disposition
           FROM billing_settlement_outbox WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        },
    );
    let (actual, reference, usage_json, outbox_state, disposition) = match outbox {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => anyhow::bail!("settlement outbox row missing"),
        Err(error) => return Err(error.into()),
    };
    let reservation = tx.query_row(
        "SELECT account_id,key,hold_nano,state,actual_nano,balance_after_settle_nano, \
                COALESCE(group_id,request_id),attempt \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i32>(7)?,
            ))
        },
    )?;
    if matches!(reservation.3.as_str(), "settled" | "canceled") || outbox_state == "done" {
        let expected_actual = sqlite_terminal_expected_actual(&tx, request_id, actual)?;
        if reservation.4 != Some(expected_actual) {
            anyhow::bail!("stored settlement differs from outbox");
        }
        tx.execute(
            "UPDATE billing_settlement_outbox SET state='done',committed_ts=COALESCE(committed_ts,?2), \
             updated_ts=?2,last_error=NULL WHERE request_id=?1",
            rusqlite::params![request_id, now()],
        )?;
        tx.commit()?;
        return Ok(reservation.5);
    }
    if reservation.3 != "reserved" && reservation.3 != "delivering" {
        anyhow::bail!("reservation is not settleable");
    }
    let usage = usage_json
        .as_deref()
        .map(serde_json::from_str::<UsageEventInput>)
        .transpose()?;
    let mut losing_attempt: Option<(String, String, i32)> = None;
    let effective_actual = if actual > 0 {
        tx.execute(
            "INSERT INTO execution_group_winner(group_id,winner_request_id,decided_at)
             VALUES(?1,?2,?3) ON CONFLICT(group_id) DO NOTHING",
            rusqlite::params![reservation.6, request_id, now()],
        )?;
        let winner: String = tx.query_row(
            "SELECT winner_request_id FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![reservation.6],
            |row| row.get(0),
        )?;
        if winner == request_id {
            actual
        } else {
            losing_attempt = Some((reservation.6.clone(), winner, reservation.7));
            0
        }
    } else {
        0
    };
    let effective_usage = losing_attempt.is_none().then_some(usage.as_ref()).flatten();
    let effective_disposition = if losing_attempt.is_some() {
        "cancel"
    } else {
        disposition.as_str()
    };
    let balance = {
        account_settle_in(
            &tx,
            &reservation.0,
            &reservation.1,
            reservation.2,
            effective_actual,
            reference.as_deref(),
            effective_usage,
        )?
        .ok_or_else(|| anyhow::anyhow!("settlement account no longer exists"))?
    };
    let timestamp = now();
    let final_state = if effective_disposition == "cancel" {
        "canceled"
    } else {
        "settled"
    };
    tx.execute(
        "UPDATE billing_reservations SET state=?2,actual_nano=?3, \
         balance_after_settle_nano=?4,reference=?5,updated_ts=?6,settled_ts=?6 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![
            request_id,
            final_state,
            effective_actual,
            balance,
            reference,
            timestamp
        ],
    )?;
    tx.execute(
        "UPDATE billing_settlement_outbox SET state='done',attempts=attempts+1, \
         updated_ts=?2,committed_ts=?2,last_error=NULL WHERE request_id=?1",
        rusqlite::params![request_id, timestamp],
    )?;
    tx.commit()?;
    if let Some((group_id, winner_request_id, attempt)) = losing_attempt {
        record_execution_group_loser(&group_id, &winner_request_id, request_id, attempt);
    }
    Ok(Some(balance))
}

pub fn sqlite_reserve_request_for_execution(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    lease_secs: i64,
    execution: &ExecutionAttempt,
) -> Result<Option<i64>> {
    if request_id.trim().is_empty()
        || account_id.trim().is_empty()
        || key.trim().is_empty()
        || hold_nano < 0
        || lease_secs <= 0
    {
        anyhow::bail!("invalid SQLite reservation parameters");
    }
    let tx = conn.unchecked_transaction()?;
    let existing = tx.query_row(
        "SELECT account_id,key,hold_nano,state,balance_after_reserve_nano,group_id,attempt \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i32>(6)?,
            ))
        },
    );
    match existing {
        Ok((stored_account, stored_key, stored_hold, state, balance, group_id, attempt)) => {
            if stored_account != account_id
                || stored_key != key
                || stored_hold != hold_nano
                || group_id.as_deref() != execution.group_id()
                || attempt != execution.attempt()
            {
                anyhow::bail!("reservation request ID was reused with different parameters");
            }
            if state == "reserved" || state == "delivering" {
                tx.commit()?;
                return Ok(Some(balance));
            }
            anyhow::bail!("reservation request is already terminal");
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        Err(error) => return Err(error.into()),
    }

    let Some(balance) = account_reserve_for_key(&tx, account_id, key, hold_nano)? else {
        tx.rollback()?;
        return Ok(None);
    };
    let timestamp = now();
    tx.execute(
        "INSERT INTO billing_reservations( \
           request_id,account_id,key,hold_nano,group_id,attempt,state,balance_after_reserve_nano, \
           lease_until,created_ts,updated_ts) \
         VALUES(?1,?2,?3,?4,?5,?6,'reserved',?7,?8,?9,?9)",
        rusqlite::params![
            request_id,
            account_id,
            key,
            hold_nano,
            execution.group_id(),
            execution.attempt(),
            balance,
            timestamp.saturating_add(lease_secs),
            timestamp
        ],
    )?;
    tx.commit()?;
    Ok(Some(balance))
}










/// Mark a reservation as provider-accepted before handing its response body to the client.
pub fn sqlite_mark_delivering(
    conn: &Connection,
    request_id: &str,
    lease_secs: i64,
) -> Result<bool> {
    if lease_secs <= 0 {
        anyhow::bail!("invalid delivery lease");
    }
    let timestamp = now();
    let changed = conn.execute(
        "UPDATE billing_reservations SET state='delivering',lease_until=?2,updated_ts=?3 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![request_id, timestamp.saturating_add(lease_secs), timestamp],
    )?;
    Ok(changed == 1)
}

pub fn sqlite_renew_reservation_lease(
    conn: &Connection,
    request_id: &str,
    lease_secs: i64,
) -> Result<bool> {
    if lease_secs <= 0 {
        anyhow::bail!("invalid reservation lease");
    }
    let timestamp = now();
    Ok(conn.execute(
        "UPDATE billing_reservations SET lease_until=?2,updated_ts=?3 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![request_id, timestamp.saturating_add(lease_secs), timestamp],
    )? == 1)
}






fn sqlite_terminal_expected_actual(
    conn: &Connection,
    request_id: &str,
    original_actual: i64,
) -> Result<i64> {
    if original_actual <= 0 {
        return Ok(original_actual.max(0));
    }
    conn.query_row(
        "SELECT CASE
           WHEN winner.winner_request_id IS NOT NULL AND winner.winner_request_id<>reservation.request_id
             THEN 0
           ELSE ?2
         END
           FROM billing_reservations reservation
           LEFT JOIN execution_group_winner winner
             ON winner.group_id=COALESCE(reservation.group_id,reservation.request_id)
          WHERE reservation.request_id=?1",
        rusqlite::params![request_id, original_actual],
        |row| row.get(0),
    )
    .context("terminal SQLite reservation is missing")
}




#[allow(clippy::too_many_arguments)]
pub fn sqlite_settle_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    if let Some(balance) = sqlite_enqueue_settlement(
        conn,
        request_id,
        account_id,
        key,
        hold_nano,
        actual_nano,
        reference,
        usage,
        "settle",
    )? {
        return Ok(Some(balance));
    }
    match sqlite_process_settlement(conn, request_id) {
        Ok(result) => Ok(result),
        Err(error) => {
            let message: String = format!("{error:#}").chars().take(1000).collect();
            let timestamp = now();
            let _ = conn.execute(
                "UPDATE billing_settlement_outbox SET attempts=attempts+1,last_error=?2, \
                 updated_ts=?3,next_attempt_ts=?3+MIN(60,MAX(1,attempts+1)) WHERE request_id=?1",
                rusqlite::params![request_id, message, timestamp],
            );
            Err(error)
        }
    }
}

/// Persist and apply an explicit cancellation. Strict policy reservations distinguish this from a
/// zero-value usage settlement so the immutable snapshot and funding allocations can be validated
/// and returned to their original buckets without weakening the settlement contract.
#[allow(clippy::too_many_arguments)]
pub fn sqlite_cancel_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
) -> Result<Option<i64>> {
    if let Some(balance) = sqlite_enqueue_settlement(
        conn, request_id, account_id, key, hold_nano, 0, None, None, "cancel",
    )? {
        return Ok(Some(balance));
    }
    match sqlite_process_settlement(conn, request_id) {
        Ok(result) => Ok(result),
        Err(error) => {
            let message: String = format!("{error:#}").chars().take(1000).collect();
            let timestamp = now();
            let _ = conn.execute(
                "UPDATE billing_settlement_outbox SET attempts=attempts+1,last_error=?2, \
                 updated_ts=?3,next_attempt_ts=?3+MIN(60,MAX(1,attempts+1)) WHERE request_id=?1",
                rusqlite::params![request_id, message, timestamp],
            );
            Err(error)
        }
    }
}

/// Retry persisted intents, then reconcile expired holds. Reserved requests are canceled; requests
/// marked delivering are charged their approved hold when exact usage never arrived.
pub fn sqlite_reconcile_expired(
    conn: &Connection,
    limit: usize,
    charge_hold_on_unknown_usage: bool,
) -> Result<crate::pg::ReconcileReport> {
    let limit = limit.clamp(1, 10_000) as i64;
    let timestamp = now();
    let pending: Vec<String> = {
        let mut statement = conn.prepare(
            "SELECT request_id FROM billing_settlement_outbox \
             WHERE state='pending' AND next_attempt_ts<=?1 ORDER BY created_ts LIMIT ?2",
        )?;
        let rows = statement
            .query_map(rusqlite::params![timestamp, limit], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    let mut report = crate::pg::ReconcileReport::default();
    for request_id in pending {
        if sqlite_process_settlement(conn, &request_id).is_ok() {
            report.processed_outbox += 1;
        }
    }

    let remaining = (limit as usize).saturating_sub(report.processed_outbox) as i64;
    if remaining == 0 {
        return Ok(report);
    }
    let expired: Vec<(String, String, String, i64, String)> = {
        let mut statement = conn.prepare(
            "SELECT request_id,account_id,key,hold_nano,state FROM billing_reservations \
             WHERE state IN ('reserved','delivering') AND lease_until<=?1 \
             ORDER BY lease_until LIMIT ?2",
        )?;
        let rows = statement
            .query_map(rusqlite::params![timestamp, remaining], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    for (request_id, account_id, key, hold, state) in expired {
        let billed = state == "delivering" && charge_hold_on_unknown_usage;
        let actual = if billed { hold } else { 0 };
        // The disposition names the recovery path, not the amount, and is an allow-listed value
        // downstream; only the settled sum changes with the policy.
        let disposition = if state == "delivering" {
            "reconcile_full_hold"
        } else {
            "cancel"
        };
        let reference = if state == "delivering" {
            "lease-expired-delivering"
        } else {
            "lease-expired-reserved"
        };
        let result = sqlite_enqueue_settlement(
            conn,
            &request_id,
            &account_id,
            &key,
            hold,
            actual,
            Some(reference),
            None,
            disposition,
        )
        .and_then(|balance| match balance {
            Some(balance) => Ok(Some(balance)),
            None => sqlite_process_settlement(conn, &request_id),
        });
        match result {
            Ok(_) if state == "delivering" => report.charged_after_delivery += 1,
            Ok(_) => report.canceled_before_delivery += 1,
            Err(error) => {
                elog::error("registry", format!("SQLite reservation recovery failed for {request_id}: {error:#}"))
            }
        }
    }
    Ok(report)
}


/// Записать usage-событие (аналитика; НЕ money-строка). Вызывается billing-writer'ом сразу после
/// `account_settle` на той же connection. `charge_nano` — фактически списанное (после наценки).
pub fn usage_event_add(
    conn: &Connection,
    account_id: &str,
    key: Option<&str>,
    u: &UsageEventInput,
    charge_nano: i64,
    reference: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_events(account_id, key, model, input_tokens, output_tokens, \
         cache_read_tokens, cache_write_5m_tokens, cache_write_1h_tokens, web_search_requests, \
         real_nano, charge_nano, ref, ts, speed, inference_geo, input_nano, output_nano, \
         cache_read_nano, cache_write_5m_nano, cache_write_1h_nano, web_search_nano, priced_ts, \
         provider) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        rusqlite::params![
            account_id,
            key,
            u.model,
            u.input_tokens,
            u.output_tokens,
            u.cache_read_tokens,
            u.cache_write_5m_tokens,
            u.cache_write_1h_tokens,
            u.web_search_requests,
            u.real_nano,
            charge_nano,
            reference,
            now(),
            u.speed,
            u.inference_geo,
            u.input_nano,
            u.output_nano,
            u.cache_read_nano,
            u.cache_write_5m_nano,
            u.cache_write_1h_nano,
            u.web_search_nano,
            u.priced_ts,
            u.provider
        ],
    )?;
    Ok(())
}

/// Агрегат usage по модели за окно. Суммы токенов по корзинам + immutable real/charge nano
/// + число тарифицируемых событий.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageModelAgg {
    pub model: String,
    pub provider: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub web_search_requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
    pub input_nano: i64,
    pub output_nano: i64,
    pub cache_read_nano: i64,
    pub cache_write_5m_nano: i64,
    pub cache_write_1h_nano: i64,
    pub web_search_nano: i64,
}

/// Точный дневной срез того же usage-окна. `day_ts` — начало UTC-дня в unix-секундах.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageDailyAgg {
    pub day_ts: i64,
    pub requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
}

/// Точный дневной срез по фактически обслужившему API-плану. Имя модели намеренно
/// не участвует: один и тот же model ID может маршрутизироваться разными провайдерами.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageDailyProviderAgg {
    pub day_ts: i64,
    pub provider: String,
    pub requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
}

/// Точный per-key срез usage-окна. Полный ключ остаётся внутри engine-процесса и маскируется
/// HTTP-слоем до ответа control API.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageKeyAgg {
    pub key: Option<String>,
    pub requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageReport {
    pub models: Vec<UsageModelAgg>,
    pub daily: Vec<UsageDailyAgg>,
    pub daily_providers: Vec<UsageDailyProviderAgg>,
    pub keys: Vec<UsageKeyAgg>,
}

fn usage_by_model_between(
    conn: &Connection,
    account_id: &str,
    since_ts: i64,
    until_ts: i64,
) -> Result<Vec<UsageModelAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(model,''), COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*), \
         COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), \
         COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_5m_tokens),0), \
         COALESCE(SUM(cache_write_1h_tokens),0), COALESCE(SUM(web_search_requests),0), \
         COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0), \
         COALESCE(SUM(input_nano),0), COALESCE(SUM(output_nano),0), \
         COALESCE(SUM(cache_read_nano),0), COALESCE(SUM(cache_write_5m_nano),0), \
         COALESCE(SUM(cache_write_1h_nano),0), COALESCE(SUM(web_search_nano),0) \
         FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
         GROUP BY model, COALESCE(NULLIF(provider,''),'anthropic') ORDER BY SUM(real_nano) DESC, model, COALESCE(NULLIF(provider,''),'anthropic')",
    )?;
    let rows = stmt.query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
        Ok(UsageModelAgg {
            model: r.get(0)?,
            provider: r.get(1)?,
            requests: r.get(2)?,
            input_tokens: r.get(3)?,
            output_tokens: r.get(4)?,
            cache_read_tokens: r.get(5)?,
            cache_write_5m_tokens: r.get(6)?,
            cache_write_1h_tokens: r.get(7)?,
            web_search_requests: r.get(8)?,
            real_nano: r.get(9)?,
            charge_nano: r.get(10)?,
            input_nano: r.get(11)?,
            output_nano: r.get(12)?,
            cache_read_nano: r.get(13)?,
            cache_write_5m_nano: r.get(14)?,
            cache_write_1h_nano: r.get(15)?,
            web_search_nano: r.get(16)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn usage_by_model(
    conn: &Connection,
    account_id: &str,
    since_ts: i64,
) -> Result<Vec<UsageModelAgg>> {
    usage_by_model_between(conn, account_id, since_ts, i64::MAX)
}

/// Один согласованный usage-отчёт на полуинтервале `[since_ts, until_ts)`. Все три среза
/// читаются из одного snapshot, поэтому параллельный settle не может попасть только в часть отчёта.
pub fn usage_report(
    conn: &Connection,
    account_id: &str,
    since_ts: i64,
    until_ts: i64,
) -> Result<UsageReport> {
    if until_ts <= since_ts {
        return Ok(UsageReport::default());
    }
    let transaction = conn.unchecked_transaction()?;
    let models = usage_by_model_between(&transaction, account_id, since_ts, until_ts)?;
    let daily = {
        let mut stmt = transaction.prepare(
            "SELECT (ts / 86400) * 86400 AS day_ts, COUNT(*), \
             COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0) \
             FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
             GROUP BY day_ts ORDER BY day_ts",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
                Ok(UsageDailyAgg {
                    day_ts: r.get(0)?,
                    requests: r.get(1)?,
                    real_nano: r.get(2)?,
                    charge_nano: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let daily_providers = {
        let mut stmt = transaction.prepare(
            "SELECT (ts / 86400) * 86400 AS day_ts, COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*), \
             COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0) \
             FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
             GROUP BY day_ts, COALESCE(NULLIF(provider,''),'anthropic') ORDER BY day_ts, COALESCE(NULLIF(provider,''),'anthropic')",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
                Ok(UsageDailyProviderAgg {
                    day_ts: r.get(0)?,
                    provider: r.get(1)?,
                    requests: r.get(2)?,
                    real_nano: r.get(3)?,
                    charge_nano: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let keys = {
        let mut stmt = transaction.prepare(
            "SELECT key, COUNT(*), COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0) \
             FROM usage_events WHERE account_id=?1 AND ts>=?2 AND ts<?3 \
             GROUP BY key ORDER BY SUM(real_nano) DESC, key",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![account_id, since_ts, until_ts], |r| {
                Ok(UsageKeyAgg {
                    key: r.get(0)?,
                    requests: r.get(1)?,
                    real_nano: r.get(2)?,
                    charge_nano: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    transaction.commit()?;
    Ok(UsageReport {
        models,
        daily,
        daily_providers,
        keys,
    })
}

/// Агрегат расхода ПО АККАУНТАМ за окно (ts ≥ `since_ts`): списано клиенту (charge) +
/// real-API стоимость + число запросов. Для панели «кто тратит» (сегодня/7д/30д).
#[derive(Debug, Clone, Default)]
pub struct SpendAccountAgg {
    pub account_id: String,
    pub handle: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
    pub last_ts: i64,
}

pub fn spend_by_account(
    conn: &Connection,
    since_ts: i64,
    limit: i64,
) -> Result<Vec<SpendAccountAgg>> {
    spend_by_account_range(conn, since_ts, i64::MAX, limit)
}

/// То же с явной верхней границей: полуоткрытое окно `since_ts ≤ ts < until_ts` (стыкующиеся
/// диапазоны не задваивают события). Для произвольного диапазона панели (/spend-stats?from&to).
pub fn spend_by_account_range(
    conn: &Connection,
    since_ts: i64,
    until_ts: i64,
    limit: i64,
) -> Result<Vec<SpendAccountAgg>> {
    let mut stmt = conn.prepare(
        "SELECT u.account_id, COALESCE(a.handle,''), COUNT(*), \
         COALESCE(SUM(u.charge_nano),0), COALESCE(SUM(u.real_nano),0), COALESCE(MAX(u.ts),0) \
         FROM usage_events u LEFT JOIN accounts a ON a.id=u.account_id \
         WHERE u.ts>=?1 AND u.ts<?2 GROUP BY u.account_id, a.handle \
         ORDER BY SUM(u.charge_nano) DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts, until_ts, limit], |r| {
        Ok(SpendAccountAgg {
            account_id: r.get(0)?,
            handle: r.get(1)?,
            requests: r.get(2)?,
            charge_nano: r.get(3)?,
            real_nano: r.get(4)?,
            last_ts: r.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Расход по ПРОВАЙДЕРУ за окно (ts ≥ `since_ts`). Claude-флот и Codex-пул сеттлятся в одни и те же
/// денежные таблицы, поэтому «сколько заработал каждый апстрим» читается только из явной колонки.
#[derive(Debug, Clone, Default)]
pub struct SpendProviderAgg {
    pub provider: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
}

pub fn spend_by_provider(conn: &Connection, since_ts: i64) -> Result<Vec<SpendProviderAgg>> {
    spend_by_provider_range(conn, since_ts, i64::MAX)
}

/// То же с явной верхней границей окна: `since_ts ≤ ts < until_ts` — см. spend_by_account_range.
pub fn spend_by_provider_range(
    conn: &Connection,
    since_ts: i64,
    until_ts: i64,
) -> Result<Vec<SpendProviderAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*), \
         COALESCE(SUM(charge_nano),0), COALESCE(SUM(real_nano),0) \
         FROM usage_events WHERE ts>=?1 AND ts<?2 GROUP BY 1 ORDER BY SUM(charge_nano) DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts, until_ts], |r| {
        Ok(SpendProviderAgg {
            provider: r.get(0)?,
            requests: r.get(1)?,
            charge_nano: r.get(2)?,
            real_nano: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Расход по МОДЕЛИ за окно (ts ≥ `since_ts`): top-`limit` по charge. Группировка по
/// (model, provider): один и тот же model ID может обслуживаться разными апстримами (см.
/// UsageDailyProviderAgg). `model` в usage_events — served id из ответа апстрима, по которому
/// реально посчитан charge (фолбэк — модель запроса), то есть разбивка отражает прайсинг,
/// а не клиентский алиас.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpendModelAgg {
    pub model: String,
    pub provider: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
}

pub fn spend_by_model(conn: &Connection, since_ts: i64, limit: i64) -> Result<Vec<SpendModelAgg>> {
    spend_by_model_range(conn, since_ts, i64::MAX, limit)
}

/// То же с явной верхней границей окна: `since_ts ≤ ts < until_ts` — см. spend_by_account_range.
pub fn spend_by_model_range(
    conn: &Connection,
    since_ts: i64,
    until_ts: i64,
    limit: i64,
) -> Result<Vec<SpendModelAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(NULLIF(model,''),'(unknown)'), COALESCE(NULLIF(provider,''),'anthropic'), \
         COUNT(*), COALESCE(SUM(charge_nano),0), COALESCE(SUM(real_nano),0) \
         FROM usage_events WHERE ts>=?1 AND ts<?2 GROUP BY 1,2 ORDER BY SUM(charge_nano) DESC, 1, 2 \
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts, until_ts, limit], |r| {
        Ok(SpendModelAgg {
            model: r.get(0)?,
            provider: r.get(1)?,
            requests: r.get(2)?,
            charge_nano: r.get(3)?,
            real_nano: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Одна failed-строка settlement_outbox для операционной диагностики. `last_error` обрезан до
/// 200 символов: тексты ошибок settle — внутренние invariant/SQLSTATE детали (request_id, суммы,
/// имена constraint'ов), токенов подписок и ключей там нет, но длинный PG-trace не должен
/// раздувать ответ панели.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettlementFailure {
    pub request_id: String,
    pub actual_nano: i64,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub updated_ts: i64,
}

/// Лаг durable-консьюмера ledger'а: max(ledger.id) против watermark'ов
/// `ledger_consumer_checkpoints` + возраст старейшей неподтверждённой строки. Растущий `unacked`
/// означает, что коммерческий pricing-воркер не дочитывает списания (и ledger_prune остановлен).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerConsumerLag {
    pub consumer: String,
    pub ledger_max_id: i64,
    /// Число (consumer, account_id) watermark'ов; 0 → консьюмер ни разу не подтверждал.
    pub checkpoints: i64,
    /// Минимальный last_ledger_id среди watermark'ов (0, когда checkpoint'ов нет).
    pub checkpoint_min: i64,
    /// Ledger-строки с id > watermark'а своего аккаунта.
    pub unacked: i64,
    /// ts старейшей неподтверждённой строки (0 — лага нет).
    pub oldest_unacked_ts: i64,
}

/// Сводка settlement pipeline для панели «тихие деньги»: counts по state, failed всего и за 24ч,
/// backlog несеттленых старше порога, последние ≤10 failed, лаг pricing-консьюмера. Читается
/// одинаково на обоих backend'ах: у SQLite-зеркала state 'failed' нет (застревшие ретраи видны
/// как `pending_with_error`), PostgreSQL паркует permanent-ошибки в 'failed' (миграция 0004);
/// state 'processing' объявлен в схеме, но пока не пишется ни одним writer'ом.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettlementHealth {
    pub pending: i64,
    pub processing: i64,
    pub done: i64,
    pub failed: i64,
    pub failed_24h: i64,
    /// pending-строки с last_error — ретраи в полёте (единственный сигнал застревания на SQLite).
    pub pending_with_error: i64,
    /// Несеттленые (pending|processing), созданные раньше `backlog_before`.
    pub backlog: i64,
    /// created_ts старейшей несеттленой строки (0 — несеттленых нет).
    pub oldest_unsettled_ts: i64,
    pub recent_failed: Vec<SettlementFailure>,
    pub ledger_consumer: LedgerConsumerLag,
}

fn settlement_consumer_lag(conn: &Connection, consumer: &str) -> Result<LedgerConsumerLag> {
    let ledger_max_id: i64 =
        conn.query_row("SELECT COALESCE(MAX(id),0) FROM ledger", [], |r| r.get(0))?;
    let (checkpoints, checkpoint_min): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(MIN(last_ledger_id),0) \
         FROM ledger_consumer_checkpoints WHERE consumer=?1",
        rusqlite::params![consumer],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (unacked, oldest_unacked_ts): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(MIN(l.ts),0) FROM ledger l \
         JOIN ledger_consumer_checkpoints c ON c.account_id=l.account_id AND c.consumer=?1 \
         WHERE l.id > c.last_ledger_id",
        rusqlite::params![consumer],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(LedgerConsumerLag {
        consumer: consumer.to_string(),
        ledger_max_id,
        checkpoints,
        checkpoint_min,
        unacked,
        oldest_unacked_ts,
    })
}

pub fn settlement_health(
    conn: &Connection,
    backlog_secs: i64,
    consumer: &str,
) -> Result<SettlementHealth> {
    let ts = now();
    let backlog_before = ts - backlog_secs.max(0);
    let failed_since = ts - 86_400;
    let mut health = SettlementHealth::default();
    {
        let mut stmt =
            conn.prepare("SELECT state, COUNT(*) FROM billing_settlement_outbox GROUP BY state")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (state, count) = row?;
            match state.as_str() {
                "pending" => health.pending = count,
                "processing" => health.processing = count,
                "done" => health.done = count,
                "failed" => health.failed = count,
                _ => {}
            }
        }
    }
    health.failed_24h = conn.query_row(
        "SELECT COUNT(*) FROM billing_settlement_outbox WHERE state='failed' AND updated_ts>=?1",
        rusqlite::params![failed_since],
        |r| r.get(0),
    )?;
    health.pending_with_error = conn.query_row(
        "SELECT COUNT(*) FROM billing_settlement_outbox \
         WHERE state='pending' AND last_error IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    health.backlog = conn.query_row(
        "SELECT COUNT(*) FROM billing_settlement_outbox \
         WHERE state IN ('pending','processing') AND created_ts<?1",
        rusqlite::params![backlog_before],
        |r| r.get(0),
    )?;
    health.oldest_unsettled_ts = conn.query_row(
        "SELECT COALESCE(MIN(created_ts),0) FROM billing_settlement_outbox \
         WHERE state IN ('pending','processing')",
        [],
        |r| r.get(0),
    )?;
    {
        let mut stmt = conn.prepare(
            "SELECT request_id, actual_nano, attempts, last_error, updated_ts \
             FROM billing_settlement_outbox WHERE state='failed' \
             ORDER BY updated_ts DESC, request_id LIMIT 10",
        )?;
        let rows = stmt.query_map([], |r| {
            let raw: Option<String> = r.get(3)?;
            Ok(SettlementFailure {
                request_id: r.get(0)?,
                actual_nano: r.get(1)?,
                attempts: r.get(2)?,
                last_error: raw.map(|e| e.chars().take(200).collect()),
                updated_ts: r.get(4)?,
            })
        })?;
        health.recent_failed = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    }
    health.ledger_consumer = settlement_consumer_lag(conn, consumer)?;
    Ok(health)
}

/// Обрезать usage_events под масштаб (как ledger_prune): удалить строки старше `older_than_ts`
/// батчами, отдавая write-lock между ними. Возвращает удалённое.
pub fn usage_prune(conn: &Connection, older_than_ts: i64) -> Result<usize> {
    const BATCH: i64 = 5000;
    let mut total = 0usize;
    loop {
        let n = conn.execute(
            "DELETE FROM usage_events WHERE id IN \
             (SELECT id FROM usage_events WHERE ts < ?1 LIMIT ?2)",
            rusqlite::params![older_than_ts, BATCH],
        )?;
        total += n;
        if (n as i64) < BATCH {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(total)
}

/// Прочитать ключ (для авторизации/`/balance`).
pub fn key_get(conn: &Connection, key: &str) -> Result<Option<KeyRow>> {
    let row = conn.query_row(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0), \
         (SELECT MAX(u.ts) FROM usage_events u WHERE u.account_id=k.account_id AND u.key=k.key), \
         COALESCE(k.status,'active') \
         FROM api_keys k WHERE k.key=?1",
        rusqlite::params![key],
        |r| {
            Ok(KeyRow {
                key: r.get::<_, String>(0)?,
                key_id: r.get::<_, String>(1)?,
                account_id: r.get::<_, Option<String>>(2)?,
                label: r.get::<_, Option<String>>(3)?,
                spent_nano: r.get::<_, i64>(4)?,
                reserved_nano: r.get(5)?,
                spend_limit_nano: r.get(6)?,
                expires_ts: r.get(7)?,
                created_ts: r.get(8)?,
                last_used_ts: r.get(9)?,
                status: r.get(10)?,
            })
        },
    );
    match row {
        Ok(k) => Ok(Some(k)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn key_set_status(conn: &Connection, key: &str, status: &str) -> Result<usize> {
    sqlite_key_set_status(conn, "key", key, status)
}


/// Change key status through its non-secret control-plane identifier.
pub fn key_set_status_by_id(conn: &Connection, key_id: &str, status: &str) -> Result<usize> {
    sqlite_key_set_status(conn, "key_id", key_id, status)
}

fn sqlite_key_set_status(
    conn: &Connection,
    identity_column: &str,
    identity: &str,
    status: &str,
) -> Result<usize> {
    if !matches!(identity_column, "key" | "key_id") {
        anyhow::bail!("invalid key status identity column");
    }
    Ok(conn.execute(
        &format!("UPDATE api_keys SET status=?1 WHERE {identity_column}=?2"),
        rusqlite::params![status, identity],
    )?)
}



/// Change key label through its non-secret control-plane identifier.
pub fn key_set_label_by_id(conn: &Connection, key_id: &str, label: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE api_keys SET label=?2 WHERE key_id=?1",
        rusqlite::params![key_id, label],
    )?)
}

/// Atomically replace a key policy without allowing a new limit to undercut committed or in-flight
/// usage. `None` clears the corresponding guardrail.
pub fn key_set_policy_by_id(
    conn: &Connection,
    account_id: &str,
    key_id: &str,
    spend_limit_nano: Option<i64>,
    expires_ts: Option<i64>,
) -> Result<KeyPolicyUpdate> {
    let updated = conn.execute(
        "UPDATE api_keys SET spend_limit_nano=?3, expires_ts=?4 \
         WHERE key_id=?1 AND account_id=?2 \
           AND (?3 IS NULL OR (reserved_nano<=?3 AND spent_nano<=?3-reserved_nano)) \
           AND (?4 IS NULL OR ?4>CAST(strftime('%s','now') AS INTEGER))",
        rusqlite::params![key_id, account_id, spend_limit_nano, expires_ts],
    )?;
    if updated == 1 {
        return Ok(KeyPolicyUpdate::Updated);
    }
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key_id=?1 AND account_id=?2)",
        rusqlite::params![key_id, account_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Ok(KeyPolicyUpdate::NotFound);
    }
    if expires_ts.is_some_and(|expires| expires <= now()) {
        return Ok(KeyPolicyUpdate::ExpiryNotFuture);
    }
    Ok(KeyPolicyUpdate::LimitBelowUsage)
}

/// Удалить ключ НАВСЕГДА (в отличие от set_status 'disabled' — строка исчезает).
pub fn key_remove(conn: &Connection, key: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys WHERE key=?1", rusqlite::params![key])?)
}

/// Удалить ВСЕ ключи (для очистки/тестов). Возвращает число удалённых.
pub fn key_clear(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys", [])?)
}

/// Ключи КОНКРЕТНОГО аккаунта (для дашборда коммерции: список ключей юзера). Ключ маскируется на выводе.
pub fn keys_by_account(conn: &Connection, account_id: &str) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0),u.last_used_ts, \
         COALESCE(k.status,'active') FROM api_keys k LEFT JOIN ( \
           SELECT key,MAX(ts) AS last_used_ts FROM usage_events WHERE account_id=?1 GROUP BY key \
         ) u ON u.key=k.key WHERE k.account_id=?1 ORDER BY COALESCE(k.created_ts,0)",
    )?;
    let rows = stmt.query_map(rusqlite::params![account_id], |r| {
        Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            key_id: r.get::<_, String>(1)?,
            account_id: r.get::<_, Option<String>>(2)?,
            label: r.get::<_, Option<String>>(3)?,
            spent_nano: r.get::<_, i64>(4)?,
            reserved_nano: r.get(5)?,
            spend_limit_nano: r.get(6)?,
            expires_ts: r.get(7)?,
            created_ts: r.get(8)?,
            last_used_ts: r.get(9)?,
            status: r.get(10)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка журнала движений баланса (для истории трат/пополнений в дашборде).
#[derive(Debug, Clone)]
pub struct LedgerRow {
    pub id: i64,
    pub key: Option<String>,
    pub kind: String, // topup | charge | adjust
    pub request_id: Option<String>,
    pub amount_nano: i64, // + пополнение / − списание
    pub reference: Option<String>,
    pub balance_after_nano: Option<i64>,
    pub ts: i64,
    pub model: Option<String>, // Claude-модель за charge (для per-model графика); topup/adjust → None
    pub provider: Option<String>,
    pub official_nano: Option<i64>,
}




pub(crate) fn resolve_ledger_provider(
    ledger_provider: Option<String>,
    usage_provider: Option<String>,
) -> Result<Option<String>> {
    match (ledger_provider, usage_provider) {
        (Some(ledger), Some(usage)) if ledger != usage => {
            anyhow::bail!("ledger provider differs from immutable usage provider")
        }
        (Some(ledger), _) => Ok(Some(ledger)),
        (None, usage) => Ok(usage),
    }
}

fn sqlite_ledger_row(row: &rusqlite::Row<'_>) -> Result<LedgerRow> {
    let provider = resolve_ledger_provider(
        row.get::<_, Option<String>>(9)?,
        row.get::<_, Option<String>>(11)?,
    )?;
    Ok(LedgerRow {
        id: row.get(0)?,
        key: row.get(1)?,
        kind: row.get(2)?,
        request_id: row.get(3)?,
        amount_nano: row.get(4)?,
        reference: row.get(5)?,
        balance_after_nano: row.get(6)?,
        ts: row.get(7)?,
        model: row.get(8)?,
        provider,
        official_nano: row.get(10)?,
    })
}

const SQLITE_LEDGER_READ_COLUMNS: &str = "
    ledger.id,ledger.key,ledger.kind,ledger.request_id,ledger.amount_nano,ledger.ref,
    ledger.balance_after_nano,ledger.ts,ledger.model,ledger.provider,ledger.official_nano,
    CASE
      WHEN ledger.kind<>'charge' THEN NULL
      WHEN ledger.request_id IS NOT NULL THEN (
        SELECT CASE
          WHEN COUNT(*) > 0
           AND COUNT(NULLIF(candidate.provider,''))=COUNT(*)
           AND COUNT(DISTINCT NULLIF(candidate.provider,''))=1
          THEN MIN(NULLIF(candidate.provider,''))
        END
          FROM usage_events candidate
         WHERE candidate.account_id=ledger.account_id
           AND candidate.request_id=ledger.request_id
      )
      ELSE (
        SELECT CASE
          WHEN COUNT(*) > 0
           AND COUNT(NULLIF(candidate.provider,''))=COUNT(*)
           AND COUNT(DISTINCT NULLIF(candidate.provider,''))=1
          THEN MIN(NULLIF(candidate.provider,''))
        END
          FROM usage_events candidate
         WHERE candidate.account_id=ledger.account_id
           AND candidate.request_id IS NULL
           AND candidate.key IS ledger.key
           AND candidate.charge_nano=ledger.amount_nano
           AND candidate.ref IS ledger.ref
           AND candidate.model IS ledger.model
           AND ABS(candidate.ts-ledger.ts)<=1
      )
    END";


fn sqlite_ledger_page(
    conn: &Connection,
    account_id: &str,
    after_id: Option<i64>,
    limit: i64,
) -> Result<Vec<LedgerRow>> {
    let predicate = if after_id.is_some() {
        "ledger.account_id=?1 AND ledger.id>?2 ORDER BY ledger.id ASC LIMIT ?3"
    } else {
        "ledger.account_id=?1 ORDER BY ledger.id DESC LIMIT ?2"
    };
    let sql = format!("SELECT {SQLITE_LEDGER_READ_COLUMNS} FROM ledger WHERE {predicate}");
    let tx = conn.unchecked_transaction()?;
    let mut statement = tx.prepare(&sql)?;
    let mut query = match after_id {
        Some(after_id) => statement.query(rusqlite::params![
            account_id,
            after_id.max(0),
            limit.clamp(1, 1000)
        ])?,
        None => statement.query(rusqlite::params![account_id, limit.clamp(1, 1000)])?,
    };
    let mut entries = Vec::new();
    while let Some(row) = query.next()? {
        entries.push(sqlite_ledger_row(row)?);
    }
    drop(query);
    drop(statement);
    tx.commit()?;
    Ok(entries)
}

/// Последние `limit` строк ledger аккаунта (свежие сверху). Для дашборда «история/расход».
pub fn ledger_recent(conn: &Connection, account_id: &str, limit: i64) -> Result<Vec<LedgerRow>> {
    sqlite_ledger_page(conn, account_id, None, limit)
}

/// Ledger cursor for durable external consumers. Rows are returned oldest-first after `after_id`.
pub fn ledger_after(
    conn: &Connection,
    account_id: &str,
    after_id: i64,
    limit: i64,
) -> Result<Vec<LedgerRow>> {
    sqlite_ledger_page(conn, account_id, Some(after_id), limit)
}

/// Все ключи (для CLI-листинга; ключ маскируется на стороне вывода).
pub fn key_list(conn: &Connection) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0),u.last_used_ts, \
         COALESCE(k.status,'active') FROM api_keys k LEFT JOIN ( \
           SELECT key,MAX(ts) AS last_used_ts FROM usage_events GROUP BY key \
         ) u ON u.key=k.key ORDER BY COALESCE(k.created_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            key_id: r.get::<_, String>(1)?,
            account_id: r.get::<_, Option<String>>(2)?,
            label: r.get::<_, Option<String>>(3)?,
            spent_nano: r.get::<_, i64>(4)?,
            reserved_nano: r.get(5)?,
            spend_limit_nano: r.get(6)?,
            expires_ts: r.get(7)?,
            created_ts: r.get(8)?,
            last_used_ts: r.get(9)?,
            status: r.get(10)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка персиста состояния пула (по подписке). Примитивы — registry не знает типов `pool`.
#[derive(Clone, Debug, Default)]
pub struct PoolStateRow {
    pub email: String,
    pub cooling_until: i64,
    pub cap5h_usd: f64,
    pub cap7d_usd: f64,
    pub spent_total_usd: f64,
    /// Process-local increment since the last successful persistence operation.
    pub spent_delta_usd: f64,
    pub util5h: f64,
    pub util7d: f64,
    pub reset5h: i64,
    pub reset7d: i64,
    pub calib_n: i64,
    /// PostgreSQL CAS version. SQLite compatibility rows use zero.
    pub version: i64,
}

/// Primitive durable state for one provider-reported OpenAI/Codex window duration.
///
/// Estimation semantics intentionally live in `forward`; registry only persists integer evidence
/// and applies compare-and-swap updates. A reset timestamp identifies the current concrete window,
/// while the primary key keeps independent duration classes from contaminating each other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexCalibrationRow {
    pub home_id: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub anchor_used_percent: i64,
    pub anchor_spend_nano: i64,
    pub used_percent: i64,
    pub observed_at: i64,
    pub sum_used_sq: i64,
    pub sum_used_spend_nano: i64,
    pub observed_points: i64,
    pub samples: i64,
    pub current_capacity_nano: Option<i64>,
    pub current_low_nano: Option<i64>,
    pub current_high_nano: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_capacity_nano: Option<i64>,
    pub last_low_nano: Option<i64>,
    pub last_high_nano: Option<i64>,
    pub last_confidence_bp: i64,
    pub last_measured_at: Option<i64>,
    /// Compatibility bit; canonical estimator v8 rows are ready to measure from the cold anchor.
    pub anchor_ready: bool,
    /// Provider utilisation in 10^-8 fraction units. The legacy percent fields remain an integer
    /// compatibility projection for binaries predating engine migration 0015.
    pub anchor_used_fraction_units: i64,
    pub used_fraction_units: i64,
    /// Exact sufficient statistics of the realized workload blend.
    pub observed_fraction_units: i64,
    pub observed_spend_nano: i64,
    /// Parallel native ChatGPT-credit estimator. `None` is the explicit pre-cutover state: old
    /// API-dollar evidence must never be reinterpreted as having consumed zero subscription quota.
    pub anchor_spend_nanocredits: Option<i64>,
    pub observed_spend_nanocredits: Option<i64>,
    pub current_capacity_nanocredits: Option<i64>,
    pub current_low_nanocredits: Option<i64>,
    pub current_high_nanocredits: Option<i64>,
    pub last_capacity_nanocredits: Option<i64>,
    pub last_low_nanocredits: Option<i64>,
    pub last_high_nanocredits: Option<i64>,
    pub credit_samples: Option<i64>,
    pub credit_estimator_version: Option<i64>,
    /// Provider quota movement that repeated without either atomic gateway ledger moving. This is
    /// possibly unattributed, not proof of external use.
    pub unattributed_fraction_units: Option<i64>,
    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

/// One raw, deduplicated pairing of provider utilisation and cumulative gateway spend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexWindowObservation {
    pub home_id: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub observed_at: i64,
    pub used_percent: i64,
    pub used_fraction_units: i64,
    pub gateway_spend_nano: i64,
    pub gateway_spend_nanocredits: Option<i64>,
}

/// One immutable successful Codex turn used as exact calibration evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexTurnCalibrationEvent {
    pub request_id: String,
    pub home_id: String,
    pub model_id: String,
    pub service_tier: String,
    pub provider_reported_tier: Option<String>,
    pub api_tariff_schedule_id: String,
    pub credit_schedule_id: String,
    pub completed_at: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub api_input_nanousd: i64,
    pub api_cached_input_nanousd: i64,
    pub api_cache_write_nanousd: i64,
    pub api_output_nanousd: i64,
    pub api_total_nanousd: i64,
    pub chatgpt_input_nanocredits: i64,
    pub chatgpt_cached_input_nanocredits: i64,
    pub chatgpt_output_nanocredits: i64,
    pub chatgpt_total_nanocredits: i64,
}

/// Durable cumulative dual ledger for one opaque Codex home.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodexHomeCalibrationSpend {
    pub spent_nano: i64,
    pub spent_nanocredits: Option<i64>,
    pub credit_tracking_started_ts: Option<i64>,
    /// True only when this call inserted the immutable event and advanced both ledgers.
    pub inserted: bool,
}

/// Exact admin aggregate; every token and monetary quantity stays integer through serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexTurnCalibrationAggregate {
    pub home_id: String,
    pub model_id: String,
    pub service_tier: String,
    pub provider_reported_tier: Option<String>,
    pub api_tariff_schedule_id: String,
    pub credit_schedule_id: String,
    pub turns: i64,
    pub first_completed_at: i64,
    pub last_completed_at: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub api_input_nanousd: i64,
    pub api_cached_input_nanousd: i64,
    pub api_cache_write_nanousd: i64,
    pub api_output_nanousd: i64,
    pub api_total_nanousd: i64,
    pub chatgpt_input_nanocredits: i64,
    pub chatgpt_cached_input_nanocredits: i64,
    pub chatgpt_output_nanocredits: i64,
    pub chatgpt_total_nanocredits: i64,
}

/// A request id already names a different immutable Codex turn. This is a permanent integrity
/// failure, not a transient database error: callers may quarantine the offending event while
/// continuing to flush later FIFO entries.
#[derive(Debug)]
pub struct CodexTurnCalibrationReplayConflict;

impl std::fmt::Display for CodexTurnCalibrationReplayConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Codex calibration request id replay conflict")
    }
}

impl std::error::Error for CodexTurnCalibrationReplayConflict {}

pub fn is_codex_turn_calibration_replay_conflict(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<CodexTurnCalibrationReplayConflict>())
}

/// Primitive durable state for one explicit Antigravity Gemini quota-summary bucket.
///
/// Estimation semantics live in `forward`. Registry keeps only exact integer evidence and CAS
/// versions. The legacy WLS accumulators remain canonical non-negative decimal strings for
/// backwards-compatible replay; `observed_spend_nano` is the exact v2 cumulative spend leg.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiCalibrationRow {
    pub profile_id: String,
    pub bucket_id: String,
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub anchor_used_fraction_units: i64,
    pub anchor_spend_nano: i64,
    pub anchor_ready: bool,
    pub used_fraction_units: i64,
    pub observed_at: i64,
    pub sum_used_sq: String,
    pub sum_used_spend_nano: String,
    pub observed_fraction_units: i64,
    pub observed_spend_nano: i64,
    pub samples: i64,
    pub current_capacity_nano: Option<i64>,
    pub current_low_nano: Option<i64>,
    pub current_high_nano: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_measured_at: Option<i64>,
    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

/// One raw, deduplicated pairing of an official Gemini quota fraction and cumulative spend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiWindowObservation {
    pub profile_id: String,
    pub bucket_id: String,
    pub window_kind: String,
    pub window_duration_mins: i64,
    pub resets_at: i64,
    pub observed_at: i64,
    pub used_fraction_units: i64,
    pub gateway_spend_nano: i64,
}

fn validate_gemini_accumulator(value: &str) -> Result<()> {
    let parsed = value
        .parse::<i128>()
        .context("parse Gemini calibration accumulator")?;
    if parsed < 0 || parsed.to_string() != value {
        bail!("Gemini calibration accumulator is not canonical");
    }
    Ok(())
}

fn valid_gemini_window_identity(bucket_id: &str, window_kind: &str, duration_mins: i64) -> bool {
    matches!(
        (bucket_id, window_kind, duration_mins),
        ("gemini-5h", "5h", 300) | ("gemini-weekly", "weekly", 10_080)
    )
}

fn validate_gemini_calibration_row(row: &GeminiCalibrationRow) -> Result<()> {
    if row.profile_id.is_empty()
        || !valid_gemini_window_identity(&row.bucket_id, &row.window_kind, row.window_duration_mins)
        || row.resets_at <= 0
        || !(0..=100_000_000).contains(&row.anchor_used_fraction_units)
        || row.anchor_spend_nano < 0
        || !(0..=100_000_000).contains(&row.used_fraction_units)
        || row.observed_at <= 0
        || row.observed_fraction_units < 0
        || row.observed_spend_nano < 0
        || row.samples < 0
        || row.current_capacity_nano.is_some_and(|value| value < 0)
        || row.current_low_nano.is_some_and(|value| value < 0)
        || row.current_high_nano.is_some_and(|value| value < 0)
        || row.current_low_nano.is_some() && row.current_capacity_nano.is_none()
        || row.current_high_nano.is_some() && row.current_capacity_nano.is_none()
        || !(0..=10_000).contains(&row.current_confidence_bp)
        || row.last_measured_at.is_some_and(|value| value <= 0)
        || row.estimator_version <= 0
        || row.version < 0
        || row.updated_ts <= 0
    {
        bail!("invalid Gemini calibration row");
    }
    validate_gemini_accumulator(&row.sum_used_sq)?;
    validate_gemini_accumulator(&row.sum_used_spend_nano)?;
    Ok(())
}

fn validate_gemini_window_observation(observation: &GeminiWindowObservation) -> Result<()> {
    if observation.profile_id.is_empty()
        || !valid_gemini_window_identity(
            &observation.bucket_id,
            &observation.window_kind,
            observation.window_duration_mins,
        )
        || observation.resets_at <= 0
        || observation.observed_at <= 0
        || !(0..=100_000_000).contains(&observation.used_fraction_units)
        || observation.gateway_spend_nano < 0
    {
        bail!("invalid Gemini calibration observation");
    }
    Ok(())
}

fn validate_gemini_calibration_pair(
    state: &GeminiCalibrationRow,
    observation: &GeminiWindowObservation,
) -> Result<()> {
    validate_gemini_calibration_row(state)?;
    validate_gemini_window_observation(observation)?;
    if state.profile_id != observation.profile_id
        || state.bucket_id != observation.bucket_id
        || state.window_kind != observation.window_kind
        || state.window_duration_mins != observation.window_duration_mins
    {
        bail!("Gemini calibration state/observation mismatch");
    }
    Ok(())
}

/// Atomically credit exact official-price spend and return the durable cumulative total.
pub fn credit_codex_home_spend(
    conn: &Connection,
    home_id: &str,
    delta_nano: i64,
    updated_ts: i64,
) -> Result<i64> {
    if home_id.is_empty() || delta_nano < 0 || updated_ts <= 0 {
        bail!("invalid Codex home spend credit");
    }
    conn.query_row(
        "INSERT INTO codex_home_spend(home_id,spent_nano,updated_ts) VALUES(?1,?2,?3) \
         ON CONFLICT(home_id) DO UPDATE SET \
           spent_nano=codex_home_spend.spent_nano+excluded.spent_nano, \
           updated_ts=excluded.updated_ts \
         RETURNING spent_nano",
        rusqlite::params![home_id, delta_nano, updated_ts],
        |row| row.get(0),
    )
    .context("credit SQLite Codex home spend")
}

pub(crate) fn validate_codex_turn_calibration_event(
    event: &CodexTurnCalibrationEvent,
) -> Result<()> {
    let token_counts = [
        event.input_tokens,
        event.cached_input_tokens,
        event.cache_write_input_tokens,
        event.output_tokens,
        event.reasoning_output_tokens,
    ];
    let api_legs = [
        event.api_input_nanousd,
        event.api_cached_input_nanousd,
        event.api_cache_write_nanousd,
        event.api_output_nanousd,
    ];
    let credit_legs = [
        event.chatgpt_input_nanocredits,
        event.chatgpt_cached_input_nanocredits,
        event.chatgpt_output_nanocredits,
    ];
    let input_subsets = event
        .cached_input_tokens
        .checked_add(event.cache_write_input_tokens);
    let api_total = api_legs.into_iter().try_fold(0i64, i64::checked_add);
    let credit_total = credit_legs.into_iter().try_fold(0i64, i64::checked_add);
    if event.request_id.is_empty()
        || event.home_id.is_empty()
        || event.model_id.is_empty()
        || !matches!(event.service_tier.as_str(), "standard" | "fast")
        || event.api_tariff_schedule_id.is_empty()
        || event.credit_schedule_id.is_empty()
        || event.completed_at <= 0
        || token_counts.into_iter().any(|value| value < 0)
        || event.input_tokens == 0 && event.output_tokens == 0
        || input_subsets.is_none_or(|value| value > event.input_tokens)
        || event.reasoning_output_tokens > event.output_tokens
        || api_legs.into_iter().any(|value| value < 0)
        || credit_legs.into_iter().any(|value| value < 0)
        || api_total != Some(event.api_total_nanousd)
        || credit_total != Some(event.chatgpt_total_nanocredits)
    {
        bail!("invalid Codex turn calibration event");
    }
    Ok(())
}

const CODEX_TURN_EVENT_COLUMNS: &str = "request_id,home_id,model_id,service_tier,\
    provider_reported_tier,api_tariff_schedule_id,credit_schedule_id,completed_at,\
    input_tokens,cached_input_tokens,cache_write_input_tokens,output_tokens,\
    reasoning_output_tokens,api_input_nanousd,api_cached_input_nanousd,\
    api_cache_write_nanousd,api_output_nanousd,api_total_nanousd,\
    chatgpt_input_nanocredits,chatgpt_cached_input_nanocredits,\
    chatgpt_output_nanocredits,chatgpt_total_nanocredits";

fn sqlite_codex_turn_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodexTurnCalibrationEvent> {
    Ok(CodexTurnCalibrationEvent {
        request_id: row.get(0)?,
        home_id: row.get(1)?,
        model_id: row.get(2)?,
        service_tier: row.get(3)?,
        provider_reported_tier: row.get(4)?,
        api_tariff_schedule_id: row.get(5)?,
        credit_schedule_id: row.get(6)?,
        completed_at: row.get(7)?,
        input_tokens: row.get(8)?,
        cached_input_tokens: row.get(9)?,
        cache_write_input_tokens: row.get(10)?,
        output_tokens: row.get(11)?,
        reasoning_output_tokens: row.get(12)?,
        api_input_nanousd: row.get(13)?,
        api_cached_input_nanousd: row.get(14)?,
        api_cache_write_nanousd: row.get(15)?,
        api_output_nanousd: row.get(16)?,
        api_total_nanousd: row.get(17)?,
        chatgpt_input_nanocredits: row.get(18)?,
        chatgpt_cached_input_nanocredits: row.get(19)?,
        chatgpt_output_nanocredits: row.get(20)?,
        chatgpt_total_nanocredits: row.get(21)?,
    })
}

/// Idempotently insert one immutable turn and advance API-dollar and ChatGPT-credit totals in the
/// same transaction. An exact replay returns the existing totals; a semantic mismatch for the
/// same request id fails closed without touching either ledger.
pub fn record_codex_turn_calibration_event(
    conn: &Connection,
    event: &CodexTurnCalibrationEvent,
) -> Result<CodexHomeCalibrationSpend> {
    validate_codex_turn_calibration_event(event)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite Codex turn calibration event")?;
    let inserted = tx.execute(
        "INSERT INTO codex_turn_calibration_events(\
           request_id,home_id,model_id,service_tier,provider_reported_tier,\
           api_tariff_schedule_id,credit_schedule_id,completed_at,input_tokens,\
           cached_input_tokens,cache_write_input_tokens,output_tokens,reasoning_output_tokens,\
           api_input_nanousd,api_cached_input_nanousd,api_cache_write_nanousd,\
           api_output_nanousd,api_total_nanousd,chatgpt_input_nanocredits,\
           chatgpt_cached_input_nanocredits,chatgpt_output_nanocredits,\
           chatgpt_total_nanocredits) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,\
                ?19,?20,?21,?22) ON CONFLICT(request_id) DO NOTHING",
        rusqlite::params![
            event.request_id,
            event.home_id,
            event.model_id,
            event.service_tier,
            event.provider_reported_tier,
            event.api_tariff_schedule_id,
            event.credit_schedule_id,
            event.completed_at,
            event.input_tokens,
            event.cached_input_tokens,
            event.cache_write_input_tokens,
            event.output_tokens,
            event.reasoning_output_tokens,
            event.api_input_nanousd,
            event.api_cached_input_nanousd,
            event.api_cache_write_nanousd,
            event.api_output_nanousd,
            event.api_total_nanousd,
            event.chatgpt_input_nanocredits,
            event.chatgpt_cached_input_nanocredits,
            event.chatgpt_output_nanocredits,
            event.chatgpt_total_nanocredits,
        ],
    )? == 1;
    if inserted {
        tx.execute(
            "INSERT INTO codex_home_spend(\
               home_id,spent_nano,spent_nanocredits,credit_tracking_started_ts,updated_ts) \
             VALUES(?1,?2,?3,?4,?4) ON CONFLICT(home_id) DO UPDATE SET \
               spent_nano=codex_home_spend.spent_nano+excluded.spent_nano, \
               spent_nanocredits=COALESCE(codex_home_spend.spent_nanocredits,0)\
                   +excluded.spent_nanocredits, \
               credit_tracking_started_ts=COALESCE(\
                   codex_home_spend.credit_tracking_started_ts,excluded.credit_tracking_started_ts), \
               updated_ts=MAX(codex_home_spend.updated_ts,excluded.updated_ts)",
            rusqlite::params![
                event.home_id,
                event.api_total_nanousd,
                event.chatgpt_total_nanocredits,
                event.completed_at,
            ],
        )?;
    } else {
        let existing = tx.query_row(
            &format!(
                "SELECT {CODEX_TURN_EVENT_COLUMNS} FROM codex_turn_calibration_events \
                 WHERE request_id=?1"
            ),
            rusqlite::params![event.request_id],
            sqlite_codex_turn_event,
        )?;
        if existing != *event {
            return Err(CodexTurnCalibrationReplayConflict.into());
        }
    }
    let mut totals = tx
        .query_row(
            "SELECT spent_nano,spent_nanocredits,credit_tracking_started_ts \
             FROM codex_home_spend WHERE home_id=?1",
            rusqlite::params![event.home_id],
            |row| {
                Ok(CodexHomeCalibrationSpend {
                    spent_nano: row.get(0)?,
                    spent_nanocredits: row.get(1)?,
                    credit_tracking_started_ts: row.get(2)?,
                    inserted: false,
                })
            },
        )
        .optional()?
        .unwrap_or_default();
    totals.inserted = inserted;
    tx.commit()?;
    Ok(totals)
}

pub fn codex_home_calibration_spend(
    conn: &Connection,
    home_id: &str,
) -> Result<CodexHomeCalibrationSpend> {
    Ok(conn
        .query_row(
            "SELECT spent_nano,spent_nanocredits,credit_tracking_started_ts \
             FROM codex_home_spend WHERE home_id=?1",
            rusqlite::params![home_id],
            |row| {
                Ok(CodexHomeCalibrationSpend {
                    spent_nano: row.get(0)?,
                    spent_nanocredits: row.get(1)?,
                    credit_tracking_started_ts: row.get(2)?,
                    inserted: false,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn codex_turn_calibration_report(
    conn: &Connection,
) -> Result<Vec<CodexTurnCalibrationAggregate>> {
    let mut statement = conn.prepare(
        "SELECT home_id,model_id,service_tier,provider_reported_tier,\
           api_tariff_schedule_id,credit_schedule_id,COUNT(*),MIN(completed_at),MAX(completed_at),\
           SUM(input_tokens),SUM(cached_input_tokens),SUM(cache_write_input_tokens),\
           SUM(output_tokens),SUM(reasoning_output_tokens),SUM(api_input_nanousd),\
           SUM(api_cached_input_nanousd),SUM(api_cache_write_nanousd),SUM(api_output_nanousd),\
           SUM(api_total_nanousd),SUM(chatgpt_input_nanocredits),\
           SUM(chatgpt_cached_input_nanocredits),SUM(chatgpt_output_nanocredits),\
           SUM(chatgpt_total_nanocredits) FROM codex_turn_calibration_events \
         GROUP BY home_id,model_id,service_tier,provider_reported_tier,\
           api_tariff_schedule_id,credit_schedule_id \
         ORDER BY home_id,model_id,service_tier,provider_reported_tier",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok(CodexTurnCalibrationAggregate {
                home_id: row.get(0)?,
                model_id: row.get(1)?,
                service_tier: row.get(2)?,
                provider_reported_tier: row.get(3)?,
                api_tariff_schedule_id: row.get(4)?,
                credit_schedule_id: row.get(5)?,
                turns: row.get(6)?,
                first_completed_at: row.get(7)?,
                last_completed_at: row.get(8)?,
                input_tokens: row.get(9)?,
                cached_input_tokens: row.get(10)?,
                cache_write_input_tokens: row.get(11)?,
                output_tokens: row.get(12)?,
                reasoning_output_tokens: row.get(13)?,
                api_input_nanousd: row.get(14)?,
                api_cached_input_nanousd: row.get(15)?,
                api_cache_write_nanousd: row.get(16)?,
                api_output_nanousd: row.get(17)?,
                api_total_nanousd: row.get(18)?,
                chatgpt_input_nanocredits: row.get(19)?,
                chatgpt_cached_input_nanocredits: row.get(20)?,
                chatgpt_output_nanocredits: row.get(21)?,
                chatgpt_total_nanocredits: row.get(22)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Atomically credit exact official-price Gemini spend and return the cumulative profile total.
pub fn credit_gemini_profile_spend(
    conn: &Connection,
    profile_id: &str,
    delta_nano: i64,
    updated_ts: i64,
) -> Result<i64> {
    if profile_id.is_empty() || delta_nano < 0 || updated_ts <= 0 {
        bail!("invalid Gemini profile spend credit");
    }
    conn.query_row(
        "INSERT INTO gemini_profile_spend(profile_id,spent_nano,updated_ts) VALUES(?1,?2,?3) \
         ON CONFLICT(profile_id) DO UPDATE SET \
           spent_nano=gemini_profile_spend.spent_nano+excluded.spent_nano, \
           updated_ts=excluded.updated_ts \
         RETURNING spent_nano",
        rusqlite::params![profile_id, delta_nano, updated_ts],
        |row| row.get(0),
    )
    .context("credit SQLite Gemini profile spend")
}

/// Durable account-level health for one Codex home.
///
/// Only the account axis is stored. Transport health belongs to one transport generation and must
/// not survive it: a restarted gateway holds a brand new bridge and deserves a fresh verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexHomeHealthRow {
    pub account_state: String,
    pub auth_fail_streak: i64,
    pub first_auth_fail_ts: i64,
    pub cooling_until: i64,
}

impl Default for CodexHomeHealthRow {
    fn default() -> Self {
        Self {
            account_state: "healthy".to_string(),
            auth_fail_streak: 0,
            first_auth_fail_ts: 0,
            cooling_until: 0,
        }
    }
}

pub fn save_codex_home_health(
    conn: &Connection,
    home_id: &str,
    row: &CodexHomeHealthRow,
    updated_ts: i64,
) -> Result<()> {
    if home_id.is_empty() || updated_ts <= 0 {
        bail!("invalid Codex home health write");
    }
    conn.execute(
        "INSERT INTO codex_home_health( \
           home_id,account_state,auth_fail_streak,first_auth_fail_ts,cooling_until,updated_ts) \
         VALUES(?1,?2,?3,?4,?5,?6) \
         ON CONFLICT(home_id) DO UPDATE SET \
           account_state=excluded.account_state, \
           auth_fail_streak=excluded.auth_fail_streak, \
           first_auth_fail_ts=excluded.first_auth_fail_ts, \
           cooling_until=excluded.cooling_until, \
           updated_ts=excluded.updated_ts",
        rusqlite::params![
            home_id,
            row.account_state,
            row.auth_fail_streak,
            row.first_auth_fail_ts,
            row.cooling_until,
            updated_ts
        ],
    )
    .context("save SQLite Codex home health")?;
    Ok(())
}

/// A home with no stored verdict starts healthy: absence of evidence is not evidence of a fault.
pub fn load_codex_home_health(conn: &Connection, home_id: &str) -> Result<CodexHomeHealthRow> {
    Ok(conn
        .query_row(
            "SELECT account_state,auth_fail_streak,first_auth_fail_ts,cooling_until \
             FROM codex_home_health WHERE home_id=?1",
            rusqlite::params![home_id],
            |row| {
                Ok(CodexHomeHealthRow {
                    account_state: row.get(0)?,
                    auth_fail_streak: row.get(1)?,
                    first_auth_fail_ts: row.get(2)?,
                    cooling_until: row.get(3)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn codex_home_spend(conn: &Connection, home_id: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT spent_nano FROM codex_home_spend WHERE home_id=?1",
            rusqlite::params![home_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

pub fn gemini_profile_spend(conn: &Connection, profile_id: &str) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT spent_nano FROM gemini_profile_spend WHERE profile_id=?1",
            rusqlite::params![profile_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn sqlite_codex_calibration_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodexCalibrationRow> {
    Ok(CodexCalibrationRow {
        home_id: row.get(0)?,
        window_duration_mins: row.get(1)?,
        resets_at: row.get(2)?,
        anchor_used_percent: row.get(3)?,
        anchor_spend_nano: row.get(4)?,
        used_percent: row.get(5)?,
        observed_at: row.get(6)?,
        sum_used_sq: row.get(7)?,
        sum_used_spend_nano: row.get(8)?,
        observed_points: row.get(9)?,
        samples: row.get(10)?,
        current_capacity_nano: row.get(11)?,
        current_low_nano: row.get(12)?,
        current_high_nano: row.get(13)?,
        current_confidence_bp: row.get(14)?,
        last_capacity_nano: row.get(15)?,
        last_low_nano: row.get(16)?,
        last_high_nano: row.get(17)?,
        last_confidence_bp: row.get(18)?,
        last_measured_at: row.get(19)?,
        estimator_version: row.get(20)?,
        version: row.get(21)?,
        updated_ts: row.get(22)?,
        anchor_ready: row.get(23)?,
        anchor_used_fraction_units: row.get(24)?,
        used_fraction_units: row.get(25)?,
        observed_fraction_units: row.get(26)?,
        observed_spend_nano: row.get(27)?,
        anchor_spend_nanocredits: row.get(28)?,
        observed_spend_nanocredits: row.get(29)?,
        current_capacity_nanocredits: row.get(30)?,
        current_low_nanocredits: row.get(31)?,
        current_high_nanocredits: row.get(32)?,
        last_capacity_nanocredits: row.get(33)?,
        last_low_nanocredits: row.get(34)?,
        last_high_nanocredits: row.get(35)?,
        credit_samples: row.get(36)?,
        credit_estimator_version: row.get(37)?,
        unattributed_fraction_units: row.get(38)?,
    })
}

const CODEX_CALIBRATION_COLUMNS: &str = "home_id,window_duration_mins,resets_at,\
    anchor_used_percent,anchor_spend_nano,used_percent,observed_at,sum_used_sq,\
    sum_used_spend_nano,observed_points,samples,current_capacity_nano,current_low_nano,\
    current_high_nano,current_confidence_bp,last_capacity_nano,last_low_nano,last_high_nano,\
    last_confidence_bp,last_measured_at,estimator_version,version,updated_ts,anchor_ready,\
    COALESCE(anchor_used_fraction_units,anchor_used_percent*1000000),\
    COALESCE(used_fraction_units,used_percent*1000000),\
    COALESCE(observed_fraction_units,observed_points*1000000),\
    COALESCE(observed_spend_nano,0),anchor_spend_nanocredits,observed_spend_nanocredits,\
    current_capacity_nanocredits,current_low_nanocredits,current_high_nanocredits,\
    last_capacity_nanocredits,last_low_nanocredits,last_high_nanocredits,credit_samples,\
    credit_estimator_version,unattributed_fraction_units";

pub fn load_codex_calibration(
    conn: &Connection,
    home_id: &str,
    window_duration_mins: i64,
) -> Result<Option<CodexCalibrationRow>> {
    conn.query_row(
        &format!(
            "SELECT {CODEX_CALIBRATION_COLUMNS} FROM codex_window_calibrations \
             WHERE home_id=?1 AND window_duration_mins=?2"
        ),
        rusqlite::params![home_id, window_duration_mins],
        sqlite_codex_calibration_row,
    )
    .optional()
    .context("load SQLite Codex calibration")
}

/// Load the immutable evidence log for a one-time estimator rebuild.
///
/// The synthetic id is the tie-breaker because provider observations can share a wall-clock
/// second. Runtime updates remain incremental once the stored estimator version is current.
pub fn load_codex_window_observations(
    conn: &Connection,
    home_id: &str,
    window_duration_mins: i64,
) -> Result<Vec<CodexWindowObservation>> {
    let mut statement = conn.prepare(
        "SELECT home_id,window_duration_mins,resets_at,observed_at,used_percent,\
                COALESCE(used_fraction_units,used_percent*1000000),gateway_spend_nano,\
                gateway_spend_nanocredits \
         FROM codex_window_observations WHERE home_id=?1 AND window_duration_mins=?2 \
         ORDER BY observed_at,id",
    )?;
    let observations = statement
        .query_map(rusqlite::params![home_id, window_duration_mins], |row| {
            Ok(CodexWindowObservation {
                home_id: row.get(0)?,
                window_duration_mins: row.get(1)?,
                resets_at: row.get(2)?,
                observed_at: row.get(3)?,
                used_percent: row.get(4)?,
                used_fraction_units: row.get(5)?,
                gateway_spend_nano: row.get(6)?,
                gateway_spend_nanocredits: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("load SQLite Codex window observations")?;
    Ok(observations)
}

/// Persist one estimator result and its raw observation in the same transaction.
///
/// `None` is an ordinary CAS conflict. Callers reload evidence and recompute; no observation from
/// the losing derivation commits on its own.
pub fn save_codex_calibration(
    conn: &Connection,
    state: &CodexCalibrationRow,
    observation: &CodexWindowObservation,
) -> Result<Option<i64>> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite Codex calibration CAS")?;
    let values = rusqlite::params![
        state.home_id,
        state.window_duration_mins,
        state.resets_at,
        state.anchor_used_percent,
        state.anchor_spend_nano,
        state.used_percent,
        state.observed_at,
        state.sum_used_sq,
        state.sum_used_spend_nano,
        state.observed_points,
        state.samples,
        state.current_capacity_nano,
        state.current_low_nano,
        state.current_high_nano,
        state.current_confidence_bp,
        state.last_capacity_nano,
        state.last_low_nano,
        state.last_high_nano,
        state.last_confidence_bp,
        state.last_measured_at,
        state.estimator_version,
        state.updated_ts,
        state.version,
        state.anchor_ready,
        state.anchor_used_fraction_units,
        state.used_fraction_units,
        state.observed_fraction_units,
        state.observed_spend_nano,
        state.anchor_spend_nanocredits,
        state.observed_spend_nanocredits,
        state.current_capacity_nanocredits,
        state.current_low_nanocredits,
        state.current_high_nanocredits,
        state.last_capacity_nanocredits,
        state.last_low_nanocredits,
        state.last_high_nanocredits,
        state.credit_samples,
        state.credit_estimator_version,
        state.unattributed_fraction_units,
    ];
    let changed = if state.version == 0 {
        tx.execute(
            "INSERT INTO codex_window_calibrations( \
               home_id,window_duration_mins,resets_at,anchor_used_percent,anchor_spend_nano,\
               used_percent,observed_at,sum_used_sq,sum_used_spend_nano,observed_points,samples,\
               current_capacity_nano,current_low_nano,current_high_nano,current_confidence_bp,\
               last_capacity_nano,last_low_nano,last_high_nano,last_confidence_bp,last_measured_at,\
               estimator_version,updated_ts,version,anchor_ready,anchor_used_fraction_units,\
               used_fraction_units,observed_fraction_units,observed_spend_nano,\
               anchor_spend_nanocredits,observed_spend_nanocredits,\
               current_capacity_nanocredits,current_low_nanocredits,current_high_nanocredits,\
               last_capacity_nanocredits,last_low_nanocredits,last_high_nanocredits,\
               credit_samples,credit_estimator_version,unattributed_fraction_units \
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,\
                      ?19,?20,?21,?22,?23+1,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,\
                      ?34,?35,?36,?37,?38,?39) \
             ON CONFLICT(home_id,window_duration_mins) DO NOTHING",
            values,
        )?
    } else {
        tx.execute(
            "UPDATE codex_window_calibrations SET \
               resets_at=?3,anchor_used_percent=?4,anchor_spend_nano=?5,used_percent=?6,\
               observed_at=?7,sum_used_sq=?8,sum_used_spend_nano=?9,observed_points=?10,\
               samples=?11,current_capacity_nano=?12,current_low_nano=?13,current_high_nano=?14,\
               current_confidence_bp=?15,last_capacity_nano=?16,last_low_nano=?17,\
               last_high_nano=?18,last_confidence_bp=?19,last_measured_at=?20,\
               estimator_version=?21,updated_ts=?22,version=version+1,anchor_ready=?24,\
               anchor_used_fraction_units=?25,used_fraction_units=?26,\
               observed_fraction_units=?27,observed_spend_nano=?28,\
               anchor_spend_nanocredits=?29,observed_spend_nanocredits=?30,\
               current_capacity_nanocredits=?31,current_low_nanocredits=?32,\
               current_high_nanocredits=?33,last_capacity_nanocredits=?34,\
               last_low_nanocredits=?35,last_high_nanocredits=?36,credit_samples=?37,\
               credit_estimator_version=?38,unattributed_fraction_units=?39 \
             WHERE home_id=?1 AND window_duration_mins=?2 AND version=?23",
            values,
        )?
    };
    if changed == 0 {
        return Ok(None);
    }
    tx.execute(
        "INSERT INTO codex_window_observations( \
           home_id,window_duration_mins,resets_at,observed_at,used_percent,used_fraction_units,\
           gateway_spend_nano,gateway_spend_nanocredits \
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT DO NOTHING",
        rusqlite::params![
            observation.home_id,
            observation.window_duration_mins,
            observation.resets_at,
            observation.observed_at,
            observation.used_percent,
            observation.used_fraction_units,
            observation.gateway_spend_nano,
            observation.gateway_spend_nanocredits,
        ],
    )?;
    tx.commit()?;
    Ok(Some(state.version.saturating_add(1)))
}

fn sqlite_gemini_calibration_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<GeminiCalibrationRow> {
    Ok(GeminiCalibrationRow {
        profile_id: row.get(0)?,
        bucket_id: row.get(1)?,
        window_kind: row.get(2)?,
        window_duration_mins: row.get(3)?,
        resets_at: row.get(4)?,
        anchor_used_fraction_units: row.get(5)?,
        anchor_spend_nano: row.get(6)?,
        anchor_ready: row.get(7)?,
        used_fraction_units: row.get(8)?,
        observed_at: row.get(9)?,
        sum_used_sq: row.get(10)?,
        sum_used_spend_nano: row.get(11)?,
        observed_fraction_units: row.get(12)?,
        observed_spend_nano: row.get(13)?,
        samples: row.get(14)?,
        current_capacity_nano: row.get(15)?,
        current_low_nano: row.get(16)?,
        current_high_nano: row.get(17)?,
        current_confidence_bp: row.get(18)?,
        last_measured_at: row.get(19)?,
        estimator_version: row.get(20)?,
        version: row.get(21)?,
        updated_ts: row.get(22)?,
    })
}

const GEMINI_CALIBRATION_COLUMNS: &str = "profile_id,bucket_id,window_kind,window_duration_mins,\
    resets_at,anchor_used_fraction_units,anchor_spend_nano,anchor_ready,used_fraction_units,\
    observed_at,sum_used_sq,sum_used_spend_nano,observed_fraction_units,observed_spend_nano,\
    samples,current_capacity_nano,current_low_nano,current_high_nano,\
    current_confidence_bp,last_measured_at,estimator_version,version,updated_ts";

pub fn load_gemini_calibration(
    conn: &Connection,
    profile_id: &str,
    bucket_id: &str,
) -> Result<Option<GeminiCalibrationRow>> {
    let row = conn
        .query_row(
            &format!(
                "SELECT {GEMINI_CALIBRATION_COLUMNS} FROM gemini_window_calibrations \
                 WHERE profile_id=?1 AND bucket_id=?2"
            ),
            rusqlite::params![profile_id, bucket_id],
            sqlite_gemini_calibration_row,
        )
        .optional()
        .context("load SQLite Gemini calibration")?;
    if let Some(row) = &row {
        validate_gemini_calibration_row(row)?;
    }
    Ok(row)
}

pub fn load_gemini_window_observations(
    conn: &Connection,
    profile_id: &str,
    bucket_id: &str,
) -> Result<Vec<GeminiWindowObservation>> {
    let mut statement = conn.prepare(
        "SELECT profile_id,bucket_id,window_kind,window_duration_mins,resets_at,observed_at,\
           used_fraction_units,gateway_spend_nano FROM gemini_window_observations \
         WHERE profile_id=?1 AND bucket_id=?2 ORDER BY observed_at,id",
    )?;
    let observations = statement
        .query_map(rusqlite::params![profile_id, bucket_id], |row| {
            Ok(GeminiWindowObservation {
                profile_id: row.get(0)?,
                bucket_id: row.get(1)?,
                window_kind: row.get(2)?,
                window_duration_mins: row.get(3)?,
                resets_at: row.get(4)?,
                observed_at: row.get(5)?,
                used_fraction_units: row.get(6)?,
                gateway_spend_nano: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("load SQLite Gemini window observations")?;
    Ok(observations)
}

/// Persist one Gemini estimator result and raw observation atomically under optimistic CAS.
pub fn save_gemini_calibration(
    conn: &Connection,
    state: &GeminiCalibrationRow,
    observation: &GeminiWindowObservation,
) -> Result<Option<i64>> {
    validate_gemini_calibration_pair(state, observation)?;
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .context("begin SQLite Gemini calibration CAS")?;
    let values = rusqlite::params![
        state.profile_id,
        state.bucket_id,
        state.window_kind,
        state.window_duration_mins,
        state.resets_at,
        state.anchor_used_fraction_units,
        state.anchor_spend_nano,
        state.anchor_ready,
        state.used_fraction_units,
        state.observed_at,
        state.sum_used_sq,
        state.sum_used_spend_nano,
        state.observed_fraction_units,
        state.observed_spend_nano,
        state.samples,
        state.current_capacity_nano,
        state.current_low_nano,
        state.current_high_nano,
        state.current_confidence_bp,
        state.last_measured_at,
        state.estimator_version,
        state.updated_ts,
        state.version,
    ];
    let changed = if state.version == 0 {
        tx.execute(
            "INSERT INTO gemini_window_calibrations( \
               profile_id,bucket_id,window_kind,window_duration_mins,resets_at,\
               anchor_used_fraction_units,anchor_spend_nano,anchor_ready,used_fraction_units,\
               observed_at,sum_used_sq,sum_used_spend_nano,observed_fraction_units,\
               observed_spend_nano,samples,current_capacity_nano,current_low_nano,current_high_nano,\
               current_confidence_bp,last_measured_at,estimator_version,updated_ts,version \
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,\
                      ?18,?19,?20,?21,?22,?23+1) \
             ON CONFLICT(profile_id,bucket_id) DO NOTHING",
            values,
        )?
    } else {
        tx.execute(
            "UPDATE gemini_window_calibrations SET \
               window_kind=?3,window_duration_mins=?4,resets_at=?5,\
               anchor_used_fraction_units=?6,anchor_spend_nano=?7,anchor_ready=?8,\
               used_fraction_units=?9,observed_at=?10,sum_used_sq=?11,\
               sum_used_spend_nano=?12,observed_fraction_units=?13,\
               observed_spend_nano=?14,samples=?15,current_capacity_nano=?16,\
               current_low_nano=?17,current_high_nano=?18,current_confidence_bp=?19,\
               last_measured_at=?20,estimator_version=?21,updated_ts=?22,version=version+1 \
             WHERE profile_id=?1 AND bucket_id=?2 AND version=?23",
            values,
        )?
    };
    if changed == 0 {
        return Ok(None);
    }
    tx.execute(
        "INSERT INTO gemini_window_observations( \
           profile_id,bucket_id,window_kind,window_duration_mins,resets_at,observed_at,\
           used_fraction_units,gateway_spend_nano \
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT DO NOTHING",
        rusqlite::params![
            observation.profile_id,
            observation.bucket_id,
            observation.window_kind,
            observation.window_duration_mins,
            observation.resets_at,
            observation.observed_at,
            observation.used_fraction_units,
            observation.gateway_spend_nano,
        ],
    )?;
    tx.commit()?;
    Ok(Some(state.version.saturating_add(1)))
}

/// Сохранить снимок состояния пула (upsert по email). Одной транзакцией — атомарно и быстро.
pub fn save_pool_state(conn: &Connection, rows: &[PoolStateRow]) -> Result<()> {
    let ts = now();
    conn.execute_batch("BEGIN")?;
    {
        let mut stmt = conn.prepare(
            "INSERT INTO pool_state(email, cooling_until, cap5h, cap7d, spent_total, util5, util7, \
             reset5, reset7, calib_n, updated_ts) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
             ON CONFLICT(email) DO UPDATE SET cooling_until=excluded.cooling_until, cap5h=excluded.cap5h, \
             cap7d=excluded.cap7d, spent_total=excluded.spent_total, util5=excluded.util5, \
             util7=excluded.util7, reset5=excluded.reset5, reset7=excluded.reset7, \
             calib_n=excluded.calib_n, updated_ts=excluded.updated_ts")?;
        for r in rows {
            stmt.execute(rusqlite::params![
                r.email,
                r.cooling_until,
                r.cap5h_usd,
                r.cap7d_usd,
                r.spent_total_usd,
                r.util5h,
                r.util7d,
                r.reset5h,
                r.reset7d,
                r.calib_n,
                ts
            ])?;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

/// Прочитать сохранённое состояние пула (для восстановления на старте).
pub fn load_pool_state(conn: &Connection) -> Result<Vec<PoolStateRow>> {
    let mut stmt = conn.prepare(
        "SELECT email, cooling_until, cap5h, cap7d, spent_total, util5, util7, reset5, reset7, calib_n \
         FROM pool_state")?;
    let rows = stmt.query_map([], |r| {
        Ok(PoolStateRow {
            email: r.get(0)?,
            cooling_until: r.get(1)?,
            cap5h_usd: r.get(2)?,
            cap7d_usd: r.get(3)?,
            spent_total_usd: r.get(4)?,
            spent_delta_usd: 0.0,
            util5h: r.get(5)?,
            util7d: r.get(6)?,
            reset5h: r.get(7)?,
            reset7d: r.get(8)?,
            calib_n: r.get(9)?,
            version: 0,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

// (Синхронная обёртка `Billing` удалена: запросный путь теперь через `forward::AsyncBilling`
//  — DB-акторы, ноль синхронных вызовов на async-воркерах. registry остаётся чистым sync-ядром.)

/// Агрегаты биллинга по всем аккаунтам клиентов (нанодоллары).
#[derive(Clone, Debug, Default)]
pub struct BillingTotals {
    pub balance_nano: i64,  // суммарный остаток на аккаунтах (клиентский флоат)
    pub spent_nano: i64,    // суммарно списано за всё время
    pub reserved_nano: i64, // сейчас в незакрытых резервах (in-flight холды)
    pub active_accounts: i64,
}

/// Суммы по accounts одним запросом (источник истины — БД). Ошибка возвращается вызывающему коду.
pub fn billing_totals(conn: &Connection) -> Result<BillingTotals> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(balance_nano),0), COALESCE(SUM(spent_nano),0), \
         COALESCE(SUM(reserved_nano),0), COALESCE(SUM(CASE WHEN COALESCE(status,'active')='active' \
         THEN 1 ELSE 0 END),0) FROM accounts",
        [],
        |r| {
            Ok(BillingTotals {
                balance_nano: r.get(0)?,
                spent_nano: r.get(1)?,
                reserved_nano: r.get(2)?,
                active_accounts: r.get(3)?,
            })
        },
    )?)
}

/// Простая UTC-строка YYYY-MM-DD HH:MM без внешних крейтов (для колонки `added`).
fn chrono_like(ts: i64) -> String {
    let days = ts.div_euclid(86400);
    let secs = ts.rem_euclid(86400);
    let (h, mi) = (secs / 3600, (secs % 3600) / 60);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

#[cfg(test)]
mod tests;
