//! PostgreSQL authority for the engine.
//!
//! All correctness-sensitive mutations are transactions. Request IDs and lease IDs are the
//! idempotency boundary; owner epochs fence stale instances. PostgreSQL is the recovery floor.

use crate::{
    mask_proxy, AccountRow, BillingTotals, KeyAuth, KeyPolicyUpdate, KeyRow, LedgerRow, PoolStateRow, Sub,
    SubAdmin, SubRow, UsageEventInput, UsageModelAgg,
};
use anyhow::{bail, Context, Result};
use postgres::{Client, NoTls, Row, Transaction};

const MIGRATION_0001: &str = include_str!("../migrations_pg/0001_engine_authority.sql");
const MIGRATION_0002: &str = include_str!("../migrations_pg/0002_api_key_policies.sql");

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

impl PgStore {
    pub fn connect(url: &str) -> Result<Self> {
        let mut client = Client::connect(url, NoTls).context("connect engine PostgreSQL")?;
        client
            .batch_execute(
                "SET statement_timeout = '15s'; SET lock_timeout = '5s'; \
                 SET idle_in_transaction_session_timeout = '15s'; SET synchronous_commit = on;",
            )
            .context("configure engine PostgreSQL session")?;
        Ok(Self { client })
    }

    pub fn migrate(&mut self) -> Result<()> {
        let mut tx = self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(836214912670::bigint)", &[])?;
        tx.batch_execute(MIGRATION_0001)
            .context("apply engine PostgreSQL migration 0001")?;
        tx.batch_execute(MIGRATION_0002)
            .context("apply engine PostgreSQL migration 0002")?;
        tx.commit()?;
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
        Ok(Owner { instance_id: instance_id.to_owned(), epoch })
    }

    pub fn heartbeat_instance(&mut self, owner: &Owner, ttl_secs: i64) -> Result<bool> {
        let ts = now();
        Ok(self.client.execute(
            "UPDATE engine_instances SET lease_until=$3, updated_ts=$4 \
             WHERE instance_id=$1 AND owner_epoch=$2",
            &[&owner.instance_id, &owner.epoch, &(ts + ttl_secs.max(1)), &ts],
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
        tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", &[&request_id])?;
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
        const OVERDRAFT_NANO: i64 = 1_000_000_000; // $1
        // Гейт `balance-hold >= -OVERDRAFT` пишем как `balance + OVERDRAFT >= hold`: вычитание двух
        // bind-параметров (`$1 - $3`) Postgres не типизирует (`operator is not unique: unknown - unknown`);
        // сложение с колонкой `balance_nano` (bigint) выводит тип $3, а сравнение — тип $1. Эквивалентно.
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1, reserved_nano=reserved_nano+$1 \
             WHERE id=$2 AND status='active' AND balance_nano + $3 >= $1 RETURNING balance_nano",
            &[&hold, &account_id, &OVERDRAFT_NANO],
        )? else {
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

    /// Mark that a successful upstream response is about to be delivered. Recovery charges the hold
    /// for an expired `delivering` reservation rather than making delivered provider usage free.
    pub fn mark_delivering(&mut self, owner: &Owner, request_id: &str, lease_secs: i64) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "UPDATE reservations SET state='delivering', lease_until=$4, updated_ts=$3 \
             WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$5 AND state='reserved'",
            &[&request_id, &owner.instance_id, &ts, &(ts + lease_secs.max(1)), &owner.epoch],
        )?;
        let ok = changed == 1 || tx.query_opt(
            "SELECT 1 FROM reservations WHERE request_id=$1 AND owner_instance=$2 AND owner_epoch=$3 \
             AND state IN ('delivering','settlement_pending','settled')",
            &[&request_id, &owner.instance_id, &owner.epoch],
        )?.is_some();
        tx.commit()?;
        Ok(ok)
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
        tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", &[&request_id])?;
        let reservation = tx.query_opt(
            "SELECT hold_nano, state FROM reservations WHERE request_id=$1 FOR UPDATE",
            &[&request_id],
        )?.context("settlement reservation does not exist")?;
        let hold: i64 = reservation.get(0);
        let state: String = reservation.get(1);
        let actual = actual_nano.max(0).min(hold);
        let u = usage.cloned().unwrap_or_default();
        let inserted = tx.execute(
            "INSERT INTO settlement_outbox(request_id,actual_nano,disposition,reference,model,input_tokens, \
             output_tokens,cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
             real_nano,state,created_ts,updated_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'pending',$13,$13) \
             ON CONFLICT(request_id) DO NOTHING",
            &[&request_id, &actual, &disposition, &reference, &u.model, &u.input_tokens,
              &u.output_tokens, &u.cache_read_tokens, &u.cache_write_5m_tokens,
              &u.cache_write_1h_tokens, &u.web_search_requests, &u.real_nano, &ts],
        )?;
        if inserted == 0 {
            let row = tx.query_one(
                "SELECT actual_nano,disposition,reference,model,input_tokens,output_tokens,cache_read_tokens, \
                 cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano \
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
                && row.get::<_, i64>(10) == u.real_nano;
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
        tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", &[&request_id])?;
        let Some(row) = tx.query_opt(
            "SELECT o.actual_nano,o.disposition,o.reference,o.model,o.input_tokens,o.output_tokens, \
             o.cache_read_tokens,o.cache_write_5m_tokens,o.cache_write_1h_tokens,o.web_search_requests, \
             o.real_nano,o.state,r.account_id,r.key,r.hold_nano,r.state \
             FROM settlement_outbox o JOIN reservations r USING(request_id) \
             WHERE o.request_id=$1 FOR UPDATE OF o,r",
            &[&request_id],
        )? else {
            tx.rollback()?;
            return Ok(None);
        };
        let outbox_state: String = row.get(11);
        let reservation_state: String = row.get(15);
        let account_id: String = row.get(12);
        if outbox_state == "done" || matches!(reservation_state.as_str(), "settled" | "canceled") {
            let balance = tx.query_opt("SELECT balance_nano FROM accounts WHERE id=$1", &[&account_id])?
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
        let account_key: String = row.get(13);
        let hold: i64 = row.get(14);
        if actual > hold {
            bail!("outbox actual exceeds reservation hold");
        }
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
            let key_still_exists = tx.query_opt(
                "SELECT 1 FROM api_keys WHERE key=$1",
                &[&account_key],
            )?.is_some();
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
                tx.execute(
                    "INSERT INTO usage_events(request_id,account_id,key,model,input_tokens,output_tokens, \
                     cache_read_tokens,cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests, \
                     real_nano,charge_nano,ref,ts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
                     ON CONFLICT(request_id) DO NOTHING",
                    &[&request_id,&account_id,&account_key,&model,&input_tokens,&output_tokens,
                      &cache_read_tokens,&cache_write_5m_tokens,&cache_write_1h_tokens,
                      &web_search_requests,&real_nano,&actual,&reference,&ts],
                )?;
            }
        }
        let final_state = if disposition == "cancel" { "canceled" } else { "settled" };
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
            "SELECT request_id FROM settlement_outbox WHERE state <> 'done' AND next_attempt_ts <= $1 \
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
                    let _ = self.client.execute(
                        "UPDATE settlement_outbox SET state='pending',attempts=attempts+1,last_error=$2, \
                         next_attempt_ts=$3,updated_ts=$4 WHERE request_id=$1 AND state <> 'done'",
                        &[&id, &message, &(ts + 1), &ts],
                    );
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
                    self.enqueue_outbox(&request_id, hold, "reconcile_full_hold", Some("expired-delivery"), None)?;
                    report.charged_after_delivery += 1;
                }
                "settlement_pending" => {}
                _ => continue,
            }
        }
        report.processed_outbox = self.drain_outbox(limit)?;
        Ok(report)
    }

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
            if !exact { bail!("capacity lease ID belongs to another operation"); }
            let lease = CapacityLease {
                lease_id: lease_id.to_owned(), request_id: request_id.to_owned(),
                subscription_email: email.to_owned(), lease_until: row.get(2),
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
        let effective5 = if reset5 > 0 && reset5 <= ts { 0.0 } else { util5 };
        let effective7 = if reset7 > 0 && reset7 <= ts { 0.0 } else { util7 };
        if cooling_until > ts || effective5 >= util_cap || effective7 >= util_cap
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
        tx.execute("UPDATE pool_state SET inflight=inflight+1 WHERE email=$1", &[&email])?;
        tx.commit()?;
        Ok(Some(CapacityLease {
            lease_id: lease_id.to_owned(), request_id: request_id.to_owned(),
            subscription_email: email.to_owned(), lease_until,
        }))
    }

    pub fn release_capacity(&mut self, owner: &Owner, lease_id: &str) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let row = tx.query_opt(
            "UPDATE capacity_leases SET state='released',released_ts=$4 \
             WHERE lease_id=$1 AND owner_instance=$2 AND owner_epoch=$3 AND state='active' \
             RETURNING subscription_email",
            &[&lease_id,&owner.instance_id,&owner.epoch,&ts],
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
            &[&name,&owner.instance_id,&owner.epoch,&(ts + ttl_secs.max(1)),&ts],
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
            if token.is_empty() { continue; }
            out.push(Sub {
                email, token, proxy: row.get(3), fleet: row.get(5), plan: row.get(6),
            });
        }
        Ok(out)
    }

    pub fn add(&mut self, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO subs(email,token,proxy,status,fleet,added_ts,added) \
             VALUES($1,$2,$3,'active',$4,$5,$6) ON CONFLICT(email) DO UPDATE SET \
             token=EXCLUDED.token,proxy=EXCLUDED.proxy,status='active',fleet=EXCLUDED.fleet",
            &[&email,&token,&proxy,&fleet,&ts,&chrono_like(ts)],
        )?;
        tx.execute("INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING", &[&email])?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_file(&mut self, email: &str, token_file: &str, proxy: &str, fleet: &str) -> Result<()> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.execute(
            "INSERT INTO subs(email,token_file,proxy,status,fleet,added_ts,added) \
             VALUES($1,$2,$3,'active',$4,$5,$6) ON CONFLICT(email) DO UPDATE SET \
             token_file=EXCLUDED.token_file,proxy=EXCLUDED.proxy,status='active',fleet=EXCLUDED.fleet",
            &[&email,&token_file,&proxy,&fleet,&ts,&chrono_like(ts)],
        )?;
        tx.execute("INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING", &[&email])?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_sub_status(&mut self, email: &str, status: &str) -> Result<usize> {
        Ok(self.client.execute("UPDATE subs SET status=$1 WHERE email=$2", &[&status,&email])? as usize)
    }
    pub fn set_plan(&mut self, email: &str, plan: &str) -> Result<usize> {
        Ok(self.client.execute("UPDATE subs SET plan=$1 WHERE email=$2", &[&plan,&email])? as usize)
    }
    pub fn set_proxy(&mut self, email: &str, proxy: &str) -> Result<usize> {
        Ok(self.client.execute("UPDATE subs SET proxy=$1 WHERE email=$2", &[&proxy,&email])? as usize)
    }
    pub fn set_fleet(&mut self, email: &str, fleet: &str) -> Result<usize> {
        Ok(self.client.execute("UPDATE subs SET fleet=$1 WHERE email=$2", &[&fleet,&email])? as usize)
    }
    pub fn set_proxy_meta(&mut self, email: &str, expire: &str, checked_ts: i64, ok: bool) -> Result<usize> {
        Ok(self.client.execute(
            "UPDATE subs SET proxy_expire=$1,proxy_checked_ts=$2,proxy_ok=$3 WHERE email=$4",
            &[&expire,&checked_ts,&ok,&email],
        )? as usize)
    }
    pub fn get_creds(&mut self, email: &str) -> Result<Option<(String, String)>> {
        let Some(row) = self.client.query_opt("SELECT token,token_file,proxy FROM subs WHERE email=$1", &[&email])? else {
            return Ok(None);
        };
        let token = crate::resolve_token(row.get(0), row.get(1));
        if token.is_empty() { Ok(None) } else { Ok(Some((token, row.get(2)))) }
    }
    pub fn remove_sub(&mut self, email: &str) -> Result<usize> {
        Ok(self.client.execute("DELETE FROM subs WHERE email=$1", &[&email])? as usize)
    }
    pub fn clear_subs(&mut self, fleet: Option<&str>) -> Result<usize> {
        Ok(match fleet {
            Some(f) => self.client.execute("DELETE FROM subs WHERE fleet=$1", &[&f])?,
            None => self.client.execute("DELETE FROM subs", &[])?,
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
             proxy_expire,proxy_ok,added_ts,added FROM subs ORDER BY added_ts",
            &[],
        )?.into_iter().map(|row| {
            let proxy: String = row.get(4);
            SubAdmin {
                email: row.get(0), status: row.get(1), fleet: row.get(2),
                has_token: row.get::<_, Option<String>>(3).is_some_and(|s| !s.is_empty()),
                proxy_host: mask_proxy(&proxy), proxy_expire: row.get(5), proxy_ok: row.get(6),
                added_ts: row.get(7), added: row.get(8),
            }
        }).collect())
    }

    // -- Accounts, keys, ledger, analytics ---------------------------------------------------

    pub fn account_create(&mut self, id: &str, handle: Option<&str>, mult_bp: i64) -> Result<()> {
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
        Ok(self.client.execute("UPDATE accounts SET status=$1 WHERE id=$2", &[&status,&id])? as usize)
    }
    pub fn account_set_mult_bp(&mut self, id: &str, mult_bp: i64) -> Result<usize> {
        Ok(self.client.execute("UPDATE accounts SET mult_bp=$1 WHERE id=$2", &[&mult_bp,&id])? as usize)
    }
    pub fn account_remove(&mut self, id: &str) -> Result<usize> {
        Ok(self.client.execute("DELETE FROM accounts WHERE id=$1", &[&id])? as usize)
    }

    pub fn account_topup(&mut self, id: &str, amount_nano: i64, reference: Option<&str>) -> Result<Option<i64>> {
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
                if !exact { bail!("idempotency reference belongs to another monetary operation"); }
                let original = row.get(3);
                tx.commit()?;
                return Ok(original);
            }
        }
        let Some(row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano+$1 WHERE id=$2 RETURNING balance_nano",
            &[&amount_nano,&id],
        )? else {
            tx.rollback()?;
            return Ok(None);
        };
        let balance: i64 = row.get(0);
        tx.execute(
            "INSERT INTO ledger(account_id,kind,amount_nano,ref,balance_after_nano,ts) \
             VALUES($1,$2,$3,$4,$5,$6)",
            &[&id,&kind,&amount_nano,&reference,&balance,&ts],
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
        let ts = now();
        self.client.execute(
            "INSERT INTO api_keys(key,key_id,account_id,label,spend_limit_nano,expires_ts,status,created_ts,created) \
             VALUES($1,'key_' || md5(random()::text || clock_timestamp()::text),$2,$3,$4,$5,'active',$6,$7) \
             ON CONFLICT(key) DO UPDATE SET account_id=EXCLUDED.account_id,label=EXCLUDED.label, \
             spend_limit_nano=EXCLUDED.spend_limit_nano,expires_ts=EXCLUDED.expires_ts,status='active'",
            &[&key,&account_id,&label,&spend_limit_nano,&expires_ts,&ts,&chrono_like(ts)],
        )?;
        Ok(())
    }
    pub fn key_account(&mut self, key: &str) -> Result<Option<KeyAuth>> {
        Ok(self.client.query_opt(
            "SELECT a.id,a.mult_bp,a.balance_nano,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,(k.status='active' AND a.status='active') \
             FROM api_keys k JOIN accounts a ON a.id=k.account_id WHERE k.key=$1",
            &[&key],
        )?.map(|row| KeyAuth {
            account_id: row.get(0), mult_bp: row.get(1), balance_nano: row.get(2),
            spent_nano: row.get(3), reserved_nano: row.get(4), spend_limit_nano: row.get(5),
            expires_ts: row.get(6), active: row.get(7),
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
        Ok(self.client.query(
            "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
             k.spend_limit_nano,k.expires_ts,k.created_ts,u.last_used_ts,k.status \
             FROM api_keys k LEFT JOIN ( \
               SELECT key,MAX(ts) AS last_used_ts FROM usage_events GROUP BY key \
             ) u ON u.key=k.key ORDER BY k.created_ts",
            &[],
        )?.into_iter().map(|r| key_row(&r)).collect())
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
        Ok(self.client.execute("UPDATE api_keys SET status=$1 WHERE key=$2", &[&status,&key])? as usize)
    }
    pub fn key_set_status_by_id(&mut self, key_id: &str, status: &str) -> Result<usize> {
        Ok(self.client.execute("UPDATE api_keys SET status=$1 WHERE key_id=$2", &[&status,&key_id])? as usize)
    }
    pub fn key_set_label_by_id(&mut self, key_id: &str, label: &str) -> Result<usize> {
        Ok(self.client.execute("UPDATE api_keys SET label=$1 WHERE key_id=$2", &[&label,&key_id])? as usize)
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
            &[&key_id,&account_id,&spend_limit_nano,&expires_ts],
        )?;
        if updated == 1 { return Ok(KeyPolicyUpdate::Updated); }
        let exists: bool = self.client.query_one(
            "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key_id=$1 AND account_id=$2)",
            &[&key_id,&account_id],
        )?.get(0);
        if !exists { return Ok(KeyPolicyUpdate::NotFound); }
        if expires_ts.is_some_and(|expires| expires <= now()) {
            return Ok(KeyPolicyUpdate::ExpiryNotFuture);
        }
        Ok(KeyPolicyUpdate::LimitBelowUsage)
    }
    pub fn key_remove(&mut self, key: &str) -> Result<usize> {
        Ok(self.client.execute("DELETE FROM api_keys WHERE key=$1", &[&key])? as usize)
    }
    pub fn key_clear(&mut self) -> Result<usize> {
        Ok(self.client.execute("DELETE FROM api_keys", &[])? as usize)
    }

    pub fn ledger_recent(&mut self, account_id: &str, limit: i64) -> Result<Vec<LedgerRow>> {
        Ok(self.client.query(
            "SELECT id,key,kind,amount_nano,ref,balance_after_nano,ts,model FROM ledger \
             WHERE account_id=$1 ORDER BY id DESC LIMIT $2",
            &[&account_id,&limit.clamp(1,1000)],
        )?.into_iter().map(|r| ledger_row(&r)).collect())
    }
    pub fn ledger_after(&mut self, account_id: &str, after_id: i64, limit: i64) -> Result<Vec<LedgerRow>> {
        Ok(self.client.query(
            "SELECT id,key,kind,amount_nano,ref,balance_after_nano,ts,model FROM ledger \
             WHERE account_id=$1 AND id>$2 ORDER BY id LIMIT $3",
            &[&account_id,&after_id.max(0),&limit.clamp(1,1000)],
        )?.into_iter().map(|r| ledger_row(&r)).collect())
    }
    pub fn usage_by_model(&mut self, account_id: &str, since_ts: i64) -> Result<Vec<UsageModelAgg>> {
        Ok(self.client.query(
            "SELECT COALESCE(model,''),COUNT(*)::bigint,COALESCE(SUM(input_tokens),0)::bigint, \
             COALESCE(SUM(output_tokens),0)::bigint,COALESCE(SUM(cache_read_tokens),0)::bigint, \
             COALESCE(SUM(cache_write_5m_tokens),0)::bigint,COALESCE(SUM(cache_write_1h_tokens),0)::bigint, \
             COALESCE(SUM(web_search_requests),0)::bigint,COALESCE(SUM(real_nano),0)::bigint, \
             COALESCE(SUM(charge_nano),0)::bigint FROM usage_events WHERE account_id=$1 AND ts >= $2 \
             GROUP BY model ORDER BY SUM(real_nano) DESC",
            &[&account_id,&since_ts],
        )?.into_iter().map(|r| UsageModelAgg {
            model:r.get(0),requests:r.get(1),input_tokens:r.get(2),output_tokens:r.get(3),
            cache_read_tokens:r.get(4),cache_write_5m_tokens:r.get(5),cache_write_1h_tokens:r.get(6),
            web_search_requests:r.get(7),real_nano:r.get(8),charge_nano:r.get(9),
        }).collect())
    }
    pub fn usage_prune(&mut self, older_than_ts: i64) -> Result<usize> {
        Ok(self.client.execute("DELETE FROM usage_events WHERE ts < $1", &[&older_than_ts])? as usize)
    }
    pub fn billing_totals(&mut self) -> Result<BillingTotals> {
        let row = self.client.query_one(
            "SELECT COALESCE(SUM(balance_nano),0)::bigint,COALESCE(SUM(spent_nano),0)::bigint, \
             COALESCE(SUM(reserved_nano),0)::bigint,COUNT(*) FILTER (WHERE status='active')::bigint FROM accounts",
            &[],
        )?;
        Ok(BillingTotals { balance_nano:row.get(0),spent_nano:row.get(1),reserved_nano:row.get(2),active_keys:row.get(3) })
    }

    // -- Fenced pool-state persistence --------------------------------------------------------

    pub fn load_pool_state(&mut self) -> Result<Vec<PoolStateRow>> {
        Ok(self.client.query(
            "SELECT email,cooling_until,cap5h,cap7d,spent_total,util5,util7,reset5,reset7,calib_n,version \
             FROM pool_state",
            &[],
        )?.into_iter().map(|r| PoolStateRow {
            email:r.get(0),cooling_until:r.get(1),cap5h_usd:r.get(2),cap7d_usd:r.get(3),
            spent_total_usd:r.get(4),util5h:r.get(5),util7d:r.get(6),reset5h:r.get(7),
            reset7d:r.get(8),calib_n:r.get(9),version:r.get(10),
        }).collect())
    }

    pub fn save_pool_state(&mut self, owner: &Owner, rows: &[PoolStateRow]) -> Result<Vec<(String, i64)>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;
        let mut versions = Vec::with_capacity(rows.len());
        for row in rows {
            tx.execute("INSERT INTO pool_state(email) VALUES($1) ON CONFLICT DO NOTHING", &[&row.email])?;
            let updated = tx.query_opt(
                "UPDATE pool_state SET cooling_until=$2,cap5h=$3,cap7d=$4,spent_total=$5,util5=$6,util7=$7, \
                 reset5=$8,reset7=$9,calib_n=$10,version=version+1,writer_instance=$11,writer_epoch=$12,updated_ts=$13 \
                 WHERE email=$1 AND version=$14 AND (writer_epoch IS NULL OR writer_epoch <= $12) RETURNING version",
                &[&row.email,&row.cooling_until,&row.cap5h_usd,&row.cap7d_usd,&row.spent_total_usd,
                  &row.util5h,&row.util7d,&row.reset5h,&row.reset7d,&row.calib_n,
                  &owner.instance_id,&owner.epoch,&ts,&row.version],
            )?;
            let Some(updated) = updated else {
                bail!("pool-state CAS conflict for {} at version {}", row.email, row.version);
            };
            versions.push((row.email.clone(), updated.get(0)));
        }
        tx.commit()?;
        Ok(versions)
    }

    pub fn pool_inflight(&mut self, email: &str) -> Result<Option<i64>> {
        Ok(self.client.query_opt("SELECT inflight FROM pool_state WHERE email=$1", &[&email])?.map(|r| r.get(0)))
    }

    /// One-time, repeatable copy from a fully drained SQLite authority. Anonymous aggregate holds
    /// cannot be safely attributed, so a non-zero `reserved_nano` aborts the migration.
    pub fn import_sqlite(&mut self, sqlite_path: &str) -> Result<ImportReport> {
        let sqlite = crate::open(sqlite_path)?;
        let source_totals = crate::billing_totals(&sqlite);
        if source_totals.reserved_nano != 0 {
            bail!(
                "SQLite contains {} anonymous reserved nanodollars; drain/reconcile before migration",
                source_totals.reserved_nano
            );
        }

        let mut tx = self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(836214912671::bigint)", &[])?;
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
            let rows = stmt.query_map([], |r| Ok((
                r.get::<_, String>(0)?,r.get::<_, Option<String>>(1)?,r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,r.get::<_, String>(4)?,r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,r.get::<_, i64>(7)?,r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,r.get::<_, Option<i64>>(10)?,r.get::<_, Option<i64>>(11)?,
            )))?;
            for row in rows {
                let (email,token,token_file,proxy,plan,status,fleet,added_ts,added,expire,checked,ok) = row?;
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
            let rows = stmt.query_map([], |r| Ok((
                r.get::<_, String>(0)?,r.get::<_, Option<String>>(1)?,r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,r.get::<_, i64>(4)?,r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,r.get::<_, i64>(7)?,r.get::<_, String>(8)?,
            )))?;
            for row in rows {
                let (id,handle,balance,spent,reserved,mult,status,created_ts,created) = row?;
                tx.execute(
                    "INSERT INTO accounts(id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                    &[&id,&handle,&balance,&spent,&reserved,&mult,&status,&created_ts,&created],
                )?;
                report.accounts += 1;
                report.balance_nano = report.balance_nano.checked_add(balance).context("balance sum overflow")?;
                report.spent_nano = report.spent_nano.checked_add(spent).context("spent sum overflow")?;
                report.reserved_nano = report.reserved_nano.checked_add(reserved).context("reserved sum overflow")?;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT key,key_id,account_id,label,spent_nano,reserved_nano,spend_limit_nano,expires_ts, \
                 COALESCE(status,'active'),COALESCE(created_ts,0),COALESCE(created,'') \
                 FROM api_keys ORDER BY key",
            )?;
            let rows = stmt.query_map([], |r| Ok((
                r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,r.get::<_, i64>(4)?,r.get::<_, i64>(5)?,
                r.get::<_, Option<i64>>(6)?,r.get::<_, Option<i64>>(7)?,r.get::<_, String>(8)?,
                r.get::<_, i64>(9)?,r.get::<_, String>(10)?,
            )))?;
            for row in rows {
                let (key,key_id,account_id,label,spent,reserved,spend_limit,expires,status,created_ts,created) = row?;
                let account_id = account_id.context("legacy key has no account_id after SQLite migration")?;
                tx.execute(
                    "INSERT INTO api_keys(key,key_id,account_id,label,spent_nano,reserved_nano, \
                     spend_limit_nano,expires_ts,status,created_ts,created) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                    &[&key,&key_id,&account_id,&label,&spent,&reserved,&spend_limit,&expires,
                      &status,&created_ts,&created],
                )?;
                report.keys += 1;
            }
        }
        {
            let mut stmt = sqlite.prepare(
                "SELECT id,account_id,key,kind,amount_nano,ref,balance_after_nano,COALESCE(ts,0),model \
                 FROM ledger ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| Ok((
                r.get::<_, i64>(0)?,r.get::<_, String>(1)?,r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,r.get::<_, i64>(4)?,r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,r.get::<_, i64>(7)?,r.get::<_, Option<String>>(8)?,
            )))?;
            for row in rows {
                let (id,account_id,key,kind,amount,reference,balance,ts,model) = row?;
                tx.execute(
                    "INSERT INTO ledger(id,account_id,key,kind,amount_nano,ref,balance_after_nano,ts,model) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                    &[&id,&account_id,&key,&kind,&amount,&reference,&balance,&ts,&model],
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
                "SELECT id,account_id,key,model,input_tokens,output_tokens,cache_read_tokens, \
                 cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano,charge_nano,ref,ts \
                 FROM usage_events ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| Ok((
                r.get::<_, i64>(0)?,r.get::<_, String>(1)?,r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,r.get::<_, i64>(4)?,r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,r.get::<_, i64>(7)?,r.get::<_, i64>(8)?,r.get::<_, i64>(9)?,
                r.get::<_, i64>(10)?,r.get::<_, i64>(11)?,r.get::<_, Option<String>>(12)?,r.get::<_, i64>(13)?,
            )))?;
            for row in rows {
                let (id,account_id,key,model,input,output,cache_read,cache5,cache1,web,real,charge,reference,ts) = row?;
                tx.execute(
                    "INSERT INTO usage_events(id,account_id,key,model,input_tokens,output_tokens,cache_read_tokens, \
                     cache_write_5m_tokens,cache_write_1h_tokens,web_search_requests,real_nano,charge_nano,ref,ts) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
                    &[&id,&account_id,&key,&model,&input,&output,&cache_read,&cache5,&cache1,&web,&real,&charge,&reference,&ts],
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
            let rows = stmt.query_map([], |r| Ok((
                r.get::<_, String>(0)?,r.get::<_, i64>(1)?,r.get::<_, f64>(2)?,r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,r.get::<_, f64>(5)?,r.get::<_, f64>(6)?,r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,r.get::<_, i64>(9)?,r.get::<_, i64>(10)?,
            )))?;
            for row in rows {
                let (email,cooling,cap5,cap7,spent,util5,util7,reset5,reset7,calib,updated) = row?;
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
        if target_accounts as usize != report.accounts || target_balance != report.balance_nano
            || target_spent != report.spent_nano || target_reserved != report.reserved_nano
            || report.balance_nano != source_totals.balance_nano
            || report.spent_nano != source_totals.spent_nano
        {
            bail!("SQLite/PostgreSQL monetary reconciliation mismatch; import rolled back");
        }
        tx.commit()?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    /// Run with an isolated database, for example:
    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry pg::tests::stage2_fault_matrix`
    #[test]
    fn stage2_fault_matrix() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!("skipping PostgreSQL fault matrix: CLAUDE_API_TEST_DATABASE_URL is unset");
            return;
        };
        let mut pg = PgStore::connect(&url).unwrap();
        pg.migrate().unwrap();
        pg.client.batch_execute(
            "TRUNCATE settlement_outbox,reservations,capacity_leases,leader_leases,engine_instances, \
             usage_events,ledger,api_keys,accounts,pool_state,subs RESTART IDENTITY CASCADE",
        ).unwrap();

        // Exercise the real one-time SQLite importer before the transactional fault matrix.
        let sqlite_path = std::env::temp_dir().join(format!(
            "claude-stage2-import-{}-{}.db", std::process::id(), now()
        ));
        let sqlite_path_s = sqlite_path.to_string_lossy().into_owned();
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            crate::add(&sqlite,"import-sub","import-token","","prod").unwrap();
            crate::account_create(&sqlite,"import-acct",Some("import-handle"),2000).unwrap();
            crate::key_issue(&sqlite,"import-key","import-acct",Some("imported")).unwrap();
            crate::account_topup(&sqlite,"import-acct",5_000,Some("import-seed")).unwrap();
            crate::account_reserve(&sqlite,"import-acct",1_000).unwrap();
            crate::account_settle(&sqlite,"import-acct","import-key",1_000,200,Some("import-charge"),None).unwrap();
            crate::save_pool_state(&sqlite,&[PoolStateRow {
                email:"import-sub".into(),cooling_until:123,version:0,..Default::default()
            }]).unwrap();
        }
        let imported = pg.import_sqlite(&sqlite_path_s).unwrap();
        assert_eq!((imported.subscriptions,imported.accounts,imported.keys),(1,1,1));
        assert_eq!((imported.balance_nano,imported.spent_nano,imported.reserved_nano),(4_800,200,0));
        {
            let sqlite = crate::open(&sqlite_path_s).unwrap();
            crate::account_reserve(&sqlite,"import-acct",100).unwrap();
        }
        assert!(pg.import_sqlite(&sqlite_path_s).is_err(),"anonymous SQLite hold must block cutover");
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
        assert_eq!(pg.account_topup("acct", 1_000, Some("seed")).unwrap(), Some(1_000));
        assert_eq!(pg.account_topup("acct", 1_000, Some("seed")).unwrap(), Some(1_000));
        assert!(pg.account_topup("acct", 999, Some("seed")).is_err());

        let owner = pg.claim_instance("engine-a", 60).unwrap();
        pg.account_create("policy-acct", None, 10_000).unwrap();
        pg.account_topup("policy-acct", 1_000, Some("policy-seed")).unwrap();
        pg.key_issue_with_policy("limited-key", "policy-acct", Some("limited"), Some(700), Some(now() + 60)).unwrap();
        assert_eq!(pg.reserve_request(&owner,"policy-1","policy-acct","limited-key",500,60).unwrap(),Some(500));
        assert_eq!(pg.reserve_request(&owner,"policy-1","policy-acct","limited-key",500,60).unwrap(),Some(500));
        assert_eq!(pg.key_get("limited-key").unwrap().unwrap().reserved_nano,500);
        let limited_key_id = pg.key_get("limited-key").unwrap().unwrap().key_id;
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct",&limited_key_id,Some(499),None).unwrap(),
            KeyPolicyUpdate::LimitBelowUsage,
        );
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct",&limited_key_id,Some(700),Some(now() + 120)).unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert_eq!(pg.reserve_request(&owner,"policy-2","policy-acct","limited-key",300,60).unwrap(),None);
        assert_eq!(pg.cancel_request("policy-1").unwrap(),Some(1_000));
        assert_eq!(pg.cancel_request("policy-1").unwrap(),Some(1_000));
        assert_eq!(pg.reserve_request(&owner,"policy-3","policy-acct","limited-key",700,60).unwrap(),Some(300));
        assert_eq!(pg.settle_request("policy-3",650,None,None).unwrap(),Some(350));
        let limited = pg.key_get("limited-key").unwrap().unwrap();
        assert_eq!((limited.spent_nano,limited.reserved_nano,limited.spend_limit_nano),(650,0,Some(700)));
        assert_eq!(pg.reserve_request(&owner,"policy-boundary","policy-acct","limited-key",50,60).unwrap(),Some(300));
        assert_eq!(pg.settle_request("policy-boundary",50,None,None).unwrap(),Some(300));
        assert_eq!(pg.reserve_request(&owner,"policy-over","policy-acct","limited-key",1,60).unwrap(),None);
        pg.key_issue_with_policy("expired-key", "policy-acct", None, None, Some(now())).unwrap();
        assert_eq!(pg.reserve_request(&owner,"policy-expired","policy-acct","expired-key",1,60).unwrap(),None);
        let expired_key_id = pg.key_get("expired-key").unwrap().unwrap().key_id;
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct",&expired_key_id,None,Some(now() + 60)).unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert!(pg.reserve_request(&owner,"policy-extended","policy-acct","expired-key",1,60).unwrap().is_some());
        pg.cancel_request("policy-extended").unwrap();
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct",&expired_key_id,None,None).unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert_eq!(
            pg.key_set_policy_by_id("policy-acct","key_missing",None,None).unwrap(),
            KeyPolicyUpdate::NotFound,
        );
        pg.key_issue_with_policy("disabled-key", "policy-acct", None, None, None).unwrap();
        pg.key_set_status("disabled-key","disabled").unwrap();
        assert_eq!(pg.reserve_request(&owner,"policy-disabled","policy-acct","disabled-key",1,60).unwrap(),None);

        pg.account_create("concurrent-policy-acct", None, 10_000).unwrap();
        pg.account_topup("concurrent-policy-acct", 1_000, Some("concurrent-policy-seed")).unwrap();
        pg.key_issue_with_policy("concurrent-limited-key", "concurrent-policy-acct", None, Some(700), None).unwrap();
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
                let result = connection.reserve_request(
                    &owner,&request_id,"concurrent-policy-acct","concurrent-limited-key",400,60,
                ).unwrap();
                (request_id,result)
            }));
        }
        policy_barrier.wait();
        let policy_results: Vec<_> = policy_joins.into_iter().map(|join| join.join().unwrap()).collect();
        assert_eq!(policy_results.iter().filter(|(_, result)| result.is_some()).count(),1,
                   "concurrent reservations must not jointly cross a key cap");
        for (request_id,result) in policy_results {
            if result.is_some() { pg.cancel_request(&request_id).unwrap(); }
        }
        assert_eq!(pg.key_get("concurrent-limited-key").unwrap().unwrap().reserved_nano,0);

        // A reserve racing a stricter policy replacement must serialize on the key row. The two
        // incompatible operations can never both succeed.
        pg.account_create("policy-update-race-acct", None, 10_000).unwrap();
        pg.account_topup("policy-update-race-acct", 1_000, Some("policy-update-race-seed")).unwrap();
        pg.key_issue_with_policy(
            "policy-update-race-key","policy-update-race-acct",None,Some(1_000),None,
        ).unwrap();
        let race_key_id = pg.key_get("policy-update-race-key").unwrap().unwrap().key_id;
        let race_barrier = Arc::new(Barrier::new(3));
        let reserve_url = url.clone();
        let reserve_owner = owner.clone();
        let reserve_barrier = Arc::clone(&race_barrier);
        let reserve_join = std::thread::spawn(move || {
            let mut connection = PgStore::connect(&reserve_url).unwrap();
            reserve_barrier.wait();
            connection.reserve_request(
                &reserve_owner,"policy-update-race-request","policy-update-race-acct",
                "policy-update-race-key",400,60,
            ).unwrap().is_some()
        });
        let update_url = url.clone();
        let update_barrier = Arc::clone(&race_barrier);
        let update_join = std::thread::spawn(move || {
            let mut connection = PgStore::connect(&update_url).unwrap();
            update_barrier.wait();
            connection.key_set_policy_by_id(
                "policy-update-race-acct",&race_key_id,Some(300),None,
            ).unwrap() == KeyPolicyUpdate::Updated
        });
        race_barrier.wait();
        let reserve_won = reserve_join.join().unwrap();
        let update_won = update_join.join().unwrap();
        assert_ne!(reserve_won,update_won,"exactly one incompatible racing operation must succeed");
        let raced_key = pg.key_get("policy-update-race-key").unwrap().unwrap();
        if let Some(limit) = raced_key.spend_limit_nano {
            assert!(raced_key.spent_nano + raced_key.reserved_nano <= limit);
        }
        assert_eq!(
            pg.account_get("policy-update-race-acct").unwrap().unwrap().reserved_nano,
            raced_key.reserved_nano,
        );
        if reserve_won { pg.cancel_request("policy-update-race-request").unwrap(); }

        assert_eq!(pg.reserve_request(&owner,"req-1","acct","key",600,60).unwrap(),Some(400));
        assert_eq!(pg.reserve_request(&owner,"req-1","acct","key",600,60).unwrap(),Some(400));
        assert!(pg.mark_delivering(&owner,"req-1",60).unwrap());
        let usage = UsageEventInput { model:"claude-test".into(),input_tokens:10,output_tokens:20,real_nano:200,..Default::default() };
        assert_eq!(pg.settle_request("req-1",250,Some("anthropic-1"),Some(&usage)).unwrap(),Some(750));
        assert_eq!(pg.settle_request("req-1",250,Some("anthropic-1"),Some(&usage)).unwrap(),Some(750));
        assert!(pg.settle_request("req-1",251,Some("anthropic-1"),Some(&usage)).is_err());
        let charge_count: i64 = pg.client.query_one(
            "SELECT COUNT(*)::bigint FROM ledger WHERE kind='charge' AND request_id='req-1'", &[],
        ).unwrap().get(0);
        assert_eq!(charge_count, 1, "exact retry must not double-charge");

        assert_eq!(pg.reserve_request(&owner,"req-2","acct","key",300,60).unwrap(),Some(450));
        assert_eq!(pg.cancel_request("req-2").unwrap(),Some(750));
        assert_eq!(pg.cancel_request("req-2").unwrap(),Some(750));

        // Crash boundary: enqueue commits but settlement application has not run. A fresh connection
        // drains the durable row exactly once.
        assert_eq!(pg.reserve_request(&owner,"req-3","acct","key",400,60).unwrap(),Some(350));
        assert!(pg.mark_delivering(&owner,"req-3",60).unwrap());
        pg.enqueue_settlement("req-3",100,Some("anthropic-3"),None).unwrap();
        drop(pg);
        let mut pg = PgStore::connect(&url).unwrap();
        assert_eq!(pg.drain_outbox(100).unwrap(),1);
        assert_eq!(pg.drain_outbox(100).unwrap(),0);
        let account = pg.account_get("acct").unwrap().unwrap();
        assert_eq!((account.balance_nano,account.spent_nano,account.reserved_nano),(650,350,0));

        // Овердрафт-буфер ($1): funded-запрос НЕ роняем из-за гонки — баланс может уйти в лёгкий минус
        // до пола −$1 (−1e9 nano), но НИКОГДА ниже; за полом любой положительный hold отбит. (`owner`
        // ещё валиден — фенсинг ниже.)
        pg.account_create("od-acct", None, 10_000).unwrap();
        pg.key_issue("od-key", "od-acct", None).unwrap();
        pg.account_topup("od-acct", 1_000, Some("od-seed")).unwrap();          // balance = 1000 nano
        // hold ≫ баланса, но в пределах balance+$1 → овердрафт пускает; баланс → −$0.999999
        assert_eq!(pg.reserve_request(&owner,"od-1","od-acct","od-key",1_000_000_000,60).unwrap(),Some(-999_999_000));
        // добираем РОВНО до пола −$1 (граница включительно)
        assert_eq!(pg.reserve_request(&owner,"od-2","od-acct","od-key",1_000,60).unwrap(),Some(-1_000_000_000));
        // на полу −$1 любой положительный hold отбит (защита от бесконечного долга)
        assert_eq!(pg.reserve_request(&owner,"od-3","od-acct","od-key",1,60).unwrap(),None);
        // на свежем аккаунте hold СВЕРХ balance+$1 → отказ (за буфером), обычный в пределах — ок
        pg.account_create("od-acct2", None, 10_000).unwrap();
        pg.key_issue("od-key2", "od-acct2", None).unwrap();
        pg.account_topup("od-acct2", 1_000, Some("od-seed2")).unwrap();        // balance = 1000 nano
        assert_eq!(pg.reserve_request(&owner,"od-4","od-acct2","od-key2",1_000_002_000,60).unwrap(),None);
        assert_eq!(pg.reserve_request(&owner,"od-5","od-acct2","od-key2",1_000,60).unwrap(),Some(0));
        // Снимаем наши holds → reserved_nano аккаунтов обратно в 0 (глобальный billing_totals ниже ждёт 0).
        pg.cancel_request("od-1").unwrap();
        pg.cancel_request("od-2").unwrap();
        pg.cancel_request("od-5").unwrap();

        // A later epoch with the same instance identity fences the stale writer.
        let owner2 = pg.claim_instance("engine-a",60).unwrap();
        assert!(owner2.epoch > owner.epoch);
        assert!(pg.reserve_request(&owner,"stale","acct","key",1,60).is_err());
        assert_eq!(pg.reserve_request(&owner2,"req-4","acct","key",100,60).unwrap(),Some(550));
        pg.cancel_request("req-4").unwrap();

        // Recovery distinguishes a request never delivered (refund) from a delivered response whose
        // exact usage was lost (conservatively charge the already approved hold).
        let dead = pg.claim_instance("dead-engine",60).unwrap();
        pg.reserve_request(&dead,"req-5","acct","key",100,1).unwrap();
        pg.reserve_request(&dead,"req-6","acct","key",100,1).unwrap();
        pg.mark_delivering(&dead,"req-6",1).unwrap();
        pg.client.execute("UPDATE engine_instances SET lease_until=0 WHERE instance_id='dead-engine'",&[]).unwrap();
        pg.client.execute("UPDATE reservations SET lease_until=0 WHERE request_id IN ('req-5','req-6')",&[]).unwrap();
        let recovered = pg.reconcile_expired(100).unwrap();
        assert_eq!(recovered.canceled_before_delivery,1);
        assert_eq!(recovered.charged_after_delivery,1);
        assert_eq!(pg.account_get("acct").unwrap().unwrap().reserved_nano,0);

        // Pool state is versioned CAS and fenced by owner epoch.
        let mut state = pg.load_pool_state().unwrap();
        assert_eq!(state.len(),1);
        let stale_state = state.clone();
        let versions = pg.save_pool_state(&owner2,&state).unwrap();
        assert_eq!(versions[0].1,1);
        assert!(pg.save_pool_state(&owner2,&stale_state).is_err());
        state[0].version = versions[0].1;
        assert!(pg.save_pool_state(&owner2,&state).is_ok());

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
                c.acquire_capacity(&owner,&format!("lease-{n}"),&format!("capacity-{n}"),"sub@test",60,3,0.95).unwrap()
            }));
        }
        barrier.wait();
        let leases: Vec<_> = joins.into_iter().filter_map(|j| j.join().unwrap()).collect();
        assert_eq!(leases.len(),3,"atomic capacity lease must not oversell");
        assert_eq!(pg.pool_inflight("sub@test").unwrap(),Some(3));
        for lease in &leases { assert!(pg.release_capacity(&owner2,&lease.lease_id).unwrap()); }
        for lease in &leases { assert!(!pg.release_capacity(&owner2,&lease.lease_id).unwrap()); }
        assert_eq!(pg.pool_inflight("sub@test").unwrap(),Some(0));

        // One PostgreSQL lease-epoch leader at a time; there is no Redlock path.
        let peer = pg.claim_instance("engine-b",60).unwrap();
        assert!(pg.acquire_leader(&owner2,"poller",60).unwrap());
        assert!(!pg.acquire_leader(&peer,"poller",60).unwrap());

        let totals = pg.billing_totals().unwrap();
        assert_eq!(totals.reserved_nano,0);
        let aggregate: i64 = pg.client.query_one(
            "SELECT COALESCE(SUM(hold_nano),0)::bigint FROM reservations WHERE state NOT IN ('settled','canceled')",
            &[],
        ).unwrap().get(0);
        assert_eq!(aggregate,0);
    }
}
