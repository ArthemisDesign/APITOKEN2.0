use anyhow::{bail, Context, Result};
use postgres::Transaction;
use sha2::{Digest, Sha256};

const FUNDING_SCHEMA_VERSION_V2: i64 = 2;
const FUNDING_ACCOUNT_LOCK_DOMAIN_V2: &str = "funding-v2-account:";
const FUNDING_SNAPSHOT_DIGEST_DOMAIN_V2: &[u8] = b"apitoken:funding-reservation-snapshot:v2\0";
const FUNDING_LOT_DIGEST_DOMAIN_V2: &[u8] = b"apitoken:funding-lot:v2\0";
const OVERDRAFT_NANO: i64 = 1_000_000_000;

#[derive(Clone, Debug)]
pub(crate) struct ActiveFundingHeadV2 {
    pub(crate) generation: i64,
    pub(crate) head_version: i64,
    generation_version: i64,
    balance_nano: i64,
}

#[derive(Clone, Debug)]
struct FundingLotV2 {
    lot_id: String,
    source_type: String,
    version: i64,
    balance_nano: i64,
}

#[derive(Clone, Debug)]
struct ReservationAllocationV2 {
    allocation_order: i64,
    lot_id: String,
    lot_source_type: String,
    lot_version: i64,
    reserved_nano: i64,
    charged_nano: Option<i64>,
    released_nano: Option<i64>,
}

#[derive(Clone, Debug)]
struct ReservationSnapshotV2 {
    account_id: String,
    funding_schema_version: i64,
    funding_generation: i64,
    funding_head_version: i64,
    hold_nano: i64,
    snapshot_digest: String,
    allocations: Vec<ReservationAllocationV2>,
}

#[derive(Clone, Debug)]
pub(crate) struct FundingLedgerAllocationV2 {
    pub(crate) allocation_order: i64,
    pub(crate) lot_id: String,
    pub(crate) lot_source_type: String,
    pub(crate) lot_version: i64,
    pub(crate) amount_nano: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SettlementFundingV2 {
    pub(crate) funding_generation: i64,
    pub(crate) allocations: Vec<FundingLedgerAllocationV2>,
    pub(crate) paid_funded_nano: i64,
    pub(crate) bonus_funded_nano: i64,
    pub(crate) other_funded_nano: i64,
}

impl SettlementFundingV2 {
    pub(crate) fn allocation_json(&self) -> Result<String> {
        let entries: Vec<serde_json::Value> = self
            .allocations
            .iter()
            .map(|allocation| {
                serde_json::json!({
                    "allocation_order": allocation.allocation_order,
                    "lot_id": allocation.lot_id,
                    "lot_source_type": allocation.lot_source_type,
                    "lot_version": allocation.lot_version,
                    "direction": "debit",
                    "amount_nano": allocation.amount_nano,
                })
            })
            .collect();
        Ok(serde_json::to_string(&entries)?)
    }
}

fn digest_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn digest_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_be_bytes());
}

fn hex_digest(hasher: Sha256, version: &str) -> String {
    let bytes = hasher.finalize();
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("sha256:{version}:{hex}")
}

fn funding_snapshot_digest_v2(
    request_id: &str,
    account_id: &str,
    generation: i64,
    head_version: i64,
    hold_nano: i64,
    allocations: &[ReservationAllocationV2],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(FUNDING_SNAPSHOT_DIGEST_DOMAIN_V2);
    digest_bytes(&mut hasher, request_id.as_bytes());
    digest_bytes(&mut hasher, account_id.as_bytes());
    digest_i64(&mut hasher, FUNDING_SCHEMA_VERSION_V2);
    digest_i64(&mut hasher, generation);
    digest_i64(&mut hasher, head_version);
    digest_i64(&mut hasher, hold_nano);
    digest_i64(&mut hasher, allocations.len() as i64);
    for allocation in allocations {
        digest_i64(&mut hasher, allocation.allocation_order);
        digest_bytes(&mut hasher, allocation.lot_id.as_bytes());
        digest_bytes(&mut hasher, allocation.lot_source_type.as_bytes());
        digest_i64(&mut hasher, allocation.lot_version);
        digest_i64(&mut hasher, allocation.reserved_nano);
    }
    hex_digest(hasher, "v2")
}

pub(crate) fn funding_lot_id_v2(
    account_id: &str,
    generation: i64,
    source_type: &str,
    source_ref: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(FUNDING_LOT_DIGEST_DOMAIN_V2);
    digest_bytes(&mut hasher, account_id.as_bytes());
    digest_i64(&mut hasher, generation);
    digest_bytes(&mut hasher, source_type.as_bytes());
    digest_bytes(&mut hasher, source_ref.as_bytes());
    format!("fundv2_{}", hex_digest(hasher, "v2"))
}

pub(crate) fn lock_funding_account_v2(tx: &mut Transaction<'_>, account_id: &str) -> Result<()> {
    if account_id.trim().is_empty() {
        bail!("funding v2 account lock requires an account id");
    }
    let identity = format!("{FUNDING_ACCOUNT_LOCK_DOMAIN_V2}{account_id}");
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        &[&identity],
    )?;
    Ok(())
}

pub(crate) fn active_funding_head_v2(
    tx: &mut Transaction<'_>,
    account_id: &str,
) -> Result<Option<ActiveFundingHeadV2>> {
    Ok(tx
        .query_opt(
            "SELECT head.active_generation,head.head_version,generation.version,
                    generation.balance_nano
               FROM account_funding_head_v2 head
               JOIN account_funding_generations_v2 generation
                 ON generation.account_id=head.account_id
                AND generation.generation=head.active_generation
              WHERE head.account_id=$1
              FOR UPDATE OF head,generation",
            &[&account_id],
        )?
        .map(|row| ActiveFundingHeadV2 {
            generation: row.get(0),
            head_version: row.get(1),
            generation_version: row.get(2),
            balance_nano: row.get(3),
        }))
}

fn load_reservation_snapshot_v2(
    tx: &mut Transaction<'_>,
    request_id: &str,
    lock_allocations: bool,
) -> Result<Option<ReservationSnapshotV2>> {
    let Some(row) = tx.query_opt(
        "SELECT account_id,funding_schema_version,funding_generation,funding_head_version,
                hold_nano,snapshot_digest
           FROM funding_reservation_snapshots_v2
          WHERE request_id=$1",
        &[&request_id],
    )?
    else {
        return Ok(None);
    };
    let lock = if lock_allocations {
        " FOR UPDATE OF allocation,lot"
    } else {
        ""
    };
    let sql = format!(
        "SELECT allocation.allocation_order,allocation.lot_id,allocation.lot_source_type,
                allocation.lot_version,allocation.reserved_nano,allocation.charged_nano,
                allocation.released_nano
           FROM funding_reservation_allocations_v2 allocation
           JOIN funding_lots_v2 lot
             ON lot.lot_id=allocation.lot_id
            AND lot.account_id=allocation.account_id
            AND lot.funding_generation=allocation.funding_generation
            AND lot.source_type=allocation.lot_source_type
          WHERE allocation.request_id=$1
          ORDER BY allocation.allocation_order{lock}"
    );
    let allocations = tx
        .query(&sql, &[&request_id])?
        .into_iter()
        .map(|allocation| ReservationAllocationV2 {
            allocation_order: allocation.get(0),
            lot_id: allocation.get(1),
            lot_source_type: allocation.get(2),
            lot_version: allocation.get(3),
            reserved_nano: allocation.get(4),
            charged_nano: allocation.get(5),
            released_nano: allocation.get(6),
        })
        .collect();
    Ok(Some(ReservationSnapshotV2 {
        account_id: row.get(0),
        funding_schema_version: row.get(1),
        funding_generation: row.get(2),
        funding_head_version: row.get(3),
        hold_nano: row.get(4),
        snapshot_digest: row.get(5),
        allocations,
    }))
}

fn validate_snapshot_v2(
    snapshot: &ReservationSnapshotV2,
    active_head: Option<&ActiveFundingHeadV2>,
    request_id: &str,
    account_id: &str,
    hold_nano: i64,
    terminal_actual: Option<i64>,
) -> Result<()> {
    if snapshot.account_id != account_id
        || snapshot.funding_schema_version != FUNDING_SCHEMA_VERSION_V2
        || snapshot.hold_nano != hold_nano
    {
        bail!("pre-cutover funding v2 snapshot identity changed");
    }
    if active_head.is_some_and(|head| {
        snapshot.funding_generation != head.generation
            || snapshot.funding_head_version != head.head_version
    }) {
        bail!("pre-cutover funding v2 snapshot no longer matches the active funding head");
    }
    let digest = funding_snapshot_digest_v2(
        request_id,
        account_id,
        snapshot.funding_generation,
        snapshot.funding_head_version,
        hold_nano,
        &snapshot.allocations,
    );
    if snapshot.snapshot_digest != digest {
        bail!("pre-cutover funding v2 snapshot digest mismatch");
    }

    let mut reserved_total = 0_i64;
    let mut charged_total = 0_i64;
    let mut released_total = 0_i64;
    let mut paid_seen = false;
    for (index, allocation) in snapshot.allocations.iter().enumerate() {
        if allocation.allocation_order != index as i64 + 1
            || !matches!(
                allocation.lot_source_type.as_str(),
                "paid" | "welcome_bonus"
            )
            || (paid_seen && allocation.lot_source_type == "welcome_bonus")
        {
            bail!("pre-cutover funding v2 allocation order is invalid");
        }
        paid_seen |= allocation.lot_source_type == "paid";
        reserved_total = reserved_total
            .checked_add(allocation.reserved_nano)
            .context("pre-cutover funding v2 reserved total overflow")?;
        match terminal_actual {
            Some(_) => {
                let charged = allocation
                    .charged_nano
                    .context("terminal funding v2 allocation lacks charged amount")?;
                let released = allocation
                    .released_nano
                    .context("terminal funding v2 allocation lacks released amount")?;
                charged_total = charged_total
                    .checked_add(charged)
                    .context("pre-cutover funding v2 charged total overflow")?;
                released_total = released_total
                    .checked_add(released)
                    .context("pre-cutover funding v2 released total overflow")?;
            }
            None if allocation.charged_nano.is_some() || allocation.released_nano.is_some() => {
                bail!("active funding v2 allocation is already terminal")
            }
            None => {}
        }
    }
    if reserved_total != hold_nano || (hold_nano > 0 && snapshot.allocations.is_empty()) {
        bail!("pre-cutover funding v2 allocations do not cover the hold");
    }
    if let Some(actual) = terminal_actual {
        let expected_release = hold_nano
            .checked_sub(actual)
            .context("terminal pre-cutover funding v2 release overflow")?
            .max(0);
        if charged_total != actual || released_total != expected_release {
            bail!("terminal pre-cutover funding v2 totals changed");
        }
    }
    Ok(())
}

pub(crate) fn validate_active_reservation_funding_v2(
    tx: &mut Transaction<'_>,
    head: &ActiveFundingHeadV2,
    request_id: &str,
    account_id: &str,
    hold_nano: i64,
) -> Result<()> {
    let snapshot = load_reservation_snapshot_v2(tx, request_id, false)?
        .context("normalized reservation lacks pre-cutover funding v2 snapshot")?;
    validate_snapshot_v2(
        &snapshot,
        Some(head),
        request_id,
        account_id,
        hold_nano,
        None,
    )
}

pub(crate) fn validate_terminal_reservation_funding_v2(
    tx: &mut Transaction<'_>,
    request_id: &str,
    account_id: &str,
    hold_nano: i64,
    actual_nano: i64,
) -> Result<()> {
    let Some(snapshot) = load_reservation_snapshot_v2(tx, request_id, false)? else {
        // A reservation that terminalized before this account was normalized legitimately has no
        // v2 snapshot even though a funding head may exist by the time the terminal replay arrives.
        return Ok(());
    };
    validate_snapshot_v2(
        &snapshot,
        None,
        request_id,
        account_id,
        hold_nano,
        Some(actual_nano),
    )
}

/// Attach an exact paid-only funding identity to legacy-format reservations while the account is
/// normalized under the shared funding lock.
///
/// This is deliberately narrower than a general reservation backfill: every active reservation
/// must still lack a funding snapshot, their holds must equal the complete account reserved
/// aggregate, and the newly materialized paid lot must carry that exact reserved amount. Callers
/// may use it only after proving that no welcome lot owns any of the active reserve. The existing
/// pricing snapshot remains untouched and continues to price settlement.
pub(crate) fn adopt_paid_only_legacy_reservations_v2(
    tx: &mut Transaction<'_>,
    account_id: &str,
    generation: i64,
    head_version: i64,
    paid_lot_id: &str,
    paid_lot_version: i64,
    expected_reserved_nano: i64,
    timestamp: i64,
) -> Result<()> {
    if account_id.trim().is_empty()
        || generation <= 0
        || head_version <= 0
        || paid_lot_id.trim().is_empty()
        || paid_lot_version < 0
        || expected_reserved_nano < 0
        || timestamp <= 0
    {
        bail!("paid-only legacy reservation adoption parameters are invalid");
    }
    let paid_reserved: i64 = tx
        .query_opt(
            "SELECT reserved_nano
               FROM funding_lots_v2
              WHERE lot_id=$1 AND account_id=$2 AND funding_generation=$3
                AND source_type='paid' AND version=$4",
            &[&paid_lot_id, &account_id, &generation, &paid_lot_version],
        )?
        .context("paid-only legacy reservation adoption lacks its exact paid lot")?
        .get(0);
    if paid_reserved != expected_reserved_nano {
        bail!("paid-only legacy reservation adoption does not match the paid lot reserve");
    }

    let reservations: Vec<(String, i64, bool)> = tx
        .query(
            "SELECT reservation.request_id,reservation.hold_nano,
                    snapshot.request_id IS NOT NULL
               FROM reservations reservation
               LEFT JOIN funding_reservation_snapshots_v2 snapshot
                 ON snapshot.request_id=reservation.request_id
              WHERE reservation.account_id=$1
                AND reservation.state IN ('reserved','delivering','settlement_pending')
              ORDER BY reservation.request_id
              FOR SHARE OF reservation",
            &[&account_id],
        )?
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect();
    if reservations
        .iter()
        .any(|(_, _, has_snapshot)| *has_snapshot)
    {
        bail!("paid-only legacy reservation adoption found a mixed snapshot state");
    }
    let reserved_total = reservations.iter().try_fold(0_i64, |total, (_, hold, _)| {
        total
            .checked_add(*hold)
            .context("paid-only legacy reservation adoption reserve overflow")
    })?;
    if reserved_total != expected_reserved_nano {
        bail!("paid-only legacy reservation adoption does not cover the account reserve");
    }

    for (request_id, hold_nano, _) in reservations {
        let allocations = [ReservationAllocationV2 {
            allocation_order: 1,
            lot_id: paid_lot_id.to_owned(),
            lot_source_type: "paid".to_owned(),
            lot_version: paid_lot_version,
            reserved_nano: hold_nano,
            charged_nano: None,
            released_nano: None,
        }];
        let snapshot_digest = funding_snapshot_digest_v2(
            &request_id,
            account_id,
            generation,
            head_version,
            hold_nano,
            &allocations,
        );
        tx.execute(
            "INSERT INTO funding_reservation_snapshots_v2(
                 request_id,account_id,funding_schema_version,funding_generation,
                 funding_head_version,hold_nano,snapshot_digest,created_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &request_id,
                &account_id,
                &FUNDING_SCHEMA_VERSION_V2,
                &generation,
                &head_version,
                &hold_nano,
                &snapshot_digest,
                &timestamp,
            ],
        )?;
        tx.execute(
            "INSERT INTO funding_reservation_allocations_v2(
                 request_id,account_id,funding_generation,allocation_order,lot_id,
                 lot_source_type,lot_version,reserved_nano)
             VALUES($1,$2,$3,1,$4,'paid',$5,$6)",
            &[
                &request_id,
                &account_id,
                &generation,
                &paid_lot_id,
                &paid_lot_version,
                &hold_nano,
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn reserve_funding_v2(
    tx: &mut Transaction<'_>,
    head: &ActiveFundingHeadV2,
    request_id: &str,
    account_id: &str,
    hold_nano: i64,
    timestamp: i64,
    allow_overdraft: bool,
) -> Result<()> {
    if hold_nano < 0 {
        bail!("pre-cutover funding v2 hold must be non-negative");
    }
    let available_floor = if allow_overdraft {
        head.balance_nano
            .checked_add(OVERDRAFT_NANO)
            .context("pre-cutover funding v2 overdraft gate overflow")?
    } else {
        head.balance_nano
    };
    if available_floor < hold_nano {
        bail!("pre-cutover funding v2 generation cannot cover the hold");
    }
    let lots: Vec<FundingLotV2> = tx
        .query(
            "SELECT lot_id,source_type,version,balance_nano
               FROM funding_lots_v2
              WHERE account_id=$1 AND funding_generation=$2 AND status<>'retired'
              ORDER BY CASE source_type WHEN 'welcome_bonus' THEN 0 ELSE 1 END,
                       created_ts,lot_id
              FOR UPDATE",
            &[&account_id, &head.generation],
        )?
        .into_iter()
        .map(|row| FundingLotV2 {
            lot_id: row.get(0),
            source_type: row.get(1),
            version: row.get(2),
            balance_nano: row.get(3),
        })
        .collect();
    let paid_anchor = lots.iter().find(|lot| lot.source_type == "paid").cloned();
    let mut remaining = hold_nano;
    let mut selected: Vec<(FundingLotV2, i64)> = Vec::new();
    for lot in lots.iter().filter(|lot| lot.balance_nano > 0) {
        if remaining == 0 {
            break;
        }
        let reserved = remaining.min(lot.balance_nano);
        selected.push((lot.clone(), reserved));
        remaining -= reserved;
    }
    if remaining > 0 {
        if !allow_overdraft || remaining > OVERDRAFT_NANO {
            bail!("pre-cutover funding v2 lots cannot cover the hold");
        }
        if let Some((_, reserved)) = selected
            .last_mut()
            .filter(|(lot, _)| lot.source_type == "paid")
        {
            *reserved = reserved
                .checked_add(remaining)
                .context("pre-cutover funding v2 paid overrun overflow")?;
        } else {
            selected.push((
                paid_anchor
                    .clone()
                    .context("pre-cutover funding v2 overdraft requires a paid lot")?,
                remaining,
            ));
        }
        remaining = 0;
    }
    if allow_overdraft
        && selected
            .last()
            .is_none_or(|(lot, _)| lot.source_type != "paid")
    {
        selected.push((
            paid_anchor.context("pre-cutover funding v2 reserve requires a paid lot")?,
            0,
        ));
    }
    if remaining != 0 {
        bail!("pre-cutover funding v2 reservation remained partially allocated");
    }

    let mut allocations = Vec::with_capacity(selected.len());
    for (index, (lot, reserved_nano)) in selected.into_iter().enumerate() {
        let lot_version = if reserved_nano == 0 {
            lot.version
        } else {
            let next_version = lot
                .version
                .checked_add(1)
                .context("pre-cutover funding v2 lot version overflow")?;
            let row = tx
                .query_opt(
                    "UPDATE funding_lots_v2
                        SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1,
                            version=$2,updated_ts=$3,
                            status=CASE WHEN balance_nano-$1>0 THEN 'active' ELSE 'exhausted' END
                      WHERE lot_id=$4 AND account_id=$5 AND funding_generation=$6
                        AND source_type=$7 AND version=$8
                        AND (source_type='paid' OR balance_nano >= $1)
                      RETURNING version",
                    &[
                        &reserved_nano,
                        &next_version,
                        &timestamp,
                        &lot.lot_id,
                        &account_id,
                        &head.generation,
                        &lot.source_type,
                        &lot.version,
                    ],
                )?
                .context("pre-cutover funding v2 lot changed during reserve")?;
            row.get(0)
        };
        allocations.push(ReservationAllocationV2 {
            allocation_order: index as i64 + 1,
            lot_id: lot.lot_id,
            lot_source_type: lot.source_type,
            lot_version,
            reserved_nano,
            charged_nano: None,
            released_nano: None,
        });
    }

    if hold_nano > 0 {
        let next_version = head
            .generation_version
            .checked_add(1)
            .context("pre-cutover funding v2 generation version overflow")?;
        if tx.execute(
            "UPDATE account_funding_generations_v2
                SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1,
                    version=$2,updated_ts=$3
              WHERE account_id=$4 AND generation=$5 AND version=$6",
            &[
                &hold_nano,
                &next_version,
                &timestamp,
                &account_id,
                &head.generation,
                &head.generation_version,
            ],
        )? != 1
        {
            bail!("pre-cutover funding v2 generation changed during reserve");
        }
    }
    let snapshot_digest = funding_snapshot_digest_v2(
        request_id,
        account_id,
        head.generation,
        head.head_version,
        hold_nano,
        &allocations,
    );
    tx.execute(
        "INSERT INTO funding_reservation_snapshots_v2(
             request_id,account_id,funding_schema_version,funding_generation,
             funding_head_version,hold_nano,snapshot_digest,created_ts)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
        &[
            &request_id,
            &account_id,
            &FUNDING_SCHEMA_VERSION_V2,
            &head.generation,
            &head.head_version,
            &hold_nano,
            &snapshot_digest,
            &timestamp,
        ],
    )?;
    for allocation in &allocations {
        tx.execute(
            "INSERT INTO funding_reservation_allocations_v2(
                 request_id,account_id,funding_generation,allocation_order,lot_id,
                 lot_source_type,lot_version,reserved_nano)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &request_id,
                &account_id,
                &head.generation,
                &allocation.allocation_order,
                &allocation.lot_id,
                &allocation.lot_source_type,
                &allocation.lot_version,
                &allocation.reserved_nano,
            ],
        )?;
    }
    Ok(())
}

/// Allocate a release-v2 balance reservation against the generation pinned by the immutable
/// release assignment. This intentionally writes the post-cutover allocation table rather than
/// the pre-cutover bridge table; both paths mutate the same aggregate/lots with identical
/// bonus-first and paid-overdraft semantics.
pub(crate) fn reserve_pricing_release_funding_v2(
    tx: &mut Transaction<'_>,
    head: &ActiveFundingHeadV2,
    request_id: &str,
    account_id: &str,
    expected_generation: i64,
    hold_nano: i64,
    timestamp: i64,
) -> Result<()> {
    if hold_nano < 0 || head.generation != expected_generation {
        bail!("pricing release funding generation/hold does not match the active account head");
    }
    let available_floor = head
        .balance_nano
        .checked_add(OVERDRAFT_NANO)
        .context("pricing release funding overdraft gate overflow")?;
    if available_floor < hold_nano {
        bail!("pricing release funding generation cannot cover the hold");
    }
    let lots: Vec<FundingLotV2> = tx
        .query(
            "SELECT lot_id,source_type,version,balance_nano
               FROM funding_lots_v2
              WHERE account_id=$1 AND funding_generation=$2 AND status<>'retired'
              ORDER BY CASE source_type WHEN 'welcome_bonus' THEN 0 ELSE 1 END,
                       created_ts,lot_id
              FOR UPDATE",
            &[&account_id, &head.generation],
        )?
        .into_iter()
        .map(|row| FundingLotV2 {
            lot_id: row.get(0),
            source_type: row.get(1),
            version: row.get(2),
            balance_nano: row.get(3),
        })
        .collect();
    let paid_anchor = lots.iter().find(|lot| lot.source_type == "paid").cloned();
    let mut remaining = hold_nano;
    let mut selected: Vec<(FundingLotV2, i64)> = Vec::new();
    for lot in lots.iter().filter(|lot| lot.balance_nano > 0) {
        if remaining == 0 {
            break;
        }
        let reserved = remaining.min(lot.balance_nano);
        selected.push((lot.clone(), reserved));
        remaining -= reserved;
    }
    if remaining > 0 {
        if remaining > OVERDRAFT_NANO {
            bail!("pricing release funding lots cannot cover the hold");
        }
        if let Some((_, reserved)) = selected
            .last_mut()
            .filter(|(lot, _)| lot.source_type == "paid")
        {
            *reserved = reserved
                .checked_add(remaining)
                .context("pricing release funding paid overrun overflow")?;
        } else {
            selected.push((
                paid_anchor
                    .clone()
                    .context("pricing release funding overdraft requires a paid lot")?,
                remaining,
            ));
        }
    }
    if selected
        .last()
        .is_none_or(|(lot, _)| lot.source_type != "paid")
    {
        selected.push((
            paid_anchor.context("pricing release funding reserve requires a paid lot")?,
            0,
        ));
    }

    let mut allocations = Vec::with_capacity(selected.len());
    for (index, (lot, reserved_nano)) in selected.into_iter().enumerate() {
        let lot_version = if reserved_nano == 0 {
            lot.version
        } else {
            let next_version = lot
                .version
                .checked_add(1)
                .context("pricing release funding lot version overflow")?;
            let row = tx
                .query_opt(
                    "UPDATE funding_lots_v2
                        SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1,
                            version=$2,updated_ts=$3,
                            status=CASE WHEN balance_nano-$1>0 THEN 'active' ELSE 'exhausted' END
                      WHERE lot_id=$4 AND account_id=$5 AND funding_generation=$6
                        AND source_type=$7 AND version=$8
                        AND (source_type='paid' OR balance_nano >= $1)
                      RETURNING version",
                    &[
                        &reserved_nano,
                        &next_version,
                        &timestamp,
                        &lot.lot_id,
                        &account_id,
                        &head.generation,
                        &lot.source_type,
                        &lot.version,
                    ],
                )?
                .context("pricing release funding lot changed during reserve")?;
            row.get(0)
        };
        allocations.push(ReservationAllocationV2 {
            allocation_order: index as i64 + 1,
            lot_id: lot.lot_id,
            lot_source_type: lot.source_type,
            lot_version,
            reserved_nano,
            charged_nano: None,
            released_nano: None,
        });
    }

    if hold_nano > 0 {
        let next_version = head
            .generation_version
            .checked_add(1)
            .context("pricing release funding generation version overflow")?;
        if tx.execute(
            "UPDATE account_funding_generations_v2
                SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1,
                    version=$2,updated_ts=$3
              WHERE account_id=$4 AND generation=$5 AND version=$6",
            &[
                &hold_nano,
                &next_version,
                &timestamp,
                &account_id,
                &head.generation,
                &head.generation_version,
            ],
        )? != 1
        {
            bail!("pricing release funding generation changed during reserve");
        }
    }
    for allocation in &allocations {
        tx.execute(
            "INSERT INTO pricing_request_funding_allocations_v2(
                 request_id,account_id,funding_generation,allocation_order,lot_id,
                 lot_source_type,lot_version,reserved_nano)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &request_id,
                &account_id,
                &head.generation,
                &allocation.allocation_order,
                &allocation.lot_id,
                &allocation.lot_source_type,
                &allocation.lot_version,
                &allocation.reserved_nano,
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn settle_funding_v2(
    tx: &mut Transaction<'_>,
    head: Option<&ActiveFundingHeadV2>,
    request_id: &str,
    account_id: &str,
    hold_nano: i64,
    actual_nano: i64,
    timestamp: i64,
) -> Result<Option<SettlementFundingV2>> {
    if actual_nano < 0 {
        bail!("pre-cutover funding v2 actual must be non-negative");
    }
    let maximum_actual = hold_nano
        .checked_add(OVERDRAFT_NANO)
        .context("pre-cutover funding v2 settlement ceiling overflow")?;
    if actual_nano > maximum_actual {
        bail!("pre-cutover funding v2 actual exceeds the paid overdraft ceiling");
    }
    let Some(mut snapshot) = load_reservation_snapshot_v2(tx, request_id, true)? else {
        if head.is_some() {
            bail!("normalized reservation lacks pre-cutover funding v2 snapshot");
        }
        return Ok(None);
    };
    let head = head.context("pre-cutover funding v2 snapshot exists without an active head")?;
    validate_snapshot_v2(
        &snapshot,
        Some(head),
        request_id,
        account_id,
        hold_nano,
        None,
    )?;

    let mut remaining = actual_nano;
    for allocation in &mut snapshot.allocations {
        let charged = remaining.min(allocation.reserved_nano);
        allocation.charged_nano = Some(charged);
        allocation.released_nano = Some(allocation.reserved_nano - charged);
        remaining -= charged;
    }
    if remaining > 0 {
        let allocation = snapshot
            .allocations
            .last_mut()
            .filter(|allocation| allocation.lot_source_type == "paid")
            .context("pre-cutover funding v2 overrun requires a final paid allocation")?;
        allocation.charged_nano = Some(
            allocation
                .charged_nano
                .unwrap_or(0)
                .checked_add(remaining)
                .context("pre-cutover funding v2 overrun overflow")?,
        );
        allocation.released_nano = Some(0);
    }

    let mut evidence = SettlementFundingV2 {
        funding_generation: snapshot.funding_generation,
        ..SettlementFundingV2::default()
    };
    for allocation in &snapshot.allocations {
        let charged = allocation
            .charged_nano
            .context("pre-cutover funding v2 charge was not derived")?;
        let released = allocation
            .released_nano
            .context("pre-cutover funding v2 release was not derived")?;
        let lot_delta = allocation
            .reserved_nano
            .checked_sub(charged)
            .context("pre-cutover funding v2 lot delta overflow")?;
        let lot_version = if allocation.reserved_nano == 0 && charged == 0 {
            allocation.lot_version
        } else {
            tx.query_one(
                "UPDATE funding_lots_v2
                    SET balance_nano=balance_nano+$1,reserved_nano=reserved_nano-$2,
                        spent_nano=spent_nano+$3,version=version+1,updated_ts=$4,
                        status=CASE
                          WHEN status='retired' THEN status
                          WHEN balance_nano+$1>0 THEN 'active'
                          ELSE 'exhausted'
                        END
                  WHERE lot_id=$5 AND account_id=$6 AND funding_generation=$7
                    AND source_type=$8 AND reserved_nano >= $2
                  RETURNING version",
                &[
                    &lot_delta,
                    &allocation.reserved_nano,
                    &charged,
                    &timestamp,
                    &allocation.lot_id,
                    &account_id,
                    &snapshot.funding_generation,
                    &allocation.lot_source_type,
                ],
            )?
            .get(0)
        };
        if tx.execute(
            "UPDATE funding_reservation_allocations_v2
                SET charged_nano=$1,released_nano=$2
              WHERE request_id=$3 AND account_id=$4 AND allocation_order=$5
                AND charged_nano IS NULL AND released_nano IS NULL",
            &[
                &charged,
                &released,
                &request_id,
                &account_id,
                &allocation.allocation_order,
            ],
        )? != 1
        {
            bail!("pre-cutover funding v2 allocation was already terminalized");
        }
        if allocation.lot_source_type == "paid" {
            evidence.paid_funded_nano = evidence
                .paid_funded_nano
                .checked_add(charged)
                .context("pre-cutover funding v2 paid total overflow")?;
        } else {
            evidence.bonus_funded_nano = evidence
                .bonus_funded_nano
                .checked_add(charged)
                .context("pre-cutover funding v2 bonus total overflow")?;
        }
        if charged > 0 {
            evidence.allocations.push(FundingLedgerAllocationV2 {
                allocation_order: evidence.allocations.len() as i64 + 1,
                lot_id: allocation.lot_id.clone(),
                lot_source_type: allocation.lot_source_type.clone(),
                lot_version,
                amount_nano: charged,
            });
        }
    }
    let balance_delta = hold_nano
        .checked_sub(actual_nano)
        .context("pre-cutover funding v2 settlement delta overflow")?;
    if hold_nano != 0 || actual_nano != 0 {
        tx.query_one(
            "UPDATE account_funding_generations_v2
                SET balance_nano=balance_nano+$1,reserved_nano=reserved_nano-$2,
                    spent_nano=spent_nano+$3,version=version+1,updated_ts=$4
              WHERE account_id=$5 AND generation=$6 AND reserved_nano >= $2
              RETURNING version",
            &[
                &balance_delta,
                &hold_nano,
                &actual_nano,
                &timestamp,
                &account_id,
                &snapshot.funding_generation,
            ],
        )?;
    }
    Ok(Some(evidence))
}

pub(crate) fn settle_pricing_release_funding_v2(
    tx: &mut Transaction<'_>,
    head: &ActiveFundingHeadV2,
    request_id: &str,
    account_id: &str,
    expected_generation: i64,
    hold_nano: i64,
    actual_nano: i64,
    timestamp: i64,
) -> Result<SettlementFundingV2> {
    if actual_nano < 0
        || head.generation != expected_generation
        || actual_nano
            > hold_nano
                .checked_add(OVERDRAFT_NANO)
                .context("pricing release settlement ceiling overflow")?
    {
        bail!("pricing release funding settlement identity/amount is invalid");
    }
    let mut allocations: Vec<ReservationAllocationV2> = tx
        .query(
            "SELECT allocation.allocation_order,allocation.lot_id,
                    allocation.lot_source_type,allocation.lot_version,
                    allocation.reserved_nano,allocation.charged_nano,
                    allocation.released_nano
               FROM pricing_request_funding_allocations_v2 allocation
               JOIN funding_lots_v2 lot
                 ON lot.lot_id=allocation.lot_id
                AND lot.account_id=allocation.account_id
                AND lot.funding_generation=allocation.funding_generation
                AND lot.source_type=allocation.lot_source_type
              WHERE allocation.request_id=$1 AND allocation.account_id=$2
                AND allocation.funding_generation=$3
              ORDER BY allocation.allocation_order
              FOR UPDATE OF allocation,lot",
            &[&request_id, &account_id, &expected_generation],
        )?
        .into_iter()
        .map(|row| ReservationAllocationV2 {
            allocation_order: row.get(0),
            lot_id: row.get(1),
            lot_source_type: row.get(2),
            lot_version: row.get(3),
            reserved_nano: row.get(4),
            charged_nano: row.get(5),
            released_nano: row.get(6),
        })
        .collect();
    let mut reserved_total = 0_i64;
    let mut paid_seen = false;
    for (index, allocation) in allocations.iter().enumerate() {
        if allocation.allocation_order != index as i64 + 1
            || allocation.charged_nano.is_some()
            || allocation.released_nano.is_some()
            || (paid_seen && allocation.lot_source_type == "welcome_bonus")
        {
            bail!("pricing release funding allocations are invalid or terminal");
        }
        paid_seen |= allocation.lot_source_type == "paid";
        reserved_total = reserved_total
            .checked_add(allocation.reserved_nano)
            .context("pricing release reserved funding total overflow")?;
    }
    if reserved_total != hold_nano || (hold_nano > 0 && allocations.is_empty()) {
        bail!("pricing release funding allocations do not cover the request hold");
    }

    let mut remaining = actual_nano;
    for allocation in &mut allocations {
        let charged = remaining.min(allocation.reserved_nano);
        allocation.charged_nano = Some(charged);
        allocation.released_nano = Some(allocation.reserved_nano - charged);
        remaining -= charged;
    }
    if remaining > 0 {
        let allocation = allocations
            .last_mut()
            .filter(|allocation| allocation.lot_source_type == "paid")
            .context("pricing release funding overrun requires a final paid allocation")?;
        allocation.charged_nano = Some(
            allocation
                .charged_nano
                .unwrap_or(0)
                .checked_add(remaining)
                .context("pricing release funding overrun overflow")?,
        );
        allocation.released_nano = Some(0);
    }

    let mut evidence = SettlementFundingV2 {
        funding_generation: expected_generation,
        ..SettlementFundingV2::default()
    };
    for allocation in &allocations {
        let charged = allocation
            .charged_nano
            .context("pricing release funding charge was not derived")?;
        let released = allocation
            .released_nano
            .context("pricing release funding release was not derived")?;
        let lot_delta = allocation
            .reserved_nano
            .checked_sub(charged)
            .context("pricing release funding lot delta overflow")?;
        let lot_version = if allocation.reserved_nano == 0 && charged == 0 {
            allocation.lot_version
        } else {
            tx.query_one(
                "UPDATE funding_lots_v2
                    SET balance_nano=balance_nano+$1,reserved_nano=reserved_nano-$2,
                        spent_nano=spent_nano+$3,version=version+1,updated_ts=$4,
                        status=CASE
                          WHEN status='retired' THEN status
                          WHEN balance_nano+$1>0 THEN 'active'
                          ELSE 'exhausted'
                        END
                  WHERE lot_id=$5 AND account_id=$6 AND funding_generation=$7
                    AND source_type=$8 AND reserved_nano >= $2
                  RETURNING version",
                &[
                    &lot_delta,
                    &allocation.reserved_nano,
                    &charged,
                    &timestamp,
                    &allocation.lot_id,
                    &account_id,
                    &expected_generation,
                    &allocation.lot_source_type,
                ],
            )?
            .get(0)
        };
        if tx.execute(
            "UPDATE pricing_request_funding_allocations_v2
                SET charged_nano=$1,released_nano=$2
              WHERE request_id=$3 AND account_id=$4 AND allocation_order=$5
                AND charged_nano IS NULL AND released_nano IS NULL",
            &[
                &charged,
                &released,
                &request_id,
                &account_id,
                &allocation.allocation_order,
            ],
        )? != 1
        {
            bail!("pricing release funding allocation was already terminalized");
        }
        match allocation.lot_source_type.as_str() {
            "paid" => {
                evidence.paid_funded_nano = evidence
                    .paid_funded_nano
                    .checked_add(charged)
                    .context("pricing release paid-funded total overflow")?;
            }
            "welcome_bonus" => {
                evidence.bonus_funded_nano = evidence
                    .bonus_funded_nano
                    .checked_add(charged)
                    .context("pricing release bonus-funded total overflow")?;
            }
            _ => {
                evidence.other_funded_nano = evidence
                    .other_funded_nano
                    .checked_add(charged)
                    .context("pricing release other-funded total overflow")?;
            }
        }
        if charged > 0 {
            evidence.allocations.push(FundingLedgerAllocationV2 {
                allocation_order: evidence.allocations.len() as i64 + 1,
                lot_id: allocation.lot_id.clone(),
                lot_source_type: allocation.lot_source_type.clone(),
                lot_version,
                amount_nano: charged,
            });
        }
    }
    let balance_delta = hold_nano
        .checked_sub(actual_nano)
        .context("pricing release settlement balance delta overflow")?;
    if hold_nano != 0 || actual_nano != 0 {
        tx.query_one(
            "UPDATE account_funding_generations_v2
                SET balance_nano=balance_nano+$1,reserved_nano=reserved_nano-$2,
                    spent_nano=spent_nano+$3,version=version+1,updated_ts=$4
              WHERE account_id=$5 AND generation=$6 AND reserved_nano >= $2
              RETURNING version",
            &[
                &balance_delta,
                &hold_nano,
                &actual_nano,
                &timestamp,
                &account_id,
                &expected_generation,
            ],
        )?;
    }
    Ok(evidence)
}

pub(crate) fn validate_active_pricing_release_funding_v2(
    tx: &mut Transaction<'_>,
    request_id: &str,
    account_id: &str,
    funding_generation: Option<i64>,
    hold_nano: i64,
) -> Result<()> {
    let row = tx.query_one(
        "SELECT count(*)::bigint,COALESCE(sum(reserved_nano),0)::bigint,
                count(*) FILTER (WHERE charged_nano IS NOT NULL OR released_nano IS NOT NULL)::bigint,
                count(DISTINCT funding_generation)::bigint
           FROM pricing_request_funding_allocations_v2
          WHERE request_id=$1 AND account_id=$2",
        &[&request_id, &account_id],
    )?;
    let count: i64 = row.get(0);
    if let Some(generation) = funding_generation {
        let matching_generation_count: i64 = tx
            .query_one(
                "SELECT count(*)::bigint
                   FROM pricing_request_funding_allocations_v2
                  WHERE request_id=$1 AND account_id=$2 AND funding_generation=$3",
                &[&request_id, &account_id, &generation],
            )?
            .get(0);
        if count == 0
            || row.get::<_, i64>(1) != hold_nano
            || row.get::<_, i64>(2) != 0
            || row.get::<_, i64>(3) != 1
            || matching_generation_count != count
        {
            bail!("active pricing release funding allocations changed");
        }
    } else if count != 0 || hold_nano != 0 {
        bail!("active meter-only pricing release request mutated funding");
    }
    Ok(())
}

pub(crate) fn validate_terminal_pricing_release_funding_v2(
    tx: &mut Transaction<'_>,
    request_id: &str,
    account_id: &str,
    funding_generation: Option<i64>,
    hold_nano: i64,
    actual_nano: i64,
) -> Result<()> {
    let row = tx.query_one(
        "SELECT count(*)::bigint,COALESCE(sum(reserved_nano),0)::bigint,
                COALESCE(sum(charged_nano),0)::bigint,
                COALESCE(sum(released_nano),0)::bigint,
                count(*) FILTER (WHERE charged_nano IS NULL OR released_nano IS NULL)::bigint,
                count(DISTINCT funding_generation)::bigint
           FROM pricing_request_funding_allocations_v2
          WHERE request_id=$1 AND account_id=$2",
        &[&request_id, &account_id],
    )?;
    let count: i64 = row.get(0);
    if let Some(generation) = funding_generation {
        let matching_generation_count: i64 = tx
            .query_one(
                "SELECT count(*)::bigint
                   FROM pricing_request_funding_allocations_v2
                  WHERE request_id=$1 AND account_id=$2 AND funding_generation=$3",
                &[&request_id, &account_id, &generation],
            )?
            .get(0);
        if row.get::<_, i64>(1) != hold_nano
            || row.get::<_, i64>(2) != actual_nano
            || row.get::<_, i64>(3) != hold_nano.saturating_sub(actual_nano).max(0)
            || row.get::<_, i64>(4) != 0
            || (count > 0 && row.get::<_, i64>(5) != 1)
            || matching_generation_count != count
            || (hold_nano > 0 && count == 0)
        {
            bail!("terminal pricing release funding allocations changed");
        }
    } else if count != 0 || hold_nano != 0 || actual_nano != 0 {
        bail!("terminal meter-only pricing release request mutated funding");
    }
    Ok(())
}

pub(crate) fn insert_settlement_ledger_allocations_v2(
    tx: &mut Transaction<'_>,
    ledger_id: i64,
    account_id: &str,
    generation: i64,
    funding: &SettlementFundingV2,
) -> Result<()> {
    for allocation in &funding.allocations {
        tx.execute(
            "INSERT INTO funding_ledger_allocations_v2(
                 ledger_id,account_id,funding_generation,allocation_order,lot_id,
                 lot_source_type,lot_version,direction,amount_nano)
             VALUES($1,$2,$3,$4,$5,$6,$7,'debit',$8)",
            &[
                &ledger_id,
                &account_id,
                &generation,
                &allocation.allocation_order,
                &allocation.lot_id,
                &allocation.lot_source_type,
                &allocation.lot_version,
                &allocation.amount_nano,
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn apply_topup_funding_v2(
    tx: &mut Transaction<'_>,
    head: &ActiveFundingHeadV2,
    ledger_id: i64,
    account_id: &str,
    amount_nano: i64,
    reference: Option<&str>,
    timestamp: i64,
) -> Result<()> {
    let source_type =
        if amount_nano >= 0 && reference.is_some_and(|value| value.starts_with("signup-bonus:")) {
            "welcome_bonus"
        } else {
            "paid"
        };
    let source_ref = reference
        .map(str::to_owned)
        .unwrap_or_else(|| format!("ledger:{ledger_id}"));
    let lot_id = funding_lot_id_v2(account_id, head.generation, source_type, &source_ref);
    let row = tx
        .query_opt(
            "INSERT INTO funding_lots_v2(
                 lot_id,account_id,funding_generation,source_type,source_ref,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts)
             VALUES($1,$2,$3,$4,$5,$6::bigint,0,0,1,
                    CASE WHEN $6::bigint>0 THEN 'active' ELSE 'exhausted' END,$7,$7)
             ON CONFLICT(account_id,funding_generation,source_type,source_ref) DO UPDATE SET
                 balance_nano=funding_lots_v2.balance_nano+EXCLUDED.balance_nano,
                 version=funding_lots_v2.version+1,updated_ts=EXCLUDED.updated_ts,
                 status=CASE
                   WHEN funding_lots_v2.balance_nano+EXCLUDED.balance_nano>0 THEN 'active'
                   ELSE 'exhausted'
                 END
             WHERE funding_lots_v2.status<>'retired'
               AND (funding_lots_v2.source_type='paid'
                    OR funding_lots_v2.balance_nano+EXCLUDED.balance_nano>=0)
             RETURNING lot_id,version",
            &[
                &lot_id,
                &account_id,
                &head.generation,
                &source_type,
                &source_ref,
                &amount_nano,
                &timestamp,
            ],
        )?
        .context("funding v2 top-up lot cannot accept the adjustment")?;
    let stored_lot_id: String = row.get(0);
    let lot_version: i64 = row.get(1);
    tx.query_one(
        "UPDATE account_funding_generations_v2
            SET balance_nano=balance_nano+$1,version=version+1,updated_ts=$2
          WHERE account_id=$3 AND generation=$4
          RETURNING version",
        &[&amount_nano, &timestamp, &account_id, &head.generation],
    )?;
    let direction = if amount_nano >= 0 { "credit" } else { "debit" };
    let allocation_amount = amount_nano
        .checked_abs()
        .context("funding v2 top-up allocation overflow")?;
    tx.execute(
        "INSERT INTO funding_ledger_allocations_v2(
             ledger_id,account_id,funding_generation,allocation_order,lot_id,
             lot_source_type,lot_version,direction,amount_nano)
         VALUES($1,$2,$3,1,$4,$5,$6,$7,$8)",
        &[
            &ledger_id,
            &account_id,
            &head.generation,
            &stored_lot_id,
            &source_type,
            &lot_version,
            &direction,
            &allocation_amount,
        ],
    )?;
    Ok(())
}
