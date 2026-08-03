//! Online, account-local transition from the legacy aggregate balance to funding v2.
//!
//! Planning is read-only and content-addressed. Apply serializes with every PostgreSQL money
//! writer through the same account advisory lock, rebuilds the plan, and inserts the generation,
//! lots, and head in one transaction. A legacy in-flight reservation blocks only its own account;
//! no global drain or maintenance window is part of this contract.

use anyhow::{bail, Context, Result};
use postgres::{Client, GenericClient, IsolationLevel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FUNDING_NORMALIZATION_SCHEMA_VERSION_V2: i64 = 2;
const INITIAL_FUNDING_GENERATION_V2: i64 = 1;
const INITIAL_FUNDING_HEAD_VERSION_V2: i64 = 1;
const INITIAL_ROW_VERSION_V2: i64 = 1;
const PAID_RESIDUAL_SOURCE_REF_V2: &str = "stage6:paid-residual:v2";
const SOURCE_DIGEST_DOMAIN_V2: &[u8] = b"apitoken:funding-normalization-source:v2\0";
const TARGET_DIGEST_DOMAIN_V2: &[u8] = b"apitoken:funding-normalization-target:v2\0";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingNormalizationPlanStatusV2 {
    Ready,
    Blocked,
    Normalized,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingNormalizationSourceV2 {
    AggregatePaidOnly,
    LedgerReplay,
    LegacyBuckets,
    StoredGeneration,
}

impl FundingNormalizationSourceV2 {
    fn as_str(self) -> &'static str {
        match self {
            Self::AggregatePaidOnly => "aggregate_paid_only",
            Self::LedgerReplay => "ledger_replay",
            Self::LegacyBuckets => "legacy_buckets",
            Self::StoredGeneration => "stored_generation",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingNormalizationBlockerCodeV2 {
    AccountDeleted,
    ActiveLegacyReservation,
    AggregateReservationMismatch,
    OrphanedFundingV2State,
    LegacyBucketMismatch,
    InvalidLedgerEvidence,
    ArithmeticOverflow,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingNormalizationBlockerV2 {
    pub code: FundingNormalizationBlockerCodeV2,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingNormalizationLotV2 {
    pub lot_id: String,
    pub source_type: String,
    pub source_ref: String,
    pub balance_nano: i64,
    pub reserved_nano: i64,
    pub spent_nano: i64,
    pub version: i64,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingNormalizationPlanV2 {
    pub account_id: String,
    pub account_status: String,
    pub status: FundingNormalizationPlanStatusV2,
    pub source: FundingNormalizationSourceV2,
    pub source_state_digest: String,
    pub normalization_digest: Option<String>,
    pub funding_generation: Option<i64>,
    pub funding_head_version: Option<i64>,
    pub balance_nano: i64,
    pub reserved_nano: i64,
    pub spent_nano: i64,
    pub lots: Vec<FundingNormalizationLotV2>,
    pub blockers: Vec<FundingNormalizationBlockerV2>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingNormalizationApplyRequestV2 {
    pub expected_source_state_digest: String,
    pub expected_normalization_digest: String,
}

impl FundingNormalizationApplyRequestV2 {
    pub fn validate(&self) -> Result<()> {
        validate_digest(
            "expected funding normalization source digest",
            &self.expected_source_state_digest,
        )?;
        validate_digest(
            "expected funding normalization target digest",
            &self.expected_normalization_digest,
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingNormalizationApplyStatusV2 {
    Stored,
    Unchanged,
    Stale,
    Blocked,
    Conflict,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FundingNormalizationApplyResultV2 {
    pub status: FundingNormalizationApplyStatusV2,
    pub normalization: FundingNormalizationPlanV2,
}

#[derive(Clone, Debug)]
struct AccountState {
    account_id: String,
    status: String,
    balance_nano: i64,
    reserved_nano: i64,
    spent_nano: i64,
    legacy_funding_enforcement: Option<String>,
    legacy_buckets: Vec<LegacyBucketState>,
    ledger: Vec<LedgerState>,
    active_reservations: Vec<ReservationState>,
    orphaned_generations: i64,
}

#[derive(Clone, Debug)]
struct LegacyBucketState {
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

#[derive(Clone, Debug)]
struct LedgerState {
    id: i64,
    kind: String,
    amount_nano: i64,
    reference: Option<String>,
    balance_after_nano: Option<i64>,
}

#[derive(Clone, Debug)]
struct ReservationState {
    request_id: String,
    state: String,
    hold_nano: i64,
    has_funding_snapshot_v2: bool,
}

#[derive(Clone, Debug)]
struct WelcomeLotState {
    source_ref: String,
    credited_nano: i64,
    balance_nano: i64,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn validate_digest(label: &str, value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:v2:") else {
        bail!("{label} must use sha256:v2 identity");
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("{label} must contain 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn digest_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn digest_string(hasher: &mut Sha256, value: &str) {
    digest_bytes(hasher, value.as_bytes());
}

fn digest_optional_string(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            digest_string(hasher, value);
        }
        None => hasher.update([0]),
    }
}

fn digest_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_be_bytes());
}

fn finish_digest(hasher: Sha256) -> String {
    let bytes = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("sha256:v2:{hex}")
}

fn source_state_digest(state: &AccountState) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SOURCE_DIGEST_DOMAIN_V2);
    digest_i64(&mut hasher, FUNDING_NORMALIZATION_SCHEMA_VERSION_V2);
    digest_string(&mut hasher, &state.account_id);
    digest_string(&mut hasher, &state.status);
    digest_i64(&mut hasher, state.balance_nano);
    digest_i64(&mut hasher, state.reserved_nano);
    digest_i64(&mut hasher, state.spent_nano);
    digest_optional_string(&mut hasher, state.legacy_funding_enforcement.as_deref());
    digest_i64(&mut hasher, state.orphaned_generations);

    digest_i64(&mut hasher, state.legacy_buckets.len() as i64);
    for bucket in &state.legacy_buckets {
        digest_string(&mut hasher, &bucket.bucket_id);
        digest_string(&mut hasher, &bucket.source_type);
        digest_string(&mut hasher, &bucket.source_ref);
        digest_string(&mut hasher, &bucket.eligibility);
        digest_i64(&mut hasher, bucket.balance_nano);
        digest_i64(&mut hasher, bucket.reserved_nano);
        digest_i64(&mut hasher, bucket.spent_nano);
        digest_i64(&mut hasher, bucket.version);
        digest_string(&mut hasher, &bucket.status);
    }

    digest_i64(&mut hasher, state.ledger.len() as i64);
    for entry in &state.ledger {
        digest_i64(&mut hasher, entry.id);
        digest_string(&mut hasher, &entry.kind);
        digest_i64(&mut hasher, entry.amount_nano);
        digest_optional_string(&mut hasher, entry.reference.as_deref());
        match entry.balance_after_nano {
            Some(value) => {
                hasher.update([1]);
                digest_i64(&mut hasher, value);
            }
            None => hasher.update([0]),
        }
    }

    digest_i64(&mut hasher, state.active_reservations.len() as i64);
    for reservation in &state.active_reservations {
        digest_string(&mut hasher, &reservation.request_id);
        digest_string(&mut hasher, &reservation.state);
        digest_i64(&mut hasher, reservation.hold_nano);
        hasher.update([u8::from(reservation.has_funding_snapshot_v2)]);
    }
    finish_digest(hasher)
}

fn normalization_digest(
    state: &AccountState,
    source: FundingNormalizationSourceV2,
    source_digest: &str,
    lots: &[FundingNormalizationLotV2],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(TARGET_DIGEST_DOMAIN_V2);
    digest_i64(&mut hasher, FUNDING_NORMALIZATION_SCHEMA_VERSION_V2);
    digest_string(&mut hasher, &state.account_id);
    digest_string(&mut hasher, source.as_str());
    digest_string(&mut hasher, source_digest);
    digest_i64(&mut hasher, INITIAL_FUNDING_GENERATION_V2);
    digest_i64(&mut hasher, INITIAL_FUNDING_HEAD_VERSION_V2);
    digest_i64(&mut hasher, state.balance_nano);
    digest_i64(&mut hasher, state.reserved_nano);
    digest_i64(&mut hasher, state.spent_nano);
    digest_i64(&mut hasher, lots.len() as i64);
    for lot in lots {
        digest_string(&mut hasher, &lot.lot_id);
        digest_string(&mut hasher, &lot.source_type);
        digest_string(&mut hasher, &lot.source_ref);
        digest_i64(&mut hasher, lot.balance_nano);
        digest_i64(&mut hasher, lot.reserved_nano);
        digest_i64(&mut hasher, lot.spent_nano);
        digest_i64(&mut hasher, lot.version);
        digest_string(&mut hasher, &lot.status);
    }
    finish_digest(hasher)
}

fn blocker(
    code: FundingNormalizationBlockerCodeV2,
    detail: impl Into<String>,
) -> FundingNormalizationBlockerV2 {
    FundingNormalizationBlockerV2 {
        code,
        detail: detail.into(),
    }
}

fn checked_i128_to_i64(label: &str, value: i128) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{label} exceeds the nanoUSD integer range"))
}

fn lot(
    account_id: &str,
    source_type: &str,
    source_ref: &str,
    balance_nano: i64,
    reserved_nano: i64,
    spent_nano: i64,
) -> FundingNormalizationLotV2 {
    FundingNormalizationLotV2 {
        lot_id: crate::funding_v2::funding_lot_id_v2(
            account_id,
            INITIAL_FUNDING_GENERATION_V2,
            source_type,
            source_ref,
        ),
        source_type: source_type.to_owned(),
        source_ref: source_ref.to_owned(),
        balance_nano,
        reserved_nano,
        spent_nano,
        version: INITIAL_ROW_VERSION_V2,
        status: if balance_nano > 0 {
            "active".to_owned()
        } else {
            "exhausted".to_owned()
        },
    }
}

fn consume_welcome(lots: &mut [WelcomeLotState], mut amount_nano: i64) -> Result<i64> {
    if amount_nano < 0 {
        bail!("welcome funding consumption cannot be negative");
    }
    let mut consumed = 0_i64;
    for lot in lots {
        if amount_nano == 0 {
            break;
        }
        let amount = amount_nano.min(lot.balance_nano);
        lot.balance_nano -= amount;
        amount_nano -= amount;
        consumed = consumed
            .checked_add(amount)
            .context("welcome funding consumption overflow")?;
    }
    Ok(consumed)
}

fn every_welcome_grant_was_exactly_revoked(ledger: &[LedgerState]) -> Result<bool> {
    let mut grants = BTreeMap::<String, i64>::new();
    let mut revocations = BTreeMap::<String, i64>::new();
    for entry in ledger {
        let Some(reference) = entry.reference.as_deref() else {
            continue;
        };
        if entry.kind == "topup" && entry.amount_nano > 0 {
            if let Some(subject) = reference.strip_prefix("signup-bonus:") {
                if subject.is_empty()
                    || grants
                        .insert(subject.to_owned(), entry.amount_nano)
                        .is_some()
                {
                    bail!("welcome top-up source reference is not unique");
                }
            }
        } else if entry.kind == "adjust" {
            if let Some(subject) = reference.strip_prefix("bonus-revoke:") {
                let revoked = entry
                    .amount_nano
                    .checked_neg()
                    .context("welcome bonus revocation cannot be negated")?;
                if subject.is_empty() || revoked <= 0 {
                    bail!("welcome bonus revocation has an invalid identity or amount");
                }
                if revocations.insert(subject.to_owned(), revoked).is_some() {
                    bail!("welcome bonus revocation source reference is not unique");
                }
            }
        }
    }
    if revocations.is_empty() {
        return Ok(false);
    }
    if grants.len() != revocations.len()
        || grants
            .iter()
            .any(|(subject, granted)| revocations.get(subject) != Some(granted))
    {
        bail!("welcome bonus revocation does not exactly match every retained grant");
    }
    Ok(!grants.is_empty())
}

fn target_from_legacy_buckets(state: &AccountState) -> Result<Vec<FundingNormalizationLotV2>> {
    let active: Vec<_> = state
        .legacy_buckets
        .iter()
        .filter(|bucket| bucket.status != "retired")
        .collect();
    if active.is_empty() {
        bail!("legacy funding rows exist without an active or exhausted bucket");
    }
    if state.legacy_buckets.iter().any(|bucket| {
        bucket.status == "retired"
            && (bucket.balance_nano != 0 || bucket.reserved_nano != 0 || bucket.spent_nano != 0)
    }) {
        bail!("retired legacy funding bucket retains monetary state");
    }

    let sum = |field: fn(&LegacyBucketState) -> i64, label: &str| -> Result<i64> {
        let total = active.iter().try_fold(0_i128, |total, bucket| {
            total
                .checked_add(i128::from(field(bucket)))
                .context("legacy funding aggregate overflow")
        })?;
        checked_i128_to_i64(label, total)
    };
    if sum(|bucket| bucket.balance_nano, "legacy funding balance")? != state.balance_nano
        || sum(|bucket| bucket.reserved_nano, "legacy funding reserved")? != state.reserved_nano
        || sum(|bucket| bucket.spent_nano, "legacy funding spent")? != state.spent_nano
    {
        bail!("legacy funding buckets do not match account aggregates");
    }

    let mut lots = Vec::new();
    let mut welcome_balance = 0_i64;
    let mut welcome_reserved = 0_i64;
    let mut welcome_spent = 0_i64;
    let mut refs = BTreeSet::new();
    for bucket in active.iter().filter(|bucket| {
        matches!(
            bucket.source_type.as_str(),
            "welcome_track_bonus" | "welcome_bonus"
        )
    }) {
        if bucket.balance_nano < 0 || bucket.reserved_nano < 0 || bucket.spent_nano < 0 {
            bail!("legacy welcome bucket has negative monetary state");
        }
        if !refs.insert(bucket.source_ref.as_str()) {
            bail!("legacy welcome buckets reuse one source reference");
        }
        welcome_balance = welcome_balance
            .checked_add(bucket.balance_nano)
            .context("legacy welcome balance overflow")?;
        welcome_reserved = welcome_reserved
            .checked_add(bucket.reserved_nano)
            .context("legacy welcome reserved overflow")?;
        welcome_spent = welcome_spent
            .checked_add(bucket.spent_nano)
            .context("legacy welcome spent overflow")?;
        lots.push(lot(
            &state.account_id,
            "welcome_bonus",
            &bucket.source_ref,
            bucket.balance_nano,
            bucket.reserved_nano,
            bucket.spent_nano,
        ));
    }

    let paid_balance = state
        .balance_nano
        .checked_sub(welcome_balance)
        .context("paid residual balance overflow")?;
    let paid_reserved = state
        .reserved_nano
        .checked_sub(welcome_reserved)
        .context("paid residual reserved overflow")?;
    let paid_spent = state
        .spent_nano
        .checked_sub(welcome_spent)
        .context("paid residual spent overflow")?;
    if paid_reserved < 0 || paid_spent < 0 {
        bail!("legacy welcome funding exceeds account reserved or spent aggregates");
    }
    lots.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    lots.push(lot(
        &state.account_id,
        "paid",
        PAID_RESIDUAL_SOURCE_REF_V2,
        paid_balance,
        paid_reserved,
        paid_spent,
    ));
    Ok(lots)
}

fn target_from_ledger(state: &AccountState) -> Result<Vec<FundingNormalizationLotV2>> {
    // A full admin revocation removes the welcome entitlement rather than spending it. Once every
    // retained grant has an exact same-subject/full-amount revocation, no historical balance gap is
    // needed to attribute the current aggregate: the complete live residual is paid authority.
    // Partial, mismatched, duplicate, or mixed active/revoked evidence remains fail-closed.
    if every_welcome_grant_was_exactly_revoked(&state.ledger)? {
        return Ok(vec![lot(
            &state.account_id,
            "paid",
            PAID_RESIDUAL_SOURCE_REF_V2,
            state.balance_nano,
            state.reserved_nano,
            state.spent_nano,
        )]);
    }
    let mut welcome = Vec::<WelcomeLotState>::new();
    let mut seen_refs = BTreeSet::new();
    let mut previous_after: Option<i64> = None;
    let mut reconstructed_spend = 0_i64;

    for entry in &state.ledger {
        let after = entry
            .balance_after_nano
            .context("funding-relevant ledger row lacks balance_after_nano")?;
        let effect = match entry.kind.as_str() {
            "topup" if entry.amount_nano >= 0 => entry.amount_nano,
            "adjust" => entry.amount_nano,
            "charge" if entry.amount_nano >= 0 => entry
                .amount_nano
                .checked_neg()
                .context("ledger charge cannot be negated")?,
            _ => bail!("funding-relevant ledger row has unsupported kind or sign"),
        };
        let before = after
            .checked_sub(effect)
            .context("ledger balance transition overflow")?;
        if let Some(previous_after) = previous_after {
            let gap = before
                .checked_sub(previous_after)
                .context("ledger balance gap overflow")?;
            if gap < 0 {
                let charge = gap.checked_neg().context("ledger charge gap overflow")?;
                reconstructed_spend = reconstructed_spend
                    .checked_add(charge)
                    .context("reconstructed spend overflow")?;
                consume_welcome(&mut welcome, charge)?;
            }
        }

        if entry.kind == "topup"
            && entry.amount_nano > 0
            && entry
                .reference
                .as_deref()
                .is_some_and(|reference| reference.starts_with("signup-bonus:"))
        {
            let source_ref = entry
                .reference
                .clone()
                .context("welcome top-up lacks its source reference")?;
            if !seen_refs.insert(source_ref.clone()) {
                bail!("welcome top-up source reference is not unique");
            }
            welcome.push(WelcomeLotState {
                source_ref,
                credited_nano: entry.amount_nano,
                balance_nano: entry.amount_nano,
            });
        } else if entry.kind == "charge" {
            reconstructed_spend = reconstructed_spend
                .checked_add(entry.amount_nano)
                .context("reconstructed spend overflow")?;
            consume_welcome(&mut welcome, entry.amount_nano)?;
        }
        previous_after = Some(after);
    }

    if let Some(previous_after) = previous_after {
        let final_gap = state
            .balance_nano
            .checked_sub(previous_after)
            .context("final ledger balance gap overflow")?;
        if final_gap < 0 {
            let charge = final_gap
                .checked_neg()
                .context("final ledger charge gap overflow")?;
            reconstructed_spend = reconstructed_spend
                .checked_add(charge)
                .context("reconstructed spend overflow")?;
            consume_welcome(&mut welcome, charge)?;
        }
    }
    if reconstructed_spend > state.spent_nano {
        bail!("reconstructed post-welcome spend exceeds the account spent aggregate");
    }

    let mut lots = Vec::new();
    let mut welcome_balance = 0_i64;
    let mut welcome_spent = 0_i64;
    for welcome_lot in welcome {
        let spent = welcome_lot
            .credited_nano
            .checked_sub(welcome_lot.balance_nano)
            .context("welcome spent overflow")?;
        welcome_balance = welcome_balance
            .checked_add(welcome_lot.balance_nano)
            .context("welcome balance overflow")?;
        welcome_spent = welcome_spent
            .checked_add(spent)
            .context("welcome spent overflow")?;
        lots.push(lot(
            &state.account_id,
            "welcome_bonus",
            &welcome_lot.source_ref,
            welcome_lot.balance_nano,
            0,
            spent,
        ));
    }
    let paid_balance = state
        .balance_nano
        .checked_sub(welcome_balance)
        .context("paid residual balance overflow")?;
    let paid_spent = state
        .spent_nano
        .checked_sub(welcome_spent)
        .context("paid residual spent overflow")?;
    lots.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    lots.push(lot(
        &state.account_id,
        "paid",
        PAID_RESIDUAL_SOURCE_REF_V2,
        paid_balance,
        0,
        paid_spent,
    ));
    Ok(lots)
}

fn load_account_state<C: GenericClient>(
    client: &mut C,
    account_id: &str,
) -> Result<Option<AccountState>> {
    let Some(account) = client.query_opt(
        "SELECT account.id,account.status,account.balance_nano,account.reserved_nano,
                account.spent_nano,binding.funding_enforcement
           FROM accounts account
           LEFT JOIN account_policy_bindings binding ON binding.account_id=account.id
          WHERE account.id=$1",
        &[&account_id],
    )?
    else {
        return Ok(None);
    };

    let legacy_buckets = client
        .query(
            "SELECT bucket_id,source_type,source_ref,eligibility,balance_nano,reserved_nano,
                    spent_nano,version,status
               FROM funding_buckets WHERE account_id=$1 ORDER BY bucket_id",
            &[&account_id],
        )?
        .into_iter()
        .map(|row| LegacyBucketState {
            bucket_id: row.get(0),
            source_type: row.get(1),
            source_ref: row.get(2),
            eligibility: row.get(3),
            balance_nano: row.get(4),
            reserved_nano: row.get(5),
            spent_nano: row.get(6),
            version: row.get(7),
            status: row.get(8),
        })
        .collect();

    let first_welcome_id: Option<i64> = client
        .query_one(
            "SELECT min(id)
               FROM ledger
              WHERE account_id=$1 AND kind='topup' AND amount_nano>0
                AND ref LIKE 'signup-bonus:%'",
            &[&account_id],
        )?
        .get(0);
    let ledger = if let Some(first_welcome_id) = first_welcome_id {
        // Charge history can be very large and is retention-prunable. Every retained top-up or
        // adjustment carries balance_after_nano, so the exact total charge between two such rows
        // is their negative balance gap; the final gap closes against the live aggregate.
        client
            .query(
                "SELECT id,kind,amount_nano,ref,balance_after_nano
                   FROM ledger
                  WHERE account_id=$1 AND id >= $2
                    AND kind IN ('topup','adjust')
                  ORDER BY id",
                &[&account_id, &first_welcome_id],
            )?
            .into_iter()
            .map(|row| LedgerState {
                id: row.get(0),
                kind: row.get(1),
                amount_nano: row.get(2),
                reference: row.get(3),
                balance_after_nano: row.get(4),
            })
            .collect()
    } else {
        Vec::new()
    };

    let active_reservations = client
        .query(
            "SELECT reservation.request_id,reservation.state,reservation.hold_nano,
                    snapshot.request_id IS NOT NULL
               FROM reservations reservation
               LEFT JOIN funding_reservation_snapshots_v2 snapshot
                 ON snapshot.request_id=reservation.request_id
              WHERE reservation.account_id=$1
                AND reservation.state IN ('reserved','delivering','settlement_pending')
              ORDER BY reservation.request_id",
            &[&account_id],
        )?
        .into_iter()
        .map(|row| ReservationState {
            request_id: row.get(0),
            state: row.get(1),
            hold_nano: row.get(2),
            has_funding_snapshot_v2: row.get(3),
        })
        .collect();
    let orphaned_generations: i64 = client
        .query_one(
            "SELECT count(*)::bigint
               FROM account_funding_generations_v2 generation
              WHERE generation.account_id=$1
                AND NOT EXISTS(
                    SELECT 1 FROM account_funding_head_v2 head
                     WHERE head.account_id=generation.account_id
                       AND head.active_generation=generation.generation
                )",
            &[&account_id],
        )?
        .get(0);

    Ok(Some(AccountState {
        account_id: account.get(0),
        status: account.get(1),
        balance_nano: account.get(2),
        reserved_nano: account.get(3),
        spent_nano: account.get(4),
        legacy_funding_enforcement: account.get(5),
        legacy_buckets,
        ledger,
        active_reservations,
        orphaned_generations,
    }))
}

fn load_normalized_plan<C: GenericClient>(
    client: &mut C,
    account_id: &str,
) -> Result<Option<FundingNormalizationPlanV2>> {
    let Some(row) = client.query_opt(
        "SELECT account.status,account.balance_nano,account.reserved_nano,account.spent_nano,
                head.active_generation,head.head_version,generation.source_state_digest,
                generation.normalization_digest,generation.balance_nano,
                generation.reserved_nano,generation.spent_nano
           FROM account_funding_head_v2 head
           JOIN accounts account ON account.id=head.account_id
           JOIN account_funding_generations_v2 generation
             ON generation.account_id=head.account_id
            AND generation.generation=head.active_generation
          WHERE head.account_id=$1",
        &[&account_id],
    )?
    else {
        return Ok(None);
    };
    let generation: i64 = row.get(4);
    let lots: Vec<FundingNormalizationLotV2> = client
        .query(
            "SELECT lot_id,source_type,source_ref,balance_nano,reserved_nano,spent_nano,
                    version,status
               FROM funding_lots_v2
              WHERE account_id=$1 AND funding_generation=$2 AND status<>'retired'
              ORDER BY CASE source_type WHEN 'welcome_bonus' THEN 0 ELSE 1 END,
                       source_ref,lot_id",
            &[&account_id, &generation],
        )?
        .into_iter()
        .map(|lot| FundingNormalizationLotV2 {
            lot_id: lot.get(0),
            source_type: lot.get(1),
            source_ref: lot.get(2),
            balance_nano: lot.get(3),
            reserved_nano: lot.get(4),
            spent_nano: lot.get(5),
            version: lot.get(6),
            status: lot.get(7),
        })
        .collect();
    let lot_totals = lots
        .iter()
        .try_fold((0_i128, 0_i128, 0_i128), |totals, lot| {
            Ok::<_, anyhow::Error>((
                totals
                    .0
                    .checked_add(i128::from(lot.balance_nano))
                    .context("stored funding balance overflow")?,
                totals
                    .1
                    .checked_add(i128::from(lot.reserved_nano))
                    .context("stored funding reserved overflow")?,
                totals
                    .2
                    .checked_add(i128::from(lot.spent_nano))
                    .context("stored funding spent overflow")?,
            ))
        })?;
    let account_totals = (
        row.get::<_, i64>(1),
        row.get::<_, i64>(2),
        row.get::<_, i64>(3),
    );
    let generation_totals = (
        row.get::<_, i64>(8),
        row.get::<_, i64>(9),
        row.get::<_, i64>(10),
    );
    if lot_totals
        != (
            i128::from(account_totals.0),
            i128::from(account_totals.1),
            i128::from(account_totals.2),
        )
        || account_totals != generation_totals
    {
        bail!("stored active funding generation does not match account aggregates");
    }
    if !lots.iter().any(|lot| lot.source_type == "paid") {
        bail!("stored active funding generation lacks the paid anchor");
    }
    let source_state_digest: String = row.get(6);
    let normalization_digest: String = row.get(7);
    validate_digest(
        "stored funding normalization source digest",
        &source_state_digest,
    )?;
    validate_digest(
        "stored funding normalization target digest",
        &normalization_digest,
    )?;
    Ok(Some(FundingNormalizationPlanV2 {
        account_id: account_id.to_owned(),
        account_status: row.get(0),
        status: FundingNormalizationPlanStatusV2::Normalized,
        source: FundingNormalizationSourceV2::StoredGeneration,
        source_state_digest,
        normalization_digest: Some(normalization_digest),
        funding_generation: Some(generation),
        funding_head_version: Some(row.get(5)),
        balance_nano: account_totals.0,
        reserved_nano: account_totals.1,
        spent_nano: account_totals.2,
        lots,
        blockers: Vec::new(),
    }))
}

fn build_plan<C: GenericClient>(
    client: &mut C,
    account_id: &str,
) -> Result<Option<FundingNormalizationPlanV2>> {
    if let Some(plan) = load_normalized_plan(client, account_id)? {
        return Ok(Some(plan));
    }
    let Some(state) = load_account_state(client, account_id)? else {
        return Ok(None);
    };
    let source_digest = source_state_digest(&state);
    let mut blockers = Vec::new();
    if state.status == "deleted" {
        blockers.push(blocker(
            FundingNormalizationBlockerCodeV2::AccountDeleted,
            "deleted account is outside the target release inventory",
        ));
    }
    if state.orphaned_generations != 0 {
        blockers.push(blocker(
            FundingNormalizationBlockerCodeV2::OrphanedFundingV2State,
            "account has a funding v2 generation without its active head",
        ));
    }
    if state
        .active_reservations
        .iter()
        .any(|reservation| !reservation.has_funding_snapshot_v2)
    {
        blockers.push(blocker(
            FundingNormalizationBlockerCodeV2::ActiveLegacyReservation,
            format!(
                "{} active legacy reservation(s) must terminalize for this account",
                state
                    .active_reservations
                    .iter()
                    .filter(|reservation| !reservation.has_funding_snapshot_v2)
                    .count()
            ),
        ));
    }
    let active_reserved_nano =
        state
            .active_reservations
            .iter()
            .try_fold(0_i128, |total, reservation| {
                total
                    .checked_add(i128::from(reservation.hold_nano))
                    .context("active reservation aggregate overflow")
            });
    if active_reserved_nano
        .as_ref()
        .is_ok_and(|reserved| *reserved != i128::from(state.reserved_nano))
    {
        blockers.push(blocker(
            FundingNormalizationBlockerCodeV2::AggregateReservationMismatch,
            "account reserved aggregate does not match active reservation holds",
        ));
    } else if let Err(error) = active_reserved_nano {
        blockers.push(blocker(
            FundingNormalizationBlockerCodeV2::ArithmeticOverflow,
            format!("{error:#}"),
        ));
    }

    let mut source = if state.ledger.is_empty() {
        FundingNormalizationSourceV2::AggregatePaidOnly
    } else {
        FundingNormalizationSourceV2::LedgerReplay
    };
    let mut lots = Vec::new();
    if blockers.is_empty() {
        let legacy_target =
            (!state.legacy_buckets.is_empty()).then(|| target_from_legacy_buckets(&state));
        let target = match legacy_target {
            Some(Ok(target)) => {
                source = FundingNormalizationSourceV2::LegacyBuckets;
                Ok(target)
            }
            Some(Err(error)) if state.legacy_funding_enforcement.as_deref() == Some("strict") => {
                Err(error)
            }
            Some(Err(_)) | None
                if state.legacy_funding_enforcement.as_deref() == Some("strict") =>
            {
                Err(anyhow::anyhow!(
                    "strict legacy funding authority has no coherent buckets"
                ))
            }
            Some(Err(_)) | None if source == FundingNormalizationSourceV2::LedgerReplay => {
                target_from_ledger(&state)
            }
            Some(Err(_)) | None => Ok(vec![lot(
                &state.account_id,
                "paid",
                PAID_RESIDUAL_SOURCE_REF_V2,
                state.balance_nano,
                state.reserved_nano,
                state.spent_nano,
            )]),
        };
        match target {
            Ok(target_lots) => lots = target_lots,
            Err(error) => {
                let message = format!("{error:#}");
                let code = if message.to_ascii_lowercase().contains("overflow") {
                    FundingNormalizationBlockerCodeV2::ArithmeticOverflow
                } else if state.legacy_funding_enforcement.as_deref() == Some("strict") {
                    FundingNormalizationBlockerCodeV2::LegacyBucketMismatch
                } else {
                    FundingNormalizationBlockerCodeV2::InvalidLedgerEvidence
                };
                blockers.push(blocker(code, message));
            }
        }
    }

    let (status, target_digest, generation, head_version) = if blockers.is_empty() {
        (
            FundingNormalizationPlanStatusV2::Ready,
            Some(normalization_digest(&state, source, &source_digest, &lots)),
            Some(INITIAL_FUNDING_GENERATION_V2),
            Some(INITIAL_FUNDING_HEAD_VERSION_V2),
        )
    } else {
        (FundingNormalizationPlanStatusV2::Blocked, None, None, None)
    };
    Ok(Some(FundingNormalizationPlanV2 {
        account_id: state.account_id,
        account_status: state.status,
        status,
        source,
        source_state_digest: source_digest,
        normalization_digest: target_digest,
        funding_generation: generation,
        funding_head_version: head_version,
        balance_nano: state.balance_nano,
        reserved_nano: state.reserved_nano,
        spent_nano: state.spent_nano,
        lots,
        blockers,
    }))
}

pub fn postgres_funding_normalization_plan_v2(
    client: &mut Client,
    account_id: &str,
) -> Result<Option<FundingNormalizationPlanV2>> {
    if account_id.trim().is_empty() {
        bail!("funding normalization requires an account id");
    }
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .context("begin funding normalization read snapshot")?;
    let plan = build_plan(&mut transaction, account_id)?;
    transaction.commit()?;
    Ok(plan)
}

pub fn postgres_apply_funding_normalization_v2(
    client: &mut Client,
    account_id: &str,
    request: &FundingNormalizationApplyRequestV2,
) -> Result<Option<FundingNormalizationApplyResultV2>> {
    if account_id.trim().is_empty() {
        bail!("funding normalization requires an account id");
    }
    request.validate()?;
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::Serializable)
        .start()
        .context("begin account-local funding normalization")?;
    crate::funding_v2::lock_funding_account_v2(&mut transaction, account_id)?;
    let Some(plan) = build_plan(&mut transaction, account_id)? else {
        transaction.rollback()?;
        return Ok(None);
    };
    if plan.status == FundingNormalizationPlanStatusV2::Normalized {
        let status = if plan.source_state_digest == request.expected_source_state_digest
            && plan.normalization_digest.as_deref()
                == Some(request.expected_normalization_digest.as_str())
        {
            FundingNormalizationApplyStatusV2::Unchanged
        } else {
            FundingNormalizationApplyStatusV2::Conflict
        };
        transaction.commit()?;
        return Ok(Some(FundingNormalizationApplyResultV2 {
            status,
            normalization: plan,
        }));
    }
    if plan.status == FundingNormalizationPlanStatusV2::Blocked {
        transaction.commit()?;
        return Ok(Some(FundingNormalizationApplyResultV2 {
            status: FundingNormalizationApplyStatusV2::Blocked,
            normalization: plan,
        }));
    }
    if plan.source_state_digest != request.expected_source_state_digest
        || plan.normalization_digest.as_deref()
            != Some(request.expected_normalization_digest.as_str())
    {
        transaction.commit()?;
        return Ok(Some(FundingNormalizationApplyResultV2 {
            status: FundingNormalizationApplyStatusV2::Stale,
            normalization: plan,
        }));
    }

    let timestamp = now();
    let normalization_digest = plan
        .normalization_digest
        .as_deref()
        .context("ready normalization lacks target digest")?;
    transaction.execute(
        "INSERT INTO account_funding_generations_v2(
             account_id,generation,schema_version,source_state_digest,normalization_digest,
             balance_nano,reserved_nano,spent_nano,version,normalized_ts,updated_ts)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10)",
        &[
            &account_id,
            &INITIAL_FUNDING_GENERATION_V2,
            &FUNDING_NORMALIZATION_SCHEMA_VERSION_V2,
            &plan.source_state_digest,
            &normalization_digest,
            &plan.balance_nano,
            &plan.reserved_nano,
            &plan.spent_nano,
            &INITIAL_ROW_VERSION_V2,
            &timestamp,
        ],
    )?;
    for lot in &plan.lots {
        transaction.execute(
            "INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$11)",
            &[
                &lot.lot_id,
                &account_id,
                &INITIAL_FUNDING_GENERATION_V2,
                &lot.source_type,
                &lot.source_ref,
                &lot.balance_nano,
                &lot.reserved_nano,
                &lot.spent_nano,
                &lot.version,
                &lot.status,
                &timestamp,
            ],
        )?;
    }
    transaction.execute(
        "INSERT INTO account_funding_head_v2(
             account_id,active_generation,head_version,updated_ts)
         VALUES($1,$2,$3,$4)",
        &[
            &account_id,
            &INITIAL_FUNDING_GENERATION_V2,
            &INITIAL_FUNDING_HEAD_VERSION_V2,
            &timestamp,
        ],
    )?;
    transaction.commit()?;

    let stored = postgres_funding_normalization_plan_v2(client, account_id)?
        .context("stored funding normalization disappeared after commit")?;
    Ok(Some(FundingNormalizationApplyResultV2 {
        status: FundingNormalizationApplyStatusV2::Stored,
        normalization: stored,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg::{PgStore, POSTGRES_DESTRUCTIVE_TEST_LOCK};

    #[test]
    fn digest_validation_requires_canonical_v2_sha256() {
        let valid = format!("sha256:v2:{}", "a".repeat(64));
        assert!(validate_digest("test", &valid).is_ok());
        assert!(validate_digest("test", &valid.to_ascii_uppercase()).is_err());
        assert!(validate_digest("test", "sha256:v1:abc").is_err());
    }

    #[test]
    fn ledger_replay_spends_welcome_first_across_pruned_charge_gaps() {
        let state = AccountState {
            account_id: "account".into(),
            status: "active".into(),
            balance_nano: 9,
            reserved_nano: 0,
            spent_nano: 6,
            legacy_funding_enforcement: None,
            legacy_buckets: Vec::new(),
            ledger: vec![
                LedgerState {
                    id: 1,
                    kind: "topup".into(),
                    amount_nano: 4,
                    reference: Some("signup-bonus:user".into()),
                    balance_after_nano: Some(14),
                },
                LedgerState {
                    id: 3,
                    kind: "topup".into(),
                    amount_nano: 1,
                    reference: Some("platega:later".into()),
                    balance_after_nano: Some(9),
                },
            ],
            active_reservations: Vec::new(),
            orphaned_generations: 0,
        };
        let lots = target_from_ledger(&state).unwrap();
        assert_eq!(lots.len(), 2);
        assert_eq!(lots[0].source_type, "welcome_bonus");
        assert_eq!((lots[0].balance_nano, lots[0].spent_nano), (0, 4));
        assert_eq!(lots[1].source_type, "paid");
        assert_eq!((lots[1].balance_nano, lots[1].spent_nano), (9, 2));
    }

    #[test]
    fn exact_welcome_revocation_makes_the_current_aggregate_paid_only() {
        let state = AccountState {
            account_id: "revoked-bonus".into(),
            status: "active".into(),
            balance_nano: -3,
            reserved_nano: 0,
            spent_nano: 3,
            legacy_funding_enforcement: None,
            legacy_buckets: Vec::new(),
            ledger: vec![
                LedgerState {
                    id: 1,
                    kind: "topup".into(),
                    amount_nano: 4,
                    reference: Some("signup-bonus:user".into()),
                    balance_after_nano: Some(4),
                },
                LedgerState {
                    id: 2,
                    kind: "adjust".into(),
                    amount_nano: -4,
                    reference: Some("bonus-revoke:user".into()),
                    balance_after_nano: Some(-2),
                },
            ],
            active_reservations: Vec::new(),
            orphaned_generations: 0,
        };
        let lots = target_from_ledger(&state).unwrap();
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].source_type, "paid");
        assert_eq!(
            (
                lots[0].balance_nano,
                lots[0].reserved_nano,
                lots[0].spent_nano
            ),
            (-3, 0, 3)
        );
    }

    #[test]
    fn partial_welcome_revocation_remains_fail_closed() {
        let state = AccountState {
            account_id: "partial-revocation".into(),
            status: "active".into(),
            balance_nano: 1,
            reserved_nano: 0,
            spent_nano: 0,
            legacy_funding_enforcement: None,
            legacy_buckets: Vec::new(),
            ledger: vec![
                LedgerState {
                    id: 1,
                    kind: "topup".into(),
                    amount_nano: 4,
                    reference: Some("signup-bonus:user".into()),
                    balance_after_nano: Some(4),
                },
                LedgerState {
                    id: 2,
                    kind: "adjust".into(),
                    amount_nano: -3,
                    reference: Some("bonus-revoke:user".into()),
                    balance_after_nano: Some(1),
                },
            ],
            active_reservations: Vec::new(),
            orphaned_generations: 0,
        };
        assert!(target_from_ledger(&state)
            .unwrap_err()
            .to_string()
            .contains("does not exactly match every retained grant"));
    }

    #[test]
    fn paid_anchor_is_always_materialized() {
        let state = AccountState {
            account_id: "bonus-only".into(),
            status: "active".into(),
            balance_nano: 5,
            reserved_nano: 0,
            spent_nano: 0,
            legacy_funding_enforcement: None,
            legacy_buckets: Vec::new(),
            ledger: vec![LedgerState {
                id: 1,
                kind: "topup".into(),
                amount_nano: 5,
                reference: Some("signup-bonus:user".into()),
                balance_after_nano: Some(5),
            }],
            active_reservations: Vec::new(),
            orphaned_generations: 0,
        };
        let lots = target_from_ledger(&state).unwrap();
        assert_eq!(lots.len(), 2);
        assert_eq!(lots[1].source_type, "paid");
        assert_eq!(lots[1].balance_nano, 0);
    }

    /// Real PostgreSQL proof for the online account-local transition. The matrix is destructive
    /// only inside the dedicated test database and is skipped when its URL is absent.
    #[test]
    fn postgres_online_funding_normalization_v2_matrix() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL funding normalization v2 matrix: \
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
            .batch_execute("TRUNCATE accounts RESTART IDENTITY CASCADE")
            .unwrap();

        // An exact admin revocation removes the complete welcome entitlement even after part of
        // the grant was spent. The current aggregate is therefore one paid lot, including debt;
        // retained pre-revocation gaps are historical evidence, not a surviving bonus.
        pg.account_create("normalize-revoked", None, 10_000)
            .unwrap();
        pg.account_topup("normalize-revoked", 4, Some("signup-bonus:revoked-user"))
            .unwrap();
        pg.client
            .execute(
                "UPDATE accounts SET balance_nano=2,spent_nano=2
                  WHERE id='normalize-revoked'",
                &[],
            )
            .unwrap();
        pg.account_topup("normalize-revoked", -4, Some("bonus-revoke:revoked-user"))
            .unwrap();
        let revoked = pg
            .funding_normalization_plan_v2("normalize-revoked")
            .unwrap()
            .unwrap();
        assert_eq!(revoked.status, FundingNormalizationPlanStatusV2::Ready);
        assert_eq!(revoked.source, FundingNormalizationSourceV2::LedgerReplay);
        assert_eq!(
            revoked
                .lots
                .iter()
                .map(|lot| format!(
                    "{}:{}:{}",
                    lot.source_type, lot.balance_nano, lot.spent_nano
                ))
                .collect::<Vec<_>>(),
            vec!["paid:-2:2"]
        );
        let revoked_stored = pg
            .apply_funding_normalization_v2(
                "normalize-revoked",
                &FundingNormalizationApplyRequestV2 {
                    expected_source_state_digest: revoked.source_state_digest,
                    expected_normalization_digest: revoked.normalization_digest.unwrap(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            revoked_stored.status,
            FundingNormalizationApplyStatusV2::Stored
        );

        // The charge row is removed to model normal ledger retention. The next immutable top-up
        // balance still makes the post-welcome spend gap exact.
        pg.account_create("normalize-ledger", None, 10_000).unwrap();
        pg.account_topup("normalize-ledger", 10, Some("platega:before-bonus"))
            .unwrap();
        pg.account_topup("normalize-ledger", 4, Some("signup-bonus:normalize-user"))
            .unwrap();
        let after_charge: i64 = pg
            .client
            .query_one(
                "UPDATE accounts
                    SET balance_nano=balance_nano-6,spent_nano=spent_nano+6
                  WHERE id='normalize-ledger' RETURNING balance_nano",
                &[],
            )
            .unwrap()
            .get(0);
        let charge_id: i64 = pg
            .client
            .query_one(
                "INSERT INTO ledger(
                     account_id,kind,amount_nano,balance_after_nano,ts)
                 VALUES('normalize-ledger','charge',6,$1,10) RETURNING id",
                &[&after_charge],
            )
            .unwrap()
            .get(0);
        pg.account_topup("normalize-ledger", 1, Some("platega:after-charge"))
            .unwrap();
        pg.client
            .execute("DELETE FROM ledger WHERE id=$1", &[&charge_id])
            .unwrap();
        pg.key_issue("normalize-key", "normalize-ledger", None)
            .unwrap();
        pg.client
            .execute(
                "INSERT INTO reservations(
                     request_id,account_id,key,hold_nano,balance_after_reserve_nano,
                     owner_instance,owner_epoch,lease_until,state,created_ts,updated_ts,
                     group_id,attempt)
                 VALUES('normalize-legacy-inflight','normalize-ledger','normalize-key',0,9,
                        'normalizer-test',1,9999999999,'reserved',20,20,NULL,1)",
                &[],
            )
            .unwrap();

        let blocked = pg
            .funding_normalization_plan_v2("normalize-ledger")
            .unwrap()
            .unwrap();
        assert_eq!(blocked.status, FundingNormalizationPlanStatusV2::Blocked);
        assert!(blocked.blockers.iter().any(|blocker| {
            blocker.code == FundingNormalizationBlockerCodeV2::ActiveLegacyReservation
        }));
        pg.client
            .execute(
                "UPDATE reservations
                    SET state='canceled',actual_nano=0,settled_ts=21,updated_ts=21
                  WHERE request_id='normalize-legacy-inflight'",
                &[],
            )
            .unwrap();

        let stale_plan = pg
            .funding_normalization_plan_v2("normalize-ledger")
            .unwrap()
            .unwrap();
        assert_eq!(stale_plan.status, FundingNormalizationPlanStatusV2::Ready);
        assert_eq!(
            stale_plan.source,
            FundingNormalizationSourceV2::LedgerReplay
        );
        assert_eq!(
            stale_plan
                .lots
                .iter()
                .map(|lot| format!(
                    "{}:{}:{}",
                    lot.source_type, lot.balance_nano, lot.spent_nano
                ))
                .collect::<Vec<_>>(),
            vec!["welcome_bonus:0:4", "paid:9:2"]
        );
        pg.account_topup("normalize-ledger", 2, Some("platega:plan-drift"))
            .unwrap();
        let stale = pg
            .apply_funding_normalization_v2(
                "normalize-ledger",
                &FundingNormalizationApplyRequestV2 {
                    expected_source_state_digest: stale_plan.source_state_digest,
                    expected_normalization_digest: stale_plan.normalization_digest.unwrap(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(stale.status, FundingNormalizationApplyStatusV2::Stale);

        let ready = stale.normalization;
        let request = FundingNormalizationApplyRequestV2 {
            expected_source_state_digest: ready.source_state_digest.clone(),
            expected_normalization_digest: ready.normalization_digest.clone().unwrap(),
        };
        let stored = pg
            .apply_funding_normalization_v2("normalize-ledger", &request)
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, FundingNormalizationApplyStatusV2::Stored);
        assert_eq!(stored.normalization.funding_generation, Some(1));

        // A writer immediately following normalization must reread the new head and update both
        // the legacy aggregate and funding-v2 generation in the same transaction.
        pg.account_topup(
            "normalize-ledger",
            5,
            Some("signup-bonus:post-normalization"),
        )
        .unwrap();
        let replay = pg
            .apply_funding_normalization_v2("normalize-ledger", &request)
            .unwrap()
            .unwrap();
        assert_eq!(replay.status, FundingNormalizationApplyStatusV2::Unchanged);
        let wrong_source_replay = pg
            .apply_funding_normalization_v2(
                "normalize-ledger",
                &FundingNormalizationApplyRequestV2 {
                    expected_source_state_digest: format!("sha256:v2:{}", "0".repeat(64)),
                    expected_normalization_digest: request.expected_normalization_digest.clone(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            wrong_source_replay.status,
            FundingNormalizationApplyStatusV2::Conflict
        );
        let parity = pg
            .client
            .query_one(
                "SELECT account.balance_nano,generation.balance_nano,
                        (SELECT sum(balance_nano)::bigint FROM funding_lots_v2 lot
                          WHERE lot.account_id=account.id AND lot.funding_generation=1),
                        (SELECT count(*)::bigint FROM funding_ledger_allocations_v2 allocation
                          JOIN ledger ON ledger.id=allocation.ledger_id
                         WHERE ledger.ref='signup-bonus:post-normalization')
                   FROM accounts account
                   JOIN account_funding_generations_v2 generation
                     ON generation.account_id=account.id AND generation.generation=1
                  WHERE account.id='normalize-ledger'",
                &[],
            )
            .unwrap();
        assert_eq!(
            (
                parity.get::<_, i64>(0),
                parity.get::<_, i64>(1),
                parity.get::<_, i64>(2),
                parity.get::<_, i64>(3),
            ),
            (16, 16, 16, 1)
        );

        // Existing strict/shadow bucket evidence wins over retained ledger and maps the historical
        // track-only source into provider-independent welcome_bonus; every other residual is paid.
        pg.account_create("normalize-buckets", None, 10_000)
            .unwrap();
        pg.account_topup("normalize-buckets", 15, Some("platega:buckets"))
            .unwrap();
        let bucket_balance: i64 = pg
            .client
            .query_one(
                "UPDATE accounts
                    SET balance_nano=balance_nano-5,spent_nano=spent_nano+5
                  WHERE id='normalize-buckets' RETURNING balance_nano",
                &[],
            )
            .unwrap()
            .get(0);
        pg.client
            .execute(
                "INSERT INTO ledger(account_id,kind,amount_nano,balance_after_nano,ts)
                 VALUES('normalize-buckets','charge',5,$1,30)",
                &[&bucket_balance],
            )
            .unwrap();
        pg.client
            .batch_execute(
                "INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts)
                 VALUES
                   ('normalize-old-welcome','normalize-buckets','welcome_track_bonus',
                    'signup-bonus:bucket-user','track',2,0,2,1,'active',1,1),
                   ('normalize-old-paid','normalize-buckets','legacy_restricted',
                    'stage6:legacy','none',8,0,3,1,'active',1,1)",
            )
            .unwrap();
        let bucket_plan = pg
            .funding_normalization_plan_v2("normalize-buckets")
            .unwrap()
            .unwrap();
        assert_eq!(
            bucket_plan.source,
            FundingNormalizationSourceV2::LegacyBuckets
        );
        assert_eq!(
            bucket_plan
                .lots
                .iter()
                .map(|lot| format!(
                    "{}:{}:{}",
                    lot.source_type, lot.balance_nano, lot.spent_nano
                ))
                .collect::<Vec<_>>(),
            vec!["welcome_bonus:2:2", "paid:8:3"]
        );
        let bucket_result = pg
            .apply_funding_normalization_v2(
                "normalize-buckets",
                &FundingNormalizationApplyRequestV2 {
                    expected_source_state_digest: bucket_plan.source_state_digest,
                    expected_normalization_digest: bucket_plan.normalization_digest.unwrap(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            bucket_result.status,
            FundingNormalizationApplyStatusV2::Stored
        );

        // Dormant/shadow legacy buckets are not money authority. If they drifted from the
        // aggregate, deterministic ledger replay replaces them instead of creating a manual
        // reviewer queue. A strict binding would fail closed on the same mismatch.
        pg.account_create("normalize-shadow", None, 10_000).unwrap();
        pg.account_topup("normalize-shadow", 4, Some("signup-bonus:shadow-user"))
            .unwrap();
        pg.client
            .batch_execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts)
                 VALUES('normalize-shadow','main','b2c',NULL,'shadow','shadow','pending',1);
                 INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts)
                 VALUES('normalize-stale-shadow','normalize-shadow','welcome_track_bonus',
                        'signup-bonus:shadow-user','track',1,0,0,1,'active',1,1)",
            )
            .unwrap();
        let shadow_plan = pg
            .funding_normalization_plan_v2("normalize-shadow")
            .unwrap()
            .unwrap();
        assert_eq!(shadow_plan.status, FundingNormalizationPlanStatusV2::Ready);
        assert_eq!(
            shadow_plan.source,
            FundingNormalizationSourceV2::LedgerReplay
        );
        assert_eq!(
            shadow_plan
                .lots
                .iter()
                .map(|lot| (lot.source_type.as_str(), lot.balance_nano))
                .collect::<Vec<_>>(),
            vec![("welcome_bonus", 4), ("paid", 0)]
        );

        assert!(pg
            .funding_normalization_plan_v2("missing-account")
            .unwrap()
            .is_none());
        pg.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }
}
