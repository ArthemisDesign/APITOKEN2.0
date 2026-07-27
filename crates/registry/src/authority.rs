//! Backend selector used by the composition and async actor layers during the controlled cutover.

use crate::{
    pg::{Owner, PgStore},
    AccountRow, BillingTotals, KeyAuth, KeyPolicyUpdate, KeyRow, LedgerRow, PoolStateRow,
    SpendAccountAgg, Sub, SubAdmin, SubHealth, SubRow, UsageModelAgg, UsageReport,
};
use anyhow::{bail, Result};
use rusqlite::Connection;

#[derive(Clone)]
pub enum AuthorityConfig {
    Sqlite { path: String },
    Postgres { url: String },
}

impl AuthorityConfig {
    pub fn new(sqlite_path: String, postgres_url: Option<String>) -> Self {
        match postgres_url.filter(|s| !s.trim().is_empty()) {
            Some(url) => Self::Postgres { url },
            None => Self::Sqlite { path: sqlite_path },
        }
    }
    pub fn connect(&self) -> Result<Authority> {
        Ok(match self {
            Self::Sqlite { path } => Authority::Sqlite(crate::open(path)?),
            Self::Postgres { url } => Authority::Postgres(PgStore::connect(url)?),
        })
    }
    pub fn is_postgres(&self) -> bool {
        matches!(self, Self::Postgres { .. })
    }
    pub fn label(&self) -> &str {
        match self {
            Self::Sqlite { path } => path,
            Self::Postgres { .. } => "engine-postgresql",
        }
    }
    pub fn sqlite_path(&self) -> Option<&str> {
        match self {
            Self::Sqlite { path } => Some(path),
            Self::Postgres { .. } => None,
        }
    }
}

pub enum Authority {
    Sqlite(Connection),
    Postgres(PgStore),
}

impl Authority {
    pub fn migrate(&mut self) -> Result<()> {
        if let Self::Postgres(pg) = self {
            pg.migrate()?;
        }
        Ok(())
    }
    pub fn claim_instance(&mut self, instance_id: &str, ttl_secs: i64) -> Result<Option<Owner>> {
        match self {
            Self::Sqlite(_) => Ok(None),
            Self::Postgres(pg) => Ok(Some(pg.claim_instance(instance_id, ttl_secs)?)),
        }
    }
    pub fn heartbeat_instance(&mut self, owner: &Owner, ttl_secs: i64) -> Result<bool> {
        match self {
            Self::Postgres(pg) => pg.heartbeat_instance(owner, ttl_secs),
            Self::Sqlite(_) => Ok(true),
        }
    }
    pub fn load_active(&mut self, fleet: Option<&str>) -> Result<Vec<Sub>> {
        match self {
            Self::Sqlite(c) => crate::load_active(c, fleet),
            Self::Postgres(pg) => pg.load_active(fleet),
        }
    }
    pub fn add(&mut self, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
        match self {
            Self::Sqlite(c) => crate::add(c, email, token, proxy, fleet),
            Self::Postgres(pg) => pg.add(email, token, proxy, fleet),
        }
    }
    pub fn add_file(
        &mut self,
        email: &str,
        token_file: &str,
        proxy: &str,
        fleet: &str,
    ) -> Result<()> {
        match self {
            Self::Sqlite(c) => crate::add_file(c, email, token_file, proxy, fleet),
            Self::Postgres(pg) => pg.add_file(email, token_file, proxy, fleet),
        }
    }
    pub fn set_sub_status(&mut self, email: &str, status: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::set_status(c, email, status),
            Self::Postgres(pg) => pg.set_sub_status(email, status),
        }
    }
    pub fn set_plan(&mut self, email: &str, plan: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::set_plan(c, email, plan),
            Self::Postgres(pg) => pg.set_plan(email, plan),
        }
    }
    pub fn get_creds(&mut self, email: &str) -> Result<Option<(String, String)>> {
        match self {
            Self::Sqlite(c) => crate::get_creds(c, email),
            Self::Postgres(pg) => pg.get_creds(email),
        }
    }
    pub fn set_proxy(&mut self, email: &str, proxy: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::set_proxy(c, email, proxy),
            Self::Postgres(pg) => pg.set_proxy(email, proxy),
        }
    }
    pub fn set_fleet(&mut self, email: &str, fleet: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::set_fleet(c, email, fleet),
            Self::Postgres(pg) => pg.set_fleet(email, fleet),
        }
    }
    pub fn set_proxy_meta(
        &mut self,
        email: &str,
        expire: &str,
        checked_ts: i64,
        ok: bool,
    ) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::set_proxy_meta(c, email, expire, checked_ts, ok),
            Self::Postgres(pg) => pg.set_proxy_meta(email, expire, checked_ts, ok),
        }
    }
    pub fn remove_sub(&mut self, email: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::remove(c, email),
            Self::Postgres(pg) => pg.remove_sub(email),
        }
    }
    pub fn clear_subs(&mut self, fleet: Option<&str>) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::clear(c, fleet),
            Self::Postgres(pg) => pg.clear_subs(fleet),
        }
    }
    pub fn list_subs(&mut self) -> Result<Vec<SubRow>> {
        match self {
            Self::Sqlite(c) => crate::list(c),
            Self::Postgres(pg) => pg.list_subs(),
        }
    }
    pub fn subs_admin(&mut self) -> Result<Vec<SubAdmin>> {
        match self {
            Self::Sqlite(c) => crate::subs_admin(c),
            Self::Postgres(pg) => pg.subs_admin(),
        }
    }
    /// Durable auth-health of subscriptions (authoritative: survives restart / blue-green).
    pub fn load_sub_health(&mut self, fleet: Option<&str>) -> Result<Vec<SubHealth>> {
        match self {
            Self::Sqlite(c) => crate::load_sub_health(c, fleet),
            Self::Postgres(pg) => pg.load_sub_health(fleet),
        }
    }
    /// Persist one subscription's auth-health verdict. PostgreSQL requires the owner epoch (fenced).
    pub fn save_sub_health(&mut self, owner: Option<&Owner>, h: &SubHealth) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::save_sub_health(c, h),
            Self::Postgres(pg) => pg.save_sub_health(
                owner.ok_or_else(|| {
                    anyhow::anyhow!("PostgreSQL sub-health write requires owner epoch")
                })?,
                h,
            ),
        }
    }

    pub fn account_create(&mut self, id: &str, handle: Option<&str>, mult_bp: i64) -> Result<()> {
        match self {
            Self::Sqlite(c) => crate::account_create(c, id, handle, mult_bp),
            Self::Postgres(pg) => pg.account_create(id, handle, mult_bp),
        }
    }
    pub fn account_get(&mut self, id: &str) -> Result<Option<AccountRow>> {
        match self {
            Self::Sqlite(c) => crate::account_get(c, id),
            Self::Postgres(pg) => pg.account_get(id),
        }
    }
    pub fn account_by_handle(&mut self, handle: &str) -> Result<Option<AccountRow>> {
        match self {
            Self::Sqlite(c) => crate::account_by_handle(c, handle),
            Self::Postgres(pg) => pg.account_by_handle(handle),
        }
    }
    pub fn account_list(&mut self) -> Result<Vec<AccountRow>> {
        match self {
            Self::Sqlite(c) => crate::account_list(c),
            Self::Postgres(pg) => pg.account_list(),
        }
    }
    pub fn account_set_status(&mut self, id: &str, status: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::account_set_status(c, id, status),
            Self::Postgres(pg) => pg.account_set_status(id, status),
        }
    }
    pub fn account_set_mult_bp(&mut self, id: &str, mult_bp: i64) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::account_set_mult_bp(c, id, mult_bp),
            Self::Postgres(pg) => pg.account_set_mult_bp(id, mult_bp),
        }
    }
    pub fn account_remove(&mut self, id: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::account_remove(c, id),
            Self::Postgres(pg) => pg.account_remove(id),
        }
    }
    pub fn account_topup(
        &mut self,
        id: &str,
        amount: i64,
        reference: Option<&str>,
    ) -> Result<Option<i64>> {
        match self {
            Self::Sqlite(c) => crate::account_topup(c, id, amount, reference),
            Self::Postgres(pg) => pg.account_topup(id, amount, reference),
        }
    }
    pub fn key_issue(&mut self, key: &str, account_id: &str, label: Option<&str>) -> Result<()> {
        match self {
            Self::Sqlite(c) => crate::key_issue(c, key, account_id, label),
            Self::Postgres(pg) => pg.key_issue(key, account_id, label),
        }
    }
    pub fn key_issue_with_policy(
        &mut self,
        key: &str,
        account_id: &str,
        label: Option<&str>,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> Result<()> {
        match self {
            Self::Sqlite(c) => crate::key_issue_with_policy(
                c,
                key,
                account_id,
                label,
                spend_limit_nano,
                expires_ts,
            ),
            Self::Postgres(pg) => {
                pg.key_issue_with_policy(key, account_id, label, spend_limit_nano, expires_ts)
            }
        }
    }
    pub fn key_account(&mut self, key: &str) -> Result<Option<KeyAuth>> {
        match self {
            Self::Sqlite(c) => crate::key_account(c, key),
            Self::Postgres(pg) => pg.key_account(key),
        }
    }
    pub fn key_get(&mut self, key: &str) -> Result<Option<KeyRow>> {
        match self {
            Self::Sqlite(c) => crate::key_get(c, key),
            Self::Postgres(pg) => pg.key_get(key),
        }
    }
    pub fn key_list(&mut self) -> Result<Vec<KeyRow>> {
        match self {
            Self::Sqlite(c) => crate::key_list(c),
            Self::Postgres(pg) => pg.key_list(),
        }
    }
    pub fn keys_by_account(&mut self, account_id: &str) -> Result<Vec<KeyRow>> {
        match self {
            Self::Sqlite(c) => crate::keys_by_account(c, account_id),
            Self::Postgres(pg) => pg.keys_by_account(account_id),
        }
    }
    pub fn key_set_status(&mut self, key: &str, status: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::key_set_status(c, key, status),
            Self::Postgres(pg) => pg.key_set_status(key, status),
        }
    }
    pub fn key_set_status_by_id(&mut self, key_id: &str, status: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::key_set_status_by_id(c, key_id, status),
            Self::Postgres(pg) => pg.key_set_status_by_id(key_id, status),
        }
    }
    pub fn key_set_label_by_id(&mut self, key_id: &str, label: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::key_set_label_by_id(c, key_id, label),
            Self::Postgres(pg) => pg.key_set_label_by_id(key_id, label),
        }
    }
    pub fn key_set_policy_by_id(
        &mut self,
        account_id: &str,
        key_id: &str,
        spend_limit_nano: Option<i64>,
        expires_ts: Option<i64>,
    ) -> Result<KeyPolicyUpdate> {
        match self {
            Self::Sqlite(c) => {
                crate::key_set_policy_by_id(c, account_id, key_id, spend_limit_nano, expires_ts)
            }
            Self::Postgres(pg) => {
                pg.key_set_policy_by_id(account_id, key_id, spend_limit_nano, expires_ts)
            }
        }
    }
    pub fn key_remove(&mut self, key: &str) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::key_remove(c, key),
            Self::Postgres(pg) => pg.key_remove(key),
        }
    }
    pub fn key_clear(&mut self) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::key_clear(c),
            Self::Postgres(pg) => pg.key_clear(),
        }
    }
    pub fn ledger_recent(&mut self, account_id: &str, limit: i64) -> Result<Vec<LedgerRow>> {
        match self {
            Self::Sqlite(c) => crate::ledger_recent(c, account_id, limit),
            Self::Postgres(pg) => pg.ledger_recent(account_id, limit),
        }
    }
    pub fn ledger_after(
        &mut self,
        account_id: &str,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<LedgerRow>> {
        match self {
            Self::Sqlite(c) => crate::ledger_after(c, account_id, after_id, limit),
            Self::Postgres(pg) => pg.ledger_after(account_id, after_id, limit),
        }
    }
    pub fn ledger_ack(
        &mut self,
        consumer: &str,
        account_id: &str,
        last_ledger_id: i64,
    ) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::ledger_ack(c, consumer, account_id, last_ledger_id),
            Self::Postgres(pg) => pg.ledger_ack(consumer, account_id, last_ledger_id),
        }
    }
    pub fn usage_by_model(
        &mut self,
        account_id: &str,
        since_ts: i64,
    ) -> Result<Vec<UsageModelAgg>> {
        match self {
            Self::Sqlite(c) => crate::usage_by_model(c, account_id, since_ts),
            Self::Postgres(pg) => pg.usage_by_model(account_id, since_ts),
        }
    }
    pub fn usage_report(
        &mut self,
        account_id: &str,
        since_ts: i64,
        until_ts: i64,
    ) -> Result<UsageReport> {
        match self {
            Self::Sqlite(c) => crate::usage_report(c, account_id, since_ts, until_ts),
            Self::Postgres(pg) => pg.usage_report(account_id, since_ts, until_ts),
        }
    }
    pub fn spend_by_account(&mut self, since_ts: i64, limit: i64) -> Result<Vec<SpendAccountAgg>> {
        match self {
            Self::Sqlite(c) => crate::spend_by_account(c, since_ts, limit),
            Self::Postgres(pg) => pg.spend_by_account(since_ts, limit),
        }
    }
    pub fn usage_prune(&mut self, older_than_ts: i64) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::usage_prune(c, older_than_ts),
            Self::Postgres(pg) => pg.usage_prune(older_than_ts),
        }
    }
    pub fn maintenance_prune(
        &mut self,
        older_than_ts: i64,
    ) -> Result<crate::pg::MaintenanceReport> {
        match self {
            Self::Sqlite(c) => crate::sqlite_maintenance_prune(c, older_than_ts),
            Self::Postgres(pg) => pg.maintenance_prune(older_than_ts),
        }
    }
    pub fn reconcile_expired(&mut self, limit: usize) -> Result<crate::pg::ReconcileReport> {
        match self {
            Self::Sqlite(c) => crate::sqlite_reconcile_expired(c, limit),
            Self::Postgres(pg) => pg.reconcile_expired(limit),
        }
    }
    pub fn ledger_prune(&mut self, older_than_ts: i64) -> Result<usize> {
        match self {
            Self::Sqlite(c) => crate::ledger_prune(c, older_than_ts),
            Self::Postgres(pg) => pg.ledger_prune(older_than_ts),
        }
    }
    pub fn billing_totals(&mut self) -> Result<BillingTotals> {
        match self {
            Self::Sqlite(c) => crate::billing_totals(c),
            Self::Postgres(pg) => pg.billing_totals(),
        }
    }
    pub fn load_pool_state(&mut self) -> Result<Vec<PoolStateRow>> {
        match self {
            Self::Sqlite(c) => crate::load_pool_state(c),
            Self::Postgres(pg) => pg.load_pool_state(),
        }
    }
    pub fn save_pool_state(
        &mut self,
        owner: Option<&Owner>,
        rows: &[PoolStateRow],
    ) -> Result<Vec<(String, i64)>> {
        match self {
            Self::Sqlite(c) => {
                crate::save_pool_state(c, rows)?;
                Ok(rows.iter().map(|r| (r.email.clone(), 0)).collect())
            }
            Self::Postgres(pg) => pg.save_pool_state(
                owner
                    .ok_or_else(|| anyhow::anyhow!("PostgreSQL pool write requires owner epoch"))?,
                rows,
            ),
        }
    }
    pub fn wal_checkpoint(&mut self) -> Result<()> {
        match self {
            Self::Sqlite(c) => crate::wal_checkpoint(c),
            Self::Postgres(_) => Ok(()),
        }
    }
    pub fn sqlite_connection(&self) -> Result<&Connection> {
        match self {
            Self::Sqlite(c) => Ok(c),
            Self::Postgres(_) => bail!("operation is SQLite-only"),
        }
    }
    pub fn postgres(&mut self) -> Result<&mut PgStore> {
        match self {
            Self::Postgres(pg) => Ok(pg),
            Self::Sqlite(_) => bail!("operation requires PostgreSQL authority"),
        }
    }
}
