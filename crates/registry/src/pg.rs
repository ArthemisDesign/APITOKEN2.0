//! PostgreSQL authority for the engine.
//!
//! All correctness-sensitive mutations are transactions. Request IDs and lease IDs are the
//! idempotency boundary; owner epochs fence stale instances. PostgreSQL is the recovery floor.

use crate::{
    mask_proxy, AccountRow, BillingTotals, CodexCalibrationRow, CodexWindowObservation, KeyAuth,
    KeyPolicyUpdate, KeyRow, LedgerRow, PoolStateRow, SpendAccountAgg, SpendProviderAgg, Sub,
    SubAdmin, SubHealth, SubRow, UsageDailyAgg, UsageDailyProviderAgg, UsageEventInput,
    UsageKeyAgg, UsageModelAgg, UsageReport,
};
use anyhow::{bail, Context, Result};
use postgres::config::{Host, SslMode};
use postgres::{Client, IsolationLevel, Row, Transaction};
use tokio_postgres_rustls::MakeRustlsConnect;

const MIGRATION_0001: &str = include_str!("../migrations_pg/0001_engine_authority.sql");
const MIGRATION_0002: &str = include_str!("../migrations_pg/0002_api_key_policies.sql");
const MIGRATION_0003: &str = include_str!("../migrations_pg/0003_subscription_auth_health.sql");
const MIGRATION_0004: &str = include_str!("../migrations_pg/0004_audit_hardening.sql");
const MIGRATION_0005: &str = include_str!("../migrations_pg/0005_provider_attribution.sql");
const MIGRATION_0006: &str = include_str!("../migrations_pg/0006_multi_discount_expand.sql");
const MIGRATION_0007: &str = include_str!("../migrations_pg/0007_multi_discount_runtime_pins.sql");
const MIGRATION_0008: &str = include_str!("../migrations_pg/0008_catalog_policy_lineage.sql");
const MIGRATION_0009: &str = include_str!("../migrations_pg/0009_pricing_shadow_admission.sql");
const MIGRATION_0010: &str = include_str!("../migrations_pg/0010_codex_window_calibration.sql");

/// Highest PostgreSQL schema version understood by this engine build.
pub const CURRENT_SCHEMA_VERSION: i64 = 10;

const ENGINE_MIGRATIONS: &[(i64, &str)] = &[
    (1, MIGRATION_0001),
    (2, MIGRATION_0002),
    (3, MIGRATION_0003),
    (4, MIGRATION_0004),
    (5, MIGRATION_0005),
    (6, MIGRATION_0006),
    (7, MIGRATION_0007),
    (8, MIGRATION_0008),
    (9, MIGRATION_0009),
    (10, MIGRATION_0010),
];

#[cfg(test)]
pub(crate) const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn chrono_like(ts: i64) -> String {
    crate::chrono_like(ts)
}

fn account_row(row: &Row) -> AccountRow {
    AccountRow {
        id: row.get(0),
        handle: row.get(1),
        balance_nano: row.get(2),
        spent_nano: row.get(3),
        reserved_nano: row.get(4),
        mult_bp: row.get(5),
        status: row.get(6),
    }
}

fn key_row(row: &Row) -> KeyRow {
    KeyRow {
        key: row.get(0),
        key_id: row.get(1),
        account_id: row.get(2),
        label: row.get(3),
        spent_nano: row.get(4),
        reserved_nano: row.get(5),
        spend_limit_nano: row.get(6),
        expires_ts: row.get(7),
        created_ts: row.get(8),
        last_used_ts: row.get(9),
        status: row.get(10),
    }
}

fn ledger_row(row: &Row) -> LedgerRow {
    LedgerRow {
        id: row.get(0),
        key: row.get(1),
        kind: row.get(2),
        amount_nano: row.get(3),
        reference: row.get(4),
        balance_after_nano: row.get(5),
        ts: row.get(6),
        model: row.get(7),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub instance_id: String,
    pub epoch: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapacityLease {
    pub lease_id: String,
    pub request_id: String,
    pub subscription_email: String,
    pub lease_until: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub canceled_before_delivery: usize,
    pub charged_after_delivery: usize,
    pub processed_outbox: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub usage_events: usize,
    pub outbox: usize,
    pub reservations: usize,
    pub pricing_snapshots_cascaded: usize,
    pub pricing_shadow_evaluations_cascaded: usize,
    pub capacity_leases: usize,
    pub engine_instances: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub subscriptions: usize,
    pub accounts: usize,
    pub keys: usize,
    pub ledger_rows: usize,
    pub usage_rows: usize,
    pub pool_rows: usize,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
}

pub struct PgStore {
    client: Client,
}

/// Error class used by the async actor. Logical/invariant failures must never be retried forever;
/// transport and PostgreSQL concurrency failures may be retried within a bounded deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    Transient,
    Fenced,
    Permanent,
}

pub fn classify_failure(error: &anyhow::Error) -> FailureClass {
    let message = format!("{error:#}").to_ascii_lowercase();
    if message.contains("owner lease is stale or fenced") || message.contains("owner was fenced") {
        return FailureClass::Fenced;
    }

    for cause in error.chain() {
        if let Some(pg) = cause.downcast_ref::<postgres::Error>() {
            let Some(db) = pg.as_db_error() else {
                // I/O, TLS and closed-connection failures have no server SQLSTATE.
                return FailureClass::Transient;
            };
            let code = db.code().code();
            if code.starts_with("08")
                || matches!(
                    code,
                    "40001" // serialization_failure
                        | "40P01" // deadlock_detected
                        | "55P03" // lock_not_available
                        | "57014" // query_canceled / statement timeout
                        | "57P01" // admin_shutdown
                        | "57P02" // crash_shutdown
                        | "57P03" // cannot_connect_now
                        | "53300" // too_many_connections
                )
            {
                return FailureClass::Transient;
            }
            return FailureClass::Permanent;
        }
    }
    FailureClass::Permanent
}

impl PgStore {
    pub fn connect(url: &str) -> Result<Self> {
        let config: postgres::Config = url.parse().context("parse engine PostgreSQL URL")?;
        let remote_tcp = config.get_hosts().iter().any(|host| match host {
            Host::Tcp(host) => {
                host != "localhost"
                    && host
                        .parse::<std::net::IpAddr>()
                        .map_or(true, |ip| !ip.is_loopback())
            }
            #[cfg(unix)]
            Host::Unix(_) => false,
        });
        if remote_tcp && config.get_ssl_mode() != SslMode::Require {
            bail!("remote PostgreSQL requires sslmode=require");
        }
        // The forward transport statically links BoringSSL through wreq. Keep PostgreSQL on
        // rustls so a single engine binary never links two libraries that export OpenSSL's ABI.
        let (connector, _certificate_load_errors) = MakeRustlsConnect::with_native_certs()
            .map_err(|errors| anyhow::anyhow!("load native certificates: {errors:?}"))?;
        let mut client = config
            .connect(connector)
            .context("connect engine PostgreSQL")?;
        client
            .batch_execute(
                "SET statement_timeout = '15s'; SET lock_timeout = '5s'; \
                 SET idle_in_transaction_session_timeout = '15s'; SET synchronous_commit = on;",
            )
            .context("configure engine PostgreSQL session")?;
        Ok(Self { client })
    }

    fn apply_migration(&mut self, version: i64, sql: &str) -> Result<()> {
        let mut tx = self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(836214912670::bigint)", &[])?;

        let migrations_table_exists: bool = tx
            .query_one(
                "SELECT to_regclass('public.engine_schema_migrations') IS NOT NULL",
                &[],
            )?
            .get(0);
        let already_applied = if migrations_table_exists {
            tx.query_opt(
                "SELECT 1 FROM engine_schema_migrations WHERE version=$1",
                &[&version],
            )?
            .is_some()
        } else {
            false
        };

        if !already_applied {
            tx.batch_execute(sql)
                .with_context(|| format!("apply engine PostgreSQL migration {version:04}"))?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Apply pending migrations explicitly. Each migration has its own transaction so a DDL
    /// lock acquired by one version cannot be held while a later version waits on another table.
    /// The advisory transaction lock still serializes concurrent migration runners.
    pub fn migrate(&mut self) -> Result<()> {
        for &(version, sql) in ENGINE_MIGRATIONS {
            self.apply_migration(version, sql)?;
        }
        Ok(())
    }

    pub fn schema_version(&mut self) -> Result<i64> {
        Ok(self
            .client
            .query_one(
                "SELECT COALESCE(MAX(version), 0)::bigint FROM engine_schema_migrations",
                &[],
            )?
            .get(0))
    }

    /// Verify the already-installed schema without issuing any DDL. Startup uses this guard;
    /// schema changes belong to the explicit `db migrate-engine` operation.
    pub fn verify_schema(&mut self) -> Result<()> {
        let migrations_table_exists: bool = self
            .client
            .query_one(
                "SELECT to_regclass('public.engine_schema_migrations') IS NOT NULL",
                &[],
            )?
            .get(0);
        if !migrations_table_exists {
            bail!(
                "engine PostgreSQL schema is missing; run `claude-api db migrate-engine` before starting the engine"
            );
        }

        let version = self.schema_version()?;
        if version < CURRENT_SCHEMA_VERSION {
            bail!(
                "engine PostgreSQL schema version {version} is older than required {CURRENT_SCHEMA_VERSION}; run `claude-api db migrate-engine`"
            );
        }
        Ok(())
    }

    pub fn claim_instance(&mut self, instance_id: &str, ttl_secs: i64) -> Result<Owner> {
        let ts = now();
        let epoch: i64 = self
            .client
            .query_one("SELECT nextval('engine_owner_epoch_seq')::bigint", &[])?
            .get(0);
        self.client.execute(
            "INSERT INTO engine_instances(instance_id, owner_epoch, lease_until, started_ts, updated_ts) \
             VALUES($1,$2,$3,$4,$4) ON CONFLICT(instance_id) DO UPDATE SET \
             owner_epoch=EXCLUDED.owner_epoch, lease_until=EXCLUDED.lease_until, \
             started_ts=EXCLUDED.started_ts, updated_ts=EXCLUDED.updated_ts",
            &[&instance_id, &epoch, &(ts + ttl_secs.max(1)), &ts],
        )?;
        Ok(Owner {
            instance_id: instance_id.to_owned(),
            epoch,
        })
    }

    pub fn heartbeat_instance(&mut self, owner: &Owner, ttl_secs: i64) -> Result<bool> {
        let ts = now();
        Ok(self.client.execute(
            "UPDATE engine_instances SET lease_until=$3, updated_ts=$4 \
             WHERE instance_id=$1 AND owner_epoch=$2",
            &[
                &owner.instance_id,
                &owner.epoch,
                &(ts + ttl_secs.max(1)),
                &ts,
            ],
        )? == 1)
    }

    fn assert_owner(tx: &mut Transaction<'_>, owner: &Owner, ts: i64) -> Result<()> {
        let valid = tx.query_opt(
            "SELECT 1 FROM engine_instances WHERE instance_id=$1 AND owner_epoch=$2 AND lease_until >= $3",
            &[&owner.instance_id, &owner.epoch, &ts],
        )?.is_some();
        if !valid {
            bail!("engine owner lease is stale or fenced");
        }
        Ok(())
    }

    /// Recheck the fence after any blocking lock acquisition and hold the owner row until commit.
    /// A concurrent `claim_instance` can then either win before this query (and fence us) or wait
    /// until this transaction has finished, but it cannot replace the epoch between the check and
    /// the money writes.
    fn assert_owner_locked(tx: &mut Transaction<'_>, owner: &Owner, ts: i64) -> Result<()> {
        let valid = tx
            .query_opt(
                "SELECT 1 FROM engine_instances
                  WHERE instance_id=$1 AND owner_epoch=$2 AND lease_until >= $3
                  FOR UPDATE",
                &[&owner.instance_id, &owner.epoch, &ts],
            )?
            .is_some();
        if !valid {
            bail!("engine owner lease is stale or fenced");
        }
        Ok(())
    }

    /// Atomically reserve money for one generated request ID. An exact retry is idempotent.
    pub fn reserve_request(
        &mut self,
        owner: &Owner,
        request_id: &str,
        account_id: &str,
        key: &str,
        hold_nano: i64,
        lease_secs: i64,
    ) -> Result<Option<i64>> {
        let hold = hold_nano.max(0);
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        if let Some(row) = tx.query_opt(
            "SELECT account_id, key, hold_nano, balance_after_reserve_nano, owner_instance, owner_epoch, state \
             FROM reservations WHERE request_id=$1",
            &[&request_id],
        )? {
            let exact = row.get::<_, String>(0) == account_id
                && row.get::<_, String>(1) == key
                && row.get::<_, i64>(2) == hold
                && row.get::<_, String>(4) == owner.instance_id
                && row.get::<_, i64>(5) == owner.epoch
                && row.get::<_, String>(6) == "reserved";
            if !exact {
                bail!("reservation request ID belongs to a different or completed operation");
            }
            let balance = row.get(3);
            tx.commit()?;
            return Ok(Some(balance));
        }
        // Овердрафт-буфер: funded-запрос НЕ роняем из-за гонки конкурентных резервов. Пускаем, пока
        // ПОСЛЕ-баланс не ниже пола −OVERDRAFT_NANO (`balance-hold >= -OVERDRAFT` ⇔ `balance >= hold-OVERDRAFT`).
        // Гейт атомарен на строке аккаунта → суммарный баланс НИКОГДА не уходит ниже −$1 даже под
        // конкуренцией (каждый успешный резерв гарантирует post_balance ≥ −$1; за полом любой h>0 отбит).
        // Стоимость: аккаунт может получить максимум $1 в долг (per-account, не per-request) — принятый
        // размен на «ноль ложных 402». Синхронно с `metering::OVERDRAFT_NANO`.
        // $1 per-account floor.
        const OVERDRAFT_NANO: i64 = 1_000_000_000;
        // Гейт `balance-hold >= -OVERDRAFT` пишем как `balance + OVERDRAFT >= hold`: вычитание двух
        // bind-параметров Postgres не типизирует, а сложение с bigint-колонкой выводит тип параметра.
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1, reserved_nano=reserved_nano+$1 \
             WHERE id=$2 AND status='active' AND balance_nano + $3 >= $1 RETURNING balance_nano",
            &[&hold, &account_id, &OVERDRAFT_NANO],
        )?
        else {
            tx.rollback()?;
            return Ok(None);
        };
        let balance: i64 = row.get(0);
        let key_updated = tx.execute(
            "UPDATE api_keys SET reserved_nano=reserved_nano+$1 \
             WHERE key=$2 AND account_id=$3 AND status='active' \
               AND (expires_ts IS NULL OR expires_ts>floor(EXTRACT(EPOCH FROM clock_timestamp()))::bigint) \
               AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+$1<=spend_limit_nano)",
            &[&hold, &key, &account_id],
        )?;
        if key_updated != 1 {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO reservations(request_id,account_id,key,hold_nano,balance_after_reserve_nano, \
             owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,'reserved',$9,$9)",
            &[&request_id, &account_id, &key, &hold, &balance, &owner.instance_id,
              &owner.epoch, &(ts + lease_secs.max(1)), &ts],
        )?;
        tx.commit()?;
        Ok(Some(balance))
    }

    /// Atomically reserve the charged legacy hold and persist its immutable pricing identity.
    ///
    /// This method has no production caller in Stage 3B1c.1. The established `reserve_request`
    /// method remains unchanged for all live traffic; an existing reservation without a snapshot
    /// is never backfilled because that would invent atomic attribution after the money commit.
    pub fn reserve_request_with_legacy_snapshot(
        &mut self,
        owner: &Owner,
        key: &str,
        lease_secs: i64,
        snapshot: &crate::pricing::LegacyScalarAdmissionSnapshot,
    ) -> Result<crate::pricing::LegacyScalarReserveOutcome> {
        self.reserve_request_with_legacy_snapshot_guarded(owner, key, lease_secs, snapshot, || true)
    }

    /// Guarded async-handoff primitive. The caller-owned gate is evaluated only for a successful
    /// insert or exact replay, after every fallible write/fence check and immediately before
    /// commit. A rejected gate rolls back this attempt without compensating a committed reserve.
    pub fn reserve_request_with_legacy_snapshot_guarded(
        &mut self,
        owner: &Owner,
        key: &str,
        lease_secs: i64,
        snapshot: &crate::pricing::LegacyScalarAdmissionSnapshot,
        mut commit_gate: impl FnMut() -> bool,
    ) -> Result<crate::pricing::LegacyScalarReserveOutcome> {
        use crate::pricing::{
            LegacyScalarReserveConflict as Conflict, LegacyScalarReserveOutcome as Outcome,
            LegacyScalarReserveReceipt as Receipt, LegacyScalarSnapshotLookup as Lookup,
        };

        snapshot.validate()?;
        if key.trim().is_empty() || lease_secs <= 0 {
            bail!("invalid PostgreSQL legacy snapshot reservation parameters");
        }
        let window_conflict = |trusted_now_ts| -> Result<Option<Conflict>> {
            match snapshot.validate_idempotency_window_at(trusted_now_ts) {
                Ok(()) => Ok(None),
                Err(crate::pricing::LegacyScalarIdempotencyWindowError::Expired) => {
                    Ok(Some(Conflict::ExpiredIdempotencyWindow))
                }
                Err(crate::pricing::LegacyScalarIdempotencyWindowError::AdmissionFromFuture) => {
                    Ok(Some(Conflict::AdmissionTimestampInFuture))
                }
                Err(
                    crate::pricing::LegacyScalarIdempotencyWindowError::InvalidTrustedTimestamp,
                ) => bail!("trusted PostgreSQL reservation clock is invalid"),
            }
        };
        let preflight_ts = now();
        if let Some(conflict) = window_conflict(preflight_ts)? {
            return Ok(Outcome::Conflict(conflict));
        }
        let request_id = snapshot.request_id.as_str();
        let account_id = snapshot.account_id.as_str();
        let hold = snapshot.charged_hold_nano;
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, preflight_ts)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        let fence_ts = now();
        Self::assert_owner_locked(&mut tx, owner, fence_ts)?;
        if let Some(conflict) = window_conflict(fence_ts)? {
            tx.rollback()?;
            return Ok(Outcome::Conflict(conflict));
        }
        if let Some(row) = tx.query_opt(
            "SELECT account_id,key,hold_nano,balance_after_reserve_nano,owner_instance,
                    owner_epoch,state
               FROM reservations
              WHERE request_id=$1
              FOR UPDATE",
            &[&request_id],
        )? {
            let stored_account: String = row.get(0);
            let stored_key: String = row.get(1);
            let stored_hold: i64 = row.get(2);
            let balance: i64 = row.get(3);
            let stored_owner: String = row.get(4);
            let stored_epoch: i64 = row.get(5);
            let state: String = row.get(6);
            let outcome = if stored_account != account_id
                || stored_key != key
                || stored_hold != hold
                || stored_owner != owner.instance_id
                || stored_epoch != owner.epoch
            {
                Outcome::Conflict(Conflict::ReservationIdentity)
            } else if state != "reserved" && state != "delivering" {
                Outcome::Conflict(Conflict::TerminalReservation)
            } else {
                match crate::pricing::postgres::postgres_legacy_scalar_snapshot_lookup(
                    &mut tx, request_id,
                )? {
                    Lookup::Missing => {
                        Outcome::Conflict(Conflict::ExistingReservationWithoutSnapshot)
                    }
                    Lookup::NonLegacy => Outcome::Conflict(Conflict::ExistingNonLegacySnapshot),
                    Lookup::Legacy(stored) if stored.as_ref() == snapshot => {
                        Outcome::Unchanged(Receipt {
                            balance_after_reserve_nano: balance,
                            snapshot: *stored,
                        })
                    }
                    Lookup::Legacy(_) => Outcome::Conflict(Conflict::SnapshotPayload),
                }
            };
            if matches!(&outcome, Outcome::Unchanged(_)) {
                Self::assert_owner_locked(&mut tx, owner, now())?;
                if !commit_gate() {
                    tx.rollback()?;
                    return Ok(Outcome::AbortedBeforeCommit);
                }
            }
            tx.commit()?;
            return Ok(outcome);
        }

        let reservation_ts = now();
        Self::assert_owner_locked(&mut tx, owner, reservation_ts)?;
        if let Some(conflict) = window_conflict(reservation_ts)? {
            tx.rollback()?;
            return Ok(Outcome::Conflict(conflict));
        }
        const OVERDRAFT_NANO: i64 = 1_000_000_000;
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1
              WHERE id=$2 AND status='active' AND balance_nano+$3 >= $1
              RETURNING balance_nano",
            &[&hold, &account_id, &OVERDRAFT_NANO],
        )?
        else {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        };
        let balance: i64 = row.get(0);
        let key_updated = tx.execute(
            "UPDATE api_keys SET reserved_nano=reserved_nano+$1
              WHERE key=$2 AND account_id=$3 AND status='active'
                AND (expires_ts IS NULL OR expires_ts>floor(EXTRACT(EPOCH FROM clock_timestamp()))::bigint)
                AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+$1<=spend_limit_nano)",
            &[&hold, &key, &account_id],
        )?;
        if key_updated != 1 {
            tx.rollback()?;
            return Ok(Outcome::NotReserved);
        }
        tx.execute(
            "INSERT INTO reservations(request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                 owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,'reserved',$9,$9)",
            &[
                &request_id,
                &account_id,
                &key,
                &hold,
                &balance,
                &owner.instance_id,
                &owner.epoch,
                &(reservation_ts.saturating_add(lease_secs)),
                &reservation_ts,
            ],
        )?;
        if let Err(error) =
            crate::pricing::postgres::postgres_insert_legacy_scalar_admission_snapshot(
                &mut tx, snapshot,
            )
        {
            let _ = tx.rollback();
            return Err(error);
        }
        Self::assert_owner_locked(&mut tx, owner, now())?;
        if !commit_gate() {
            tx.rollback()?;
            return Ok(Outcome::AbortedBeforeCommit);
        }
        tx.commit()?;
        Ok(Outcome::Inserted(Receipt {
            balance_after_reserve_nano: balance,
            snapshot: snapshot.clone(),
        }))
    }

    pub fn legacy_scalar_admission_snapshot(
        &mut self,
        request_id: &str,
    ) -> Result<Option<crate::pricing::LegacyScalarAdmissionSnapshot>> {
        use crate::pricing::LegacyScalarSnapshotLookup as Lookup;

        match crate::pricing::postgres::postgres_legacy_scalar_snapshot_lookup(
            &mut self.client,
            request_id,
        )? {
            Lookup::Missing => Ok(None),
            Lookup::Legacy(snapshot) => Ok(Some(*snapshot)),
            Lookup::NonLegacy => {
                bail!("pricing admission snapshot is not a legacy scalar snapshot")
            }
        }
    }

    /// Mark that a successful upstream response is about to be delivered. Recovery charges the hold
    /// for an expired `delivering` reservation rather than making delivered provider usage free.
    pub fn mark_delivering(
        &mut self,
        owner: &Owner,
        request_id: &str,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "UPDATE reservations SET state='delivering', lease_until=$4, updated_ts=$3 \
             WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$5 AND state='reserved'",
            &[
                &request_id,
                &owner.instance_id,
                &ts,
                &(ts + lease_secs.max(1)),
                &owner.epoch,
            ],
        )?;
        let ok = changed == 1 || tx.query_opt(
            "SELECT 1 FROM reservations WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$3 \
             AND state IN ('delivering','settlement_pending','settled')",
            &[&request_id, &owner.instance_id, &owner.epoch],
        )?.is_some();
        tx.commit()?;
        Ok(ok)
    }

    /// Renew both durable request and capacity leases for a live response stream.
    pub fn renew_stream_leases(
        &mut self,
        owner: &Owner,
        request_id: Option<&str>,
        capacity_lease_id: Option<&str>,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let lease_until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let mut valid = true;
        if let Some(request_id) = request_id {
            valid &= tx.execute(
                "UPDATE reservations SET lease_until=$4,updated_ts=$3 \
                 WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$5 \
                   AND state IN ('reserved','delivering','settlement_pending')",
                &[
                    &request_id,
                    &owner.instance_id,
                    &ts,
                    &lease_until,
                    &owner.epoch,
                ],
            )? == 1;
        }
        if let Some(lease_id) = capacity_lease_id {
            valid &= tx.execute(
                "UPDATE capacity_leases SET lease_until=$4 \
                 WHERE lease_id=$1 AND owner_instance=$2 AND owner_epoch=$3 AND state='active'",
                &[&lease_id, &owner.instance_id, &owner.epoch, &lease_until],
            )? == 1;
        }
        tx.commit()?;
        Ok(valid)
    }

    fn enqueue_outbox(
        &mut self,
        request_id: &str,
        actual_nano: i64,
        disposition: &str,
        reference: Option<&str>,
        usage: Option<&UsageEventInput>,
    ) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        let reservation = tx
            .query_opt(
                "SELECT hold_nano, state FROM reservations WHERE request_id=$1 FOR UPDATE",
                &[&request_id],
            )?
            .context("settlement reservation does not exist")?;
        let state: String = reservation.get(1);
        let actual = actual_nano.max(0);
        let u = usage.cloned().unwrap_or_default();
        let inserted = tx.execute(
            "INSERT INTO settlement_outbox(request_id,actual_nano,disposition,reference,model,input_tokens, \
             output_tokens,cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
             real_nano,speed,inference_geo,input_nano,output_nano,cache_read_nano,cache_write_5m_nano, \
             cache_write_1h_nano,web_search_nano,priced_ts,provider,state,created_ts,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22, \
                    'pending',$23,$23) \
             ON CONFLICT(request_id) DO NOTHING",
            &[&request_id, &actual, &disposition, &reference, &u.model, &u.input_tokens,
              &u.output_tokens, &u.cache_read_tokens, &u.cache_write_5m_tokens,
              &u.cache_write_1h_tokens, &u.web_search_requests, &u.real_nano, &u.speed,
              &u.inference_geo, &u.input_nano, &u.output_nano, &u.cache_read_nano,
              &u.cache_write_5m_nano, &u.cache_write_1h_nano, &u.web_search_nano, &u.priced_ts,
              &u.provider, &ts],
        )?;
        if inserted == 0 {
            let row = tx.query_one(
                "SELECT actual_nano,disposition,reference,model,input_tokens,output_tokens,cache_read_tokens, \
                 cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano,speed, \
                 inference_geo,input_nano,output_nano,cache_read_nano,cache_write_5m_nano, \
                 cache_write_1h_nano,web_search_nano,priced_ts \
                 FROM settlement_outbox WHERE request_id=$1",
                &[&request_id],
            )?;
            let exact = row.get::<_, i64>(0) == actual
                && row.get::<_, String>(1) == disposition
                && row.get::<_, Option<String>>(2).as_deref() == reference
                && row.get::<_, String>(3) == u.model
                && row.get::<_, i64>(4) == u.input_tokens
                && row.get::<_, i64>(5) == u.output_tokens
                && row.get::<_, i64>(6) == u.cache_read_tokens
                && row.get::<_, i64>(7) == u.cache_write_5m_tokens
                && row.get::<_, i64>(8) == u.cache_write_1h_tokens
                && row.get::<_, i64>(9) == u.web_search_requests
                && row.get::<_, i64>(10) == u.real_nano
                && row.get::<_, String>(11) == u.speed
                && row.get::<_, String>(12) == u.inference_geo
                && row.get::<_, i64>(13) == u.input_nano
                && row.get::<_, i64>(14) == u.output_nano
                && row.get::<_, i64>(15) == u.cache_read_nano
                && row.get::<_, i64>(16) == u.cache_write_5m_nano
                && row.get::<_, i64>(17) == u.cache_write_1h_nano
                && row.get::<_, i64>(18) == u.web_search_nano
                && row.get::<_, i64>(19) == u.priced_ts;
            if !exact {
                bail!("settlement request ID conflicts with different outbox payload");
            }
        }
        if !matches!(state.as_str(), "settled" | "canceled") {
            tx.execute(
                "UPDATE reservations SET state='settlement_pending', actual_nano=$2, updated_ts=$3 \
                 WHERE request_id=$1",
                &[&request_id, &actual, &ts],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn enqueue_settlement(
        &mut self,
        request_id: &str,
        actual_nano: i64,
        reference: Option<&str>,
        usage: Option<&UsageEventInput>,
    ) -> Result<()> {
        self.enqueue_outbox(request_id, actual_nano, "settle", reference, usage)
    }

    pub fn enqueue_cancel(&mut self, request_id: &str) -> Result<()> {
        self.enqueue_outbox(request_id, 0, "cancel", None, None)
    }

    fn process_outbox_request(&mut self, request_id: &str) -> Result<Option<i64>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&request_id],
        )?;
        let Some(row) = tx.query_opt(
            "SELECT o.actual_nano,o.disposition,o.reference,o.model,o.input_tokens,o.output_tokens, \
             o.cache_read_tokens,o.cache_write_5m_tokens,o.cache_write_1h_tokens,o.web_search_requests, \
             o.real_nano,o.speed,o.inference_geo,o.input_nano,o.output_nano,o.cache_read_nano, \
             o.cache_write_5m_nano,o.cache_write_1h_nano,o.web_search_nano,o.priced_ts,o.provider, \
             o.state,r.account_id,r.key,r.hold_nano,r.state \
             FROM settlement_outbox o JOIN reservations r USING(request_id) \
             WHERE o.request_id=$1 FOR UPDATE OF o,r",
            &[&request_id],
        )? else {
            tx.rollback()?;
            return Ok(None);
        };
        let provider: String = row.get(20);
        let outbox_state: String = row.get(21);
        let reservation_state: String = row.get(25);
        let account_id: String = row.get(22);
        if outbox_state == "done" || matches!(reservation_state.as_str(), "settled" | "canceled") {
            let balance = tx
                .query_opt(
                    "SELECT balance_nano FROM accounts WHERE id=$1",
                    &[&account_id],
                )?
                .map(|r| r.get(0));
            tx.execute(
                "UPDATE settlement_outbox SET state='done', committed_ts=COALESCE(committed_ts,$2),updated_ts=$2 \
                 WHERE request_id=$1",
                &[&request_id, &ts],
            )?;
            tx.commit()?;
            return Ok(balance);
        }
        let actual: i64 = row.get(0);
        let disposition: String = row.get(1);
        let reference: Option<String> = row.get(2);
        let model: String = row.get(3);
        let account_key: String = row.get(23);
        let hold: i64 = row.get(24);
        let balance: i64 = tx.query_one(
            "UPDATE accounts SET balance_nano=balance_nano+$1-$2, spent_nano=spent_nano+$2, \
             reserved_nano=reserved_nano-$1 WHERE id=$3 AND reserved_nano >= $1 RETURNING balance_nano",
            &[&hold, &actual, &account_id],
        ).context("reservation/account aggregate invariant failed")?.get(0);
        let key_updated = tx.execute(
            "UPDATE api_keys SET spent_nano=spent_nano+$1, \
             reserved_nano=CASE WHEN reserved_nano >= $2 THEN reserved_nano-$2 ELSE reserved_nano END \
             WHERE key=$3 AND (reserved_nano >= $2 OR spend_limit_nano IS NULL)",
            &[&actual, &hold, &account_key],
        )?;
        if key_updated != 1 {
            let key_still_exists = tx
                .query_opt("SELECT 1 FROM api_keys WHERE key=$1", &[&account_key])?
                .is_some();
            if key_still_exists {
                bail!("reservation/key aggregate invariant failed");
            }
        }
        if actual > 0 {
            tx.execute(
                "INSERT INTO ledger(account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model) \
                 VALUES($1,$2,'charge',$3,$4,$5,$6,$7,NULLIF($8,'')) ON CONFLICT DO NOTHING",
                &[&account_id, &account_key, &request_id, &actual, &reference, &balance, &ts, &model],
            )?;
            if !model.is_empty() {
                let input_tokens: i64 = row.get(4);
                let output_tokens: i64 = row.get(5);
                let cache_read_tokens: i64 = row.get(6);
                let cache_write_5m_tokens: i64 = row.get(7);
                let cache_write_1h_tokens: i64 = row.get(8);
                let web_search_requests: i64 = row.get(9);
                let real_nano: i64 = row.get(10);
                let speed: String = row.get(11);
                let inference_geo: String = row.get(12);
                let input_nano: i64 = row.get(13);
                let output_nano: i64 = row.get(14);
                let cache_read_nano: i64 = row.get(15);
                let cache_write_5m_nano: i64 = row.get(16);
                let cache_write_1h_nano: i64 = row.get(17);
                let web_search_nano: i64 = row.get(18);
                let priced_ts: i64 = row.get(19);
                tx.execute(
                    "INSERT INTO usage_events(request_id,account_id,key,model,input_tokens,output_tokens, \
                     cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
                     real_nano,charge_nano,ref,ts,speed,inference_geo,input_nano,output_nano,cache_read_nano, \
                     cache_write_5m_nano,cache_write_1h_nano,web_search_nano,priced_ts,provider) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24) \
                     ON CONFLICT(request_id) DO NOTHING",
                    &[&request_id,&account_id,&account_key,&model,&input_tokens,&output_tokens,
                      &cache_read_tokens,&cache_write_5m_tokens,&cache_write_1h_tokens,
                      &web_search_requests,&real_nano,&actual,&reference,&ts,&speed,&inference_geo,
                      &input_nano,&output_nano,&cache_read_nano,&cache_write_5m_nano,
                      &cache_write_1h_nano,&web_search_nano,&priced_ts,&provider],
                )?;
            }
        }
        let final_state = if disposition == "cancel" {
            "canceled"
        } else {
            "settled"
        };
        tx.execute(
            "UPDATE reservations SET state=$2,actual_nano=$3,settled_ts=$4,updated_ts=$4 WHERE request_id=$1",
            &[&request_id, &final_state, &actual, &ts],
        )?;
        tx.execute(
            "UPDATE settlement_outbox SET state='done',attempts=attempts+1,committed_ts=$2,updated_ts=$2, \
             last_error=NULL WHERE request_id=$1",
            &[&request_id, &ts],
        )?;
        tx.commit()?;
        Ok(Some(balance))
    }

    pub fn settle_request(
        &mut self,
        request_id: &str,
        actual_nano: i64,
        reference: Option<&str>,
        usage: Option<&UsageEventInput>,
    ) -> Result<Option<i64>> {
        self.enqueue_settlement(request_id, actual_nano, reference, usage)?;
        self.process_outbox_request(request_id)
    }

    pub fn cancel_request(&mut self, request_id: &str) -> Result<Option<i64>> {
        self.enqueue_cancel(request_id)?;
        self.process_outbox_request(request_id)
    }

    pub fn drain_outbox(&mut self, limit: usize) -> Result<usize> {
        let ids: Vec<String> = self.client.query(
            "SELECT request_id FROM settlement_outbox WHERE state='pending' AND next_attempt_ts <= $1 \
             ORDER BY created_ts LIMIT $2",
            &[&now(), &(limit.clamp(1, 10_000) as i64)],
        )?.into_iter().map(|r| r.get(0)).collect();
        let mut done = 0;
        for id in ids {
            match self.process_outbox_request(&id) {
                Ok(_) => done += 1,
                Err(err) => {
                    let ts = now();
                    let message = format!("{err:#}");
                    let state = if classify_failure(&err) == FailureClass::Permanent {
                        "failed"
                    } else {
                        "pending"
                    };
                    let next_attempt = if state == "failed" { 0 } else { ts + 1 };
                    let _ = self.client.execute(
                        "UPDATE settlement_outbox SET state=$2,attempts=attempts+1,last_error=$3, \
                         next_attempt_ts=$4,updated_ts=$5 WHERE request_id=$1 AND state <> 'done'",
                        &[&id, &state, &message, &next_attempt, &ts],
                    );
                    if state == "failed" {
                        eprintln!("billing outbox request {id} moved to failed: {message}");
                    }
                }
            }
        }
        Ok(done)
    }

    /// Recover only reservations whose exact owner epoch is provably dead/fenced.
    pub fn reconcile_expired(&mut self, limit: usize) -> Result<ReconcileReport> {
        let ts = now();
        let rows = self.client.query(
            "SELECT r.request_id,r.state,r.hold_nano FROM reservations r \
             LEFT JOIN engine_instances i ON i.instance_id=r.owner_instance AND i.owner_epoch=r.owner_epoch \
             WHERE r.state IN ('reserved','delivering','settlement_pending') AND r.lease_until < $1 \
             AND (i.instance_id IS NULL OR i.lease_until < $1) ORDER BY r.created_ts LIMIT $2",
            &[&ts, &(limit.clamp(1, 10_000) as i64)],
        )?;
        let mut report = ReconcileReport::default();
        for row in rows {
            let request_id: String = row.get(0);
            let state: String = row.get(1);
            let hold: i64 = row.get(2);
            match state.as_str() {
                "reserved" => {
                    self.enqueue_outbox(&request_id, 0, "cancel", None, None)?;
                    report.canceled_before_delivery += 1;
                }
                "delivering" => {
                    self.enqueue_outbox(
                        &request_id,
                        hold,
                        "reconcile_full_hold",
                        Some("expired-delivery"),
                        None,
                    )?;
                    report.charged_after_delivery += 1;
                }
                "settlement_pending" => {}
                _ => continue,
            }
        }
        report.processed_outbox = self.drain_outbox(limit)?;
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn acquire_capacity(
        &mut self,
        owner: &Owner,
        lease_id: &str,
        request_id: &str,
        email: &str,
        lease_secs: i64,
        max_inflight: i64,
        util_cap: f64,
    ) -> Result<Option<CapacityLease>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        if let Some(row) = tx.query_opt(
            "SELECT request_id,subscription_email,lease_until,state,owner_instance,owner_epoch \
             FROM capacity_leases WHERE lease_id=$1",
            &[&lease_id],
        )? {
            let exact = row.get::<_, String>(0) == request_id
                && row.get::<_, String>(1) == email
                && row.get::<_, String>(3) == "active"
                && row.get::<_, String>(4) == owner.instance_id
                && row.get::<_, i64>(5) == owner.epoch;
            if !exact {
                bail!("capacity lease ID belongs to another operation");
            }
            let lease = CapacityLease {
                lease_id: lease_id.to_owned(),
                request_id: request_id.to_owned(),
                subscription_email: email.to_owned(),
                lease_until: row.get(2),
            };
            tx.commit()?;
            return Ok(Some(lease));
        }
        let expired = tx.execute(
            "UPDATE capacity_leases SET state='expired',released_ts=$2 \
             WHERE subscription_email=$1 AND state='active' AND lease_until < $2",
            &[&email, &ts],
        )? as i64;
        if expired > 0 {
            tx.execute(
                "UPDATE pool_state SET inflight=GREATEST(0,inflight-$2) WHERE email=$1",
                &[&email, &expired],
            )?;
        }
        let Some(state) = tx.query_opt(
            "SELECT cooling_until,util5,util7,reset5,reset7,inflight FROM pool_state WHERE email=$1 FOR UPDATE",
            &[&email],
        )? else {
            tx.rollback()?;
            return Ok(None);
        };
        let cooling_until: i64 = state.get(0);
        let util5: f64 = state.get(1);
        let util7: f64 = state.get(2);
        let reset5: i64 = state.get(3);
        let reset7: i64 = state.get(4);
        let inflight: i64 = state.get(5);
        let effective5 = if reset5 > 0 && reset5 <= ts {
            0.0
        } else {
            util5
        };
        let effective7 = if reset7 > 0 && reset7 <= ts {
            0.0
        } else {
            util7
        };
        if cooling_until > ts
            || effective5 >= util_cap
            || effective7 >= util_cap
            || inflight >= max_inflight.max(1)
        {
            tx.rollback()?;
            return Ok(None);
        }
        let lease_until = ts + lease_secs.max(1);
        tx.execute(
            "INSERT INTO capacity_leases(lease_id,request_id,subscription_email,owner_instance,owner_epoch, \
             lease_until,state,created_ts) VALUES($1,$2,$3,$4,$5,$6,'active',$7)",
            &[&lease_id,&request_id,&email,&owner.instance_id,&owner.epoch,&lease_until,&ts],
        )?;
        tx.execute(
            "UPDATE pool_state SET inflight=inflight+1 WHERE email=$1",
            &[&email],
        )?;
        tx.commit()?;
        Ok(Some(CapacityLease {
            lease_id: lease_id.to_owned(),
            request_id: request_id.to_owned(),
            subscription_email: email.to_owned(),
            lease_until,
        }))
    }

    pub fn release_capacity(&mut self, owner: &Owner, lease_id: &str) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let row = tx.query_opt(
            "UPDATE capacity_leases SET state='released',released_ts=$4 \
             WHERE lease_id=$1 AND owner_instance=$2 AND owner_epoch=$3 AND state='active' \
             RETURNING subscription_email",
            &[&lease_id, &owner.instance_id, &owner.epoch, &ts],
        )?;
        if let Some(row) = row {
            let email: String = row.get(0);
            tx.execute(
                "UPDATE pool_state SET inflight=GREATEST(0,inflight-1) WHERE email=$1",
                &[&email],
            )?;
            tx.commit()?;
            Ok(true)
        } else {
            tx.rollback()?;
            Ok(false)
        }
    }

    pub fn acquire_leader(&mut self, owner: &Owner, name: &str, ttl_secs: i64) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        tx.execute(
            "INSERT INTO leader_leases(name,owner_instance,owner_epoch,lease_until,updated_ts) \
             VALUES($1,$2,$3,$4,$5) ON CONFLICT(name) DO NOTHING",
            &[
                &name,
                &owner.instance_id,
                &owner.epoch,
                &(ts + ttl_secs.max(1)),
                &ts,
            ],
        )?;
        let changed = tx.execute(
            "UPDATE leader_leases SET owner_instance=$2,owner_epoch=$3,lease_until=$4,updated_ts=$5 \
             WHERE name=$1 AND ((owner_instance=$2 AND owner_epoch=$3) OR lease_until < $5)",
            &[&name,&owner.instance_id,&owner.epoch,&(ts + ttl_secs.max(1)),&ts],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    // -- Subscription registry ---------------------------------------------------------------

    pub fn load_active(&mut self, fleet: Option<&str>) -> Result<Vec<Sub>> {
        let rows = self.client.query(
            "SELECT email,token,token_file,proxy,status,fleet,plan FROM subs \
             WHERE status='active' AND ($1::text IS NULL OR fleet=$1) ORDER BY added_ts",
            &[&fleet],
        )?;
        let mut out = Vec::new();
        for row in rows {
            let email: String = row.get(0);
            let token = crate::resolve_token(row.get(1), row.get(2));
            if token.is_empty() {
                continue;
            }
            out.push(Sub {
                email,
                token,
                proxy: row.get(3),
                fleet: row.get(5),
                plan: row.get(6),
            });
        }
        Ok(out)
    }

    pub fn add(&mut self, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO subs(email,token,token_file,proxy,status,fleet,added_ts,added) \
             VALUES($1,$2,NULL,$3,'active',$4,$5,$6) ON CONFLICT(email) DO UPDATE SET \
             token=EXCLUDED.token,token_file=NULL,proxy=EXCLUDED.proxy,status='active',fleet=EXCLUDED.fleet, \
             auth_state=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 'healthy' ELSE subs.auth_state END, \
             auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.auth_fail_streak END, \
             first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.first_auth_fail_ts END, \
             last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.last_auth_fail_ts END, \
             last_auth_http=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.last_auth_http END, \
             dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN 0 ELSE subs.dead_since_ts END, \
             dead_reason=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN '' ELSE subs.dead_reason END, \
             auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>EXCLUDED.token OR COALESCE(subs.token_file,'')<>'' THEN '' ELSE subs.auth_token_fp END",
            &[&email,&token,&proxy,&fleet,&ts,&chrono_like(ts)],
        )?;
        tx.execute(
            "INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING",
            &[&email],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_file(
        &mut self,
        email: &str,
        token_file: &str,
        proxy: &str,
        fleet: &str,
    ) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO subs(email,token,token_file,proxy,status,fleet,added_ts,added) \
             VALUES($1,NULL,$2,$3,'active',$4,$5,$6) ON CONFLICT(email) DO UPDATE SET \
             token=NULL,token_file=EXCLUDED.token_file,proxy=EXCLUDED.proxy,status='active',fleet=EXCLUDED.fleet, \
             auth_state=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 'healthy' ELSE subs.auth_state END, \
             auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.auth_fail_streak END, \
             first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.first_auth_fail_ts END, \
             last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.last_auth_fail_ts END, \
             last_auth_http=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.last_auth_http END, \
             dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN 0 ELSE subs.dead_since_ts END, \
             dead_reason=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN '' ELSE subs.dead_reason END, \
             auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>EXCLUDED.token_file THEN '' ELSE subs.auth_token_fp END",
            &[&email,&token_file,&proxy,&fleet,&ts,&chrono_like(ts)],
        )?;
        tx.execute(
            "INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING",
            &[&email],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_sub_status(&mut self, email: &str, status: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE subs SET status=$1 WHERE email=$2",
            &[&status, &email],
        )? as usize)
    }
    pub fn set_plan(&mut self, email: &str, plan: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE subs SET plan=$1 WHERE email=$2", &[&plan, &email])?
            as usize)
    }
    pub fn set_proxy(&mut self, email: &str, proxy: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE subs SET proxy=$1 WHERE email=$2", &[&proxy, &email])?
            as usize)
    }
    pub fn set_fleet(&mut self, email: &str, fleet: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE subs SET fleet=$1 WHERE email=$2", &[&fleet, &email])?
            as usize)
    }
    pub fn set_proxy_meta(
        &mut self,
        email: &str,
        expire: &str,
        checked_ts: i64,
        ok: bool,
    ) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE subs SET proxy_expire=$1,proxy_checked_ts=$2,proxy_ok=$3 WHERE email=$4",
            &[&expire, &checked_ts, &ok, &email],
        )? as usize)
    }
    pub fn get_creds(&mut self, email: &str) -> Result<Option<(String, String)>> {
        let Some(row) = self.client.query_opt(
            "SELECT token,token_file,proxy FROM subs WHERE email=$1",
            &[&email],
        )?
        else {
            return Ok(None);
        };
        let token = crate::resolve_token(row.get(0), row.get(1));
        if token.is_empty() {
            Ok(None)
        } else {
            Ok(Some((token, row.get(2))))
        }
    }
    pub fn remove_sub(&mut self, email: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE subs SET status='deleted' WHERE email=$1 AND status<>'deleted'",
            &[&email],
        )? as usize)
    }
    pub fn clear_subs(&mut self, fleet: Option<&str>) -> Result<usize> {
        Ok(match fleet {
            Some(f) => self.client.execute(
                "UPDATE subs SET status='deleted' WHERE fleet=$1 AND status<>'deleted'",
                &[&f],
            )?,
            None => self.client.execute(
                "UPDATE subs SET status='deleted' WHERE status<>'deleted'",
                &[],
            )?,
        } as usize)
    }
    pub fn list_subs(&mut self) -> Result<Vec<SubRow>> {
        Ok(self.client.query(
            "SELECT email,status,fleet,plan,COALESCE(NULLIF(token,''),NULLIF(token_file,'')),proxy \
             FROM subs ORDER BY added_ts",
            &[],
        )?.into_iter().map(|row| SubRow {
            email: row.get(0), status: row.get(1), fleet: row.get(2), plan: row.get(3),
            has_token: row.get::<_, Option<String>>(4).is_some_and(|s| !s.is_empty()), proxy: row.get(5),
        }).collect())
    }
    pub fn subs_admin(&mut self) -> Result<Vec<SubAdmin>> {
        Ok(self.client.query(
            "SELECT email,status,fleet,COALESCE(NULLIF(token,''),NULLIF(token_file,'')),proxy, \
             proxy_expire,proxy_ok,added_ts,added, \
             COALESCE(auth_state,'healthy'),COALESCE(dead_reason,''),COALESCE(dead_since_ts,0) \
             FROM subs ORDER BY added_ts",
            &[],
        )?.into_iter().map(|row| {
            let proxy: String = row.get(4);
            SubAdmin {
                email: row.get(0), status: row.get(1), fleet: row.get(2),
                has_token: row.get::<_, Option<String>>(3).is_some_and(|s| !s.is_empty()),
                proxy_host: mask_proxy(&proxy), proxy_expire: row.get(5), proxy_ok: row.get(6),
                added_ts: row.get(7), added: row.get(8),
                auth_state: row.get(9), dead_reason: row.get(10), dead_since_ts: row.get(11),
            }
        }).collect())
    }

    /// Load durable auth-health for every subscription (engine seeds in-memory state at startup).
    pub fn load_sub_health(&mut self, fleet: Option<&str>) -> Result<Vec<SubHealth>> {
        Ok(self.client.query(
            "SELECT email,COALESCE(auth_state,'healthy'),COALESCE(auth_fail_streak,0), \
             COALESCE(first_auth_fail_ts,0),COALESCE(last_auth_fail_ts,0),COALESCE(last_auth_http,0), \
             COALESCE(dead_since_ts,0),COALESCE(dead_reason,''),COALESCE(auth_token_fp,'') \
             FROM subs WHERE ($1::text IS NULL OR fleet=$1) ORDER BY added_ts",
            &[&fleet],
        )?.into_iter().map(|row| SubHealth {
            email: row.get(0),
            auth_state: row.get(1),
            auth_fail_streak: row.get::<_, i32>(2) as i64,
            first_auth_fail_ts: row.get(3),
            last_auth_fail_ts: row.get(4),
            last_auth_http: row.get::<_, i32>(5) as i64,
            dead_since_ts: row.get(6),
            dead_reason: row.get(7),
            auth_token_fp: row.get(8),
        }).collect())
    }

    /// Persist one subscription's durable auth-health verdict. Owner-fenced: a stale/fenced engine
    /// (lost the epoch) must not stamp health, exactly like money/pool-state writes.
    pub fn save_sub_health(&mut self, owner: &Owner, h: &SubHealth) -> Result<usize> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let streak = h.auth_fail_streak as i32;
        let http = h.last_auth_http as i32;
        let first = (h.first_auth_fail_ts != 0).then_some(h.first_auth_fail_ts);
        let last = (h.last_auth_fail_ts != 0).then_some(h.last_auth_fail_ts);
        let http = (http != 0).then_some(http);
        let dead_since = (h.dead_since_ts != 0).then_some(h.dead_since_ts);
        let reason = (!h.dead_reason.is_empty()).then_some(h.dead_reason.as_str());
        let fp = (!h.auth_token_fp.is_empty()).then_some(h.auth_token_fp.as_str());
        let n = tx.execute(
            "UPDATE subs SET auth_state=$1,auth_fail_streak=$2,first_auth_fail_ts=$3, \
             last_auth_fail_ts=$4,last_auth_http=$5,dead_since_ts=$6,dead_reason=$7,auth_token_fp=$8 \
             WHERE email=$9",
            &[&h.auth_state,&streak,&first,&last,&http,&dead_since,&reason,&fp,&h.email],
        )?;
        tx.commit()?;
        Ok(n as usize)
    }

    // -- Accounts, keys, ledger, analytics ---------------------------------------------------

    pub fn account_create(&mut self, id: &str, handle: Option<&str>, mult_bp: i64) -> Result<()> {
        if id.trim().is_empty() || handle.is_some_and(|value| value.trim().is_empty()) {
            bail!("account id and supplied handle must not be empty");
        }
        if !(0..=10_000).contains(&mult_bp) {
            bail!("account multiplier must be within 0..=10000 basis points");
        }
        let ts = now();
        self.client.execute(
            "INSERT INTO accounts(id,handle,mult_bp,status,created_ts,created) VALUES($1,$2,$3,'active',$4,$5)",
            &[&id,&handle,&mult_bp,&ts,&chrono_like(ts)],
        )?;
        Ok(())
    }
    pub fn account_get(&mut self, id: &str) -> Result<Option<AccountRow>> {
        Ok(self.client.query_opt(
            "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status FROM accounts WHERE id=$1",
            &[&id],
        )?.map(|r| account_row(&r)))
    }
    pub fn account_by_handle(&mut self, handle: &str) -> Result<Option<AccountRow>> {
        Ok(self.client.query_opt(
            "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status FROM accounts WHERE handle=$1",
            &[&handle],
        )?.map(|r| account_row(&r)))
    }
    pub fn account_list(&mut self) -> Result<Vec<AccountRow>> {
        Ok(self.client.query(
            "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status FROM accounts ORDER BY created_ts",
            &[],
        )?.into_iter().map(|r| account_row(&r)).collect())
    }
    pub fn account_set_status(&mut self, id: &str, status: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("UPDATE accounts SET status=$1 WHERE id=$2", &[&status, &id])?
            as usize)
    }
    pub fn account_set_mult_bp(&mut self, id: &str, mult_bp: i64) -> Result<usize> {
        if !(0..=10_000).contains(&mult_bp) {
            bail!("invalid account multiplier");
        }
        Ok(self.client.execute(
            "UPDATE accounts SET mult_bp=$1 WHERE id=$2",
            &[&mult_bp, &id],
        )? as usize)
    }
    pub fn account_remove(&mut self, id: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE accounts SET status='deleted' WHERE id=$1 AND status<>'deleted'",
            &[&id],
        )? as usize)
    }

    pub fn account_topup(
        &mut self,
        id: &str,
        amount_nano: i64,
        reference: Option<&str>,
    ) -> Result<Option<i64>> {
        if matches!(reference, Some(r) if r.trim().is_empty()) {
            bail!("monetary idempotency reference must not be empty");
        }
        let ts = now();
        let kind = if amount_nano >= 0 { "topup" } else { "adjust" };
        let mut tx = self.client.transaction()?;
        if let Some(reference) = reference {
            if let Some(row) = tx.query_opt(
                "SELECT account_id,kind,amount_nano,balance_after_nano FROM ledger \
                 WHERE ref=$1 AND kind IN ('topup','adjust')",
                &[&reference],
            )? {
                let exact = row.get::<_, String>(0) == id
                    && row.get::<_, String>(1) == kind
                    && row.get::<_, i64>(2) == amount_nano;
                if !exact {
                    bail!("idempotency reference belongs to another monetary operation");
                }
                let original = row.get(3);
                tx.commit()?;
                return Ok(original);
            }
        }
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano+$1 WHERE id=$2 RETURNING balance_nano",
            &[&amount_nano, &id],
        )?
        else {
            tx.rollback()?;
            return Ok(None);
        };
        let balance: i64 = row.get(0);
        tx.execute(
            "INSERT INTO ledger(account_id,kind,amount_nano,ref,balance_after_nano,ts) \
             VALUES($1,$2,$3,$4,$5,$6)",
            &[&id, &kind, &amount_nano, &reference, &balance, &ts],
        )?;
        tx.commit()?;
        Ok(Some(balance))
    }

    pub fn key_issue(&mut self, key: &str, account_id: &str, label: Option<&str>) -> Result<()> {
        self.key_issue_with_policy(key, account_id, label, None, None)
    }
    pub fn key_issue_with_policy(
        &mut self,
        key: &str,
        account_id: &str,
        label: Option<&str>,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> Result<()> {
        if key.trim().is_empty() || account_id.trim().is_empty() {
            bail!("key and account id must not be empty");
        }
        let ts = now();
        let changed = self.client.execute(
            "INSERT INTO api_keys(key,key_id,account_id,label,spend_limit_nano,expires_ts,status,created_ts,created) \
             VALUES($1,'key_' || md5(random()::text || clock_timestamp()::text),$2,$3,$4,$5,'active',$6,$7) \
             ON CONFLICT(key) DO UPDATE SET label=EXCLUDED.label, \
             spend_limit_nano=EXCLUDED.spend_limit_nano,expires_ts=EXCLUDED.expires_ts \
             WHERE api_keys.account_id=EXCLUDED.account_id",
            &[&key,&account_id,&label,&spend_limit_nano,&expires_ts,&ts,&chrono_like(ts)],
        )?;
        if changed == 0 {
            bail!("key is already owned by another account");
        }
        Ok(())
    }
    pub fn key_account(&mut self, key: &str) -> Result<Option<KeyAuth>> {
        Ok(self
            .client
            .query_opt(
                "SELECT a.id,a.mult_bp,a.balance_nano,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,(k.status='active' AND a.status='active') \
             FROM api_keys k JOIN accounts a ON a.id=k.account_id WHERE k.key=$1",
                &[&key],
            )?
            .map(|row| KeyAuth {
                account_id: row.get(0),
                mult_bp: row.get(1),
                balance_nano: row.get(2),
                spent_nano: row.get(3),
                reserved_nano: row.get(4),
                spend_limit_nano: row.get(5),
                expires_ts: row.get(6),
                active: row.get(7),
            }))
    }
    pub fn key_get(&mut self, key: &str) -> Result<Option<KeyRow>> {
        Ok(self.client.query_opt(
            "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,k.created_ts, \
             (SELECT MAX(u.ts) FROM usage_events u WHERE u.account_id=k.account_id AND u.key=k.key), \
             k.status \
             FROM api_keys k WHERE k.key=$1",
            &[&key],
        )?.map(|r| key_row(&r)))
    }
    pub fn key_list(&mut self) -> Result<Vec<KeyRow>> {
        Ok(self
            .client
            .query(
                "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,k.created_ts,u.last_used_ts,k.status \
             FROM api_keys k LEFT JOIN ( \
               SELECT key,MAX(ts) AS last_used_ts FROM usage_events GROUP BY key \
             ) u ON u.key=k.key ORDER BY k.created_ts",
                &[],
            )?
            .into_iter()
            .map(|r| key_row(&r))
            .collect())
    }
    pub fn keys_by_account(&mut self, account_id: &str) -> Result<Vec<KeyRow>> {
        Ok(self.client.query(
            "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,k.created_ts,u.last_used_ts,k.status \
             FROM api_keys k LEFT JOIN ( \
               SELECT key,MAX(ts) AS last_used_ts FROM usage_events WHERE account_id=$1 GROUP BY key \
             ) u ON u.key=k.key WHERE k.account_id=$1 ORDER BY k.created_ts",
            &[&account_id],
        )?.into_iter().map(|r| key_row(&r)).collect())
    }
    pub fn key_set_status(&mut self, key: &str, status: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE api_keys SET status=$1 WHERE key=$2",
            &[&status, &key],
        )? as usize)
    }
    pub fn key_set_status_by_id(&mut self, key_id: &str, status: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE api_keys SET status=$1 WHERE key_id=$2",
            &[&status, &key_id],
        )? as usize)
    }
    pub fn key_set_label_by_id(&mut self, key_id: &str, label: &str) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE api_keys SET label=$1 WHERE key_id=$2",
            &[&label, &key_id],
        )? as usize)
    }
    pub fn key_set_policy_by_id(
        &mut self,
        account_id: &str,
        key_id: &str,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> Result<KeyPolicyUpdate> {
        let updated = self.client.execute(
            "UPDATE api_keys SET spend_limit_nano=$3,expires_ts=$4 \
             WHERE key_id=$1 AND account_id=$2 \
               AND ($3::bigint IS NULL OR (reserved_nano<=$3 AND spent_nano<=$3-reserved_nano)) \
               AND ($4::bigint IS NULL OR $4>EXTRACT(EPOCH FROM clock_timestamp())::bigint)",
            &[&key_id, &account_id, &spend_limit_nano, &expires_ts],
        )?;
        if updated == 1 {
            return Ok(KeyPolicyUpdate::Updated);
        }
        let exists: bool = self
            .client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key_id=$1 AND account_id=$2)",
                &[&key_id, &account_id],
            )?
            .get(0);
        if !exists {
            return Ok(KeyPolicyUpdate::NotFound);
        }
        if expires_ts.is_some_and(|expires| expires <= now()) {
            return Ok(KeyPolicyUpdate::ExpiryNotFuture);
        }
        Ok(KeyPolicyUpdate::LimitBelowUsage)
    }
    pub fn key_remove(&mut self, key: &str) -> Result<usize> {
        Ok(self
            .client
            .execute("DELETE FROM api_keys WHERE key=$1", &[&key])? as usize)
    }
    pub fn key_clear(&mut self) -> Result<usize> {
        Ok(self.client.execute("DELETE FROM api_keys", &[])? as usize)
    }

    pub fn ledger_recent(&mut self, account_id: &str, limit: i64) -> Result<Vec<LedgerRow>> {
        Ok(self
            .client
            .query(
                "SELECT id,key,kind,amount_nano,ref,balance_after_nano,ts,model FROM ledger \
             WHERE account_id=$1 ORDER BY id DESC LIMIT $2",
                &[&account_id, &limit.clamp(1, 1000)],
            )?
            .into_iter()
            .map(|r| ledger_row(&r))
            .collect())
    }
    pub fn ledger_after(
        &mut self,
        account_id: &str,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<LedgerRow>> {
        Ok(self
            .client
            .query(
                "SELECT id,key,kind,amount_nano,ref,balance_after_nano,ts,model FROM ledger \
             WHERE account_id=$1 AND id>$2 ORDER BY id LIMIT $3",
                &[&account_id, &after_id.max(0), &limit.clamp(1, 1000)],
            )?
            .into_iter()
            .map(|r| ledger_row(&r))
            .collect())
    }
    pub fn ledger_ack(
        &mut self,
        consumer: &str,
        account_id: &str,
        last_ledger_id: i64,
    ) -> Result<usize> {
        if consumer.trim().is_empty() || last_ledger_id < 0 {
            bail!("invalid ledger checkpoint");
        }
        Ok(self.client.execute(
            "INSERT INTO ledger_consumer_checkpoints(consumer,account_id,last_ledger_id,updated_ts) \
             VALUES($1,$2,$3,$4) ON CONFLICT(consumer,account_id) DO UPDATE SET \
             last_ledger_id=GREATEST(ledger_consumer_checkpoints.last_ledger_id,EXCLUDED.last_ledger_id), \
             updated_ts=EXCLUDED.updated_ts",
            &[&consumer,&account_id,&last_ledger_id,&now()],
        )? as usize)
    }
    pub fn ledger_prune(&mut self, older_than_ts: i64) -> Result<usize> {
        Ok(self.client.execute(
            "DELETE FROM ledger WHERE id IN ( \
               SELECT l.id FROM ledger l JOIN ledger_consumer_checkpoints c \
                 ON c.account_id=l.account_id AND c.consumer='pricing' \
               WHERE l.kind='charge' AND l.ts < $1 AND l.id <= c.last_ledger_id \
               ORDER BY l.id LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize)
    }
    pub fn usage_by_model(
        &mut self,
        account_id: &str,
        since_ts: i64,
    ) -> Result<Vec<UsageModelAgg>> {
        self.usage_by_model_between(account_id, since_ts, i64::MAX)
    }
    fn usage_by_model_between(
        &mut self,
        account_id: &str,
        since_ts: i64,
        until_ts: i64,
    ) -> Result<Vec<UsageModelAgg>> {
        Ok(self.client.query(
            "SELECT COALESCE(model,''),COALESCE(NULLIF(provider,''),'anthropic'),COUNT(*)::bigint,COALESCE(SUM(input_tokens),0)::bigint, \
             COALESCE(SUM(output_tokens),0)::bigint,COALESCE(SUM(cache_read_tokens),0)::bigint, \
             COALESCE(SUM(cache_write_5m_tokens),0)::bigint,COALESCE(SUM(cache_write_1h_tokens),0)::bigint, \
             COALESCE(SUM(web_search_requests),0)::bigint,COALESCE(SUM(real_nano),0)::bigint, \
             COALESCE(SUM(charge_nano),0)::bigint,COALESCE(SUM(input_nano),0)::bigint, \
             COALESCE(SUM(output_nano),0)::bigint,COALESCE(SUM(cache_read_nano),0)::bigint, \
             COALESCE(SUM(cache_write_5m_nano),0)::bigint,COALESCE(SUM(cache_write_1h_nano),0)::bigint, \
             COALESCE(SUM(web_search_nano),0)::bigint FROM usage_events \
             WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY model,COALESCE(NULLIF(provider,''),'anthropic') ORDER BY SUM(real_nano) DESC,model,COALESCE(NULLIF(provider,''),'anthropic')",
            &[&account_id,&since_ts,&until_ts],
        )?.into_iter().map(|r| UsageModelAgg {
            model:r.get(0),provider:r.get(1),requests:r.get(2),input_tokens:r.get(3),output_tokens:r.get(4),
            cache_read_tokens:r.get(5),cache_write_5m_tokens:r.get(6),cache_write_1h_tokens:r.get(7),
            web_search_requests:r.get(8),real_nano:r.get(9),charge_nano:r.get(10),
            input_nano:r.get(11),output_nano:r.get(12),cache_read_nano:r.get(13),
            cache_write_5m_nano:r.get(14),cache_write_1h_nano:r.get(15),web_search_nano:r.get(16),
        }).collect())
    }
    pub fn usage_report(
        &mut self,
        account_id: &str,
        since_ts: i64,
        until_ts: i64,
    ) -> Result<UsageReport> {
        if until_ts <= since_ts {
            return Ok(UsageReport::default());
        }
        let mut transaction = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let models = transaction.query(
            "SELECT COALESCE(model,''),COALESCE(NULLIF(provider,''),'anthropic'),COUNT(*)::bigint,COALESCE(SUM(input_tokens),0)::bigint, \
             COALESCE(SUM(output_tokens),0)::bigint,COALESCE(SUM(cache_read_tokens),0)::bigint, \
             COALESCE(SUM(cache_write_5m_tokens),0)::bigint,COALESCE(SUM(cache_write_1h_tokens),0)::bigint, \
             COALESCE(SUM(web_search_requests),0)::bigint,COALESCE(SUM(real_nano),0)::bigint, \
             COALESCE(SUM(charge_nano),0)::bigint,COALESCE(SUM(input_nano),0)::bigint, \
             COALESCE(SUM(output_nano),0)::bigint,COALESCE(SUM(cache_read_nano),0)::bigint, \
             COALESCE(SUM(cache_write_5m_nano),0)::bigint,COALESCE(SUM(cache_write_1h_nano),0)::bigint, \
             COALESCE(SUM(web_search_nano),0)::bigint FROM usage_events \
             WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY model,COALESCE(NULLIF(provider,''),'anthropic') ORDER BY SUM(real_nano) DESC,model,COALESCE(NULLIF(provider,''),'anthropic')",
            &[&account_id, &since_ts, &until_ts],
        )?.into_iter().map(|r| UsageModelAgg {
            model:r.get(0),provider:r.get(1),requests:r.get(2),input_tokens:r.get(3),output_tokens:r.get(4),
            cache_read_tokens:r.get(5),cache_write_5m_tokens:r.get(6),cache_write_1h_tokens:r.get(7),
            web_search_requests:r.get(8),real_nano:r.get(9),charge_nano:r.get(10),
            input_nano:r.get(11),output_nano:r.get(12),cache_read_nano:r.get(13),
            cache_write_5m_nano:r.get(14),cache_write_1h_nano:r.get(15),web_search_nano:r.get(16),
        }).collect();
        let daily = transaction
            .query(
                "SELECT (ts / 86400) * 86400 AS day_ts, COUNT(*)::bigint, \
             COALESCE(SUM(real_nano),0)::bigint, COALESCE(SUM(charge_nano),0)::bigint \
             FROM usage_events WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY day_ts ORDER BY day_ts",
                &[&account_id, &since_ts, &until_ts],
            )?
            .into_iter()
            .map(|r| UsageDailyAgg {
                day_ts: r.get(0),
                requests: r.get(1),
                real_nano: r.get(2),
                charge_nano: r.get(3),
            })
            .collect();
        let daily_providers = transaction
            .query(
                "SELECT (ts / 86400) * 86400 AS day_ts, COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*)::bigint, \
             COALESCE(SUM(real_nano),0)::bigint, COALESCE(SUM(charge_nano),0)::bigint \
             FROM usage_events WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY day_ts, COALESCE(NULLIF(provider,''),'anthropic') ORDER BY day_ts, COALESCE(NULLIF(provider,''),'anthropic')",
                &[&account_id, &since_ts, &until_ts],
            )?
            .into_iter()
            .map(|r| UsageDailyProviderAgg {
                day_ts: r.get(0),
                provider: r.get(1),
                requests: r.get(2),
                real_nano: r.get(3),
                charge_nano: r.get(4),
            })
            .collect();
        let keys = transaction
            .query(
                "SELECT key, COUNT(*)::bigint, COALESCE(SUM(real_nano),0)::bigint, \
             COALESCE(SUM(charge_nano),0)::bigint \
             FROM usage_events WHERE account_id=$1 AND ts >= $2 AND ts < $3 \
             GROUP BY key ORDER BY SUM(real_nano) DESC, key",
                &[&account_id, &since_ts, &until_ts],
            )?
            .into_iter()
            .map(|r| UsageKeyAgg {
                key: r.get(0),
                requests: r.get(1),
                real_nano: r.get(2),
                charge_nano: r.get(3),
            })
            .collect();
        transaction.commit()?;
        Ok(UsageReport {
            models,
            daily,
            daily_providers,
            keys,
        })
    }
    pub fn spend_by_account(&mut self, since_ts: i64, limit: i64) -> Result<Vec<SpendAccountAgg>> {
        Ok(self
            .client
            .query(
                "SELECT u.account_id, COALESCE(a.handle,''), COUNT(*)::bigint, \
                 COALESCE(SUM(u.charge_nano),0)::bigint, COALESCE(SUM(u.real_nano),0)::bigint, \
                 COALESCE(MAX(u.ts),0)::bigint \
                 FROM usage_events u LEFT JOIN accounts a ON a.id=u.account_id \
                 WHERE u.ts>=$1 GROUP BY u.account_id, a.handle \
                 ORDER BY SUM(u.charge_nano) DESC LIMIT $2",
                &[&since_ts, &limit],
            )?
            .into_iter()
            .map(|r| SpendAccountAgg {
                account_id: r.get(0),
                handle: r.get(1),
                requests: r.get(2),
                charge_nano: r.get(3),
                real_nano: r.get(4),
                last_ts: r.get(5),
            })
            .collect())
    }
    pub fn spend_by_provider(&mut self, since_ts: i64) -> Result<Vec<SpendProviderAgg>> {
        Ok(self
            .client
            .query(
                "SELECT COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*)::bigint, \
                 COALESCE(SUM(charge_nano),0)::bigint, COALESCE(SUM(real_nano),0)::bigint \
                 FROM usage_events WHERE ts>=$1 GROUP BY 1 ORDER BY SUM(charge_nano) DESC",
                &[&since_ts],
            )?
            .into_iter()
            .map(|r| SpendProviderAgg {
                provider: r.get(0),
                requests: r.get(1),
                charge_nano: r.get(2),
                real_nano: r.get(3),
            })
            .collect())
    }

    pub fn usage_prune(&mut self, older_than_ts: i64) -> Result<usize> {
        Ok(self.client.execute(
            "DELETE FROM usage_events WHERE id IN ( \
               SELECT id FROM usage_events WHERE ts < $1 ORDER BY id LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize)
    }

    /// Bounded lifecycle cleanup. Financial outcomes remain in ledger; transient request/lease
    /// machinery is removed only after it is terminal and older than the retention cutoff.
    pub fn maintenance_prune(&mut self, older_than_ts: i64) -> Result<MaintenanceReport> {
        crate::pricing::validate_request_lifecycle_prune_cutoff(older_than_ts, now())?;
        let mut tx = self.client.transaction()?;
        let outbox = tx.execute(
            "DELETE FROM settlement_outbox WHERE request_id IN ( \
               SELECT request_id FROM settlement_outbox \
               WHERE state='done' AND committed_ts < $1 \
               ORDER BY committed_ts,request_id LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize;
        let lifecycle_counts = tx.query_one(
            "WITH doomed AS MATERIALIZED ( \
               SELECT r.request_id FROM reservations r \
               WHERE r.state IN ('settled','canceled') AND r.settled_ts < $1 \
                 AND NOT EXISTS (SELECT 1 FROM settlement_outbox o WHERE o.request_id=r.request_id) \
               ORDER BY r.settled_ts,r.request_id LIMIT 5000 FOR UPDATE \
             ), child_counts AS MATERIALIZED ( \
               SELECT \
                 (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots s \
                   WHERE EXISTS (SELECT 1 FROM doomed d WHERE d.request_id=s.request_id)) \
                   AS pricing_snapshots, \
                 (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations e \
                   WHERE EXISTS (SELECT 1 FROM doomed d WHERE d.request_id=e.request_id)) \
                   AS shadow_evaluations \
             ), deleted AS ( \
               DELETE FROM reservations r USING doomed d \
                WHERE r.request_id=d.request_id \
                RETURNING r.request_id \
             ) \
             SELECT \
               (SELECT COUNT(*)::bigint FROM deleted), \
               child_counts.pricing_snapshots, child_counts.shadow_evaluations \
              FROM child_counts",
            &[&older_than_ts],
        )?;
        let reservations = lifecycle_counts.get::<_, i64>(0) as usize;
        let pricing_snapshots_cascaded = lifecycle_counts.get::<_, i64>(1) as usize;
        let pricing_shadow_evaluations_cascaded = lifecycle_counts.get::<_, i64>(2) as usize;
        let capacity_leases = tx.execute(
            "DELETE FROM capacity_leases WHERE lease_id IN ( \
               SELECT lease_id FROM capacity_leases \
               WHERE state IN ('released','expired') AND released_ts < $1 \
               ORDER BY released_ts LIMIT 5000 \
             )",
            &[&older_than_ts],
        )? as usize;
        let engine_instances = tx.execute(
            "DELETE FROM engine_instances i WHERE lease_until < $1 \
               AND NOT EXISTS (SELECT 1 FROM reservations r WHERE r.owner_instance=i.instance_id \
                               AND r.owner_epoch=i.owner_epoch AND r.state NOT IN ('settled','canceled')) \
               AND NOT EXISTS (SELECT 1 FROM capacity_leases c WHERE c.owner_instance=i.instance_id \
                               AND c.owner_epoch=i.owner_epoch AND c.state='active')",
            &[&older_than_ts],
        )? as usize;
        tx.commit()?;
        Ok(MaintenanceReport {
            outbox,
            reservations,
            pricing_snapshots_cascaded,
            pricing_shadow_evaluations_cascaded,
            capacity_leases,
            engine_instances,
            ..MaintenanceReport::default()
        })
    }
    pub fn billing_totals(&mut self) -> Result<BillingTotals> {
        let row = self.client.query_one(
            "SELECT COALESCE(SUM(balance_nano),0)::bigint,COALESCE(SUM(spent_nano),0)::bigint, \
             COALESCE(SUM(reserved_nano),0)::bigint,COUNT(*) FILTER (WHERE status='active')::bigint FROM accounts",
            &[],
        )?;
        Ok(BillingTotals {
            balance_nano: row.get(0),
            spent_nano: row.get(1),
            reserved_nano: row.get(2),
            active_accounts: row.get(3),
        })
    }

    // -- Durable OpenAI/Codex capacity evidence ----------------------------------------------

    pub fn credit_codex_home_spend(
        &mut self,
        home_id: &str,
        delta_nano: i64,
        updated_ts: i64,
    ) -> Result<i64> {
        if home_id.is_empty() || delta_nano < 0 || updated_ts <= 0 {
            bail!("invalid Codex home spend credit");
        }
        Ok(self
            .client
            .query_one(
                "INSERT INTO codex_home_spend(home_id,spent_nano,updated_ts) VALUES($1,$2,$3) \
                 ON CONFLICT(home_id) DO UPDATE SET \
                   spent_nano=codex_home_spend.spent_nano+EXCLUDED.spent_nano, \
                   updated_ts=EXCLUDED.updated_ts RETURNING spent_nano",
                &[&home_id, &delta_nano, &updated_ts],
            )?
            .get(0))
    }

    pub fn codex_home_spend(&mut self, home_id: &str) -> Result<i64> {
        Ok(self
            .client
            .query_opt(
                "SELECT spent_nano FROM codex_home_spend WHERE home_id=$1",
                &[&home_id],
            )?
            .map(|row| row.get(0))
            .unwrap_or(0))
    }

    pub fn load_codex_calibration(
        &mut self,
        home_id: &str,
        window_duration_mins: i64,
    ) -> Result<Option<CodexCalibrationRow>> {
        Ok(self
            .client
            .query_opt(
                "SELECT home_id,window_duration_mins,resets_at,anchor_used_percent,\
                   anchor_spend_nano,used_percent,observed_at,sum_used_sq,sum_used_spend_nano,\
                   observed_points,samples,current_capacity_nano,current_low_nano,current_high_nano,\
                   current_confidence_bp,last_capacity_nano,last_low_nano,last_high_nano,\
                   last_confidence_bp,last_measured_at,estimator_version,version,updated_ts \
                 FROM codex_window_calibrations WHERE home_id=$1 AND window_duration_mins=$2",
                &[&home_id, &window_duration_mins],
            )?
            .map(|row| CodexCalibrationRow {
                home_id: row.get(0),
                window_duration_mins: row.get(1),
                resets_at: row.get(2),
                anchor_used_percent: row.get(3),
                anchor_spend_nano: row.get(4),
                used_percent: row.get(5),
                observed_at: row.get(6),
                sum_used_sq: row.get(7),
                sum_used_spend_nano: row.get(8),
                observed_points: row.get(9),
                samples: row.get(10),
                current_capacity_nano: row.get(11),
                current_low_nano: row.get(12),
                current_high_nano: row.get(13),
                current_confidence_bp: row.get(14),
                last_capacity_nano: row.get(15),
                last_low_nano: row.get(16),
                last_high_nano: row.get(17),
                last_confidence_bp: row.get(18),
                last_measured_at: row.get(19),
                estimator_version: row.get(20),
                version: row.get(21),
                updated_ts: row.get(22),
            }))
    }

    /// Save calibration evidence with optimistic concurrency. A conflict returns `None` and rolls
    /// back the raw observation together with the stale derived row.
    pub fn save_codex_calibration(
        &mut self,
        state: &CodexCalibrationRow,
        observation: &CodexWindowObservation,
    ) -> Result<Option<i64>> {
        let mut tx = self.client.transaction()?;
        let version = if state.version == 0 {
            tx.query_opt(
                "INSERT INTO codex_window_calibrations( \
                   home_id,window_duration_mins,resets_at,anchor_used_percent,anchor_spend_nano,\
                   used_percent,observed_at,sum_used_sq,sum_used_spend_nano,observed_points,samples,\
                   current_capacity_nano,current_low_nano,current_high_nano,current_confidence_bp,\
                   last_capacity_nano,last_low_nano,last_high_nano,last_confidence_bp,last_measured_at,\
                   estimator_version,updated_ts,version \
                 ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,\
                          $19,$20,$21,$22,1) \
                 ON CONFLICT(home_id,window_duration_mins) DO NOTHING RETURNING version",
                &[&state.home_id,&state.window_duration_mins,&state.resets_at,
                  &state.anchor_used_percent,&state.anchor_spend_nano,&state.used_percent,
                  &state.observed_at,&state.sum_used_sq,&state.sum_used_spend_nano,
                  &state.observed_points,&state.samples,&state.current_capacity_nano,
                  &state.current_low_nano,&state.current_high_nano,&state.current_confidence_bp,
                  &state.last_capacity_nano,&state.last_low_nano,&state.last_high_nano,
                  &state.last_confidence_bp,&state.last_measured_at,&state.estimator_version,
                  &state.updated_ts],
            )?
        } else {
            tx.query_opt(
                "UPDATE codex_window_calibrations SET \
                   resets_at=$3,anchor_used_percent=$4,anchor_spend_nano=$5,used_percent=$6,\
                   observed_at=$7,sum_used_sq=$8,sum_used_spend_nano=$9,observed_points=$10,\
                   samples=$11,current_capacity_nano=$12,current_low_nano=$13,current_high_nano=$14,\
                   current_confidence_bp=$15,last_capacity_nano=$16,last_low_nano=$17,\
                   last_high_nano=$18,last_confidence_bp=$19,last_measured_at=$20,\
                   estimator_version=$21,updated_ts=$22,version=version+1 \
                 WHERE home_id=$1 AND window_duration_mins=$2 AND version=$23 RETURNING version",
                &[&state.home_id,&state.window_duration_mins,&state.resets_at,
                  &state.anchor_used_percent,&state.anchor_spend_nano,&state.used_percent,
                  &state.observed_at,&state.sum_used_sq,&state.sum_used_spend_nano,
                  &state.observed_points,&state.samples,&state.current_capacity_nano,
                  &state.current_low_nano,&state.current_high_nano,&state.current_confidence_bp,
                  &state.last_capacity_nano,&state.last_low_nano,&state.last_high_nano,
                  &state.last_confidence_bp,&state.last_measured_at,&state.estimator_version,
                  &state.updated_ts,&state.version],
            )?
        };
        let Some(version) = version.map(|row| row.get::<_, i64>(0)) else {
            return Ok(None);
        };
        tx.execute(
            "INSERT INTO codex_window_observations( \
               home_id,window_duration_mins,resets_at,observed_at,used_percent,gateway_spend_nano \
             ) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
            &[
                &observation.home_id,
                &observation.window_duration_mins,
                &observation.resets_at,
                &observation.observed_at,
                &observation.used_percent,
                &observation.gateway_spend_nano,
            ],
        )?;
        tx.commit()?;
        Ok(Some(version))
    }

    // -- Fenced pool-state persistence --------------------------------------------------------

    pub fn load_pool_state(&mut self) -> Result<Vec<PoolStateRow>> {
        Ok(self.client.query(
            "SELECT email,cooling_until,cap5h,cap7d,spent_total,util5,util7,reset5,reset7,calib_n,version \
             FROM pool_state",
            &[],
        )?.into_iter().map(|r| PoolStateRow {
            email:r.get(0),cooling_until:r.get(1),cap5h_usd:r.get(2),cap7d_usd:r.get(3),
            spent_total_usd:r.get(4),spent_delta_usd:0.0,util5h:r.get(5),util7d:r.get(6),reset5h:r.get(7),
            reset7d:r.get(8),calib_n:r.get(9),version:r.get(10),
        }).collect())
    }

    pub fn save_pool_state(
        &mut self,
        owner: &Owner,
        rows: &[PoolStateRow],
    ) -> Result<Vec<(String, i64)>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let mut versions = Vec::with_capacity(rows.len());
        for row in rows {
            tx.execute(
                "INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING",
                &[&row.email],
            )?;
            let updated = tx.query_opt(
                "UPDATE pool_state SET cooling_until=$2,cap5h=$3,cap7d=$4,spent_total=spent_total+$5,util5=$6,util7=$7, \
                 reset5=$8,reset7=$9,calib_n=$10,version=version+1,writer_instance=$11,writer_epoch=$12,updated_ts=$13 \
                 WHERE email=$1 AND version=$14 RETURNING version",
                &[&row.email,&row.cooling_until,&row.cap5h_usd,&row.cap7d_usd,&row.spent_delta_usd,
                  &row.util5h,&row.util7d,&row.reset5h,&row.reset7d,&row.calib_n,
                  &owner.instance_id,&owner.epoch,&ts,&row.version],
            )?;
            let Some(updated) = updated else {
                bail!(
                    "pool-state CAS conflict for {} at version {}",
                    row.email,
                    row.version
                );
            };
            versions.push((row.email.clone(), updated.get(0)));
        }
        tx.commit()?;
        Ok(versions)
    }

    pub fn pool_inflight(&mut self, email: &str) -> Result<Option<i64>> {
        Ok(self
            .client
            .query_opt("SELECT inflight FROM pool_state WHERE email=$1", &[&email])?
            .map(|r| r.get(0)))
    }

    /// One-time, repeatable copy from a fully drained SQLite authority. Anonymous aggregate holds
    /// cannot be safely attributed, so a non-zero `reserved_nano` aborts the migration.
    pub fn import_sqlite(&mut self, sqlite_path: &str) -> Result<ImportReport> {
        let sqlite = crate::open(sqlite_path)?;
        let policy_state_rows: i64 = sqlite.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM pricing_catalog_versions)
               + (SELECT COUNT(*) FROM provider_switch_versions)
               + (SELECT COUNT(*) FROM account_policy_versions)
               + (SELECT COUNT(*) FROM account_policy_bindings)
               + (SELECT COUNT(*) FROM funding_buckets)
               + (SELECT COUNT(*) FROM pricing_admission_snapshots)
               + (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations)
               + (SELECT COUNT(*) FROM reservation_funding_allocations)
               + (SELECT COUNT(*) FROM ledger_funding_allocations)",
            [],
            |row| row.get(0),
        )?;
        if policy_state_rows != 0 {
            bail!(
                "SQLite contains policy/funding state unsupported by the legacy importer; \
                 use the policy-aware migration path"
            );
        }
        let attribution_predicate = crate::SQLITE_ATTRIBUTION_COLUMNS
            .iter()
            .map(|(name, _)| format!("\"{name}\" IS NOT NULL"))
            .collect::<Vec<_>>()
            .join(" OR ");
        for table in ["billing_settlement_outbox", "usage_events", "ledger"] {
            let predicate = if table == "ledger" {
                format!("({attribution_predicate}) OR official_nano IS NOT NULL")
            } else {
                attribution_predicate.clone()
            };
            let rows: i64 = sqlite.query_row(
                &format!("SELECT COUNT(*) FROM \"{table}\" WHERE {predicate}"),
                [],
                |row| row.get(0),
            )?;
            if rows != 0 {
                bail!(
                    "SQLite contains policy attribution unsupported by the legacy importer; \
                     use the policy-aware migration path"
                );
            }
        }
        let unresolved_request_lifecycle: i64 = sqlite.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM billing_reservations
                    WHERE state NOT IN ('settled','canceled'))
               + (SELECT COUNT(*) FROM billing_settlement_outbox
                    WHERE state <> 'done')",
            [],
            |row| row.get(0),
        )?;
        if unresolved_request_lifecycle != 0 {
            bail!("SQLite contains unresolved request lifecycle rows; drain before migration");
        }
        let source_totals = crate::billing_totals(&sqlite)?;
        if source_totals.reserved_nano != 0 {
            bail!(
                "SQLite contains {} anonymous reserved nanodollars; drain/reconcile before migration",
                source_totals.reserved_nano
            );
        }

        let mut tx = self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(836214912671::bigint)", &[])?;
        let target_policy_state_rows: i64 = tx
            .query_one(
                "SELECT (
                     (SELECT COUNT(*) FROM pricing_catalog_versions)
                   + (SELECT COUNT(*) FROM provider_switch_versions)
                   + (SELECT COUNT(*) FROM account_policy_versions)
                   + (SELECT COUNT(*) FROM account_policy_bindings)
                   + (SELECT COUNT(*) FROM funding_buckets)
                   + (SELECT COUNT(*) FROM pricing_admission_snapshots)
                   + (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations)
                   + (SELECT COUNT(*) FROM reservation_funding_allocations)
                   + (SELECT COUNT(*) FROM ledger_funding_allocations)
                 )::bigint",
                &[],
            )?
            .get(0);
        if target_policy_state_rows != 0 {
            bail!(
                "PostgreSQL already contains policy/funding authority; \
                 refusing the legacy SQLite import"
            );
        }
        let active_runtime: i64 = tx.query_one(
            "SELECT (SELECT COUNT(*) FROM reservations WHERE state NOT IN ('settled','canceled')) + \
             (SELECT COUNT(*) FROM capacity_leases WHERE state='active')",
            &[],
        )?.get(0);
        if active_runtime != 0 {
            bail!("PostgreSQL already has active runtime leases; refusing SQLite import");
        }

        // Runtime-only rows never come from SQLite and must not survive a re-run of the import.
        tx.execute("DELETE FROM settlement_outbox", &[])?;
        tx.execute("DELETE FROM reservations", &[])?;
        tx.execute("DELETE FROM capacity_leases", &[])?;
        tx.execute("DELETE FROM leader_leases", &[])?;
        tx.execute("DELETE FROM engine_instances", &[])?;
        tx.execute("DELETE FROM usage_events", &[])?;
        tx.execute("DELETE FROM ledger", &[])?;
        tx.execute("DELETE FROM api_keys", &[])?;
        tx.execute("DELETE FROM accounts", &[])?;
        tx.execute("DELETE FROM pool_state", &[])?;
        tx.execute("DELETE FROM subs", &[])?;

        let mut report = ImportReport::default();

        {
            let mut stmt = sqlite.prepare(
                "SELECT email,token,token_file,COALESCE(proxy,''),COALESCE(plan,''),COALESCE(status,'active'), \
                 COALESCE(fleet,'prod'),COALESCE(added_ts,0),COALESCE(added,''),COALESCE(proxy_expire,''), \
                 proxy_checked_ts,proxy_ok FROM subs ORDER BY email",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<i64>>(10)?,
                    r.get::<_, Option<i64>>(11)?,
                ))
            })?;
            for row in rows {
                let (
                    email,
                    token,
                    token_file,
                    proxy,
                    plan,
                    status,
                    fleet,
                    added_ts,
                    added,
                    expire,
                    checked,
                    ok,
                ) = row?;
                let proxy_ok = ok.map(|n| n != 0);
                tx.execute(
                    "INSERT INTO subs(email,token,token_file,proxy,plan,status,fleet,added_ts,added, \
                     proxy_expire,proxy_checked_ts,proxy_ok) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
                    &[&email,&token,&token_file,&proxy,&plan,&status,&fleet,&added_ts,&added,&expire,&checked,&proxy_ok],
                )?;
                report.subscriptions += 1;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,COALESCE(status,'active'), \
                 COALESCE(created_ts,0),COALESCE(created,'') FROM accounts ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, String>(8)?,
                ))
            })?;
            for row in rows {
                let (id, handle, balance, spent, reserved, mult, status, created_ts, created) =
                    row?;
                tx.execute(
                    "INSERT INTO accounts(id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                    &[&id,&handle,&balance,&spent,&reserved,&mult,&status,&created_ts,&created],
                )?;
                report.accounts += 1;
                report.balance_nano = report
                    .balance_nano
                    .checked_add(balance)
                    .context("balance sum overflow")?;
                report.spent_nano = report
                    .spent_nano
                    .checked_add(spent)
                    .context("spent sum overflow")?;
                report.reserved_nano = report
                    .reserved_nano
                    .checked_add(reserved)
                    .context("reserved sum overflow")?;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT key,key_id,account_id,label,spent_nano,reserved_nano,spend_limit_nano,expires_ts, \
                 COALESCE(status,'active'),COALESCE(created_ts,0),COALESCE(created,'') \
                 FROM api_keys ORDER BY key",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, String>(10)?,
                ))
            })?;
            for row in rows {
                let (
                    key,
                    key_id,
                    account_id,
                    label,
                    spent,
                    reserved,
                    spend_limit,
                    expires,
                    status,
                    created_ts,
                    created,
                ) = row?;
                let account_id =
                    account_id.context("legacy key has no account_id after SQLite migration")?;
                tx.execute(
                    "INSERT INTO api_keys(key,key_id,account_id,label,spent_nano,reserved_nano, \
                     spend_limit_nano,expires_ts,status,created_ts,created) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                    &[
                        &key,
                        &key_id,
                        &account_id,
                        &label,
                        &spent,
                        &reserved,
                        &spend_limit,
                        &expires,
                        &status,
                        &created_ts,
                        &created,
                    ],
                )?;
                report.keys += 1;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT id,account_id,key,kind,request_id,amount_nano,ref,balance_after_nano, \
                 COALESCE(ts,0),model,provider \
                 FROM ledger ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                ))
            })?;
            for row in rows {
                let (
                    id,
                    account_id,
                    key,
                    kind,
                    request_id,
                    amount,
                    reference,
                    balance,
                    ts,
                    model,
                    provider,
                ) = row?;
                tx.execute(
                    "INSERT INTO ledger(
                         id,account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,
                         ts,model,provider
                     ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                    &[
                        &id,
                        &account_id,
                        &key,
                        &kind,
                        &request_id,
                        &amount,
                        &reference,
                        &balance,
                        &ts,
                        &model,
                        &provider,
                    ],
                )?;
                report.ledger_rows += 1;
            }
            tx.query_one(
                "SELECT setval(pg_get_serial_sequence('ledger','id'),GREATEST(COALESCE(MAX(id),0),1), \
                 COALESCE(MAX(id),0) > 0) FROM ledger",
                &[],
            )?;
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT id,request_id,account_id,key,model,input_tokens,output_tokens, \
                 cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
                 real_nano,charge_nano,ref,ts,speed,inference_geo,input_nano,output_nano, \
                 cache_read_nano,cache_write_5m_nano,cache_write_1h_nano,web_search_nano, \
                 priced_ts,COALESCE(NULLIF(provider,''),'anthropic') \
                 FROM usage_events ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, i64>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, i64>(12)?,
                    r.get::<_, Option<String>>(13)?,
                    r.get::<_, i64>(14)?,
                    r.get::<_, String>(15)?,
                    r.get::<_, String>(16)?,
                    r.get::<_, i64>(17)?,
                    r.get::<_, i64>(18)?,
                    r.get::<_, i64>(19)?,
                    r.get::<_, i64>(20)?,
                    r.get::<_, i64>(21)?,
                    r.get::<_, i64>(22)?,
                    r.get::<_, i64>(23)?,
                    r.get::<_, String>(24)?,
                ))
            })?;
            for row in rows {
                let (
                    id,
                    request_id,
                    account_id,
                    key,
                    model,
                    input,
                    output,
                    cache_read,
                    cache5,
                    cache1,
                    web,
                    real,
                    charge,
                    reference,
                    ts,
                    speed,
                    inference_geo,
                    input_nano,
                    output_nano,
                    cache_read_nano,
                    cache_write_5m_nano,
                    cache_write_1h_nano,
                    web_search_nano,
                    priced_ts,
                    provider,
                ) = row?;
                tx.execute(
                    "INSERT INTO usage_events(
                         id,request_id,account_id,key,model,input_tokens,output_tokens,
                         cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,
                         web_search_requests,real_nano,charge_nano,ref,ts,speed,inference_geo,
                         input_nano,output_nano,cache_read_nano,cache_write_5m_nano,
                         cache_write_1h_nano,web_search_nano,priced_ts,provider
                     ) VALUES(
                         $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,
                         $18,$19,$20,$21,$22,$23,$24,$25
                     )",
                    &[
                        &id,
                        &request_id,
                        &account_id,
                        &key,
                        &model,
                        &input,
                        &output,
                        &cache_read,
                        &cache5,
                        &cache1,
                        &web,
                        &real,
                        &charge,
                        &reference,
                        &ts,
                        &speed,
                        &inference_geo,
                        &input_nano,
                        &output_nano,
                        &cache_read_nano,
                        &cache_write_5m_nano,
                        &cache_write_1h_nano,
                        &web_search_nano,
                        &priced_ts,
                        &provider,
                    ],
                )?;
                report.usage_rows += 1;
            }
            tx.query_one(
                "SELECT setval(pg_get_serial_sequence('usage_events','id'),GREATEST(COALESCE(MAX(id),0),1), \
                 COALESCE(MAX(id),0) > 0) FROM usage_events",
                &[],
            )?;
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT email,COALESCE(cooling_until,0),COALESCE(cap5h,0),COALESCE(cap7d,0), \
                 COALESCE(spent_total,0),COALESCE(util5,0),COALESCE(util7,0),COALESCE(reset5,0), \
                 COALESCE(reset7,0),COALESCE(calib_n,0),COALESCE(updated_ts,0) FROM pool_state ORDER BY email",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, f64>(5)?,
                    r.get::<_, f64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, i64>(10)?,
                ))
            })?;
            for row in rows {
                let (
                    email,
                    cooling,
                    cap5,
                    cap7,
                    spent,
                    util5,
                    util7,
                    reset5,
                    reset7,
                    calib,
                    updated,
                ) = row?;
                tx.execute(
                    "INSERT INTO pool_state(email,cooling_until,cap5h,cap7d,spent_total,util5,util7,reset5,reset7, \
                     calib_n,updated_ts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                    &[&email,&cooling,&cap5,&cap7,&spent,&util5,&util7,&reset5,&reset7,&calib,&updated],
                )?;
                report.pool_rows += 1;
            }
        }
        // Every subscription needs a capacity row even if old SQLite had never persisted its live state.
        tx.execute(
            "INSERT INTO pool_state(email) SELECT email FROM subs ON CONFLICT(email) DO NOTHING",
            &[],
        )?;

        let target = tx.query_one(
            "SELECT COUNT(*)::bigint,COALESCE(SUM(balance_nano),0)::bigint, \
             COALESCE(SUM(spent_nano),0)::bigint,COALESCE(SUM(reserved_nano),0)::bigint FROM accounts",
            &[],
        )?;
        let target_accounts: i64 = target.get(0);
        let target_balance: i64 = target.get(1);
        let target_spent: i64 = target.get(2);
        let target_reserved: i64 = target.get(3);
        if target_accounts as usize != report.accounts
            || target_balance != report.balance_nano
            || target_spent != report.spent_nano
            || target_reserved != report.reserved_nano
            || report.balance_nano != source_totals.balance_nano
            || report.spent_nano != source_totals.spent_nano
        {
            bail!("SQLite/PostgreSQL monetary reconciliation mismatch; import rolled back");
        }
        tx.commit()?;
        Ok(report)
    }
}

impl PgStore {
    pub fn pricing_shadow_admission_evaluation(
        &mut self,
        request_id: &str,
    ) -> Result<Option<crate::pricing::PricingShadowAdmissionEvaluation>> {
        crate::pricing::postgres::postgres_pricing_shadow_admission_evaluation(
            &mut self.client,
            request_id,
        )
    }

    pub fn insert_pricing_shadow_admission_evaluation(
        &mut self,
        input: &crate::pricing::PricingShadowAdmissionEvaluationInput,
    ) -> Result<crate::pricing::PricingShadowEvaluationWrite> {
        crate::pricing::postgres::postgres_insert_pricing_shadow_admission_evaluation(
            &mut self.client,
            input,
        )
    }

    pub fn pricing_read_bundle(
        &mut self,
        account_id: &str,
    ) -> Result<crate::pricing::PricingReadBundle> {
        crate::pricing::postgres::postgres_pricing_read_bundle(&mut self.client, account_id)
    }

    pub fn pricing_catalog_by_generation(
        &mut self,
        product_id: &str,
        generation: i64,
    ) -> Result<Option<crate::pricing::PricingCatalogSpec>> {
        crate::pricing::postgres::postgres_pricing_catalog_by_generation(
            &mut self.client,
            product_id,
            generation,
        )
    }

    pub fn active_pricing_catalog(
        &mut self,
        product_id: &str,
    ) -> Result<Option<crate::pricing::PricingCatalogSpec>> {
        crate::pricing::postgres::postgres_active_pricing_catalog(&mut self.client, product_id)
    }

    pub fn prepare_pricing_catalog(
        &mut self,
        spec: &crate::pricing::PricingCatalogSpec,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_prepare_pricing_catalog(&mut self.client, spec)
    }

    pub fn activate_pricing_catalog(
        &mut self,
        product_id: &str,
        target: &crate::pricing::VersionTarget,
        expectation: &crate::pricing::ActiveExpectation,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_activate_pricing_catalog(
            &mut self.client,
            product_id,
            target,
            expectation,
        )
    }

    pub fn provider_switches_by_generation(
        &mut self,
        generation: i64,
    ) -> Result<Option<crate::pricing::ProviderSwitchSpec>> {
        crate::pricing::postgres::postgres_provider_switches_by_generation(
            &mut self.client,
            generation,
        )
    }

    pub fn active_provider_switches(
        &mut self,
    ) -> Result<Option<crate::pricing::ProviderSwitchSpec>> {
        crate::pricing::postgres::postgres_active_provider_switches(&mut self.client)
    }

    pub fn prepare_provider_switches(
        &mut self,
        spec: &crate::pricing::ProviderSwitchSpec,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_prepare_provider_switches(&mut self.client, spec)
    }

    pub fn activate_provider_switches(
        &mut self,
        target: &crate::pricing::VersionTarget,
        expectation: &crate::pricing::ActiveExpectation,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_activate_provider_switches(
            &mut self.client,
            target,
            expectation,
        )
    }

    pub fn account_policy_by_version(
        &mut self,
        account_id: &str,
        effective_version: i64,
    ) -> Result<Option<crate::pricing::AccountPolicySpec>> {
        crate::pricing::postgres::postgres_account_policy_by_version(
            &mut self.client,
            account_id,
            effective_version,
        )
    }

    pub fn active_account_policy(
        &mut self,
        account_id: &str,
    ) -> Result<Option<crate::pricing::ActiveAccountPolicy>> {
        crate::pricing::postgres::postgres_active_account_policy(&mut self.client, account_id)
    }

    pub fn prepare_account_policy(
        &mut self,
        spec: &crate::pricing::AccountPolicySpec,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_prepare_account_policy(&mut self.client, spec)
    }

    pub fn activate_account_policy(
        &mut self,
        activation: &crate::pricing::AccountPolicyActivationSpec,
        expectation: &crate::pricing::PolicyActiveExpectation,
    ) -> Result<crate::pricing::PricingMutation> {
        crate::pricing::postgres::postgres_activate_account_policy(
            &mut self.client,
            activation,
            expectation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn legacy_snapshot(
        request_id: &str,
        account_id: &str,
        official_hold_nano: i64,
        charged_hold_nano: i64,
    ) -> crate::pricing::LegacyScalarAdmissionSnapshot {
        legacy_snapshot_at(
            request_id,
            account_id,
            official_hold_nano,
            charged_hold_nano,
            now(),
        )
    }

    fn legacy_snapshot_at(
        request_id: &str,
        account_id: &str,
        official_hold_nano: i64,
        charged_hold_nano: i64,
        admission_ts: i64,
    ) -> crate::pricing::LegacyScalarAdmissionSnapshot {
        crate::pricing::LegacyScalarAdmissionSnapshot::new(
            crate::pricing::LegacyScalarAdmissionSnapshotInput {
                request_id: request_id.into(),
                account_id: account_id.into(),
                provider: crate::pricing::SnapshotProvider::Anthropic,
                requested_model_id: "claude-sonnet-5".into(),
                canonical_model_id: "claude-sonnet-5".into(),
                alias_generation: 1,
                tariff_schedule_id: "anthropic/standard/sonnet-current/v1".into(),
                tariff_priced_ts: admission_ts,
                admission_ts,
                payable_multiplier_bp: 2_000,
                official_hold_nano,
                charged_hold_nano,
                premium_modifiers: crate::pricing::LegacyPremiumModifiers::AnthropicV1 {
                    speed: crate::pricing::SnapshotAnthropicSpeed::Standard,
                    inference_geo: crate::pricing::SnapshotAnthropicInferenceGeo::Global,
                    inference_geo_basis_points: 10_000,
                },
            },
        )
        .unwrap()
    }

    fn openai_legacy_snapshot(
        request_id: &str,
        account_id: &str,
        official_hold_nano: i64,
        charged_hold_nano: i64,
    ) -> crate::pricing::LegacyScalarAdmissionSnapshot {
        let admission_ts = now();
        crate::pricing::LegacyScalarAdmissionSnapshot::new(
            crate::pricing::LegacyScalarAdmissionSnapshotInput {
                request_id: request_id.into(),
                account_id: account_id.into(),
                provider: crate::pricing::SnapshotProvider::OpenAi,
                requested_model_id: "gpt-5.6".into(),
                canonical_model_id: "gpt-5.6-sol".into(),
                alias_generation: 1,
                tariff_schedule_id: "openai/gpt-5.6-sol/epoch-0/v1".into(),
                tariff_priced_ts: admission_ts,
                admission_ts,
                payable_multiplier_bp: 2_000,
                official_hold_nano,
                charged_hold_nano,
                premium_modifiers: crate::pricing::LegacyPremiumModifiers::OpenAiV1 {
                    service_tier: crate::pricing::SnapshotOpenAiServiceTier::Fast,
                    service_tier_multiplier_basis_points: 25_000,
                    context_tier: crate::pricing::SnapshotOpenAiContextTier::Long,
                    input_multiplier_basis_points: 20_000,
                    output_multiplier_basis_points: 15_000,
                },
            },
        )
        .unwrap()
    }

    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pg::tests::postgres_legacy_snapshot_contract_matrix`
    #[test]
    fn postgres_legacy_snapshot_contract_matrix() {
        use crate::pricing::{
            LegacyScalarReserveConflict as Conflict, LegacyScalarReserveOutcome as O,
        };

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL legacy snapshot contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE account_policy_bindings,account_policy_rules,account_policy_versions,
                 provider_switch_head,provider_switch_entries,provider_switch_versions,
                 pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
                 settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
                 usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
            )
            .unwrap();

        let owner = pg.claim_instance("snapshot-engine", 600).unwrap();
        pg.account_create("snapshot-account", None, 2_000).unwrap();
        pg.account_topup("snapshot-account", 1_000, None).unwrap();
        pg.key_issue("snapshot-key", "snapshot-account", None)
            .unwrap();

        let current = now();
        let money_before_window_checks: (i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                            (SELECT COUNT(*)::bigint FROM ledger) \
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        let expired = legacy_snapshot_at(
            "expired-window-request",
            "snapshot-account",
            500,
            100,
            current - 2 * crate::pricing::LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
        );
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &expired)
                .unwrap(),
            O::Conflict(Conflict::ExpiredIdempotencyWindow)
        );
        let future = legacy_snapshot_at(
            "future-window-request",
            "snapshot-account",
            500,
            100,
            current + 2 * crate::pricing::LEGACY_SCALAR_REPLAY_MAX_AGE_SECS,
        );
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &future)
                .unwrap(),
            O::Conflict(Conflict::AdmissionTimestampInFuture)
        );
        let money_after_window_checks: (i64, i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                            (SELECT COUNT(*)::bigint FROM ledger) \
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        assert_eq!(money_after_window_checks, money_before_window_checks);
        let rejected_window_rows = pg
            .client
            .query_one(
                "SELECT \
                    (SELECT COUNT(*)::bigint FROM reservations \
                      WHERE request_id IN ('expired-window-request','future-window-request')), \
                    (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                      WHERE request_id IN ('expired-window-request','future-window-request'))",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                rejected_window_rows.get::<_, i64>(0),
                rejected_window_rows.get::<_, i64>(1),
            ),
            (0, 0)
        );

        let aborted_snapshot =
            legacy_snapshot("aborted-before-commit", "snapshot-account", 500, 100);
        let mut insert_gate_calls = 0;
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot_guarded(
                &owner,
                "snapshot-key",
                60,
                &aborted_snapshot,
                || {
                    insert_gate_calls += 1;
                    false
                },
            )
            .unwrap(),
            O::AbortedBeforeCommit
        );
        assert_eq!(insert_gate_calls, 1);
        let aborted_counts = pg
            .client
            .query_one(
                "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                        (SELECT COUNT(*)::bigint FROM reservations \
                          WHERE request_id='aborted-before-commit'), \
                        (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                          WHERE request_id='aborted-before-commit') \
                   FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                  WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                aborted_counts.get::<_, i64>(0),
                aborted_counts.get::<_, i64>(1),
                aborted_counts.get::<_, i64>(2),
                aborted_counts.get::<_, i64>(3),
                aborted_counts.get::<_, i64>(4),
            ),
            (
                money_before_window_checks.0,
                money_before_window_checks.1,
                money_before_window_checks.2,
                0,
                0,
            )
        );

        let snapshot = legacy_snapshot("snapshot-request", "snapshot-account", 500, 100);

        let inserted = pg
            .reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &snapshot)
            .unwrap();
        let O::Inserted(inserted) = inserted else {
            panic!("first PostgreSQL snapshot reservation was not inserted");
        };
        assert_eq!(inserted.balance_after_reserve_nano, 900);
        assert_eq!(inserted.snapshot, snapshot);
        assert_eq!(
            pg.legacy_scalar_admission_snapshot("snapshot-request")
                .unwrap()
                .unwrap(),
            snapshot
        );
        let mut replay_gate_calls = 0;
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot_guarded(
                &owner,
                "snapshot-key",
                60,
                &snapshot,
                || {
                    replay_gate_calls += 1;
                    false
                },
            )
            .unwrap(),
            O::AbortedBeforeCommit
        );
        assert_eq!(replay_gate_calls, 1);
        let replay_abort_counts = pg
            .client
            .query_one(
                "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano, \
                        (SELECT COUNT(*)::bigint FROM reservations \
                          WHERE request_id='snapshot-request'), \
                        (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                          WHERE request_id='snapshot-request') \
                   FROM accounts a JOIN api_keys k ON k.account_id=a.id \
                  WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                replay_abort_counts.get::<_, i64>(0),
                replay_abort_counts.get::<_, i64>(1),
                replay_abort_counts.get::<_, i64>(2),
                replay_abort_counts.get::<_, i64>(3),
                replay_abort_counts.get::<_, i64>(4),
            ),
            (900, 100, 100, 1, 1)
        );
        let reserved_lease: i64 = pg
            .client
            .query_one(
                "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 9_999, &snapshot)
                .unwrap(),
            O::Unchanged(_)
        ));
        assert_eq!(
            pg.client
                .query_one(
                    "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                    &[],
                )
                .unwrap()
                .get::<_, i64>(0),
            reserved_lease
        );
        assert!(pg.mark_delivering(&owner, "snapshot-request", 60).unwrap());
        let delivering_lease: i64 = pg
            .client
            .query_one(
                "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 9_999, &snapshot)
                .unwrap(),
            O::Unchanged(_)
        ));
        assert_eq!(
            pg.client
                .query_one(
                    "SELECT lease_until FROM reservations WHERE request_id='snapshot-request'",
                    &[],
                )
                .unwrap()
                .get::<_, i64>(0),
            delivering_lease
        );

        let different = legacy_snapshot("snapshot-request", "snapshot-account", 501, 100);
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &different)
                .unwrap(),
            O::Conflict(Conflict::SnapshotPayload)
        );
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "different-key", 60, &snapshot)
                .unwrap(),
            O::Conflict(Conflict::ReservationIdentity)
        );

        assert_eq!(
            pg.reserve_request(
                &owner,
                "legacy-only",
                "snapshot-account",
                "snapshot-key",
                50,
                60
            )
            .unwrap(),
            Some(850)
        );
        let legacy_only = legacy_snapshot("legacy-only", "snapshot-account", 250, 50);
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &legacy_only)
                .unwrap(),
            O::Conflict(Conflict::ExistingReservationWithoutSnapshot)
        );
        assert!(pg
            .legacy_scalar_admission_snapshot("legacy-only")
            .unwrap()
            .is_none());

        pg.client
            .batch_execute(
                "DROP TRIGGER IF EXISTS reject_test_legacy_snapshot
                     ON pricing_admission_snapshots;
                 DROP FUNCTION IF EXISTS reject_test_legacy_snapshot();
                 CREATE FUNCTION reject_test_legacy_snapshot()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.request_id = 'rollback-request' THEN
                         RAISE EXCEPTION 'injected snapshot failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER reject_test_legacy_snapshot
                 BEFORE INSERT ON pricing_admission_snapshots
                 FOR EACH ROW EXECUTE FUNCTION reject_test_legacy_snapshot();",
            )
            .unwrap();
        let before: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        let rollback = legacy_snapshot("rollback-request", "snapshot-account", 500, 100);
        assert!(pg
            .reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &rollback)
            .is_err());
        let after: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        assert_eq!(after, before);
        let rollback_counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='rollback-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='rollback-request')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                rollback_counts.get::<_, i64>(0),
                rollback_counts.get::<_, i64>(1),
            ),
            (0, 0)
        );
        pg.client
            .batch_execute(
                "DROP TRIGGER reject_test_legacy_snapshot ON pricing_admission_snapshots;
                 DROP FUNCTION reject_test_legacy_snapshot();",
            )
            .unwrap();

        pg.account_create("disabled-account", None, 2_000).unwrap();
        pg.account_topup("disabled-account", 1_000, None).unwrap();
        pg.key_issue("disabled-key", "disabled-account", None)
            .unwrap();
        pg.key_set_status("disabled-key", "disabled").unwrap();
        let disabled = legacy_snapshot("disabled-request", "disabled-account", 500, 100);
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "disabled-key", 60, &disabled)
                .unwrap(),
            O::NotReserved
        );
        let disabled_counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='disabled-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='disabled-request')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                disabled_counts.get::<_, i64>(0),
                disabled_counts.get::<_, i64>(1),
            ),
            (0, 0)
        );

        pg.account_create("openai-snapshot-account", None, 2_000)
            .unwrap();
        pg.account_topup("openai-snapshot-account", 1_000, None)
            .unwrap();
        pg.key_issue("openai-snapshot-key", "openai-snapshot-account", None)
            .unwrap();
        let openai_snapshot = openai_legacy_snapshot(
            "openai-snapshot-request",
            "openai-snapshot-account",
            500,
            100,
        );
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(
                &owner,
                "openai-snapshot-key",
                60,
                &openai_snapshot
            )
            .unwrap(),
            O::Inserted(_)
        ));
        assert_eq!(
            pg.legacy_scalar_admission_snapshot("openai-snapshot-request")
                .unwrap()
                .unwrap(),
            openai_snapshot
        );
        assert!(pg
            .legacy_scalar_admission_snapshot("invalid\0request")
            .is_err());

        let concurrent_snapshot =
            legacy_snapshot("concurrent-snapshot-request", "snapshot-account", 125, 25);
        let concurrent_money_before: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        let barrier = Arc::new(Barrier::new(3));
        let spawn_reserve = |barrier: Arc<Barrier>| {
            let worker_url = url.clone();
            let worker_owner = owner.clone();
            let worker_snapshot = concurrent_snapshot.clone();
            std::thread::spawn(move || {
                let mut worker = PgStore::connect(&worker_url).unwrap();
                worker
                    .client
                    .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
                    .unwrap();
                barrier.wait();
                worker
                    .reserve_request_with_legacy_snapshot(
                        &worker_owner,
                        "snapshot-key",
                        60,
                        &worker_snapshot,
                    )
                    .unwrap()
            })
        };
        let first = spawn_reserve(barrier.clone());
        let second = spawn_reserve(barrier.clone());
        barrier.wait();
        let outcomes = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, O::Inserted(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, O::Unchanged(_)))
                .count(),
            1
        );
        let concurrent_money_after: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        assert_eq!(
            concurrent_money_after,
            (
                concurrent_money_before.0 - 25,
                concurrent_money_before.1 + 25,
                concurrent_money_before.2 + 25,
            )
        );

        pg.cancel_request("snapshot-request").unwrap();
        assert_eq!(
            pg.reserve_request_with_legacy_snapshot(&owner, "snapshot-key", 60, &snapshot)
                .unwrap(),
            O::Conflict(Conflict::TerminalReservation)
        );
        assert_eq!(
            pg.legacy_scalar_admission_snapshot("snapshot-request")
                .unwrap()
                .unwrap(),
            snapshot
        );

        let counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='snapshot-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='snapshot-request')",
                &[],
            )
            .unwrap();
        assert_eq!((counts.get::<_, i64>(0), counts.get::<_, i64>(1)), (1, 1));

        // Deterministically fence an old writer while it is waiting for this request's advisory
        // lock. The locked recheck after the wait must reject it without touching customer money.
        let fence_snapshot = legacy_snapshot("fence-race-request", "snapshot-account", 500, 100);
        let money_before_fence: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        let mut blocker = PgStore::connect(&url).unwrap();
        blocker
            .client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        blocker
            .client
            .query_one(
                "SELECT pg_advisory_lock(hashtextextended($1, 0))",
                &[&fence_snapshot.request_id.as_str()],
            )
            .unwrap();

        let worker_url = url.clone();
        let worker_owner = owner.clone();
        let worker_snapshot = fence_snapshot.clone();
        let worker = std::thread::spawn(
            move || -> anyhow::Result<crate::pricing::LegacyScalarReserveOutcome> {
                let mut worker = PgStore::connect(&worker_url)?;
                worker
                    .client
                    .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")?;
                worker.reserve_request_with_legacy_snapshot(
                    &worker_owner,
                    "snapshot-key",
                    60,
                    &worker_snapshot,
                )
            },
        );

        let wait_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let waiting: i64 = pg
                .client
                .query_one(
                    "SELECT COUNT(*)::bigint FROM pg_locks
                      WHERE locktype='advisory' AND NOT granted",
                    &[],
                )
                .unwrap()
                .get(0);
            if waiting > 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < wait_deadline,
                "snapshot writer did not reach the advisory-lock wait"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let replacement_owner = pg.claim_instance("snapshot-engine", 600).unwrap();
        assert!(replacement_owner.epoch > owner.epoch);
        let unlocked: bool = blocker
            .client
            .query_one(
                "SELECT pg_advisory_unlock(hashtextextended($1, 0))",
                &[&fence_snapshot.request_id.as_str()],
            )
            .unwrap()
            .get(0);
        assert!(unlocked);
        let fenced_error = worker
            .join()
            .expect("snapshot fence worker panicked")
            .unwrap_err();
        assert!(fenced_error
            .to_string()
            .contains("engine owner lease is stale or fenced"));

        let money_after_fence: (i64, i64, i64) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,k.reserved_nano
                       FROM accounts a JOIN api_keys k ON k.account_id=a.id
                      WHERE a.id='snapshot-account' AND k.key='snapshot-key'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2))
        };
        assert_eq!(money_after_fence, money_before_fence);
        let fence_counts = pg
            .client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM reservations WHERE request_id='fence-race-request'),
                     (SELECT COUNT(*) FROM pricing_admission_snapshots
                       WHERE request_id='fence-race-request')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (fence_counts.get::<_, i64>(0), fence_counts.get::<_, i64>(1),),
            (0, 0)
        );
        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    fn shadow_pg_catalog(generation: i64, digest: &str) -> crate::pricing::PricingCatalogSpec {
        crate::pricing::PricingCatalogSpec {
            product_id: "main".into(),
            generation,
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".into(),
            content_digest: digest.into(),
            entries: vec![crate::pricing::PricingCatalogEntrySpec {
                provider_id: "anthropic".into(),
                canonical_model_id: "claude-sonnet-5".into(),
                enabled: true,
            }],
        }
    }

    fn shadow_pg_switches(
        generation: i64,
        catalog_generation: i64,
        digest: &str,
    ) -> crate::pricing::ProviderSwitchSpec {
        crate::pricing::ProviderSwitchSpec {
            generation,
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".into(),
            content_digest: digest.into(),
            entries: vec![
                crate::pricing::ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: crate::pricing::ProviderSwitchScope::Master,
                    catalog_generation: None,
                    enabled: true,
                },
                crate::pricing::ProviderSwitchEntrySpec {
                    provider_id: "anthropic".into(),
                    scope: crate::pricing::ProviderSwitchScope::Segment {
                        product_id: "main".into(),
                        segment: crate::pricing::PolicySegment::B2b,
                    },
                    catalog_generation: Some(catalog_generation),
                    enabled: true,
                },
            ],
        }
    }

    fn shadow_pg_rule() -> crate::pricing::AccountPolicyRuleSpec {
        crate::pricing::AccountPolicyRuleSpec {
            rule_id: "anthropic-discount".into(),
            rule_digest: "anthropic-discount-digest".into(),
            scope: crate::pricing::PolicyRuleScope::Provider {
                provider_id: "anthropic".into(),
            },
            pricing_mode: crate::pricing::PricingMode::Discount,
            rule_origin: crate::pricing::RuleOrigin::Managed,
            discount_bps: Some(1_000),
            payable_multiplier_bp: 9_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
        }
    }

    fn shadow_pg_policy() -> crate::pricing::AccountPolicySpec {
        crate::pricing::AccountPolicySpec {
            account_id: "shadow-pg-account".into(),
            effective_version: 1,
            policy_id: "b2b:shadow-pg-account".into(),
            policy_version: 1,
            source_policy_digest: "source-1".into(),
            owner_type: crate::pricing::PolicyOwnerType::B2bClient,
            owner_id: "shadow-pg-account".into(),
            account_class: crate::pricing::AccountClass::B2b,
            product_id: "main".into(),
            schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: "shadow-policy-1".into(),
            replacement_locked: false,
            rules: vec![shadow_pg_rule()],
        }
    }

    fn shadow_pg_dependency(version: i64, digest: &str) -> crate::pricing::PricingShadowDependency {
        crate::pricing::PricingShadowDependency {
            target: crate::pricing::VersionTarget::new(version, digest),
            pricing_schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".into(),
        }
    }

    fn shadow_pg_manifest() -> crate::pricing::PricingRuntimeManifestEvidence {
        crate::pricing::PricingRuntimeManifestEvidence::new(
            1,
            vec![crate::pricing::PricingRuntimeCapabilityEvidence::new(
                crate::pricing::PRICING_SCHEMA_VERSION,
                17,
                "capability-17",
            )
            .unwrap()],
        )
        .unwrap()
    }

    fn shadow_pg_resolved(
        actual: &crate::pricing::ShadowActualSnapshotRef,
    ) -> crate::pricing::PricingShadowEvaluationOutcome {
        crate::pricing::PricingShadowEvaluationOutcome::Resolved(Box::new(
            crate::pricing::PricingShadowResolved::new(
                actual,
                crate::pricing::PricingShadowResolvedInput {
                    observed_multiplier_bp: 2_000,
                    product_id: "main".into(),
                    account_class: crate::pricing::AccountClass::B2b,
                    policy: crate::pricing::PricingShadowPolicyIdentity {
                        target: crate::pricing::VersionTarget::new(1, "shadow-policy-1"),
                        policy_id: "b2b:shadow-pg-account".into(),
                        policy_version: 1,
                        source_policy_digest: "source-1".into(),
                        schema_version: crate::pricing::PRICING_SCHEMA_VERSION,
                    },
                    policy_lineage: crate::pricing::PricingShadowLineage {
                        catalog: shadow_pg_dependency(1, "shadow-catalog-1"),
                        switches: shadow_pg_dependency(1, "shadow-switches-1"),
                    },
                    admission_lineage: crate::pricing::PricingShadowLineage {
                        catalog: shadow_pg_dependency(2, "shadow-catalog-2"),
                        switches: shadow_pg_dependency(2, "shadow-switches-2"),
                    },
                    rule: shadow_pg_rule(),
                },
            )
            .unwrap(),
        ))
    }

    fn shadow_pg_input(
        snapshot: &crate::pricing::LegacyScalarAdmissionSnapshot,
        outcome: crate::pricing::PricingShadowEvaluationOutcome,
        enqueued_ts: i64,
        evaluated_ts: i64,
        diagnostic: serde_json::Value,
    ) -> crate::pricing::PricingShadowAdmissionEvaluationInput {
        crate::pricing::PricingShadowAdmissionEvaluationInput::new(
            crate::pricing::ShadowActualSnapshotRef::from_snapshot(snapshot).unwrap(),
            crate::pricing::PRICING_SCHEMA_VERSION,
            shadow_pg_manifest(),
            enqueued_ts,
            evaluated_ts,
            outcome,
            crate::pricing::ShadowDiagnosticContext::new(diagnostic).unwrap(),
        )
        .unwrap()
    }

    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pg::tests::postgres_typed_shadow_evaluation_contract`
    #[test]
    fn postgres_typed_shadow_evaluation_contract() {
        use crate::pricing::{
            LegacyScalarReserveOutcome, PricingMutation, PricingShadowEvaluationConflict,
            PricingShadowEvaluationOutcome, PricingShadowEvaluationWrite as Write,
            PricingShadowReadErrorCode, PricingShadowRejectionCode, ShadowActualSnapshotRef,
        };
        use serde_json::json;

        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL typed shadow contract: \
                 CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        pg.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        pg.client
            .batch_execute(
                "TRUNCATE account_policy_bindings,account_policy_rules,account_policy_versions,
                 provider_switch_head,provider_switch_entries,provider_switch_versions,
                 pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions,
                 settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances,
                 usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
            )
            .unwrap();

        let owner = pg.claim_instance("shadow-pg-engine", 600).unwrap();
        pg.account_create("shadow-pg-account", None, 2_000).unwrap();
        pg.account_topup("shadow-pg-account", 2_000_000_000, None)
            .unwrap();
        pg.key_issue("shadow-pg-key", "shadow-pg-account", None)
            .unwrap();
        for catalog in [
            shadow_pg_catalog(1, "shadow-catalog-1"),
            shadow_pg_catalog(2, "shadow-catalog-2"),
        ] {
            assert_eq!(
                pg.prepare_pricing_catalog(&catalog).unwrap(),
                PricingMutation::Stored
            );
        }
        for switches in [
            shadow_pg_switches(1, 1, "shadow-switches-1"),
            shadow_pg_switches(2, 2, "shadow-switches-2"),
        ] {
            assert_eq!(
                pg.prepare_provider_switches(&switches).unwrap(),
                PricingMutation::Stored
            );
        }
        assert_eq!(
            pg.prepare_account_policy(&shadow_pg_policy()).unwrap(),
            PricingMutation::Stored
        );

        let snapshot = legacy_snapshot(
            "shadow-pg-request",
            "shadow-pg-account",
            500_000_000,
            100_000_000,
        );
        assert!(matches!(
            pg.reserve_request_with_legacy_snapshot(&owner, "shadow-pg-key", 60, &snapshot,)
                .unwrap(),
            LegacyScalarReserveOutcome::Inserted(_)
        ));
        let actual = ShadowActualSnapshotRef::from_snapshot(&snapshot).unwrap();
        let first_enqueued_ts = snapshot.admission_ts() + 1;
        let first_evaluated_ts = first_enqueued_ts + 1;
        let input = shadow_pg_input(
            &snapshot,
            shadow_pg_resolved(&actual),
            first_enqueued_ts,
            first_evaluated_ts,
            json!({"writer": "concurrent"}),
        );
        let money_before: (i64, i64, i64, String) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,r.hold_nano,r.state
                       FROM accounts a JOIN reservations r ON r.account_id=a.id
                      WHERE r.request_id='shadow-pg-request'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };

        let barrier = Arc::new(Barrier::new(2));
        let writers = [input.clone(), input.clone()].map(|input| {
            let url = url.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut writer = PgStore::connect(&url).unwrap();
                barrier.wait();
                writer
                    .insert_pricing_shadow_admission_evaluation(&input)
                    .unwrap()
            })
        });
        let outcomes = writers.map(|writer| writer.join().unwrap());
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Write::Inserted(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Write::Unchanged(_)))
                .count(),
            1
        );
        let stored = pg
            .pricing_shadow_admission_evaluation("shadow-pg-request")
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.evaluation_digest(),
            input.to_evaluation().unwrap().evaluation_digest()
        );

        let replay = shadow_pg_input(
            &snapshot,
            shadow_pg_resolved(&actual),
            first_enqueued_ts + 8,
            first_evaluated_ts + 17,
            json!({"writer": "lost-ack-replay"}),
        );
        let Write::Unchanged(first) = pg
            .insert_pricing_shadow_admission_evaluation(&replay)
            .unwrap()
        else {
            panic!("PostgreSQL exact shadow replay was not unchanged");
        };
        assert_eq!(first.enqueued_ts(), first_enqueued_ts);
        assert_eq!(
            first.diagnostic_context().value(),
            &json!({"writer": "concurrent"})
        );

        let conflict = shadow_pg_input(
            &snapshot,
            PricingShadowEvaluationOutcome::Rejected {
                reason: PricingShadowRejectionCode::MissingRule,
                observed_multiplier_bp: 2_000,
            },
            first_enqueued_ts,
            first_evaluated_ts,
            json!({}),
        );
        assert_eq!(
            pg.insert_pricing_shadow_admission_evaluation(&conflict)
                .unwrap(),
            Write::Conflict(PricingShadowEvaluationConflict::ExistingSemanticResult)
        );
        assert_eq!(
            pg.client
                .query_one(
                    "SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations
                      WHERE request_id='shadow-pg-request'",
                    &[],
                )
                .unwrap()
                .get::<_, i64>(0),
            1
        );

        let money_after_shadow: (i64, i64, i64, String) = {
            let row = pg
                .client
                .query_one(
                    "SELECT a.balance_nano,a.reserved_nano,r.hold_nano,r.state
                       FROM accounts a JOIN reservations r ON r.account_id=a.id
                      WHERE r.request_id='shadow-pg-request'",
                    &[],
                )
                .unwrap();
            (row.get(0), row.get(1), row.get(2), row.get(3))
        };
        assert_eq!(money_after_shadow, money_before);

        for (request_id, outcome) in [
            (
                "shadow-pg-rejected",
                PricingShadowEvaluationOutcome::Rejected {
                    reason: PricingShadowRejectionCode::NoPolicyBinding,
                    observed_multiplier_bp: 2_000,
                },
            ),
            (
                "shadow-pg-read-error",
                PricingShadowEvaluationOutcome::ReadError {
                    reason: PricingShadowReadErrorCode::PricingReadFailed,
                },
            ),
        ] {
            let snapshot =
                legacy_snapshot(request_id, "shadow-pg-account", 500_000_000, 100_000_000);
            assert!(matches!(
                pg.reserve_request_with_legacy_snapshot(&owner, "shadow-pg-key", 60, &snapshot,)
                    .unwrap(),
                LegacyScalarReserveOutcome::Inserted(_)
            ));
            let diagnostic = if request_id == "shadow-pg-read-error" {
                let empty = serde_json::to_string(&json!({"payload": ""})).unwrap();
                let boundary = json!({"payload": "x".repeat(4_096 - empty.len())});
                assert_eq!(serde_json::to_string(&boundary).unwrap().len(), 4_096);
                boundary
            } else {
                json!({})
            };
            let input = shadow_pg_input(
                &snapshot,
                outcome.clone(),
                snapshot.admission_ts() + 1,
                snapshot.admission_ts() + 2,
                diagnostic,
            );
            assert!(matches!(
                pg.insert_pricing_shadow_admission_evaluation(&input)
                    .unwrap(),
                Write::Inserted(_)
            ));
            assert_eq!(
                pg.pricing_shadow_admission_evaluation(request_id)
                    .unwrap()
                    .unwrap()
                    .outcome(),
                &outcome
            );
        }

        pg.settle_request(
            "shadow-pg-read-error",
            10,
            Some("shadow-retention-settle"),
            None,
        )
        .unwrap();
        assert!(pg.maintenance_prune(now()).is_err());
        let rows_after_unsafe_prune = pg
            .client
            .query_one(
                "SELECT \
                    (SELECT COUNT(*)::bigint FROM reservations \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations \
                      WHERE request_id='shadow-pg-read-error')",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                rows_after_unsafe_prune.get::<_, i64>(0),
                rows_after_unsafe_prune.get::<_, i64>(1),
                rows_after_unsafe_prune.get::<_, i64>(2),
            ),
            (1, 1, 1)
        );
        pg.client
            .batch_execute(
                "UPDATE reservations SET settled_ts=100 \
                   WHERE request_id='shadow-pg-read-error'; \
                 UPDATE settlement_outbox SET committed_ts=100,state='done' \
                   WHERE request_id='shadow-pg-read-error';",
            )
            .unwrap();
        let ledger_before_retention: i64 = pg
            .client
            .query_one("SELECT COUNT(*)::bigint FROM ledger", &[])
            .unwrap()
            .get(0);
        let retention = pg.maintenance_prune(200).unwrap();
        assert_eq!(retention.outbox, 1);
        assert_eq!(retention.reservations, 1);
        assert_eq!(retention.pricing_snapshots_cascaded, 1);
        assert_eq!(retention.pricing_shadow_evaluations_cascaded, 1);
        let retained_counts = pg
            .client
            .query_one(
                "SELECT \
                    (SELECT COUNT(*)::bigint FROM reservations \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM pricing_shadow_admission_evaluations \
                      WHERE request_id='shadow-pg-read-error'), \
                    (SELECT COUNT(*)::bigint FROM ledger)",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                retained_counts.get::<_, i64>(0),
                retained_counts.get::<_, i64>(1),
                retained_counts.get::<_, i64>(2),
                retained_counts.get::<_, i64>(3),
            ),
            (0, 0, 0, ledger_before_retention)
        );

        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }

    #[test]
    fn engine_migration_plan_is_contiguous() {
        let versions: Vec<_> = ENGINE_MIGRATIONS
            .iter()
            .map(|(version, _)| *version)
            .collect();
        assert_eq!(versions, (1..=CURRENT_SCHEMA_VERSION).collect::<Vec<_>>());
    }

    /// Run with an isolated database, for example:
    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry pg::tests::stage2_fault_matrix`
    #[test]
    fn stage2_fault_matrix() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!("skipping PostgreSQL fault matrix: CLAUDE_API_TEST_DATABASE_URL is unset");
            return;
        };
        // Keep the destructive-test lock on a dedicated session: this matrix intentionally drops
        // and recreates its working PgStore while exercising crash recovery.
        let mut lock_holder = PgStore::connect(&url).unwrap();
        lock_holder
            .client
            .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
            .unwrap();
        lock_holder
            .client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = PgStore::connect(&url).unwrap();
        pg.client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .unwrap();
        pg.migrate().unwrap();
        assert_eq!(pg.schema_version().unwrap(), 10);
        pg.migrate().unwrap();
        assert_eq!(pg.schema_version().unwrap(), 10);
        let runtime_pin_constraints: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM pg_constraint
                 WHERE conname IN (
                     'provider_switch_versions_capability_identity',
                     'provider_switch_versions_ack_identity',
                     'provider_switch_entries_catalog_fk',
                     'provider_switch_entries_catalog_scope',
                     'account_policy_versions_switch_fk',
                     'account_policy_versions_ack_identity',
                     'pricing_catalog_versions_capability_generation',
                     'pricing_catalog_versions_ack_identity',
                     'account_policy_versions_source_identity',
                     'account_policy_versions_class_identity',
                     'account_policy_versions_lineage_identity',
                     'account_policy_bindings_active_class_fk'
                 )",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(runtime_pin_constraints, 12);
        pg.client
            .batch_execute(
                "TRUNCATE account_policy_bindings,account_policy_rules,account_policy_versions, \
             provider_switch_head,provider_switch_entries,provider_switch_versions, \
             pricing_catalog_heads,pricing_catalog_entries,pricing_catalog_versions, \
             codex_window_observations,codex_window_calibrations,codex_home_spend, \
             settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
            )
            .unwrap();

        assert_eq!(
            pg.credit_codex_home_spend("stage2-codex-home", 40_000_000_000, 100)
                .unwrap(),
            40_000_000_000
        );
        assert_eq!(
            pg.credit_codex_home_spend("stage2-codex-home", 60_000_000_000, 101)
                .unwrap(),
            100_000_000_000
        );
        let state = CodexCalibrationRow {
            home_id: "stage2-codex-home".into(),
            window_duration_mins: 300,
            resets_at: 2_000_000_000,
            anchor_used_percent: 10,
            anchor_spend_nano: 100_000_000_000,
            used_percent: 10,
            observed_at: 101,
            sum_used_sq: 0,
            sum_used_spend_nano: 0,
            observed_points: 0,
            samples: 0,
            current_capacity_nano: None,
            current_low_nano: None,
            current_high_nano: None,
            current_confidence_bp: 0,
            last_capacity_nano: None,
            last_low_nano: None,
            last_high_nano: None,
            last_confidence_bp: 0,
            last_measured_at: None,
            estimator_version: 1,
            version: 0,
            updated_ts: 101,
        };
        let observation = CodexWindowObservation {
            home_id: state.home_id.clone(),
            window_duration_mins: state.window_duration_mins,
            resets_at: state.resets_at,
            observed_at: state.observed_at,
            used_percent: state.used_percent,
            gateway_spend_nano: state.anchor_spend_nano,
        };
        assert_eq!(
            pg.save_codex_calibration(&state, &observation).unwrap(),
            Some(1)
        );
        assert_eq!(
            pg.save_codex_calibration(&state, &observation).unwrap(),
            None
        );
        assert_eq!(
            pg.load_codex_calibration("stage2-codex-home", 300)
                .unwrap()
                .unwrap()
                .version,
            1
        );
        pg.client
            .batch_execute(
                "DELETE FROM codex_window_observations WHERE home_id='stage2-codex-home'; \
                 DELETE FROM codex_window_calibrations WHERE home_id='stage2-codex-home'; \
                 DELETE FROM codex_home_spend WHERE home_id='stage2-codex-home';",
            )
            .unwrap();

        let trigger_count: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM pg_trigger \
                 WHERE tgname IN ('pricing_snapshot_reservation_account', \
                                  'pricing_snapshot_immutable_update', \
                                  'pricing_shadow_admission_evaluation_rule_identity', \
                                  'pricing_shadow_admission_evaluation_immutable_update', \
                                  'ledger_funding_allocation_account') \
                   AND NOT tgisinternal",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(trigger_count, 5);
        let seeded_policy_rows: i64 = pg
            .client
            .query_one(
                "SELECT (SELECT COUNT(*) FROM pricing_catalog_versions) \
                      + (SELECT COUNT(*) FROM provider_switch_versions) \
                      + (SELECT COUNT(*) FROM account_policy_versions) \
                      + (SELECT COUNT(*) FROM funding_buckets) \
                      + (SELECT COUNT(*) FROM pricing_admission_snapshots) \
                      + (SELECT COUNT(*) FROM pricing_shadow_admission_evaluations)",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(seeded_policy_rows, 0);

        pg.client
            .batch_execute(
                "INSERT INTO accounts(id,mult_bp,status,created_ts,created) \
                   VALUES('schema-a',2000,'active',1,''),('schema-b',3000,'active',1,''); \
                 INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES(
                     'schema-request','schema-a','schema-key',100,0,
                     'schema-engine',1,100,'reserved',1,1
                 );",
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-request','schema-b','legacy_scalar',1,'anthropic',
                     'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                     'legacy-tariff',1,1,100,20,'{}'::jsonb,'snapshot'
                 )",
                &[],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-request','schema-a','legacy_scalar',1,'anthropic',
                     'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                     'legacy-tariff',1,1,100,20,'{}'::jsonb,'snapshot'
                 )",
                &[],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "UPDATE pricing_admission_snapshots
                 SET charged_hold_nano=21 WHERE request_id='schema-request'",
                &[],
            )
            .is_err());
        assert!(pg
            .client
            .execute(
                "INSERT INTO pricing_shadow_admission_evaluations(
                     request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,
                     provider_id,requested_model_id,canonical_model_id,
                     alias_generation,evaluator_schema_version,runtime_manifest_generation,
                     runtime_manifest_digest,enqueued_ts,evaluated_ts,outcome,reason_code,
                     authorized_multiplier_bp,observed_multiplier_bp,official_hold_nano,
                     legacy_hold_nano,
                     comparison_result,diagnostic_context,evaluation_digest
                 ) VALUES(
                     'schema-request','schema-b','legacy_scalar','snapshot',
                     'anthropic','claude-test','claude-test',
                     1,1,1,'runtime-manifest',1,2,'rejected','no_policy_binding',
                     2000,2000,100,20,'not_comparable','{}'::jsonb,'shadow-rejected'
                 )",
                &[],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO pricing_shadow_admission_evaluations(
                     request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,
                     provider_id,requested_model_id,canonical_model_id,
                     alias_generation,evaluator_schema_version,runtime_manifest_generation,
                     runtime_manifest_digest,enqueued_ts,evaluated_ts,outcome,reason_code,
                     authorized_multiplier_bp,observed_multiplier_bp,official_hold_nano,
                     legacy_hold_nano,
                     comparison_result,diagnostic_context,evaluation_digest
                 ) VALUES(
                     'schema-request','schema-a','legacy_scalar','snapshot',
                     'anthropic','claude-test','claude-test',
                     1,1,1,'runtime-manifest',1,2,'rejected','no_policy_binding',
                     2000,2000,100,20,'not_comparable','{}'::jsonb,'shadow-rejected'
                 )",
                &[],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "UPDATE pricing_shadow_admission_evaluations
                 SET reason_code='different_reason' WHERE request_id='schema-request'",
                &[],
            )
            .is_err());
        pg.client
            .batch_execute(
                "INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES
                     ('schema-paid-a','schema-a','paid','primary','any',1000,0,0,1,'active',1,1),
                     ('schema-paid-b','schema-b','paid','primary','any',1000,0,0,1,'active',1,1);
                 INSERT INTO ledger(
                     account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts
                 ) VALUES('schema-b','schema-key','charge','schema-ledger-request',10,'schema-charge',990,1);",
            )
            .unwrap();
        let ledger_id: i64 = pg
            .client
            .query_one(
                "SELECT id FROM ledger WHERE request_id='schema-ledger-request'",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(pg
            .client
            .execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano
                 ) VALUES($1,'schema-a','schema-paid-a','paid',1,'debit',10)",
                &[&ledger_id],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano
                 ) VALUES($1,'schema-b','schema-paid-b','paid',1,'debit',10)",
                &[&ledger_id],
            )
            .unwrap();
        pg.client
            .batch_execute(
                "INSERT INTO pricing_catalog_versions(
                     product_id,generation,schema_version,capability_generation,capability_digest,
                     content_digest,created_ts
                 ) VALUES('schema-main',1,1,1,'capability','catalog',1);
                 INSERT INTO provider_switch_versions(
                     generation,schema_version,capability_generation,capability_digest,
                     content_digest,created_ts
                 ) VALUES(1,1,1,'capability','switch',1);
                 INSERT INTO account_policy_versions(
                     account_id,effective_version,policy_id,policy_version,source_policy_digest,
                     owner_type,owner_id,account_class,product_id,schema_version,
                     catalog_generation,switch_generation,
                     content_digest,replacement_locked,created_ts
                 ) VALUES(
                     'schema-a',1,'schema-policy',1,'source-policy','global_b2c','global','b2c',
                     'schema-main',1,1,1,'policy',false,1
                 );",
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES(
                     'schema-a','schema-main','b2b',1,
                     'shadow','legacy_single','pending',1
                 )",
                &[],
            )
            .is_err());
        pg.client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES(
                     'schema-a','schema-main','b2c',1,
                     'shadow','legacy_single','pending',1
                 )",
                &[],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                "INSERT INTO account_policy_rules(
                     account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                     canonical_model_id,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
                 ) VALUES(
                     'schema-a',1,'missing-discount','rule','model','anthropic','claude-test',
                     'discount','managed',NULL,5000,false,false,false
                 )",
                &[],
            )
            .is_err());
        pg.client
            .batch_execute(
                "INSERT INTO pricing_catalog_entries(
                     product_id,generation,provider_id,canonical_model_id,enabled
                 ) VALUES('schema-main',1,'anthropic','claude-test',true);
                 INSERT INTO provider_switch_entries(
                     generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
                 ) VALUES(1,'anthropic','segment','schema-main','b2c',1,true);
                 INSERT INTO account_policy_rules(
                     account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                     canonical_model_id,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
                 ) VALUES(
                     'schema-a',1,'managed-rule','managed-rule-digest','provider','anthropic',NULL,
                     'discount','managed',6000,4000,false,false,false
                 );
                 INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES
                     (
                         'schema-policy-request','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     ),
                     (
                         'schema-shadow-request','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,product_id,
                     account_class,requested_model_id,canonical_model_id,alias_generation,
                     rule_id,rule_digest,rule_scope,pricing_mode,rule_origin,discount_bps,
                     payable_multiplier_bp,policy_id,policy_version,effective_policy_version,
                     policy_digest,catalog_generation,switch_generation,tariff_schedule_id,
                     tariff_priced_ts,admission_ts,official_hold_nano,charged_hold_nano,
                     track_eligible,retention_eligible,commission_eligible,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-policy-request','schema-a','policy_v1',1,'anthropic','schema-main',
                     'b2c','claude-test','claude-test',1,'managed-rule','managed-rule-digest',
                     'provider','discount','managed',6000,4000,'schema-policy',1,1,'policy',1,1,
                     'tariff',1,1,100,40,false,false,false,'{}'::jsonb,'policy-snapshot'
                 );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES(
                     'schema-shadow-request','schema-a','legacy_scalar',1,'anthropic',
                     'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                     'legacy-tariff',1,1,100,20,'{}'::jsonb,'actual-snapshot'
                 );",
            )
            .unwrap();
        let resolved_shadow_sql = "INSERT INTO pricing_shadow_admission_evaluations(
                 request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,evaluator_schema_version,
                 runtime_manifest_generation,runtime_manifest_digest,enqueued_ts,evaluated_ts,
                 outcome,authorized_multiplier_bp,observed_multiplier_bp,official_hold_nano,
                 legacy_hold_nano,product_id,account_class,effective_policy_version,policy_id,
                 policy_version,source_policy_digest,policy_digest,policy_schema_version,
                 policy_catalog_generation,policy_catalog_schema_version,
                 policy_catalog_capability_generation,policy_catalog_capability_digest,
                 policy_catalog_digest,policy_switch_generation,policy_switch_schema_version,
                 policy_switch_capability_generation,policy_switch_capability_digest,
                 policy_switch_digest,admission_catalog_generation,admission_catalog_schema_version,
                 admission_catalog_capability_generation,admission_catalog_capability_digest,
                 admission_catalog_digest,admission_switch_generation,admission_switch_schema_version,
                 admission_switch_capability_generation,admission_switch_capability_digest,
                 admission_switch_digest,rule_id,rule_digest,rule_scope,pricing_mode,rule_origin,
                 discount_bps,payable_multiplier_bp,track_eligible,retention_eligible,
                 commission_eligible,policy_hold_nano,comparison_result,diagnostic_context,
                 evaluation_digest
             ) VALUES(
                 'schema-shadow-request','schema-a','legacy_scalar',$1,$2,
                 'claude-test','claude-test',1,1,1,'runtime-manifest',1,2,
                 'resolved',$3,2000,$4,$5,'schema-main','b2c',1,'schema-policy',1,
                 'source-policy','policy',1,1,
                 CASE WHEN $11='policy_catalog_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_catalog_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_catalog_capability_digest' THEN NULL ELSE $6 END,
                 'catalog',1,
                 CASE WHEN $11='policy_switch_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_switch_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='policy_switch_capability_digest' THEN NULL ELSE $6 END,
                 'switch',1,
                 CASE WHEN $11='admission_catalog_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_catalog_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_catalog_capability_digest' THEN NULL ELSE $6 END,
                 'catalog',1,
                 CASE WHEN $11='admission_switch_schema_version' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_switch_capability_generation' THEN NULL ELSE 1 END,
                 CASE WHEN $11='admission_switch_capability_digest' THEN NULL ELSE $6 END,
                 'switch','managed-rule','managed-rule-digest','provider',
                 'discount','managed',$7,$8,false,false,false,$9,'different','{}'::jsonb,$10
             )";
        let mut assert_shadow_rejected =
            |actual_digest: &str,
             provider: &str,
             authorized_multiplier_bp: i64,
             official_hold_nano: i64,
             legacy_hold_nano: i64,
             capability_digest: &str,
             discount_bps: i64,
             payable_multiplier_bp: i64,
             evaluation_digest: &str| {
                assert!(pg
                    .client
                    .execute(
                        resolved_shadow_sql,
                        &[
                            &actual_digest,
                            &provider,
                            &authorized_multiplier_bp,
                            &official_hold_nano,
                            &legacy_hold_nano,
                            &capability_digest,
                            &discount_bps,
                            &payable_multiplier_bp,
                            &40_i64,
                            &evaluation_digest,
                            &"",
                        ],
                    )
                    .is_err());
            };
        assert_shadow_rejected(
            "wrong-actual-snapshot",
            "anthropic",
            2000,
            100,
            20,
            "capability",
            6000,
            4000,
            "wrong-actual-digest",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "openai",
            2000,
            100,
            20,
            "capability",
            6000,
            4000,
            "wrong-actual-provider",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2001,
            100,
            20,
            "capability",
            6000,
            4000,
            "wrong-actual-multiplier",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            101,
            20,
            "capability",
            6000,
            4000,
            "wrong-official-hold",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            100,
            21,
            "capability",
            6000,
            4000,
            "wrong-legacy-hold",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            100,
            20,
            "wrong-capability",
            6000,
            4000,
            "wrong-capability",
        );
        assert_shadow_rejected(
            "actual-snapshot",
            "anthropic",
            2000,
            100,
            20,
            "capability",
            5000,
            5000,
            "wrong-rule-economics",
        );
        for null_field in [
            "policy_catalog_schema_version",
            "policy_catalog_capability_generation",
            "policy_catalog_capability_digest",
            "policy_switch_schema_version",
            "policy_switch_capability_generation",
            "policy_switch_capability_digest",
            "admission_catalog_schema_version",
            "admission_catalog_capability_generation",
            "admission_catalog_capability_digest",
            "admission_switch_schema_version",
            "admission_switch_capability_generation",
            "admission_switch_capability_digest",
        ] {
            assert!(pg
                .client
                .execute(
                    resolved_shadow_sql,
                    &[
                        &"actual-snapshot",
                        &"anthropic",
                        &2000_i64,
                        &100_i64,
                        &20_i64,
                        &"capability",
                        &6000_i64,
                        &4000_i64,
                        &40_i64,
                        &null_field,
                        &null_field,
                    ],
                )
                .is_err());
        }
        pg.client
            .execute(
                resolved_shadow_sql,
                &[
                    &"actual-snapshot",
                    &"anthropic",
                    &2000_i64,
                    &100_i64,
                    &20_i64,
                    &"capability",
                    &6000_i64,
                    &4000_i64,
                    &40_i64,
                    &"shadow-resolved",
                    &"",
                ],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                resolved_shadow_sql,
                &[
                    &"actual-snapshot",
                    &"anthropic",
                    &2000_i64,
                    &100_i64,
                    &20_i64,
                    &"capability",
                    &6000_i64,
                    &4000_i64,
                    &40_i64,
                    &"shadow-resolved",
                    &"",
                ],
            )
            .is_err());
        pg.client
            .batch_execute(
                "INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts
                 ) VALUES
                     (
                         'schema-shadow-read-error','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     ),
                     (
                         'schema-shadow-rejected','schema-a','schema-key',100,0,
                         'schema-engine',1,100,'reserved',1,1
                     );
                 INSERT INTO pricing_admission_snapshots(
                     request_id,account_id,snapshot_kind,schema_version,provider_id,
                     requested_model_id,canonical_model_id,alias_generation,pricing_mode,
                     rule_origin,payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,
                     admission_ts,official_hold_nano,charged_hold_nano,premium_modifiers,
                     snapshot_digest
                 ) VALUES
                     (
                         'schema-shadow-read-error','schema-a','legacy_scalar',1,'anthropic',
                         'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                         'legacy-tariff',1,1,100,20,'{}'::jsonb,'failure-actual'
                     ),
                     (
                         'schema-shadow-rejected','schema-a','legacy_scalar',1,'anthropic',
                         'claude-test','claude-test',1,'legacy_scalar','legacy',2000,
                         'legacy-tariff',1,1,100,20,'{}'::jsonb,'failure-actual'
                     );",
            )
            .unwrap();
        let failure_shadow_sql = "INSERT INTO pricing_shadow_admission_evaluations(
                 request_id,account_id,actual_snapshot_kind,actual_snapshot_digest,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,evaluator_schema_version,
                 runtime_manifest_generation,runtime_manifest_digest,enqueued_ts,evaluated_ts,
                 outcome,reason_code,authorized_multiplier_bp,observed_multiplier_bp,
                 official_hold_nano,legacy_hold_nano,comparison_result,diagnostic_context,
                 evaluation_digest
             ) VALUES(
                 $1,'schema-a','legacy_scalar','failure-actual','anthropic',
                 'claude-test','claude-test',1,1,1,'runtime-manifest',1,2,
                 $2,'authority_read',2000,$3,100,20,'not_comparable','{}'::jsonb,$4
             )";
        assert!(pg
            .client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-read-error",
                    &"rejected",
                    &Option::<i64>::None,
                    &"missing-rejected-observation",
                ],
            )
            .is_err());
        pg.client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-read-error",
                    &"read_error",
                    &Option::<i64>::None,
                    &"read-error",
                ],
            )
            .unwrap();
        assert!(pg
            .client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-rejected",
                    &"read_error",
                    &Some(2000_i64),
                    &"unexpected-read-observation",
                ],
            )
            .is_err());
        pg.client
            .execute(
                failure_shadow_sql,
                &[
                    &"schema-shadow-rejected",
                    &"rejected",
                    &Some(2000_i64),
                    &"rejected",
                ],
            )
            .unwrap();
        pg.client.batch_execute(
            "TRUNCATE settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE; \
             DELETE FROM provider_switch_entries; \
             DELETE FROM provider_switch_head; \
             DELETE FROM provider_switch_versions; \
             DELETE FROM pricing_catalog_entries; \
             DELETE FROM pricing_catalog_heads; \
             DELETE FROM pricing_catalog_versions;",
        ).unwrap();

        // Exercise the real one-time SQLite importer before the transactional fault matrix.
        let sqlite_path = std::env::temp_dir().join(format!(
            "claude-stage2-import-{}-{}.db",
            std::process::id(),
            now()
        ));
        let sqlite_path_s = sqlite_path.to_string_lossy().into_owned();
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            crate::add(&sqlite, "import-sub", "import-token", "", "prod").unwrap();
            crate::account_create(&sqlite, "import-acct", Some("import-handle"), 2000).unwrap();
            crate::key_issue(&sqlite, "import-key", "import-acct", Some("imported")).unwrap();
            crate::account_topup(&sqlite, "import-acct", 5_000, Some("import-seed")).unwrap();
            crate::account_reserve(&sqlite, "import-acct", 1_000).unwrap();
            crate::account_settle(
                &sqlite,
                "import-acct",
                "import-key",
                1_000,
                200,
                Some("import-charge"),
                Some(&UsageEventInput {
                    model: "gpt-import-test".into(),
                    provider: crate::PROVIDER_OPENAI.into(),
                    input_tokens: 11,
                    output_tokens: 12,
                    cache_read_tokens: 13,
                    cache_write_5m_tokens: 14,
                    cache_write_1h_tokens: 15,
                    web_search_requests: 16,
                    real_nano: 180,
                    speed: "fast".into(),
                    inference_geo: "us-east".into(),
                    input_nano: 21,
                    output_nano: 22,
                    cache_read_nano: 23,
                    cache_write_5m_nano: 24,
                    cache_write_1h_nano: 25,
                    web_search_nano: 65,
                    priced_ts: 123_456,
                }),
            )
            .unwrap();
            crate::save_pool_state(
                &sqlite,
                &[PoolStateRow {
                    email: "import-sub".into(),
                    cooling_until: 123,
                    version: 0,
                    ..Default::default()
                }],
            )
            .unwrap();
        }
        let imported = pg.import_sqlite(&sqlite_path_s).unwrap();
        assert_eq!(
            (imported.subscriptions, imported.accounts, imported.keys),
            (1, 1, 1)
        );
        assert_eq!(
            (
                imported.balance_nano,
                imported.spent_nano,
                imported.reserved_nano
            ),
            (4_800, 200, 0)
        );
        let imported_usage = pg
            .client
            .query_one(
                "SELECT request_id,account_id,key,model,provider,
                        input_tokens,output_tokens,cache_read_tokens,
                        cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,
                        real_nano,charge_nano,ref,speed,inference_geo,
                        input_nano,output_nano,cache_read_nano,cache_write_5m_nano,
                        cache_write_1h_nano,web_search_nano,priced_ts
                 FROM usage_events",
                &[],
            )
            .unwrap();
        assert_eq!(imported_usage.get::<_, Option<String>>(0), None);
        assert_eq!(imported_usage.get::<_, String>(1), "import-acct");
        assert_eq!(
            imported_usage.get::<_, Option<String>>(2).as_deref(),
            Some("import-key")
        );
        assert_eq!(
            (
                imported_usage.get::<_, Option<String>>(3).as_deref(),
                imported_usage.get::<_, String>(4).as_str()
            ),
            (Some("gpt-import-test"), crate::PROVIDER_OPENAI)
        );
        assert_eq!(
            (
                imported_usage.get::<_, i64>(5),
                imported_usage.get::<_, i64>(6),
                imported_usage.get::<_, i64>(7),
                imported_usage.get::<_, i64>(8),
                imported_usage.get::<_, i64>(9),
                imported_usage.get::<_, i64>(10)
            ),
            (11, 12, 13, 14, 15, 16)
        );
        assert_eq!(
            (
                imported_usage.get::<_, i64>(11),
                imported_usage.get::<_, i64>(12),
                imported_usage.get::<_, Option<String>>(13).as_deref(),
                imported_usage.get::<_, String>(14).as_str(),
                imported_usage.get::<_, String>(15).as_str()
            ),
            (180, 200, Some("import-charge"), "fast", "us-east")
        );
        assert_eq!(
            (
                imported_usage.get::<_, i64>(16),
                imported_usage.get::<_, i64>(17),
                imported_usage.get::<_, i64>(18),
                imported_usage.get::<_, i64>(19),
                imported_usage.get::<_, i64>(20),
                imported_usage.get::<_, i64>(21),
                imported_usage.get::<_, i64>(22)
            ),
            (21, 22, 23, 24, 25, 65, 123_456)
        );
        pg.client
            .execute(
                "INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES(
                     'target-policy-bucket','import-acct','paid','primary','any',
                     4800,0,200,1,'active',1,1
                 )",
                &[],
            )
            .unwrap();
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "materialized PostgreSQL policy/funding authority must block the legacy importer"
        );
        pg.client
            .execute(
                "DELETE FROM funding_buckets WHERE bucket_id='target-policy-bucket'",
                &[],
            )
            .unwrap();
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            sqlite
                .execute(
                    "INSERT INTO funding_buckets(
                         bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                         reserved_nano,spent_nano,version,status,created_ts,updated_ts
                     ) VALUES(
                         'import-policy-bucket','import-acct','paid','primary','any',
                         4800,0,200,1,'active',1,1
                     )",
                    [],
                )
                .unwrap();
        }
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "policy/funding state must require the policy-aware migration path"
        );
        let preserved_account = pg
            .client
            .query_one(
                "SELECT balance_nano,spent_nano,reserved_nano FROM accounts WHERE id='import-acct'",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                preserved_account.get::<_, i64>(0),
                preserved_account.get::<_, i64>(1),
                preserved_account.get::<_, i64>(2)
            ),
            (4_800, 200, 0),
            "a failed policy-aware preflight must not delete PostgreSQL authority"
        );
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            sqlite
                .execute(
                    "DELETE FROM funding_buckets WHERE bucket_id='import-policy-bucket'",
                    [],
                )
                .unwrap();
            sqlite
                .execute(
                    "UPDATE ledger SET official_nano=180 WHERE ref='import-charge'",
                    [],
                )
                .unwrap();
        }
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "new official-cost attribution must require the policy-aware migration path"
        );
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            sqlite
                .execute(
                    "UPDATE ledger SET official_nano=NULL WHERE ref='import-charge'",
                    [],
                )
                .unwrap();
            crate::account_reserve(&sqlite, "import-acct", 100).unwrap();
        }
        assert!(
            pg.import_sqlite(&sqlite_path_s).is_err(),
            "anonymous SQLite hold must block cutover"
        );
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{sqlite_path_s}{suffix}"));
        }
        pg.client.batch_execute(
            "TRUNCATE settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
        ).unwrap();

        pg.add("sub@test", "token", "", "prod").unwrap();
        pg.account_create("acct", Some("handle"), 2000).unwrap();
        pg.key_issue("key", "acct", Some("primary")).unwrap();
        assert_eq!(
            pg.account_topup("acct", 1_000, Some("seed")).unwrap(),
            Some(1_000)
        );
        assert_eq!(
            pg.account_topup("acct", 1_000, Some("seed")).unwrap(),
            Some(1_000)
        );
        assert!(pg.account_topup("acct", 999, Some("seed")).is_err());

        let owner = pg.claim_instance("engine-a", 60).unwrap();
        pg.account_create("policy-acct", None, 10_000).unwrap();
        pg.account_topup("policy-acct", 1_000, Some("policy-seed"))
            .unwrap();
        pg.key_issue_with_policy(
            "limited-key",
            "policy-acct",
            Some("limited"),
            Some(700),
            Some(now() + 60),
        )
        .unwrap();
        assert_eq!(
            pg.reserve_request(&owner, "policy-1", "policy-acct", "limited-key", 500, 60)
                .unwrap(),
            Some(500)
        );
        assert_eq!(
            pg.reserve_request(&owner, "policy-1", "policy-acct", "limited-key", 500, 60)
                .unwrap(),
            Some(500)
        );
        assert_eq!(
            pg.key_get("limited-key").unwrap().unwrap().reserved_nano,
            500
        );
        let limited_key_id = pg.key_get("limited-key").unwrap().unwrap().key_id;
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &limited_key_id, Some(499), None)
                .unwrap(),
            KeyPolicyUpdate::LimitBelowUsage,
        );
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &limited_key_id, Some(700), Some(now() + 120))
                .unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert_eq!(
            pg.reserve_request(&owner, "policy-2", "policy-acct", "limited-key", 300, 60)
                .unwrap(),
            None
        );
        assert_eq!(pg.cancel_request("policy-1").unwrap(), Some(1_000));
        assert_eq!(pg.cancel_request("policy-1").unwrap(), Some(1_000));
        assert_eq!(
            pg.reserve_request(&owner, "policy-3", "policy-acct", "limited-key", 700, 60)
                .unwrap(),
            Some(300)
        );
        assert_eq!(
            pg.settle_request("policy-3", 650, None, None).unwrap(),
            Some(350)
        );
        let limited = pg.key_get("limited-key").unwrap().unwrap();
        assert_eq!(
            (
                limited.spent_nano,
                limited.reserved_nano,
                limited.spend_limit_nano
            ),
            (650, 0, Some(700))
        );
        assert_eq!(
            pg.reserve_request(
                &owner,
                "policy-boundary",
                "policy-acct",
                "limited-key",
                50,
                60
            )
            .unwrap(),
            Some(300)
        );
        assert_eq!(
            pg.settle_request("policy-boundary", 50, None, None)
                .unwrap(),
            Some(300)
        );
        assert_eq!(
            pg.reserve_request(&owner, "policy-over", "policy-acct", "limited-key", 1, 60)
                .unwrap(),
            None
        );
        pg.key_issue_with_policy("expired-key", "policy-acct", None, None, Some(now()))
            .unwrap();
        assert_eq!(
            pg.reserve_request(
                &owner,
                "policy-expired",
                "policy-acct",
                "expired-key",
                1,
                60
            )
            .unwrap(),
            None
        );
        let expired_key_id = pg.key_get("expired-key").unwrap().unwrap().key_id;
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &expired_key_id, None, Some(now() + 60))
                .unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert!(pg
            .reserve_request(
                &owner,
                "policy-extended",
                "policy-acct",
                "expired-key",
                1,
                60
            )
            .unwrap()
            .is_some());
        pg.cancel_request("policy-extended").unwrap();
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", &expired_key_id, None, None)
                .unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct", "key_missing", None, None)
                .unwrap(),
            KeyPolicyUpdate::NotFound,
        );
        pg.key_issue_with_policy("disabled-key", "policy-acct", None, None, None)
            .unwrap();
        pg.key_set_status("disabled-key", "disabled").unwrap();
        assert_eq!(
            pg.reserve_request(
                &owner,
                "policy-disabled",
                "policy-acct",
                "disabled-key",
                1,
                60
            )
            .unwrap(),
            None
        );

        pg.account_create("concurrent-policy-acct", None, 10_000)
            .unwrap();
        pg.account_topup(
            "concurrent-policy-acct",
            1_000,
            Some("concurrent-policy-seed"),
        )
        .unwrap();
        pg.key_issue_with_policy(
            "concurrent-limited-key",
            "concurrent-policy-acct",
            None,
            Some(700),
            None,
        )
        .unwrap();
        let policy_barrier = Arc::new(Barrier::new(3));
        let mut policy_joins = Vec::new();
        for n in 0..2 {
            let url = url.clone();
            let owner = owner.clone();
            let barrier = Arc::clone(&policy_barrier);
            policy_joins.push(std::thread::spawn(move || {
                let mut connection = PgStore::connect(&url).unwrap();
                let request_id = format!("concurrent-policy-{n}");
                barrier.wait();
                let result = connection
                    .reserve_request(
                        &owner,
                        &request_id,
                        "concurrent-policy-acct",
                        "concurrent-limited-key",
                        400,
                        60,
                    )
                    .unwrap();
                (request_id, result)
            }));
        }
        policy_barrier.wait();
        let policy_results: Vec<_> = policy_joins
            .into_iter()
            .map(|join| join.join().unwrap())
            .collect();
        assert_eq!(
            policy_results
                .iter()
                .filter(|(_, result)| result.is_some())
                .count(),
            1,
            "concurrent reservations must not jointly cross a key cap"
        );
        for (request_id, result) in policy_results {
            if result.is_some() {
                pg.cancel_request(&request_id).unwrap();
            }
        }
        assert_eq!(
            pg.key_get("concurrent-limited-key")
                .unwrap()
                .unwrap()
                .reserved_nano,
            0
        );

        // A reserve racing a stricter policy replacement must serialize on the key row. The two
        // incompatible operations can never both succeed.
        pg.account_create("policy-update-race-acct", None, 10_000)
            .unwrap();
        pg.account_topup(
            "policy-update-race-acct",
            1_000,
            Some("policy-update-race-seed"),
        )
        .unwrap();
        pg.key_issue_with_policy(
            "policy-update-race-key",
            "policy-update-race-acct",
            None,
            Some(1_000),
            None,
        )
        .unwrap();
        let race_key_id = pg
            .key_get("policy-update-race-key")
            .unwrap()
            .unwrap()
            .key_id;
        let race_barrier = Arc::new(Barrier::new(3));
        let reserve_url = url.clone();
        let reserve_owner = owner.clone();
        let reserve_barrier = Arc::clone(&race_barrier);
        let reserve_join = std::thread::spawn(move || {
            let mut connection = PgStore::connect(&reserve_url).unwrap();
            reserve_barrier.wait();
            connection
                .reserve_request(
                    &reserve_owner,
                    "policy-update-race-request",
                    "policy-update-race-acct",
                    "policy-update-race-key",
                    400,
                    60,
                )
                .unwrap()
                .is_some()
        });
        let update_url = url.clone();
        let update_barrier = Arc::clone(&race_barrier);
        let update_join = std::thread::spawn(move || {
            let mut connection = PgStore::connect(&update_url).unwrap();
            update_barrier.wait();
            connection
                .key_set_policy_by_id("policy-update-race-acct", &race_key_id, Some(300), None)
                .unwrap()
                == KeyPolicyUpdate::Updated
        });
        race_barrier.wait();
        let reserve_won = reserve_join.join().unwrap();
        let update_won = update_join.join().unwrap();
        assert_ne!(
            reserve_won, update_won,
            "exactly one incompatible racing operation must succeed"
        );
        let raced_key = pg.key_get("policy-update-race-key").unwrap().unwrap();
        if let Some(limit) = raced_key.spend_limit_nano {
            assert!(raced_key.spent_nano + raced_key.reserved_nano <= limit);
        }
        assert_eq!(
            pg.account_get("policy-update-race-acct")
                .unwrap()
                .unwrap()
                .reserved_nano,
            raced_key.reserved_nano,
        );
        if reserve_won {
            pg.cancel_request("policy-update-race-request").unwrap();
        }

        assert_eq!(
            pg.reserve_request(&owner, "req-1", "acct", "key", 600, 60)
                .unwrap(),
            Some(400)
        );
        assert_eq!(
            pg.reserve_request(&owner, "req-1", "acct", "key", 600, 60)
                .unwrap(),
            Some(400)
        );
        assert!(pg.mark_delivering(&owner, "req-1", 60).unwrap());
        let usage = UsageEventInput {
            model: "claude-test".into(),
            input_tokens: 10,
            output_tokens: 20,
            real_nano: 200,
            ..Default::default()
        };
        assert_eq!(
            pg.settle_request("req-1", 250, Some("anthropic-1"), Some(&usage))
                .unwrap(),
            Some(750)
        );
        assert_eq!(
            pg.settle_request("req-1", 250, Some("anthropic-1"), Some(&usage))
                .unwrap(),
            Some(750)
        );
        assert!(pg
            .settle_request("req-1", 251, Some("anthropic-1"), Some(&usage))
            .is_err());
        let charge_count: i64 = pg
            .client
            .query_one(
                "SELECT COUNT(*)::bigint FROM ledger WHERE kind='charge' AND request_id='req-1'",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(charge_count, 1, "exact retry must not double-charge");

        assert_eq!(
            pg.reserve_request(&owner, "req-2", "acct", "key", 300, 60)
                .unwrap(),
            Some(450)
        );
        assert_eq!(pg.cancel_request("req-2").unwrap(), Some(750));
        assert_eq!(pg.cancel_request("req-2").unwrap(), Some(750));

        // Crash boundary: enqueue commits but settlement application has not run. A fresh connection
        // drains the durable row exactly once.
        assert_eq!(
            pg.reserve_request(&owner, "req-3", "acct", "key", 400, 60)
                .unwrap(),
            Some(350)
        );
        assert!(pg.mark_delivering(&owner, "req-3", 60).unwrap());
        pg.enqueue_settlement("req-3", 100, Some("anthropic-3"), None)
            .unwrap();
        drop(pg);
        let mut pg = PgStore::connect(&url).unwrap();
        assert_eq!(pg.drain_outbox(100).unwrap(), 1);
        assert_eq!(pg.drain_outbox(100).unwrap(), 0);
        let account = pg.account_get("acct").unwrap().unwrap();
        assert_eq!(
            (
                account.balance_nano,
                account.spent_nano,
                account.reserved_nano
            ),
            (650, 350, 0)
        );

        // Овердрафт-буфер ($1): funded-запрос НЕ роняем из-за гонки — баланс может уйти в лёгкий минус
        // до пола −$1 (−1e9 nano), но НИКОГДА ниже; за полом любой положительный hold отбит. (`owner`
        // ещё валиден — фенсинг ниже.)
        pg.account_create("od-acct", None, 10_000).unwrap();
        pg.key_issue("od-key", "od-acct", None).unwrap();
        pg.account_topup("od-acct", 1_000, Some("od-seed")).unwrap();
        // hold ≫ баланса, но в пределах balance+$1 → овердрафт пускает; баланс → −$0.999999.
        assert_eq!(
            pg.reserve_request(&owner, "od-1", "od-acct", "od-key", 1_000_000_000, 60)
                .unwrap(),
            Some(-999_999_000)
        );
        // добираем РОВНО до пола −$1 (граница включительно)
        assert_eq!(
            pg.reserve_request(&owner, "od-2", "od-acct", "od-key", 1_000, 60)
                .unwrap(),
            Some(-1_000_000_000)
        );
        // на полу −$1 любой положительный hold отбит (защита от бесконечного долга)
        assert_eq!(
            pg.reserve_request(&owner, "od-3", "od-acct", "od-key", 1, 60)
                .unwrap(),
            None
        );
        // на свежем аккаунте hold СВЕРХ balance+$1 → отказ (за буфером), обычный в пределах — ок
        pg.account_create("od-acct2", None, 10_000).unwrap();
        pg.key_issue("od-key2", "od-acct2", None).unwrap();
        pg.account_topup("od-acct2", 1_000, Some("od-seed2"))
            .unwrap(); // balance = 1000 nano
        assert_eq!(
            pg.reserve_request(&owner, "od-4", "od-acct2", "od-key2", 1_000_002_000, 60)
                .unwrap(),
            None
        );
        assert_eq!(
            pg.reserve_request(&owner, "od-5", "od-acct2", "od-key2", 1_000, 60)
                .unwrap(),
            Some(0)
        );
        // Снимаем наши holds → reserved_nano аккаунтов обратно в 0 (глобальный billing_totals ниже ждёт 0).
        pg.cancel_request("od-1").unwrap();
        pg.cancel_request("od-2").unwrap();
        pg.cancel_request("od-5").unwrap();

        // A later epoch with the same instance identity fences the stale writer.
        let owner2 = pg.claim_instance("engine-a", 60).unwrap();
        assert!(owner2.epoch > owner.epoch);
        assert!(pg
            .reserve_request(&owner, "stale", "acct", "key", 1, 60)
            .is_err());
        assert_eq!(
            pg.reserve_request(&owner2, "req-4", "acct", "key", 100, 60)
                .unwrap(),
            Some(550)
        );
        pg.cancel_request("req-4").unwrap();

        // Recovery distinguishes a request never delivered (refund) from a delivered response whose
        // exact usage was lost (conservatively charge the already approved hold).
        let dead = pg.claim_instance("dead-engine", 60).unwrap();
        pg.reserve_request(&dead, "req-5", "acct", "key", 100, 1)
            .unwrap();
        pg.reserve_request(&dead, "req-6", "acct", "key", 100, 1)
            .unwrap();
        pg.mark_delivering(&dead, "req-6", 1).unwrap();
        pg.client
            .execute(
                "UPDATE engine_instances SET lease_until=0 WHERE instance_id='dead-engine'",
                &[],
            )
            .unwrap();
        pg.client
            .execute(
                "UPDATE reservations SET lease_until=0 WHERE request_id IN ('req-5','req-6')",
                &[],
            )
            .unwrap();
        let recovered = pg.reconcile_expired(100).unwrap();
        assert_eq!(recovered.canceled_before_delivery, 1);
        assert_eq!(recovered.charged_after_delivery, 1);
        assert_eq!(pg.account_get("acct").unwrap().unwrap().reserved_nano, 0);

        // Pool state is versioned CAS and fenced by owner epoch.
        let mut state = pg.load_pool_state().unwrap();
        assert_eq!(state.len(), 1);
        let stale_state = state.clone();
        let versions = pg.save_pool_state(&owner2, &state).unwrap();
        assert_eq!(versions[0].1, 1);
        assert!(pg.save_pool_state(&owner2, &stale_state).is_err());
        state[0].version = versions[0].1;
        assert!(pg.save_pool_state(&owner2, &state).is_ok());

        // Atomic capacity transaction: concurrent contenders cannot exceed the envelope.
        let barrier = Arc::new(Barrier::new(9));
        let mut joins = Vec::new();
        for n in 0..8 {
            let url = url.clone();
            let owner = owner2.clone();
            let barrier = Arc::clone(&barrier);
            joins.push(std::thread::spawn(move || {
                let mut c = PgStore::connect(&url).unwrap();
                barrier.wait();
                c.acquire_capacity(
                    &owner,
                    &format!("lease-{n}"),
                    &format!("capacity-{n}"),
                    "sub@test",
                    60,
                    3,
                    0.95,
                )
                .unwrap()
            }));
        }
        barrier.wait();
        let leases: Vec<_> = joins
            .into_iter()
            .filter_map(|j| j.join().unwrap())
            .collect();
        assert_eq!(leases.len(), 3, "atomic capacity lease must not oversell");
        assert_eq!(pg.pool_inflight("sub@test").unwrap(), Some(3));
        for lease in &leases {
            assert!(pg.release_capacity(&owner2, &lease.lease_id).unwrap());
        }
        for lease in &leases {
            assert!(!pg.release_capacity(&owner2, &lease.lease_id).unwrap());
        }
        assert_eq!(pg.pool_inflight("sub@test").unwrap(), Some(0));

        // One PostgreSQL lease-epoch leader at a time; there is no Redlock path.
        let peer = pg.claim_instance("engine-b", 60).unwrap();
        assert!(pg.acquire_leader(&owner2, "poller", 60).unwrap());
        assert!(!pg.acquire_leader(&peer, "poller", 60).unwrap());

        let totals = pg.billing_totals().unwrap();
        assert_eq!(totals.reserved_nano, 0);
        let aggregate: i64 = pg.client.query_one(
            "SELECT COALESCE(SUM(hold_nano),0)::bigint FROM reservations WHERE state NOT IN ('settled','canceled')",
            &[],
        ).unwrap().get(0);
        assert_eq!(aggregate, 0);

        // Cross-authority conservation: commerce-originated topups/adjustments are the only
        // funding source, while the engine may retain them as balance, completed spend, or an
        // in-flight hold. Pin this per account so opposing errors cannot cancel in a global sum.
        const DIVERGENCE_SQL: &str = "\
            WITH funding AS ( \
              SELECT account_id, COALESCE(SUM(amount_nano),0)::bigint AS funded_nano \
              FROM ledger WHERE kind IN ('topup','adjust') GROUP BY account_id \
            ) \
            SELECT COALESCE(MAX(ABS( \
              a.balance_nano + a.spent_nano + a.reserved_nano \
              - COALESCE(f.funded_nano,0) \
            )),0)::bigint \
            FROM accounts a LEFT JOIN funding f ON f.account_id=a.id";
        let divergence: i64 = pg.client.query_one(DIVERGENCE_SQL, &[]).unwrap().get(0);
        assert_eq!(
            divergence, 0,
            "every account must conserve all durable funding"
        );

        let hold_mismatches: i64 = pg
            .client
            .query_one(
                "WITH holds AS ( \
                   SELECT account_id,COALESCE(SUM(hold_nano),0)::bigint AS held_nano \
                   FROM reservations WHERE state NOT IN ('settled','canceled') GROUP BY account_id \
                 ) \
                 SELECT COUNT(*)::bigint FROM accounts a LEFT JOIN holds h ON h.account_id=a.id \
                 WHERE a.reserved_nano <> COALESCE(h.held_nano,0)",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(
            hold_mismatches, 0,
            "reserved aggregates must equal their source holds"
        );

        // Prove the production gauge's equation is sensitive rather than a zero-valued tautology.
        pg.client.batch_execute("BEGIN").unwrap();
        pg.client
            .execute(
                "UPDATE accounts SET balance_nano=balance_nano+17 WHERE id='acct'",
                &[],
            )
            .unwrap();
        let corrupted: i64 = pg.client.query_one(DIVERGENCE_SQL, &[]).unwrap().get(0);
        assert_eq!(corrupted, 17);
        pg.client.batch_execute("ROLLBACK").unwrap();
        let restored: i64 = pg.client.query_one(DIVERGENCE_SQL, &[]).unwrap().get(0);
        assert_eq!(restored, 0);
        lock_holder
            .client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }
}
