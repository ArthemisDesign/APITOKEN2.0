//! Backend selector used by the composition and async actor layers during the controlled cutover.

use crate::{
    pg::{Owner, PgStore},
    AccountRow, BillingTotals, ClaudeLifecycleProfile, KeyAuth, KeyPolicyUpdate, KeyRow, LedgerRow,
    PoolStateRow, SpendAccountAgg, Sub, SubAdmin, SubHealth, SubRow, UsageModelAgg, UsageReport,
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
        self.connect_with_application_name(crate::pg::DEFAULT_APPLICATION_NAME)
    }
    pub fn connect_with_application_name(&self, application_name: &str) -> Result<Authority> {
        Ok(match self {
            Self::Sqlite { path } => Authority::Sqlite(crate::open(path)?),
            Self::Postgres { url } => Authority::Postgres(PgStore::connect_with_application_name(
                url,
                application_name,
            )?),
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
    pub fn verify_schema(&mut self) -> Result<()> {
        if let Self::Postgres(pg) = self {
            pg.verify_schema()?;
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
    /// Operator routability switch for roster-backed fleets (Gemini/Codex/KIMI/GLM). Claude
    /// subscriptions do NOT go through here — they carry `active|paused|disabled` and are served
    /// by `set_sub_status` above, so a subscription never has two competing switches.
    pub fn pool_member_set_disabled(
        &mut self,
        provider: &str,
        member_id: &str,
        disabled: bool,
        hidden: bool,
        actor: &str,
        reason: &str,
    ) -> Result<()> {
        match self {
            Self::Sqlite(_) => bail!("operation requires PostgreSQL authority"),
            Self::Postgres(pg) => {
                pg.pool_member_set_disabled(provider, member_id, disabled, hidden, actor, reason)
            }
        }
    }
    /// Disabled members mapped to whether they are also hidden from the operator's list.
    pub fn pool_member_disables(
        &mut self,
        provider: &str,
    ) -> Result<std::collections::HashMap<String, bool>> {
        match self {
            Self::Sqlite(_) => bail!("operation requires PostgreSQL authority"),
            Self::Postgres(pg) => pg.pool_member_disables(provider),
        }
    }
    pub fn pool_member_disabled(
        &mut self,
        provider: &str,
    ) -> Result<std::collections::HashSet<String>> {
        match self {
            Self::Sqlite(_) => bail!("operation requires PostgreSQL authority"),
            Self::Postgres(pg) => pg.pool_member_disabled(provider),
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
    pub fn load_claude_lifecycle(&mut self) -> Result<Vec<ClaudeLifecycleProfile>> {
        match self {
            Self::Sqlite(c) => crate::load_claude_lifecycle(c),
            Self::Postgres(pg) => pg.load_claude_lifecycle(),
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
    pub fn reconcile_expired(
        &mut self,
        limit: usize,
        charge_hold_on_unknown_usage: bool,
    ) -> Result<crate::pg::ReconcileReport> {
        match self {
            Self::Sqlite(c) => {
                crate::sqlite_reconcile_expired(c, limit, charge_hold_on_unknown_usage)
            }
            Self::Postgres(pg) => pg.reconcile_expired(limit, charge_hold_on_unknown_usage),
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

impl Authority {
    fn gemini_batch_postgres(&mut self) -> Result<&mut PgStore> {
        match self {
            Self::Postgres(pg) => Ok(pg),
            Self::Sqlite(_) => Err(crate::GeminiBatchUnsupported.into()),
        }
    }

    pub fn gemini_batch_create(
        &mut self,
        create: &crate::GeminiBatchCreate,
        creator_key: &str,
    ) -> Result<crate::GeminiBatchCreateOutcome> {
        self.gemini_batch_postgres()?
            .gemini_batch_create(create, creator_key)
    }

    pub fn gemini_batch_get(
        &mut self,
        account_id: &str,
        job_id: &str,
    ) -> Result<Option<crate::GeminiBatchJobDetail>> {
        self.gemini_batch_postgres()?
            .gemini_batch_get(account_id, job_id)
    }

    pub fn gemini_batch_list(
        &mut self,
        account_id: &str,
        cursor: Option<&crate::GeminiBatchPageCursor>,
        limit: i64,
    ) -> Result<crate::GeminiBatchJobPage> {
        self.gemini_batch_postgres()?
            .gemini_batch_list(account_id, cursor, limit)
    }

    pub fn gemini_batch_file_create(
        &mut self,
        create: &crate::GeminiBatchFileCreate,
    ) -> Result<crate::GeminiBatchFileCreateOutcome> {
        self.gemini_batch_postgres()?
            .gemini_batch_file_create(create)
    }
    pub fn gemini_batch_file_append_chunk(
        &mut self,
        account_id: &str,
        file_id: &str,
        chunk: &crate::GeminiBatchFileChunk,
    ) -> Result<bool> {
        self.gemini_batch_postgres()?
            .gemini_batch_file_append_chunk(account_id, file_id, chunk)
    }
    pub fn gemini_batch_file_complete(
        &mut self,
        account_id: &str,
        file_id: &str,
        completion: &crate::GeminiBatchFileCompletion,
    ) -> Result<bool> {
        self.gemini_batch_postgres()?
            .gemini_batch_file_complete(account_id, file_id, completion)
    }
    pub fn gemini_batch_file_get(
        &mut self,
        account_id: &str,
        file_id: &str,
    ) -> Result<Option<crate::GeminiBatchFile>> {
        self.gemini_batch_postgres()?
            .gemini_batch_file_get(account_id, file_id)
    }
    pub fn gemini_batch_file_list(
        &mut self,
        account_id: &str,
        limit: i64,
    ) -> Result<Vec<crate::GeminiBatchFile>> {
        self.gemini_batch_postgres()?
            .gemini_batch_file_list(account_id, limit)
    }
    pub fn gemini_batch_file_delete(&mut self, account_id: &str, file_id: &str) -> Result<bool> {
        self.gemini_batch_postgres()?
            .gemini_batch_file_delete(account_id, file_id)
    }
    pub fn gemini_batch_blob_get(
        &mut self,
        account_id: &str,
        job_id: &str,
        item_index: i64,
        kind: &str,
    ) -> Result<Option<crate::GeminiBatchEncryptedBlob>> {
        self.gemini_batch_postgres()?
            .gemini_batch_blob_get(account_id, job_id, item_index, kind)
    }
    pub fn gemini_batch_file_chunk_page(
        &mut self,
        account_id: &str,
        file_id: &str,
        after_chunk_index: Option<i64>,
        limit: i64,
    ) -> Result<crate::GeminiBatchFileChunkPage> {
        self.gemini_batch_postgres()?.gemini_batch_file_chunk_page(
            account_id,
            file_id,
            after_chunk_index,
            limit,
        )
    }
    pub fn gemini_batch_link_output_file(
        &mut self,
        account_id: &str,
        job_id: &str,
        file_id: &str,
    ) -> Result<bool> {
        self.gemini_batch_postgres()?
            .gemini_batch_link_output_file(account_id, job_id, file_id)
    }

    pub fn acquire_gemini_batch_leader(&mut self, owner: &Owner, ttl_secs: i64) -> Result<bool> {
        self.gemini_batch_postgres()?
            .acquire_gemini_batch_leader(owner, ttl_secs)
    }
    pub fn claim_gemini_batch_item(
        &mut self,
        owner: &Owner,
        profile_id: &str,
        model_id: &str,
        lease_secs: i64,
    ) -> Result<Option<crate::GeminiBatchClaimedItem>> {
        self.gemini_batch_postgres()?
            .claim_gemini_batch_item(owner, profile_id, model_id, lease_secs)
    }

    pub fn mark_gemini_batch_dispatching(&mut self, owner: &Owner, claim: &crate::GeminiBatchClaim, lease_secs: i64) -> Result<bool> {
        self.gemini_batch_postgres()?.mark_gemini_batch_dispatching(owner, claim, lease_secs)
    }
    pub fn mark_gemini_batch_actual_send(&mut self, owner: &Owner, claim: &crate::GeminiBatchClaim, lease_secs: i64) -> Result<bool> {
        self.gemini_batch_postgres()?.mark_gemini_batch_actual_send(owner, claim, lease_secs)
    }
    pub fn renew_gemini_batch_claim(&mut self, owner: &Owner, claim: &crate::GeminiBatchClaim, lease_secs: i64) -> Result<bool> {
        self.gemini_batch_postgres()?.renew_gemini_batch_claim(owner, claim, lease_secs)
    }
    pub fn requeue_gemini_batch_claim(&mut self, owner: &Owner, claim: &crate::GeminiBatchClaim, next_attempt_ts: i64) -> Result<bool> {
        self.gemini_batch_postgres()?.requeue_gemini_batch_claim(owner, claim, next_attempt_ts)
    }
    pub fn reconcile_expired_gemini_batch_claims(&mut self, limit: usize) -> Result<crate::pg::gemini_batch_claims::GeminiBatchReconcileReport> {
        self.gemini_batch_postgres()?.reconcile_expired_gemini_batch_claims(limit)
    }

    pub fn gemini_batch_cancel(
        &mut self,
        account_id: &str,
        job_id: &str,
    ) -> Result<Option<crate::GeminiBatchCancelResult>> {
        self.gemini_batch_postgres()?
            .gemini_batch_cancel(account_id, job_id)
    }

    pub fn enqueue_gemini_batch_settlement(
        &mut self,
        owner: &Owner,
        claim: &crate::GeminiBatchClaim,
        intent: &crate::GeminiBatchSettlementIntent,
    ) -> Result<()> {
        self.gemini_batch_postgres()?
            .enqueue_gemini_batch_settlement(owner, claim, intent)
    }
    pub fn process_gemini_batch_settlement(&mut self, request_id: &str) -> Result<Option<i64>> {
        self.gemini_batch_postgres()?
            .process_gemini_batch_settlement(request_id)
    }
    pub fn drain_gemini_batch_settlements(&mut self, limit: usize) -> Result<usize> {
        self.gemini_batch_postgres()?
            .drain_gemini_batch_settlements(limit)
    }
    pub fn gemini_batch_delete(&mut self, account_id: &str, job_id: &str) -> Result<bool> {
        self.gemini_batch_postgres()?
            .gemini_batch_delete(account_id, job_id)
    }
    pub fn gemini_batch_operational_report(
        &mut self,
    ) -> Result<crate::GeminiBatchOperationalReport> {
        self.gemini_batch_postgres()?
            .gemini_batch_operational_report()
    }

    pub fn prune_gemini_batch(
        &mut self,
        older_than: i64,
        limit: usize,
    ) -> Result<crate::GeminiBatchPruneReport> {
        self.gemini_batch_postgres()?
            .prune_gemini_batch(older_than, limit)
    }
}
