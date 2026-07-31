//! Deterministic Stage 6 migration from the legacy aggregate balance to engine-owned funding
//! buckets. The planner is deliberately offline: live reserve, settlement and top-up writers stay
//! on the legacy aggregate path until the later strict-funding checkpoint.

use crate::pricing::{AccountClass, ReconciliationState};
use anyhow::{bail, Context, Result};
use postgres::{Client, GenericClient, IsolationLevel};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FUNDING_RECONCILIATION_SCHEMA_VERSION: i64 = 1;
pub const WELCOME_TRACK_BONUS_NANO: i64 = 4_000_000_000;
const POSTGRES_FUNDING_RECONCILIATION_LOCK: i64 = 831_572_908_442;
const PAID_SOURCE_REF: &str = "stage6:paid:v1";
const LEGACY_SOURCE_REF: &str = "stage6:ambiguous:v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct PaidReferenceRule {
    prefix: &'static str,
    account_classes: Vec<AccountClass>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct FundingSourcePolicy {
    schema_version: i64,
    paid_reference_rules: Vec<PaidReferenceRule>,
}

fn source_policy() -> FundingSourcePolicy {
    FundingSourcePolicy {
        schema_version: FUNDING_RECONCILIATION_SCHEMA_VERSION,
        paid_reference_rules: vec![
            PaidReferenceRule {
                prefix: "cryptomus:",
                account_classes: vec![AccountClass::B2c, AccountClass::B2b],
            },
            PaidReferenceRule {
                prefix: "openkeys:",
                account_classes: vec![AccountClass::OpenKeys],
            },
            PaidReferenceRule {
                prefix: "platega:",
                account_classes: vec![AccountClass::B2c, AccountClass::B2b],
            },
        ],
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingReconciliationDisposition {
    Ready,
    Exception,
    Blocked,
    Replay,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingReconciliationIssueCode {
    MissingPolicyBinding,
    OutstandingReservations,
    ExistingBucketConflict,
    AmbiguousCredit,
    LegacyOpeningBalance,
    LedgerGap,
    MissingLedgerBalance,
    UnsupportedLedgerEntry,
    AdjustmentRequiresReview,
    InvalidWelcomeCredit,
    WelcomeCreditOutsideB2c,
    MultipleWelcomeCredits,
    BalanceMismatch,
    ArithmeticOverflow,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingReconciliationIssue {
    pub code: FundingReconciliationIssueCode,
    pub ledger_id: Option<i64>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingBucketPlan {
    pub bucket_id: String,
    pub source_type: String,
    pub source_ref: String,
    pub eligibility: String,
    pub balance_nano: i64,
    pub reserved_nano: i64,
    pub spent_nano: i64,
    pub version: i64,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingAccountReconciliationPlan {
    pub account_id: String,
    pub account_class: Option<AccountClass>,
    pub live_balance_nano: i64,
    pub live_reserved_nano: i64,
    pub ledger_rows: i64,
    pub ledger_last_id: i64,
    pub source_state_digest: String,
    pub account_plan_digest: String,
    pub disposition: FundingReconciliationDisposition,
    pub target_reconciliation_state: ReconciliationState,
    pub buckets: Vec<FundingBucketPlan>,
    pub issues: Vec<FundingReconciliationIssue>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingReconciliationPlan {
    pub schema_version: i64,
    pub source_policy_digest: String,
    pub plan_digest: String,
    pub ready_accounts: i64,
    pub exception_accounts: i64,
    pub blocked_accounts: i64,
    pub replay_accounts: i64,
    pub accounts: Vec<FundingAccountReconciliationPlan>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingReconciliationApplyReport {
    pub schema_version: i64,
    pub plan_digest: String,
    pub inserted_buckets: i64,
    pub verified_accounts: i64,
    pub exception_accounts: i64,
    pub blocked_accounts: i64,
    pub replay_accounts: i64,
    pub fully_applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct LedgerSnapshot {
    id: i64,
    kind: String,
    amount_nano: i64,
    reference: Option<String>,
    balance_after_nano: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct StoredBucketSnapshot {
    bucket_id: String,
    source_type: String,
    source_ref: String,
    eligibility: String,
    balance_nano: i64,
    reserved_nano: i64,
    spent_nano: i64,
    version: i64,
    status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct AccountSnapshot {
    account_id: String,
    balance_nano: i64,
    reserved_nano: i64,
    account_class: Option<AccountClass>,
    reconciliation_state: Option<ReconciliationState>,
    ledger: Vec<LedgerSnapshot>,
    buckets: Vec<StoredBucketSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FreeKind {
    Welcome,
    Legacy,
}

#[derive(Clone, Debug)]
struct FreeLot {
    kind: FreeKind,
    credited: i128,
    remaining: i128,
}

#[derive(Default)]
struct ReplayState {
    free: VecDeque<FreeLot>,
    paid_credited: i128,
    paid_balance: i128,
    welcome_reference: Option<String>,
    issues: Vec<FundingReconciliationIssue>,
    hard_exception: bool,
}

impl ReplayState {
    fn issue(
        &mut self,
        code: FundingReconciliationIssueCode,
        ledger_id: Option<i64>,
        detail: impl Into<String>,
    ) {
        self.issues.push(FundingReconciliationIssue {
            code,
            ledger_id,
            detail: detail.into(),
        });
    }

    fn add_free(&mut self, kind: FreeKind, amount: i128) {
        if amount > 0 {
            self.free.push_back(FreeLot {
                kind,
                credited: amount,
                remaining: amount,
            });
        }
    }

    fn charge_free_first(&mut self, mut amount: i128) {
        for lot in &mut self.free {
            if amount == 0 {
                break;
            }
            let used = amount.min(lot.remaining);
            lot.remaining -= used;
            amount -= used;
        }
        self.paid_balance -= amount;
    }

    fn total(&self) -> i128 {
        self.paid_balance + self.free.iter().map(|lot| lot.remaining).sum::<i128>()
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn digest<T: Serialize>(value: &T) -> String {
    let encoded = serde_json::to_vec(value).expect("typed funding reconciliation value serializes");
    let mut hasher = Sha256::new();
    hasher.update(encoded);
    format!("sha256:{:x}", hasher.finalize())
}

fn bucket_id(account_id: &str, source_type: &str, source_ref: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"funding-bucket-v1\0");
    hasher.update(account_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(source_type.as_bytes());
    hasher.update(b"\0");
    hasher.update(source_ref.as_bytes());
    format!("fb_{:x}", hasher.finalize())
}

fn is_paid_reference(account_class: AccountClass, reference: Option<&str>) -> bool {
    let Some(reference) = reference else {
        return false;
    };
    source_policy().paid_reference_rules.iter().any(|rule| {
        rule.account_classes.contains(&account_class) && reference.starts_with(rule.prefix)
    })
}

fn issue(
    code: FundingReconciliationIssueCode,
    ledger_id: Option<i64>,
    detail: impl Into<String>,
) -> FundingReconciliationIssue {
    FundingReconciliationIssue {
        code,
        ledger_id,
        detail: detail.into(),
    }
}

fn bucket(
    account_id: &str,
    source_type: &str,
    source_ref: &str,
    eligibility: &str,
    balance_nano: i64,
    spent_nano: i64,
) -> FundingBucketPlan {
    FundingBucketPlan {
        bucket_id: bucket_id(account_id, source_type, source_ref),
        source_type: source_type.to_owned(),
        source_ref: source_ref.to_owned(),
        eligibility: eligibility.to_owned(),
        balance_nano,
        reserved_nano: 0,
        spent_nano,
        version: 1,
        status: if balance_nano == 0 {
            "exhausted".to_owned()
        } else {
            "active".to_owned()
        },
    }
}

fn exact_buckets(account_id: &str, state: &ReplayState) -> Result<Vec<FundingBucketPlan>> {
    let welcome_credited: i128 = state
        .free
        .iter()
        .filter(|lot| lot.kind == FreeKind::Welcome)
        .map(|lot| lot.credited)
        .sum();
    let welcome_balance: i128 = state
        .free
        .iter()
        .filter(|lot| lot.kind == FreeKind::Welcome)
        .map(|lot| lot.remaining)
        .sum();
    let legacy_credited: i128 = state
        .free
        .iter()
        .filter(|lot| lot.kind == FreeKind::Legacy)
        .map(|lot| lot.credited)
        .sum();
    let legacy_balance: i128 = state
        .free
        .iter()
        .filter(|lot| lot.kind == FreeKind::Legacy)
        .map(|lot| lot.remaining)
        .sum();
    let convert = |label: &str, value: i128| -> Result<i64> {
        i64::try_from(value).with_context(|| format!("{label} exceeds the engine nanoUSD range"))
    };
    let paid_balance = convert("paid balance", state.paid_balance)?;
    let paid_spent = convert("paid spend", state.paid_credited - state.paid_balance)?;
    let mut buckets = vec![bucket(
        account_id,
        "paid",
        PAID_SOURCE_REF,
        "any",
        paid_balance,
        paid_spent,
    )];
    if welcome_credited > 0 {
        let source_ref = state
            .welcome_reference
            .as_deref()
            .context("welcome funding exists without its source reference")?;
        buckets.push(bucket(
            account_id,
            "welcome_track_bonus",
            source_ref,
            "track",
            convert("welcome balance", welcome_balance)?,
            convert("welcome spend", welcome_credited - welcome_balance)?,
        ));
    }
    if legacy_credited > 0 {
        buckets.push(bucket(
            account_id,
            "legacy_restricted",
            LEGACY_SOURCE_REF,
            "none",
            convert("legacy restricted balance", legacy_balance)?,
            convert("legacy restricted spend", legacy_credited - legacy_balance)?,
        ));
    }
    buckets.sort_by(|left, right| left.bucket_id.cmp(&right.bucket_id));
    Ok(buckets)
}

fn conservative_buckets(account_id: &str, balance_nano: i64) -> Vec<FundingBucketPlan> {
    let mut buckets = Vec::new();
    if balance_nano < 0 {
        buckets.push(bucket(
            account_id,
            "paid",
            PAID_SOURCE_REF,
            "any",
            balance_nano,
            balance_nano.saturating_neg(),
        ));
    } else {
        buckets.push(bucket(account_id, "paid", PAID_SOURCE_REF, "any", 0, 0));
        if balance_nano > 0 {
            buckets.push(bucket(
                account_id,
                "legacy_restricted",
                LEGACY_SOURCE_REF,
                "none",
                balance_nano,
                0,
            ));
        }
    }
    buckets.sort_by(|left, right| left.bucket_id.cmp(&right.bucket_id));
    buckets
}

fn stored_buckets_match(stored: &[StoredBucketSnapshot], planned: &[FundingBucketPlan]) -> bool {
    stored.len() == planned.len()
        && stored.iter().zip(planned).all(|(left, right)| {
            left.bucket_id == right.bucket_id
                && left.source_type == right.source_type
                && left.source_ref == right.source_ref
                && left.eligibility == right.eligibility
                && left.balance_nano == right.balance_nano
                && left.reserved_nano == right.reserved_nano
                && left.spent_nano == right.spent_nano
                && left.version == right.version
                && left.status == right.status
        })
}

#[derive(Serialize)]
struct AccountPlanDigest<'a> {
    account_id: &'a str,
    account_class: Option<AccountClass>,
    live_balance_nano: i64,
    live_reserved_nano: i64,
    ledger_rows: i64,
    ledger_last_id: i64,
    source_state_digest: &'a str,
    disposition: FundingReconciliationDisposition,
    target_reconciliation_state: ReconciliationState,
    buckets: &'a [FundingBucketPlan],
    issues: &'a [FundingReconciliationIssue],
}

fn build_account_plan(snapshot: &AccountSnapshot) -> FundingAccountReconciliationPlan {
    let source_state_digest = digest(snapshot);
    let ledger_rows = snapshot.ledger.len() as i64;
    let ledger_last_id = snapshot.ledger.last().map_or(0, |entry| entry.id);
    let Some(account_class) = snapshot.account_class else {
        let issues = vec![issue(
            FundingReconciliationIssueCode::MissingPolicyBinding,
            None,
            "account has no reviewed Stage 5 policy binding",
        )];
        return finish_account_plan(
            snapshot,
            None,
            ledger_rows,
            ledger_last_id,
            source_state_digest,
            FundingReconciliationDisposition::Blocked,
            ReconciliationState::Exception,
            Vec::new(),
            issues,
        );
    };
    if snapshot.reserved_nano != 0 {
        let issues = vec![issue(
            FundingReconciliationIssueCode::OutstandingReservations,
            None,
            format!(
                "account has {} reserved nanoUSD; drain reservations before migration",
                snapshot.reserved_nano
            ),
        )];
        return finish_account_plan(
            snapshot,
            Some(account_class),
            ledger_rows,
            ledger_last_id,
            source_state_digest,
            FundingReconciliationDisposition::Blocked,
            ReconciliationState::Exception,
            Vec::new(),
            issues,
        );
    }

    let mut replay = ReplayState::default();
    let mut running: Option<i128> = None;
    for entry in &snapshot.ledger {
        let Some(balance_after) = entry.balance_after_nano else {
            replay.issue(
                FundingReconciliationIssueCode::MissingLedgerBalance,
                Some(entry.id),
                "ledger row has no immutable balance_after_nano",
            );
            replay.hard_exception = true;
            break;
        };
        let effect = match entry.kind.as_str() {
            "topup" if entry.amount_nano >= 0 => i128::from(entry.amount_nano),
            "charge" if entry.amount_nano >= 0 => -i128::from(entry.amount_nano),
            "adjust" => {
                replay.issue(
                    FundingReconciliationIssueCode::AdjustmentRequiresReview,
                    Some(entry.id),
                    "adjustment has no immutable original funding allocation",
                );
                replay.hard_exception = true;
                break;
            }
            _ => {
                replay.issue(
                    FundingReconciliationIssueCode::UnsupportedLedgerEntry,
                    Some(entry.id),
                    format!(
                        "unsupported ledger kind {:?} or signed amount {}",
                        entry.kind, entry.amount_nano
                    ),
                );
                replay.hard_exception = true;
                break;
            }
        };
        let before = i128::from(balance_after) - effect;
        match running {
            None => {
                if before > 0 {
                    replay.add_free(FreeKind::Legacy, before);
                    replay.issue(
                        FundingReconciliationIssueCode::LegacyOpeningBalance,
                        Some(entry.id),
                        format!(
                            "ledger starts after an unattributed {} nanoUSD balance",
                            before
                        ),
                    );
                } else if before < 0 {
                    replay.issue(
                        FundingReconciliationIssueCode::LedgerGap,
                        Some(entry.id),
                        format!("ledger starts from a negative {} nanoUSD balance", before),
                    );
                    replay.hard_exception = true;
                    break;
                }
            }
            Some(expected) if expected != before => {
                let gap = before - expected;
                if gap > 0 {
                    replay.add_free(FreeKind::Legacy, gap);
                    replay.issue(
                        FundingReconciliationIssueCode::LedgerGap,
                        Some(entry.id),
                        format!(
                            "ledger contains an unattributed positive {} nanoUSD gap",
                            gap
                        ),
                    );
                } else {
                    replay.issue(
                        FundingReconciliationIssueCode::LedgerGap,
                        Some(entry.id),
                        format!(
                            "ledger contains an unattributed negative {} nanoUSD gap",
                            gap
                        ),
                    );
                    replay.hard_exception = true;
                    break;
                }
            }
            Some(_) => {}
        }

        match entry.kind.as_str() {
            "topup" => {
                let amount = i128::from(entry.amount_nano);
                let reference = entry.reference.as_deref();
                if reference.is_some_and(|value| value.starts_with("signup-bonus:")) {
                    if account_class != AccountClass::B2c {
                        replay.add_free(FreeKind::Legacy, amount);
                        replay.issue(
                            FundingReconciliationIssueCode::WelcomeCreditOutsideB2c,
                            Some(entry.id),
                            "signup bonus reference belongs to a non-B2C account",
                        );
                    } else if entry.amount_nano != WELCOME_TRACK_BONUS_NANO {
                        replay.add_free(FreeKind::Legacy, amount);
                        replay.issue(
                            FundingReconciliationIssueCode::InvalidWelcomeCredit,
                            Some(entry.id),
                            format!(
                                "signup bonus is {} nanoUSD instead of {}",
                                entry.amount_nano, WELCOME_TRACK_BONUS_NANO
                            ),
                        );
                    } else if replay.welcome_reference.is_some() {
                        replay.add_free(FreeKind::Legacy, amount);
                        replay.issue(
                            FundingReconciliationIssueCode::MultipleWelcomeCredits,
                            Some(entry.id),
                            "account has more than one distinct signup bonus credit",
                        );
                    } else {
                        replay.welcome_reference = entry.reference.clone();
                        replay.add_free(FreeKind::Welcome, amount);
                    }
                } else if is_paid_reference(account_class, reference) {
                    replay.paid_credited += amount;
                    replay.paid_balance += amount;
                } else {
                    replay.add_free(FreeKind::Legacy, amount);
                    replay.issue(
                        FundingReconciliationIssueCode::AmbiguousCredit,
                        Some(entry.id),
                        "credit reference is not a reviewed paid source",
                    );
                }
            }
            "charge" => replay.charge_free_first(i128::from(entry.amount_nano)),
            _ => unreachable!("ledger entry kind was validated above"),
        }
        if replay.total() != i128::from(balance_after) {
            replay.issue(
                FundingReconciliationIssueCode::BalanceMismatch,
                Some(entry.id),
                format!(
                    "replayed buckets total {} but ledger balance is {}",
                    replay.total(),
                    balance_after
                ),
            );
            replay.hard_exception = true;
            break;
        }
        running = Some(i128::from(balance_after));
    }

    if !replay.hard_exception && replay.total() != i128::from(snapshot.balance_nano) {
        replay.issue(
            FundingReconciliationIssueCode::BalanceMismatch,
            None,
            format!(
                "replayed buckets total {} but live balance is {}",
                replay.total(),
                snapshot.balance_nano
            ),
        );
        replay.hard_exception = true;
    }

    let buckets = if replay.hard_exception {
        conservative_buckets(&snapshot.account_id, snapshot.balance_nano)
    } else {
        match exact_buckets(&snapshot.account_id, &replay) {
            Ok(buckets) => buckets,
            Err(error) => {
                replay.issue(
                    FundingReconciliationIssueCode::ArithmeticOverflow,
                    None,
                    format!("{error:#}"),
                );
                conservative_buckets(&snapshot.account_id, snapshot.balance_nano)
            }
        }
    };
    let bucket_total: i128 = buckets
        .iter()
        .map(|planned| i128::from(planned.balance_nano))
        .sum();
    if bucket_total != i128::from(snapshot.balance_nano) {
        replay.issue(
            FundingReconciliationIssueCode::BalanceMismatch,
            None,
            "planned bucket sum does not equal the live account balance",
        );
    }

    let target = if replay.issues.is_empty() {
        ReconciliationState::Verified
    } else {
        ReconciliationState::Exception
    };
    let mut disposition = if replay.issues.is_empty() {
        FundingReconciliationDisposition::Ready
    } else {
        FundingReconciliationDisposition::Exception
    };
    if !snapshot.buckets.is_empty() {
        if stored_buckets_match(&snapshot.buckets, &buckets) {
            disposition = FundingReconciliationDisposition::Replay;
        } else {
            replay.issue(
                FundingReconciliationIssueCode::ExistingBucketConflict,
                None,
                "existing funding buckets differ from the deterministic Stage 6 plan",
            );
            disposition = FundingReconciliationDisposition::Blocked;
        }
    }

    finish_account_plan(
        snapshot,
        Some(account_class),
        ledger_rows,
        ledger_last_id,
        source_state_digest,
        disposition,
        if disposition == FundingReconciliationDisposition::Blocked {
            ReconciliationState::Exception
        } else {
            target
        },
        buckets,
        replay.issues,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_account_plan(
    snapshot: &AccountSnapshot,
    account_class: Option<AccountClass>,
    ledger_rows: i64,
    ledger_last_id: i64,
    source_state_digest: String,
    disposition: FundingReconciliationDisposition,
    target_reconciliation_state: ReconciliationState,
    buckets: Vec<FundingBucketPlan>,
    issues: Vec<FundingReconciliationIssue>,
) -> FundingAccountReconciliationPlan {
    let account_plan_digest = digest(&AccountPlanDigest {
        account_id: &snapshot.account_id,
        account_class,
        live_balance_nano: snapshot.balance_nano,
        live_reserved_nano: snapshot.reserved_nano,
        ledger_rows,
        ledger_last_id,
        source_state_digest: &source_state_digest,
        disposition,
        target_reconciliation_state,
        buckets: &buckets,
        issues: &issues,
    });
    FundingAccountReconciliationPlan {
        account_id: snapshot.account_id.clone(),
        account_class,
        live_balance_nano: snapshot.balance_nano,
        live_reserved_nano: snapshot.reserved_nano,
        ledger_rows,
        ledger_last_id,
        source_state_digest,
        account_plan_digest,
        disposition,
        target_reconciliation_state,
        buckets,
        issues,
    }
}

#[derive(Serialize)]
struct PlanDigest<'a> {
    schema_version: i64,
    source_policy_digest: &'a str,
    accounts: &'a [FundingAccountReconciliationPlan],
}

fn build_plan(mut snapshots: Vec<AccountSnapshot>) -> FundingReconciliationPlan {
    snapshots.sort_by(|left, right| left.account_id.cmp(&right.account_id));
    let accounts: Vec<_> = snapshots.iter().map(build_account_plan).collect();
    let source_policy_digest = digest(&source_policy());
    let plan_digest = digest(&PlanDigest {
        schema_version: FUNDING_RECONCILIATION_SCHEMA_VERSION,
        source_policy_digest: &source_policy_digest,
        accounts: &accounts,
    });
    let count = |disposition| {
        accounts
            .iter()
            .filter(|account| account.disposition == disposition)
            .count() as i64
    };
    FundingReconciliationPlan {
        schema_version: FUNDING_RECONCILIATION_SCHEMA_VERSION,
        source_policy_digest,
        plan_digest,
        ready_accounts: count(FundingReconciliationDisposition::Ready),
        exception_accounts: count(FundingReconciliationDisposition::Exception),
        blocked_accounts: count(FundingReconciliationDisposition::Blocked),
        replay_accounts: count(FundingReconciliationDisposition::Replay),
        accounts,
    }
}

fn sqlite_snapshots(conn: &Connection) -> Result<Vec<AccountSnapshot>> {
    let mut accounts = conn.prepare(
        "SELECT a.id,a.balance_nano,a.reserved_nano,b.account_class,b.reconciliation_state \
         FROM accounts a LEFT JOIN account_policy_bindings b ON b.account_id=a.id \
         WHERE COALESCE(a.status,'active')<>'deleted' ORDER BY a.id",
    )?;
    let rows = accounts.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut snapshots = Vec::new();
    for row in rows {
        let (account_id, balance_nano, reserved_nano, class, state) = row?;
        let account_class = class.as_deref().map(AccountClass::from_db).transpose()?;
        let reconciliation_state = state
            .as_deref()
            .map(ReconciliationState::from_db)
            .transpose()?;
        let mut ledger_stmt = conn.prepare(
            "SELECT id,kind,amount_nano,ref,balance_after_nano FROM ledger \
             WHERE account_id=?1 ORDER BY id",
        )?;
        let ledger = ledger_stmt
            .query_map([&account_id], |entry| {
                Ok(LedgerSnapshot {
                    id: entry.get(0)?,
                    kind: entry.get(1)?,
                    amount_nano: entry.get(2)?,
                    reference: entry.get(3)?,
                    balance_after_nano: entry.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut bucket_stmt = conn.prepare(
            "SELECT bucket_id,source_type,source_ref,eligibility,balance_nano,reserved_nano, \
                    spent_nano,version,status FROM funding_buckets WHERE account_id=?1 ORDER BY bucket_id",
        )?;
        let buckets = bucket_stmt
            .query_map([&account_id], |bucket| {
                Ok(StoredBucketSnapshot {
                    bucket_id: bucket.get(0)?,
                    source_type: bucket.get(1)?,
                    source_ref: bucket.get(2)?,
                    eligibility: bucket.get(3)?,
                    balance_nano: bucket.get(4)?,
                    reserved_nano: bucket.get(5)?,
                    spent_nano: bucket.get(6)?,
                    version: bucket.get(7)?,
                    status: bucket.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        snapshots.push(AccountSnapshot {
            account_id,
            balance_nano,
            reserved_nano,
            account_class,
            reconciliation_state,
            ledger,
            buckets,
        });
    }
    Ok(snapshots)
}

fn postgres_snapshots<C: GenericClient>(client: &mut C) -> Result<Vec<AccountSnapshot>> {
    let rows = client.query(
        "SELECT a.id,a.balance_nano,a.reserved_nano,b.account_class,b.reconciliation_state \
         FROM accounts a LEFT JOIN account_policy_bindings b ON b.account_id=a.id \
         WHERE a.status<>'deleted' ORDER BY a.id",
        &[],
    )?;
    let mut snapshots = Vec::with_capacity(rows.len());
    for row in rows {
        let account_id: String = row.get(0);
        let class: Option<String> = row.get(3);
        let state: Option<String> = row.get(4);
        let account_class = class.as_deref().map(AccountClass::from_db).transpose()?;
        let reconciliation_state = state
            .as_deref()
            .map(ReconciliationState::from_db)
            .transpose()?;
        let ledger = client
            .query(
                "SELECT id,kind,amount_nano,ref,balance_after_nano FROM ledger \
                 WHERE account_id=$1 ORDER BY id",
                &[&account_id],
            )?
            .into_iter()
            .map(|entry| LedgerSnapshot {
                id: entry.get(0),
                kind: entry.get(1),
                amount_nano: entry.get(2),
                reference: entry.get(3),
                balance_after_nano: entry.get(4),
            })
            .collect();
        let buckets = client
            .query(
                "SELECT bucket_id,source_type,source_ref,eligibility,balance_nano,reserved_nano, \
                        spent_nano,version,status FROM funding_buckets WHERE account_id=$1 ORDER BY bucket_id",
                &[&account_id],
            )?
            .into_iter()
            .map(|bucket| StoredBucketSnapshot {
                bucket_id: bucket.get(0),
                source_type: bucket.get(1),
                source_ref: bucket.get(2),
                eligibility: bucket.get(3),
                balance_nano: bucket.get(4),
                reserved_nano: bucket.get(5),
                spent_nano: bucket.get(6),
                version: bucket.get(7),
                status: bucket.get(8),
            })
            .collect();
        snapshots.push(AccountSnapshot {
            account_id,
            balance_nano: row.get(1),
            reserved_nano: row.get(2),
            account_class,
            reconciliation_state,
            ledger,
            buckets,
        });
    }
    Ok(snapshots)
}

pub fn sqlite_funding_reconciliation_plan(conn: &Connection) -> Result<FundingReconciliationPlan> {
    let tx = conn.unchecked_transaction()?;
    let plan = build_plan(sqlite_snapshots(&tx)?);
    tx.commit()?;
    Ok(plan)
}

pub(crate) fn postgres_funding_reconciliation_plan(
    client: &mut Client,
) -> Result<FundingReconciliationPlan> {
    let mut tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .read_only(true)
        .start()?;
    let plan = build_plan(postgres_snapshots(&mut tx)?);
    tx.commit()?;
    Ok(plan)
}

fn ensure_approved(plan: &FundingReconciliationPlan, approved_digest: &str) -> Result<()> {
    if approved_digest.trim().is_empty() || approved_digest != plan.plan_digest {
        bail!(
            "funding reconciliation plan drift: approved digest {:?}, current digest {:?}",
            approved_digest,
            plan.plan_digest
        );
    }
    Ok(())
}

fn ensure_exception_authority(
    plan: &FundingReconciliationPlan,
    allow_exceptions: bool,
) -> Result<()> {
    let unresolved_exceptions = plan
        .accounts
        .iter()
        .filter(|account| account.target_reconciliation_state == ReconciliationState::Exception)
        .count();
    if !allow_exceptions && unresolved_exceptions > 0 {
        bail!(
            "funding reconciliation has {} unresolved exception-state accounts ({} new exceptions, {} blocked); review the report and pass the explicit exception authority only for a maintenance-approved partial apply",
            unresolved_exceptions,
            plan.exception_accounts,
            plan.blocked_accounts
        );
    }
    Ok(())
}

fn sqlite_apply_account(
    conn: &Connection,
    account: &FundingAccountReconciliationPlan,
    ts: i64,
) -> Result<i64> {
    if account.disposition == FundingReconciliationDisposition::Blocked {
        conn.execute(
            "UPDATE account_policy_bindings SET reconciliation_state='exception',updated_ts=?2 \
             WHERE account_id=?1",
            rusqlite::params![account.account_id, ts],
        )?;
        return Ok(0);
    }
    let mut inserted = 0;
    if account.disposition != FundingReconciliationDisposition::Replay {
        for bucket in &account.buckets {
            inserted += conn.execute(
                "INSERT INTO funding_buckets( \
                   bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,reserved_nano, \
                   spent_nano,version,status,created_ts,updated_ts \
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
                rusqlite::params![
                    bucket.bucket_id,
                    account.account_id,
                    bucket.source_type,
                    bucket.source_ref,
                    bucket.eligibility,
                    bucket.balance_nano,
                    bucket.reserved_nano,
                    bucket.spent_nano,
                    bucket.version,
                    bucket.status,
                    ts,
                ],
            )? as i64;
        }
    }
    let updated = conn.execute(
        "UPDATE account_policy_bindings SET reconciliation_state=?2,updated_ts=?3 WHERE account_id=?1",
        rusqlite::params![
            account.account_id,
            account.target_reconciliation_state.as_str(),
            ts
        ],
    )?;
    if updated != 1 {
        bail!("funding reconciliation lost account policy binding");
    }
    Ok(inserted)
}

fn postgres_apply_account<C: GenericClient>(
    client: &mut C,
    account: &FundingAccountReconciliationPlan,
    ts: i64,
) -> Result<i64> {
    if account.disposition == FundingReconciliationDisposition::Blocked {
        client.execute(
            "UPDATE account_policy_bindings SET reconciliation_state='exception',updated_ts=$2 \
             WHERE account_id=$1",
            &[&account.account_id, &ts],
        )?;
        return Ok(0);
    }
    let mut inserted = 0;
    if account.disposition != FundingReconciliationDisposition::Replay {
        for bucket in &account.buckets {
            inserted += client.execute(
                "INSERT INTO funding_buckets( \
                   bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,reserved_nano, \
                   spent_nano,version,status,created_ts,updated_ts \
                 ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$11)",
                &[
                    &bucket.bucket_id,
                    &account.account_id,
                    &bucket.source_type,
                    &bucket.source_ref,
                    &bucket.eligibility,
                    &bucket.balance_nano,
                    &bucket.reserved_nano,
                    &bucket.spent_nano,
                    &bucket.version,
                    &bucket.status,
                    &ts,
                ],
            )? as i64;
        }
    }
    let updated = client.execute(
        "UPDATE account_policy_bindings SET reconciliation_state=$2,updated_ts=$3 WHERE account_id=$1",
        &[
            &account.account_id,
            &account.target_reconciliation_state.as_str(),
            &ts,
        ],
    )?;
    if updated != 1 {
        bail!("funding reconciliation lost account policy binding");
    }
    Ok(inserted)
}

fn apply_report(
    plan: &FundingReconciliationPlan,
    inserted_buckets: i64,
) -> FundingReconciliationApplyReport {
    FundingReconciliationApplyReport {
        schema_version: FUNDING_RECONCILIATION_SCHEMA_VERSION,
        plan_digest: plan.plan_digest.clone(),
        inserted_buckets,
        verified_accounts: plan
            .accounts
            .iter()
            .filter(|account| account.target_reconciliation_state == ReconciliationState::Verified)
            .count() as i64,
        exception_accounts: plan
            .accounts
            .iter()
            .filter(|account| account.target_reconciliation_state == ReconciliationState::Exception)
            .count() as i64,
        blocked_accounts: plan.blocked_accounts,
        replay_accounts: plan.replay_accounts,
        fully_applied: plan.blocked_accounts == 0,
    }
}

pub fn sqlite_apply_funding_reconciliation(
    conn: &Connection,
    approved_plan_digest: &str,
    allow_exceptions: bool,
) -> Result<FundingReconciliationApplyReport> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let plan = build_plan(sqlite_snapshots(&tx)?);
    ensure_approved(&plan, approved_plan_digest)?;
    ensure_exception_authority(&plan, allow_exceptions)?;
    let ts = now();
    let mut inserted = 0;
    for account in &plan.accounts {
        inserted += sqlite_apply_account(&tx, account, ts)?;
    }
    tx.commit()?;
    Ok(apply_report(&plan, inserted))
}

pub(crate) fn postgres_apply_funding_reconciliation(
    client: &mut Client,
    approved_plan_digest: &str,
    allow_exceptions: bool,
) -> Result<FundingReconciliationApplyReport> {
    let mut tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()?;
    tx.query_one(
        "SELECT pg_advisory_xact_lock($1)",
        &[&POSTGRES_FUNDING_RECONCILIATION_LOCK],
    )?;
    let plan = build_plan(postgres_snapshots(&mut tx)?);
    ensure_approved(&plan, approved_plan_digest)?;
    ensure_exception_authority(&plan, allow_exceptions)?;
    let ts = now();
    let mut inserted = 0;
    for account in &plan.accounts {
        inserted += postgres_apply_account(&mut tx, account, ts)?;
    }
    tx.commit()?;
    Ok(apply_report(&plan, inserted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{account_create, account_topup, open};

    fn bind(conn: &Connection, account_id: &str, account_class: &str) {
        conn.execute(
            "INSERT INTO account_policy_bindings( \
               account_id,product_id,account_class,active_effective_version,policy_enforcement, \
               funding_enforcement,reconciliation_state,updated_ts \
             ) VALUES(?1,'main',?2,NULL,'shadow','legacy_single','pending',1)",
            rusqlite::params![account_id, account_class],
        )
        .unwrap();
    }

    fn add_charge(conn: &Connection, account_id: &str, amount: i64) {
        let balance: i64 = conn
            .query_row(
                "UPDATE accounts SET balance_nano=balance_nano-?2,spent_nano=spent_nano+?2 \
                 WHERE id=?1 RETURNING balance_nano",
                rusqlite::params![account_id, amount],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO ledger(account_id,kind,amount_nano,balance_after_nano,ts) \
             VALUES(?1,'charge',?2,?3,1)",
            rusqlite::params![account_id, amount, balance],
        )
        .unwrap();
    }

    #[test]
    fn reviewed_source_policy_has_a_stable_golden_identity() {
        assert_eq!(
            digest(&source_policy()),
            "sha256:d0ef9852c09db1c10d056675add87431656343d5c9bcdc11487c8fb294a89750"
        );
        assert!(is_paid_reference(
            AccountClass::B2c,
            Some("cryptomus:payment")
        ));
        assert!(is_paid_reference(
            AccountClass::OpenKeys,
            Some("openkeys:batch:key")
        ));
        assert!(!is_paid_reference(
            AccountClass::B2c,
            Some("openkeys:batch:key")
        ));
        assert!(!is_paid_reference(
            AccountClass::Service,
            Some("platega:payment")
        ));
    }

    #[test]
    fn sqlite_reconciles_welcome_free_first_and_replays_exact_apply() {
        let conn = open(":memory:").unwrap();
        account_create(&conn, "b2c", None, 10_000).unwrap();
        bind(&conn, "b2c", "b2c");
        account_topup(
            &conn,
            "b2c",
            WELCOME_TRACK_BONUS_NANO,
            Some("signup-bonus:user-1"),
        )
        .unwrap();
        account_topup(&conn, "b2c", 10_000_000_000, Some("platega:payment-1")).unwrap();
        add_charge(&conn, "b2c", 5_000_000_000);

        let plan = sqlite_funding_reconciliation_plan(&conn).unwrap();
        assert_eq!(plan.ready_accounts, 1);
        assert_eq!(plan.exception_accounts, 0);
        let account = &plan.accounts[0];
        let welcome = account
            .buckets
            .iter()
            .find(|bucket| bucket.source_type == "welcome_track_bonus")
            .unwrap();
        let paid = account
            .buckets
            .iter()
            .find(|bucket| bucket.source_type == "paid")
            .unwrap();
        assert_eq!(welcome.balance_nano, 0);
        assert_eq!(welcome.spent_nano, WELCOME_TRACK_BONUS_NANO);
        assert_eq!(paid.balance_nano, 9_000_000_000);
        assert_eq!(paid.spent_nano, 1_000_000_000);

        let applied = sqlite_apply_funding_reconciliation(&conn, &plan.plan_digest, false).unwrap();
        assert_eq!(applied.inserted_buckets, 2);
        assert!(applied.fully_applied);
        let replay = sqlite_funding_reconciliation_plan(&conn).unwrap();
        assert_eq!(replay.replay_accounts, 1);
        let replayed =
            sqlite_apply_funding_reconciliation(&conn, &replay.plan_digest, false).unwrap();
        assert_eq!(replayed.inserted_buckets, 0);
    }

    #[test]
    fn sqlite_quarantines_ambiguous_credit_and_requires_exception_authority() {
        let conn = open(":memory:").unwrap();
        account_create(&conn, "promo", None, 10_000).unwrap();
        bind(&conn, "promo", "b2c");
        account_topup(&conn, "promo", 2_000_000_000, Some("promo:legacy")).unwrap();

        let plan = sqlite_funding_reconciliation_plan(&conn).unwrap();
        assert_eq!(plan.exception_accounts, 1);
        assert!(plan.accounts[0]
            .issues
            .iter()
            .any(|issue| issue.code == FundingReconciliationIssueCode::AmbiguousCredit));
        assert!(sqlite_apply_funding_reconciliation(&conn, &plan.plan_digest, false).is_err());
        let applied = sqlite_apply_funding_reconciliation(&conn, &plan.plan_digest, true).unwrap();
        assert_eq!(applied.exception_accounts, 1);
        let restricted: i64 = conn
            .query_row(
                "SELECT balance_nano FROM funding_buckets WHERE account_id='promo' \
                 AND source_type='legacy_restricted'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restricted, 2_000_000_000);
    }

    #[test]
    fn sqlite_accepts_openkeys_credit_only_for_the_reviewed_account_class() {
        let conn = open(":memory:").unwrap();
        for (account_id, account_class) in [("openkeys", "openkeys"), ("b2c", "b2c")] {
            account_create(&conn, account_id, None, 10_000).unwrap();
            bind(&conn, account_id, account_class);
            account_topup(
                &conn,
                account_id,
                1_000_000_000,
                Some(&format!("openkeys:batch:{account_id}")),
            )
            .unwrap();
        }
        let plan = sqlite_funding_reconciliation_plan(&conn).unwrap();
        let openkeys = plan
            .accounts
            .iter()
            .find(|account| account.account_id == "openkeys")
            .unwrap();
        assert_eq!(
            openkeys.disposition,
            FundingReconciliationDisposition::Ready
        );
        assert_eq!(
            openkeys
                .buckets
                .iter()
                .find(|bucket| bucket.source_type == "paid")
                .unwrap()
                .balance_nano,
            1_000_000_000
        );
        let b2c = plan
            .accounts
            .iter()
            .find(|account| account.account_id == "b2c")
            .unwrap();
        assert_eq!(b2c.disposition, FundingReconciliationDisposition::Exception);
        assert_eq!(
            b2c.buckets
                .iter()
                .find(|bucket| bucket.source_type == "legacy_restricted")
                .unwrap()
                .balance_nano,
            1_000_000_000
        );
    }

    #[test]
    fn sqlite_rejects_approved_plan_after_money_drift() {
        let conn = open(":memory:").unwrap();
        account_create(&conn, "drift", None, 10_000).unwrap();
        bind(&conn, "drift", "b2c");
        account_topup(&conn, "drift", 10, Some("cryptomus:first")).unwrap();
        let plan = sqlite_funding_reconciliation_plan(&conn).unwrap();
        account_topup(&conn, "drift", 1, Some("cryptomus:second")).unwrap();
        assert!(sqlite_apply_funding_reconciliation(&conn, &plan.plan_digest, false).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM funding_buckets WHERE account_id='drift'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn sqlite_blocks_unbound_and_reserved_accounts_without_guessing() {
        let conn = open(":memory:").unwrap();
        account_create(&conn, "unbound", None, 10_000).unwrap();
        account_create(&conn, "reserved", None, 10_000).unwrap();
        bind(&conn, "reserved", "b2c");
        conn.execute(
            "UPDATE accounts SET reserved_nano=1 WHERE id='reserved'",
            [],
        )
        .unwrap();
        let plan = sqlite_funding_reconciliation_plan(&conn).unwrap();
        assert_eq!(plan.blocked_accounts, 2);
        assert!(plan
            .accounts
            .iter()
            .all(|account| account.buckets.is_empty()));
    }
}
