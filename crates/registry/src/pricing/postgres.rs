//! PostgreSQL persistence and dormant Stage 3B snapshot read for the pricing contract.
//!
//! Every mutation is serialized by a namespaced transaction advisory lock. Row locks alone are
//! insufficient because an absent head or binding has no row to lock. Immutable versions and their
//! children are always written in one transaction; an active pointer moves only through an
//! explicit compare-and-set.

use super::{
    expectation_matches, invalid, missing, normalize_catalog, normalize_policy, normalize_switches,
    policy_expectation_matches, require_id, validate_account_policy,
    validate_account_policy_activation, validate_account_policy_binding,
    validate_account_policy_shape, validate_active_expectation,
    validate_legacy_snapshot_request_id, validate_policy_active_expectation,
    validate_pricing_catalog, validate_provider_switches, validate_version_target, AccountClass,
    AccountPolicyActivationSpec, AccountPolicyBindingSpec, AccountPolicyRuleSpec,
    AccountPolicySpec, ActiveAccountPolicy, ActiveExpectation, ActivePolicyTarget,
    FundingEnforcement, LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot,
    LegacyScalarAdmissionSnapshotInput, LegacyScalarSnapshotLookup, PolicyActiveExpectation,
    PolicyBindingState, PolicyEnforcement, PolicyOwnerType, PolicyRuleScope,
    PricingCatalogEntrySpec, PricingCatalogSpec, PricingMode, PricingMutation,
    PricingPolicySnapshot, PricingReadBundle, PricingRejection, ProviderSwitchEntrySpec,
    ProviderSwitchScope, ProviderSwitchSpec, ReconciliationState, RuleOrigin, SnapshotProvider,
    VersionTarget,
};
use anyhow::{bail, Context, Result};
use postgres::{Client, GenericClient, IsolationLevel, Transaction};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn stale(actual: Option<VersionTarget>) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::Stale { actual })
}

fn version_conflict() -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::VersionConflict)
}

fn cas_mismatch(actual: Option<VersionTarget>) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::CasMismatch { actual })
}

fn policy_cas_mismatch(actual: PolicyBindingState) -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { actual })
}

fn locked() -> PricingMutation {
    PricingMutation::Rejected(PricingRejection::Locked)
}

fn commit_mutation(
    transaction: Transaction<'_>,
    mutation: PricingMutation,
    operation: &str,
) -> Result<PricingMutation> {
    transaction
        .commit()
        .with_context(|| format!("commit PostgreSQL {operation}"))?;
    Ok(mutation)
}

fn advisory_lock(transaction: &mut Transaction<'_>, key: &str) -> Result<()> {
    transaction
        .query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&key],
        )
        .with_context(|| format!("lock PostgreSQL pricing mutation namespace {key:?}"))?;
    Ok(())
}

fn catalog_lock_key(product_id: &str) -> String {
    format!("multi-discount:catalog:{product_id}")
}

fn policy_lock_key(account_id: &str) -> String {
    format!("multi-discount:policy:{account_id}")
}

const SWITCH_LOCK_KEY: &str = "multi-discount:switches";

pub(crate) fn postgres_legacy_scalar_snapshot_lookup<C: GenericClient>(
    client: &mut C,
    request_id: &str,
) -> Result<LegacyScalarSnapshotLookup> {
    validate_legacy_snapshot_request_id(request_id)?;
    let Some(row) = client
        .query_opt(
            "SELECT snapshot_kind,schema_version,account_id,provider_id,requested_model_id,
                    canonical_model_id,alias_generation,pricing_mode,rule_origin,
                    tariff_schedule_id,tariff_priced_ts,admission_ts,payable_multiplier_bp,
                    official_hold_nano,charged_hold_nano,premium_modifiers::text,snapshot_digest
               FROM pricing_admission_snapshots
              WHERE request_id=$1",
            &[&request_id],
        )
        .context("read PostgreSQL pricing admission snapshot")?
    else {
        return Ok(LegacyScalarSnapshotLookup::Missing);
    };

    let snapshot_kind: String = row.get(0);
    if snapshot_kind != "legacy_scalar" {
        return Ok(LegacyScalarSnapshotLookup::NonLegacy);
    }
    let pricing_mode: String = row.get(7);
    let rule_origin: String = row.get(8);
    if pricing_mode != "legacy_scalar" || rule_origin != "legacy" {
        bail!("stored legacy scalar snapshot has an invalid fixed shape");
    }
    let provider_id: String = row.get(3);
    let premium_modifiers_json: String = row.get(15);
    let snapshot = LegacyScalarAdmissionSnapshot::from_stored(
        row.get(1),
        LegacyScalarAdmissionSnapshotInput {
            request_id: request_id.to_owned(),
            account_id: row.get(2),
            provider: SnapshotProvider::from_db(&provider_id)?,
            requested_model_id: row.get(4),
            canonical_model_id: row.get(5),
            alias_generation: row.get(6),
            tariff_schedule_id: row.get(9),
            tariff_priced_ts: row.get(10),
            admission_ts: row.get(11),
            payable_multiplier_bp: row.get(12),
            official_hold_nano: row.get(13),
            charged_hold_nano: row.get(14),
            premium_modifiers: LegacyPremiumModifiers::from_json(&premium_modifiers_json)?,
        },
        row.get(16),
    )?;
    Ok(LegacyScalarSnapshotLookup::Legacy(Box::new(snapshot)))
}

pub(crate) fn postgres_insert_legacy_scalar_admission_snapshot<C: GenericClient>(
    client: &mut C,
    snapshot: &LegacyScalarAdmissionSnapshot,
) -> Result<()> {
    snapshot.validate()?;
    let provider_id = snapshot.provider.as_str();
    let premium_modifiers = snapshot.premium_modifiers_json()?;
    let snapshot_digest = snapshot.snapshot_digest().as_str();
    let inserted = client
        .execute(
            "INSERT INTO pricing_admission_snapshots(
                 request_id,account_id,snapshot_kind,schema_version,provider_id,
                 requested_model_id,canonical_model_id,alias_generation,pricing_mode,rule_origin,
                 payable_multiplier_bp,tariff_schedule_id,tariff_priced_ts,admission_ts,
                 official_hold_nano,charged_hold_nano,premium_modifiers,snapshot_digest
             ) VALUES(
                 $1,$2,'legacy_scalar',$3,$4,$5,$6,$7,'legacy_scalar','legacy',
                 $8,$9,$10,$11,$12,$13,$14::text::jsonb,$15
             )",
            &[
                &snapshot.request_id,
                &snapshot.account_id,
                &snapshot.schema_version,
                &provider_id,
                &snapshot.requested_model_id,
                &snapshot.canonical_model_id,
                &snapshot.alias_generation,
                &snapshot.payable_multiplier_bp,
                &snapshot.tariff_schedule_id,
                &snapshot.tariff_priced_ts,
                &snapshot.admission_ts,
                &snapshot.official_hold_nano,
                &snapshot.charged_hold_nano,
                &premium_modifiers,
                &snapshot_digest,
            ],
        )
        .context("insert PostgreSQL legacy scalar admission snapshot")?;
    if inserted != 1 {
        bail!("PostgreSQL legacy scalar admission snapshot insert changed no row");
    }
    Ok(())
}

fn catalog_by_generation<C: GenericClient>(
    client: &mut C,
    product_id: &str,
    generation: i64,
    key_share: bool,
) -> Result<Option<PricingCatalogSpec>> {
    let sql = if key_share {
        "SELECT product_id,generation,schema_version,capability_generation,
                capability_digest,content_digest
         FROM pricing_catalog_versions
         WHERE product_id=$1 AND generation=$2
         FOR KEY SHARE"
    } else {
        "SELECT product_id,generation,schema_version,capability_generation,
                capability_digest,content_digest
         FROM pricing_catalog_versions
         WHERE product_id=$1 AND generation=$2"
    };
    let Some(header) = client.query_opt(sql, &[&product_id, &generation])? else {
        return Ok(None);
    };
    let mut spec = PricingCatalogSpec {
        product_id: header.get(0),
        generation: header.get(1),
        schema_version: header.get(2),
        capability_generation: header.get(3),
        capability_digest: header.get(4),
        content_digest: header.get(5),
        entries: Vec::new(),
    };
    spec.entries = client
        .query(
            "SELECT provider_id,canonical_model_id,enabled
             FROM pricing_catalog_entries
             WHERE product_id=$1 AND generation=$2
             ORDER BY provider_id,canonical_model_id",
            &[&product_id, &generation],
        )?
        .into_iter()
        .map(|row| PricingCatalogEntrySpec {
            provider_id: row.get(0),
            canonical_model_id: row.get(1),
            enabled: row.get(2),
        })
        .collect();
    spec = normalize_catalog(&spec);
    validate_pricing_catalog(&spec).context("validate stored PostgreSQL pricing catalog")?;
    Ok(Some(spec))
}

pub(crate) fn postgres_pricing_catalog_by_generation(
    client: &mut Client,
    product_id: &str,
    generation: i64,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    if generation <= 0 {
        bail!("catalog generation must be positive");
    }
    catalog_by_generation(client, product_id, generation, false)
}

fn catalog_head_locked(
    transaction: &mut Transaction<'_>,
    product_id: &str,
) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM pricing_catalog_heads h
         LEFT JOIN pricing_catalog_versions v
           ON v.product_id=h.product_id AND v.generation=h.active_generation
         WHERE h.product_id=$1
         FOR UPDATE OF h",
        &[&product_id],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("pricing catalog head references a missing version")?,
    )))
}

fn catalog_head_shared(
    transaction: &mut Transaction<'_>,
    product_id: &str,
) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM pricing_catalog_heads h
         LEFT JOIN pricing_catalog_versions v
           ON v.product_id=h.product_id AND v.generation=h.active_generation
         WHERE h.product_id=$1
         FOR SHARE OF h",
        &[&product_id],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("pricing catalog head references a missing version")?,
    )))
}

fn active_pricing_catalog<C: GenericClient>(
    client: &mut C,
    product_id: &str,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    let Some(row) = client.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM pricing_catalog_heads h
         LEFT JOIN pricing_catalog_versions v
           ON v.product_id=h.product_id AND v.generation=h.active_generation
         WHERE h.product_id=$1",
        &[&product_id],
    )?
    else {
        return Ok(None);
    };
    let generation: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    digest.context("pricing catalog head references a missing version")?;
    catalog_by_generation(client, product_id, generation, false)?
        .map(Some)
        .context("pricing catalog head references a missing version")
}

pub(crate) fn postgres_active_pricing_catalog(
    client: &mut Client,
    product_id: &str,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    active_pricing_catalog(client, product_id)
}

fn switches_by_generation<C: GenericClient>(
    client: &mut C,
    generation: i64,
    key_share: bool,
) -> Result<Option<ProviderSwitchSpec>> {
    let sql = if key_share {
        "SELECT generation,schema_version,capability_generation,capability_digest,content_digest
         FROM provider_switch_versions
         WHERE generation=$1
         FOR KEY SHARE"
    } else {
        "SELECT generation,schema_version,capability_generation,capability_digest,content_digest
         FROM provider_switch_versions
         WHERE generation=$1"
    };
    let Some(header) = client.query_opt(sql, &[&generation])? else {
        return Ok(None);
    };
    let mut spec = ProviderSwitchSpec {
        generation: header.get(0),
        schema_version: header.get(1),
        capability_generation: header.get(2),
        capability_digest: header.get(3),
        content_digest: header.get(4),
        entries: Vec::new(),
    };
    let rows = client.query(
        "SELECT provider_id,scope_type,product_id,segment,catalog_generation,enabled
         FROM provider_switch_entries
         WHERE generation=$1
         ORDER BY provider_id,scope_type,product_id,segment",
        &[&generation],
    )?;
    spec.entries = rows
        .into_iter()
        .map(|row| {
            let provider_id: String = row.get(0);
            let scope_type: String = row.get(1);
            let product_id: String = row.get(2);
            let segment: String = row.get(3);
            Ok(ProviderSwitchEntrySpec {
                provider_id,
                scope: ProviderSwitchScope::from_db(&scope_type, product_id, segment)?,
                catalog_generation: row.get(4),
                enabled: row.get(5),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    spec = normalize_switches(&spec);
    validate_provider_switches(&spec).context("validate stored PostgreSQL provider switches")?;
    Ok(Some(spec))
}

pub(crate) fn postgres_provider_switches_by_generation(
    client: &mut Client,
    generation: i64,
) -> Result<Option<ProviderSwitchSpec>> {
    if generation <= 0 {
        bail!("provider switch generation must be positive");
    }
    switches_by_generation(client, generation, false)
}

fn switch_head_locked(transaction: &mut Transaction<'_>) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM provider_switch_head h
         LEFT JOIN provider_switch_versions v ON v.generation=h.active_generation
         WHERE h.singleton=1
         FOR UPDATE OF h",
        &[],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("provider switch head references a missing version")?,
    )))
}

fn switch_head_shared(transaction: &mut Transaction<'_>) -> Result<Option<VersionTarget>> {
    let Some(row) = transaction.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM provider_switch_head h
         LEFT JOIN provider_switch_versions v ON v.generation=h.active_generation
         WHERE h.singleton=1
         FOR SHARE OF h",
        &[],
    )?
    else {
        return Ok(None);
    };
    let version: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    Ok(Some(VersionTarget::new(
        version,
        digest.context("provider switch head references a missing version")?,
    )))
}

fn active_provider_switches<C: GenericClient>(
    client: &mut C,
) -> Result<Option<ProviderSwitchSpec>> {
    let Some(row) = client.query_opt(
        "SELECT h.active_generation,v.content_digest
         FROM provider_switch_head h
         LEFT JOIN provider_switch_versions v ON v.generation=h.active_generation
         WHERE h.singleton=1",
        &[],
    )?
    else {
        return Ok(None);
    };
    let generation: i64 = row.get(0);
    let digest: Option<String> = row.get(1);
    digest.context("provider switch head references a missing version")?;
    switches_by_generation(client, generation, false)?
        .map(Some)
        .context("provider switch head references a missing version")
}

pub(crate) fn postgres_active_provider_switches(
    client: &mut Client,
) -> Result<Option<ProviderSwitchSpec>> {
    active_provider_switches(client)
}

fn policy_by_version<C: GenericClient>(
    client: &mut C,
    account_id: &str,
    effective_version: i64,
    key_share: bool,
) -> Result<Option<AccountPolicySpec>> {
    let sql = if key_share {
        "SELECT account_id,effective_version,policy_id,policy_version,source_policy_digest,
                owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                switch_generation,content_digest,replacement_locked
         FROM account_policy_versions
         WHERE account_id=$1 AND effective_version=$2
         FOR KEY SHARE"
    } else {
        "SELECT account_id,effective_version,policy_id,policy_version,source_policy_digest,
                owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                switch_generation,content_digest,replacement_locked
         FROM account_policy_versions
         WHERE account_id=$1 AND effective_version=$2"
    };
    let Some(header) = client.query_opt(sql, &[&account_id, &effective_version])? else {
        return Ok(None);
    };
    let owner_type: String = header.get(5);
    let account_class: String = header.get(7);
    let rows = client.query(
        "SELECT rule_id,rule_digest,scope_type,provider_id,canonical_model_id,
                pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                track_eligible,retention_eligible,commission_eligible
         FROM account_policy_rules
         WHERE account_id=$1 AND effective_version=$2
         ORDER BY provider_id,scope_type,canonical_model_id,rule_id",
        &[&account_id, &effective_version],
    )?;
    let rules = rows
        .into_iter()
        .map(|row| {
            let scope_type: String = row.get(2);
            let provider_id: String = row.get(3);
            let canonical_model_id: Option<String> = row.get(4);
            let pricing_mode: String = row.get(5);
            let rule_origin: String = row.get(6);
            Ok(AccountPolicyRuleSpec {
                rule_id: row.get(0),
                rule_digest: row.get(1),
                scope: PolicyRuleScope::from_db(&scope_type, provider_id, canonical_model_id)?,
                pricing_mode: PricingMode::from_db(&pricing_mode)?,
                rule_origin: RuleOrigin::from_db(&rule_origin)?,
                discount_bps: row.get(7),
                payable_multiplier_bp: row.get(8),
                track_eligible: row.get(9),
                retention_eligible: row.get(10),
                commission_eligible: row.get(11),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(normalize_policy(&AccountPolicySpec {
        account_id: header.get(0),
        effective_version: header.get(1),
        policy_id: header.get(2),
        policy_version: header.get(3),
        source_policy_digest: header.get(4),
        owner_type: PolicyOwnerType::from_db(&owner_type)?,
        owner_id: header.get(6),
        account_class: AccountClass::from_db(&account_class)?,
        product_id: header.get(8),
        schema_version: header.get(9),
        catalog_generation: header.get(10),
        switch_generation: header.get(11),
        content_digest: header.get(12),
        replacement_locked: header.get(13),
        rules,
    })))
}

pub(crate) fn postgres_account_policy_by_version(
    client: &mut Client,
    account_id: &str,
    effective_version: i64,
) -> Result<Option<AccountPolicySpec>> {
    require_id("account id", account_id)?;
    if effective_version <= 0 {
        bail!("effective policy version must be positive");
    }
    policy_by_version(client, account_id, effective_version, false)
}

#[derive(Clone, Debug)]
struct StoredPolicyBinding {
    product_id: String,
    account_class: AccountClass,
    active_target: Option<VersionTarget>,
    binding: AccountPolicyBindingSpec,
}

impl StoredPolicyBinding {
    fn active_policy_target(&self) -> Option<ActivePolicyTarget> {
        self.active_target.clone().map(|target| ActivePolicyTarget {
            target,
            binding: self.binding.clone(),
        })
    }

    fn state(&self) -> PolicyBindingState {
        match self.active_policy_target() {
            Some(active) => PolicyBindingState::Active(active),
            None => PolicyBindingState::Inactive(self.binding.clone()),
        }
    }
}

fn policy_binding_from_row(row: postgres::Row) -> Result<StoredPolicyBinding> {
    let account_class: String = row.get(1);
    let active_version: Option<i64> = row.get(2);
    let active_digest: Option<String> = row.get(3);
    let policy_enforcement: String = row.get(4);
    let funding_enforcement: String = row.get(5);
    let reconciliation_state: String = row.get(6);
    let active_target = match (active_version, active_digest) {
        (None, None) => None,
        (Some(version), Some(digest)) => Some(VersionTarget::new(version, digest)),
        _ => bail!("account policy binding references a missing effective version"),
    };
    let binding = AccountPolicyBindingSpec {
        policy_enforcement: PolicyEnforcement::from_db(&policy_enforcement)?,
        funding_enforcement: FundingEnforcement::from_db(&funding_enforcement)?,
        reconciliation_state: ReconciliationState::from_db(&reconciliation_state)?,
    };
    validate_account_policy_binding(&binding)
        .context("validate stored PostgreSQL account policy binding")?;
    Ok(StoredPolicyBinding {
        product_id: row.get(0),
        account_class: AccountClass::from_db(&account_class)?,
        active_target,
        binding,
    })
}

fn policy_binding_locked(
    transaction: &mut Transaction<'_>,
    account_id: &str,
) -> Result<Option<StoredPolicyBinding>> {
    transaction
        .query_opt(
            "SELECT b.product_id,b.account_class,b.active_effective_version,v.content_digest,
                    b.policy_enforcement,b.funding_enforcement,b.reconciliation_state
             FROM account_policy_bindings b
             LEFT JOIN account_policy_versions v
               ON v.account_id=b.account_id
              AND v.effective_version=b.active_effective_version
              AND v.product_id=b.product_id
             WHERE b.account_id=$1
             FOR UPDATE OF b",
            &[&account_id],
        )?
        .map(policy_binding_from_row)
        .transpose()
}

fn policy_binding<C: GenericClient>(
    client: &mut C,
    account_id: &str,
) -> Result<Option<StoredPolicyBinding>> {
    client
        .query_opt(
            "SELECT b.product_id,b.account_class,b.active_effective_version,v.content_digest,
                    b.policy_enforcement,b.funding_enforcement,b.reconciliation_state
             FROM account_policy_bindings b
             LEFT JOIN account_policy_versions v
               ON v.account_id=b.account_id
              AND v.effective_version=b.active_effective_version
              AND v.product_id=b.product_id
             WHERE b.account_id=$1",
            &[&account_id],
        )?
        .map(policy_binding_from_row)
        .transpose()
}

pub(crate) fn postgres_active_account_policy(
    client: &mut Client,
    account_id: &str,
) -> Result<Option<ActiveAccountPolicy>> {
    require_id("account id", account_id)?;
    let Some(stored_binding) = policy_binding(client, account_id)? else {
        return Ok(None);
    };
    let Some(target) = &stored_binding.active_target else {
        return Ok(None);
    };
    let policy = policy_by_version(client, account_id, target.version, false)?
        .context("account policy binding references a missing effective version")?;
    if policy.content_digest != target.content_digest
        || policy.product_id != stored_binding.product_id
        || policy.account_class != stored_binding.account_class
    {
        bail!("account policy binding identity does not match its active policy");
    }
    Ok(Some(ActiveAccountPolicy {
        policy,
        binding: stored_binding.binding,
    }))
}

/// Read the account scalar, exact policy dependencies and independently moving admission heads
/// from one PostgreSQL snapshot.
///
/// `REPEATABLE READ` gives the multi-statement decoder one coherent historical view without
/// locking activation writers. The transaction is explicitly read-only: this dormant Stage 3B
/// primitive cannot move a head, change a binding, or affect billing state. An active policy must
/// resolve both immutable dependency versions; a missing pinned row is an integrity error rather
/// than an incomplete active bundle.
pub(crate) fn postgres_pricing_read_bundle(
    client: &mut Client,
    account_id: &str,
) -> Result<PricingReadBundle> {
    require_id("account id", account_id)?;
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .context("begin PostgreSQL pricing read snapshot")?;
    let account_multiplier_bp = transaction
        .query_opt("SELECT mult_bp FROM accounts WHERE id=$1", &[&account_id])?
        .map(|row| row.get::<_, i64>(0))
        .context("PostgreSQL pricing bundle account does not exist")?;

    let Some(stored_binding) = policy_binding(&mut transaction, account_id)? else {
        let bundle = PricingReadBundle {
            account_id: account_id.to_owned(),
            account_multiplier_bp,
            policy: PricingPolicySnapshot::Unbound,
            policy_catalog: None,
            policy_switches: None,
            admission_catalog: None,
            admission_switches: None,
        };
        transaction
            .commit()
            .context("commit PostgreSQL pricing read snapshot")?;
        return Ok(bundle);
    };

    let product_id = stored_binding.product_id.clone();
    let (policy, policy_catalog, policy_switches) = match stored_binding.active_target.as_ref() {
        None => (
            PricingPolicySnapshot::Inactive {
                product_id: product_id.clone(),
                account_class: stored_binding.account_class,
                binding: stored_binding.binding,
            },
            None,
            None,
        ),
        Some(target) => {
            let policy = policy_by_version(&mut transaction, account_id, target.version, false)?
                .context("account policy binding references a missing effective version")?;
            if policy.content_digest != target.content_digest
                || policy.product_id != product_id
                || policy.account_class != stored_binding.account_class
            {
                bail!("account policy binding identity does not match its active policy");
            }
            let policy_catalog = catalog_by_generation(
                &mut transaction,
                &policy.product_id,
                policy.catalog_generation,
                false,
            )?
            .context("active account policy references a missing pricing catalog generation")?;
            let policy_switches =
                switches_by_generation(&mut transaction, policy.switch_generation, false)?
                    .context(
                        "active account policy references a missing provider switch generation",
                    )?;
            (
                PricingPolicySnapshot::Active(ActiveAccountPolicy {
                    policy,
                    binding: stored_binding.binding,
                }),
                Some(policy_catalog),
                Some(policy_switches),
            )
        }
    };
    let admission_catalog = active_pricing_catalog(&mut transaction, &product_id)?;
    let admission_switches = active_provider_switches(&mut transaction)?;
    let bundle = PricingReadBundle {
        account_id: account_id.to_owned(),
        account_multiplier_bp,
        policy,
        policy_catalog,
        policy_switches,
        admission_catalog,
        admission_switches,
    };
    transaction
        .commit()
        .context("commit PostgreSQL pricing read snapshot")?;
    Ok(bundle)
}

fn newest_catalog_target(
    transaction: &mut Transaction<'_>,
    product_id: &str,
) -> Result<Option<VersionTarget>> {
    Ok(transaction
        .query_opt(
            "SELECT generation,content_digest
             FROM pricing_catalog_versions
             WHERE product_id=$1
             ORDER BY generation DESC
             LIMIT 1",
            &[&product_id],
        )?
        .map(|row| VersionTarget::new(row.get(0), row.get::<_, String>(1))))
}

fn insert_catalog(
    transaction: &mut Transaction<'_>,
    spec: &PricingCatalogSpec,
    created_ts: i64,
) -> Result<bool> {
    let inserted = transaction.execute(
        "INSERT INTO pricing_catalog_versions(
             product_id,generation,schema_version,capability_generation,
             capability_digest,content_digest,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT(product_id,generation) DO NOTHING",
        &[
            &spec.product_id,
            &spec.generation,
            &spec.schema_version,
            &spec.capability_generation,
            &spec.capability_digest,
            &spec.content_digest,
            &created_ts,
        ],
    )? == 1;
    if !inserted {
        return Ok(false);
    }
    for entry in &spec.entries {
        transaction.execute(
            "INSERT INTO pricing_catalog_entries(
                 product_id,generation,provider_id,canonical_model_id,enabled
             ) VALUES($1,$2,$3,$4,$5)",
            &[
                &spec.product_id,
                &spec.generation,
                &entry.provider_id,
                &entry.canonical_model_id,
                &entry.enabled,
            ],
        )?;
    }
    Ok(true)
}

pub(crate) fn postgres_prepare_pricing_catalog(
    client: &mut Client,
    incoming: &PricingCatalogSpec,
) -> Result<PricingMutation> {
    let incoming = normalize_catalog(incoming);
    if let Err(error) = validate_pricing_catalog(&incoming) {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing catalog prepare")?;
    advisory_lock(&mut transaction, &catalog_lock_key(&incoming.product_id))?;

    if let Some(existing) = catalog_by_generation(
        &mut transaction,
        &incoming.product_id,
        incoming.generation,
        true,
    )? {
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "pricing catalog prepare");
    }

    if let Some(newest) = newest_catalog_target(&mut transaction, &incoming.product_id)? {
        if incoming.generation < newest.version {
            return commit_mutation(transaction, stale(Some(newest)), "pricing catalog prepare");
        }
    }

    if !insert_catalog(&mut transaction, &incoming, now())? {
        let existing = catalog_by_generation(
            &mut transaction,
            &incoming.product_id,
            incoming.generation,
            true,
        )?
        .context("catalog insert conflict did not expose the conflicting version")?;
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "pricing catalog prepare");
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "pricing catalog prepare",
    )
}

pub(crate) fn postgres_activate_pricing_catalog(
    client: &mut Client,
    product_id: &str,
    target: &VersionTarget,
    expectation: &ActiveExpectation,
) -> Result<PricingMutation> {
    if let Err(error) = require_id("product id", product_id)
        .and_then(|_| validate_version_target(target))
        .and_then(|_| validate_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL pricing catalog activation")?;
    advisory_lock(&mut transaction, &catalog_lock_key(product_id))?;
    let actual = catalog_head_locked(&mut transaction, product_id)?;
    let Some(stored) = catalog_by_generation(&mut transaction, product_id, target.version, true)?
    else {
        return commit_mutation(
            transaction,
            missing(format!(
                "pricing catalog {product_id:?} generation {}",
                target.version
            )),
            "pricing catalog activation",
        );
    };
    if stored.content_digest != target.content_digest {
        return commit_mutation(
            transaction,
            version_conflict(),
            "pricing catalog activation",
        );
    }
    if actual.as_ref() == Some(target) {
        return commit_mutation(
            transaction,
            PricingMutation::Unchanged,
            "pricing catalog activation",
        );
    }
    if actual
        .as_ref()
        .is_some_and(|current| target.version < current.version)
    {
        return commit_mutation(transaction, stale(actual), "pricing catalog activation");
    }
    if !expectation_matches(expectation, actual.as_ref()) {
        return commit_mutation(
            transaction,
            cas_mismatch(actual),
            "pricing catalog activation",
        );
    }

    let updated_ts = now();
    let affected = match expectation {
        ActiveExpectation::Absent => transaction.execute(
            "INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
             VALUES($1,$2,$3)
             ON CONFLICT(product_id) DO NOTHING",
            &[&product_id, &target.version, &updated_ts],
        )?,
        ActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE pricing_catalog_heads h
             SET active_generation=$2,updated_ts=$3
             WHERE h.product_id=$1
               AND h.active_generation=$4
               AND EXISTS (
                   SELECT 1
                   FROM pricing_catalog_versions v
                   WHERE v.product_id=h.product_id
                     AND v.generation=h.active_generation
                     AND v.content_digest=$5
               )",
            &[
                &product_id,
                &target.version,
                &updated_ts,
                &expected.version,
                &expected.content_digest,
            ],
        )?,
    };
    if affected != 1 {
        transaction
            .rollback()
            .context("rollback lost PostgreSQL pricing catalog CAS")?;
        return Ok(cas_mismatch(actual));
    }
    commit_mutation(
        transaction,
        PricingMutation::Applied,
        "pricing catalog activation",
    )
}

enum DependencyCheck<T> {
    Ready(T),
    Rejected(PricingMutation),
}

#[derive(Clone, Debug)]
struct SwitchDependencies {
    catalogs: Vec<(String, VersionTarget)>,
}

fn switch_dependencies(
    transaction: &mut Transaction<'_>,
    switches: &ProviderSwitchSpec,
) -> Result<DependencyCheck<SwitchDependencies>> {
    let mut targets = BTreeSet::new();
    for entry in &switches.entries {
        let (product_id, generation) = match (&entry.scope, entry.catalog_generation) {
            (
                ProviderSwitchScope::Product { product_id }
                | ProviderSwitchScope::Segment { product_id, .. },
                Some(generation),
            ) => (product_id, generation),
            _ => continue,
        };
        targets.insert((product_id.clone(), generation));
    }
    let mut catalogs = Vec::with_capacity(targets.len());
    for (product_id, generation) in targets {
        let Some(catalog) = catalog_by_generation(transaction, &product_id, generation, true)?
        else {
            return Ok(DependencyCheck::Rejected(missing(format!(
                "pricing catalog {product_id:?} generation {generation}"
            ))));
        };
        if catalog.capability_generation != switches.capability_generation
            || catalog.capability_digest != switches.capability_digest
        {
            return Ok(DependencyCheck::Rejected(invalid(anyhow::anyhow!(
                "provider switches and catalog {product_id:?} generation {generation} \
                 have different capability pins"
            ))));
        }
        if let Some(entry) = switches.entries.iter().find(|entry| {
            matches!(
                &entry.scope,
                ProviderSwitchScope::Product {
                    product_id: scoped_product
                } | ProviderSwitchScope::Segment {
                    product_id: scoped_product,
                    ..
                } if scoped_product == &product_id
            ) && entry.catalog_generation == Some(generation)
                && !catalog
                    .entries
                    .iter()
                    .any(|catalog_entry| catalog_entry.provider_id == entry.provider_id)
        }) {
            return Ok(DependencyCheck::Rejected(invalid(anyhow::anyhow!(
                "provider switch {:?} references provider {:?} absent from catalog {:?} \
                 generation {}",
                switches.content_digest,
                entry.provider_id,
                product_id,
                generation
            ))));
        }
        catalogs.push((product_id, catalog.target()));
    }
    Ok(DependencyCheck::Ready(SwitchDependencies { catalogs }))
}

fn active_switch_dependencies(
    transaction: &mut Transaction<'_>,
    dependencies: &SwitchDependencies,
) -> Result<Option<PricingMutation>> {
    for (product_id, expected) in &dependencies.catalogs {
        let actual = catalog_head_shared(transaction, product_id)?;
        if actual.as_ref() != Some(expected) {
            return Ok(Some(missing(format!(
                "active pricing catalog {product_id:?} target {expected:?}"
            ))));
        }
    }
    Ok(None)
}

fn newest_switch_target(transaction: &mut Transaction<'_>) -> Result<Option<VersionTarget>> {
    Ok(transaction
        .query_opt(
            "SELECT generation,content_digest
             FROM provider_switch_versions
             ORDER BY generation DESC
             LIMIT 1",
            &[],
        )?
        .map(|row| VersionTarget::new(row.get(0), row.get::<_, String>(1))))
}

fn insert_switches(
    transaction: &mut Transaction<'_>,
    spec: &ProviderSwitchSpec,
    created_ts: i64,
) -> Result<bool> {
    let inserted = transaction.execute(
        "INSERT INTO provider_switch_versions(
             generation,schema_version,capability_generation,capability_digest,
             content_digest,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6)
         ON CONFLICT(generation) DO NOTHING",
        &[
            &spec.generation,
            &spec.schema_version,
            &spec.capability_generation,
            &spec.capability_digest,
            &spec.content_digest,
            &created_ts,
        ],
    )? == 1;
    if !inserted {
        return Ok(false);
    }
    for entry in &spec.entries {
        let (scope_type, product_id, segment) = entry.scope.db_parts();
        transaction.execute(
            "INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
             ) VALUES($1,$2,$3,$4,$5,$6,$7)",
            &[
                &spec.generation,
                &entry.provider_id,
                &scope_type,
                &product_id,
                &segment,
                &entry.catalog_generation,
                &entry.enabled,
            ],
        )?;
    }
    Ok(true)
}

pub(crate) fn postgres_prepare_provider_switches(
    client: &mut Client,
    incoming: &ProviderSwitchSpec,
) -> Result<PricingMutation> {
    let incoming = normalize_switches(incoming);
    if let Err(error) = validate_provider_switches(&incoming) {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL provider switch prepare")?;
    advisory_lock(&mut transaction, SWITCH_LOCK_KEY)?;

    if let Some(existing) = switches_by_generation(&mut transaction, incoming.generation, true)? {
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "provider switch prepare");
    }
    if let Some(newest) = newest_switch_target(&mut transaction)? {
        if incoming.generation < newest.version {
            return commit_mutation(transaction, stale(Some(newest)), "provider switch prepare");
        }
    }
    match switch_dependencies(&mut transaction, &incoming)? {
        DependencyCheck::Ready(_) => {}
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "provider switch prepare");
        }
    }
    if !insert_switches(&mut transaction, &incoming, now())? {
        let existing = switches_by_generation(&mut transaction, incoming.generation, true)?
            .context("switch insert conflict did not expose the conflicting version")?;
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "provider switch prepare");
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "provider switch prepare",
    )
}

pub(crate) fn postgres_activate_provider_switches(
    client: &mut Client,
    target: &VersionTarget,
    expectation: &ActiveExpectation,
) -> Result<PricingMutation> {
    if let Err(error) =
        validate_version_target(target).and_then(|_| validate_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL provider switch activation")?;
    advisory_lock(&mut transaction, SWITCH_LOCK_KEY)?;
    let actual = switch_head_locked(&mut transaction)?;
    let Some(stored) = switches_by_generation(&mut transaction, target.version, true)? else {
        return commit_mutation(
            transaction,
            missing(format!("provider switch generation {}", target.version)),
            "provider switch activation",
        );
    };
    if stored.content_digest != target.content_digest {
        return commit_mutation(
            transaction,
            version_conflict(),
            "provider switch activation",
        );
    }
    if actual.as_ref() == Some(target) {
        return commit_mutation(
            transaction,
            PricingMutation::Unchanged,
            "provider switch activation",
        );
    }
    if actual
        .as_ref()
        .is_some_and(|current| target.version < current.version)
    {
        return commit_mutation(transaction, stale(actual), "provider switch activation");
    }
    if !expectation_matches(expectation, actual.as_ref()) {
        return commit_mutation(
            transaction,
            cas_mismatch(actual),
            "provider switch activation",
        );
    }
    let dependencies = match switch_dependencies(&mut transaction, &stored)? {
        DependencyCheck::Ready(dependencies) => dependencies,
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "provider switch activation");
        }
    };
    if let Some(outcome) = active_switch_dependencies(&mut transaction, &dependencies)? {
        return commit_mutation(transaction, outcome, "provider switch activation");
    }

    let updated_ts = now();
    let affected = match expectation {
        ActiveExpectation::Absent => transaction.execute(
            "INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
             VALUES(1,$1,$2)
             ON CONFLICT(singleton) DO NOTHING",
            &[&target.version, &updated_ts],
        )?,
        ActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE provider_switch_head h
             SET active_generation=$1,updated_ts=$2
             WHERE h.singleton=1
               AND h.active_generation=$3
               AND EXISTS (
                   SELECT 1
                   FROM provider_switch_versions v
                   WHERE v.generation=h.active_generation
                     AND v.content_digest=$4
               )",
            &[
                &target.version,
                &updated_ts,
                &expected.version,
                &expected.content_digest,
            ],
        )?,
    };
    if affected != 1 {
        transaction
            .rollback()
            .context("rollback lost PostgreSQL provider switch CAS")?;
        return Ok(cas_mismatch(actual));
    }
    commit_mutation(
        transaction,
        PricingMutation::Applied,
        "provider switch activation",
    )
}

#[derive(Clone, Debug)]
struct PolicyLineage {
    target: VersionTarget,
    policy_id: String,
    policy_version: i64,
    source_policy_digest: String,
    owner_type: PolicyOwnerType,
    owner_id: String,
    account_class: AccountClass,
    product_id: String,
}

fn policy_lineage_from_row(row: postgres::Row) -> Result<PolicyLineage> {
    let owner_type: String = row.get(5);
    let account_class: String = row.get(7);
    Ok(PolicyLineage {
        target: VersionTarget::new(row.get(0), row.get::<_, String>(1)),
        policy_id: row.get(2),
        policy_version: row.get(3),
        source_policy_digest: row.get(4),
        owner_type: PolicyOwnerType::from_db(&owner_type)?,
        owner_id: row.get(6),
        account_class: AccountClass::from_db(&account_class)?,
        product_id: row.get(8),
    })
}

fn newest_policy_lineage(
    transaction: &mut Transaction<'_>,
    account_id: &str,
) -> Result<Option<PolicyLineage>> {
    transaction
        .query_opt(
            "SELECT effective_version,content_digest,policy_id,policy_version,
                    source_policy_digest,owner_type,owner_id,account_class,product_id
             FROM account_policy_versions
             WHERE account_id=$1
             ORDER BY effective_version DESC
             LIMIT 1",
            &[&account_id],
        )?
        .map(policy_lineage_from_row)
        .transpose()
}

fn locked_policy_lineage(
    transaction: &mut Transaction<'_>,
    account_id: &str,
) -> Result<Option<PolicyLineage>> {
    transaction
        .query_opt(
            "SELECT effective_version,content_digest,policy_id,policy_version,
                    source_policy_digest,owner_type,owner_id,account_class,product_id
             FROM account_policy_versions
             WHERE account_id=$1 AND replacement_locked
             ORDER BY effective_version
             LIMIT 1
             FOR KEY SHARE",
            &[&account_id],
        )?
        .map(policy_lineage_from_row)
        .transpose()
}

fn policy_identity_matches(lineage: &PolicyLineage, incoming: &AccountPolicySpec) -> bool {
    lineage.policy_id == incoming.policy_id
        && lineage.owner_type == incoming.owner_type
        && lineage.owner_id == incoming.owner_id
        && lineage.account_class == incoming.account_class
        && lineage.product_id == incoming.product_id
}

fn binding_identity_matches(binding: &StoredPolicyBinding, policy: &AccountPolicySpec) -> bool {
    binding.product_id == policy.product_id && binding.account_class == policy.account_class
}

#[derive(Clone, Debug)]
struct PolicyDependencies {
    catalog: PricingCatalogSpec,
    switches: ProviderSwitchSpec,
}

fn policy_dependencies(
    transaction: &mut Transaction<'_>,
    policy: &AccountPolicySpec,
) -> Result<DependencyCheck<PolicyDependencies>> {
    let Some(catalog) = catalog_by_generation(
        transaction,
        &policy.product_id,
        policy.catalog_generation,
        true,
    )?
    else {
        return Ok(DependencyCheck::Rejected(missing(format!(
            "pricing catalog {:?} generation {}",
            policy.product_id, policy.catalog_generation
        ))));
    };
    let Some(switches) = switches_by_generation(transaction, policy.switch_generation, true)?
    else {
        return Ok(DependencyCheck::Rejected(missing(format!(
            "provider switch generation {}",
            policy.switch_generation
        ))));
    };
    let Some(account) = transaction.query_opt(
        "SELECT mult_bp
         FROM accounts
         WHERE id=$1
         FOR SHARE",
        &[&policy.account_id],
    )?
    else {
        return Ok(DependencyCheck::Rejected(missing(format!(
            "engine account {:?}",
            policy.account_id
        ))));
    };
    let legacy_multiplier_bp: i64 = account.get(0);
    if let Err(error) =
        validate_account_policy(policy, &catalog, &switches, Some(legacy_multiplier_bp))
    {
        return Ok(DependencyCheck::Rejected(invalid(error)));
    }
    Ok(DependencyCheck::Ready(PolicyDependencies {
        catalog,
        switches,
    }))
}

fn active_policy_dependencies(
    transaction: &mut Transaction<'_>,
    dependencies: &PolicyDependencies,
) -> Result<Option<PricingMutation>> {
    // Keep the same lock order as switch activation: switch head first, then catalog heads.
    // Otherwise policy activation could deadlock with switch activation while each held one head.
    let expected_switch = dependencies.switches.target();
    let actual_switch = switch_head_shared(transaction)?;
    if actual_switch.as_ref() != Some(&expected_switch) {
        return Ok(Some(missing(format!(
            "active provider switch target {expected_switch:?}"
        ))));
    }

    let expected_catalog = dependencies.catalog.target();
    let actual_catalog = catalog_head_shared(transaction, &dependencies.catalog.product_id)?;
    if actual_catalog.as_ref() != Some(&expected_catalog) {
        return Ok(Some(missing(format!(
            "active pricing catalog {:?} target {expected_catalog:?}",
            dependencies.catalog.product_id
        ))));
    }
    Ok(None)
}

fn insert_policy(
    transaction: &mut Transaction<'_>,
    spec: &AccountPolicySpec,
    created_ts: i64,
) -> Result<bool> {
    let inserted = transaction.execute(
        "INSERT INTO account_policy_versions(
             account_id,effective_version,policy_id,policy_version,source_policy_digest,
             owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
             switch_generation,content_digest,replacement_locked,created_ts
         ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
         ON CONFLICT(account_id,effective_version) DO NOTHING",
        &[
            &spec.account_id,
            &spec.effective_version,
            &spec.policy_id,
            &spec.policy_version,
            &spec.source_policy_digest,
            &spec.owner_type.as_str(),
            &spec.owner_id,
            &spec.account_class.as_str(),
            &spec.product_id,
            &spec.schema_version,
            &spec.catalog_generation,
            &spec.switch_generation,
            &spec.content_digest,
            &spec.replacement_locked,
            &created_ts,
        ],
    )? == 1;
    if !inserted {
        return Ok(false);
    }
    for rule in &spec.rules {
        let (scope_type, provider_id, canonical_model_id) = rule.scope.db_parts();
        transaction.execute(
            "INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,
                 payable_multiplier_bp,track_eligible,retention_eligible,commission_eligible
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
            &[
                &spec.account_id,
                &spec.effective_version,
                &rule.rule_id,
                &rule.rule_digest,
                &scope_type,
                &provider_id,
                &canonical_model_id,
                &rule.pricing_mode.as_str(),
                &rule.rule_origin.as_str(),
                &rule.discount_bps,
                &rule.payable_multiplier_bp,
                &rule.track_eligible,
                &rule.retention_eligible,
                &rule.commission_eligible,
            ],
        )?;
    }
    Ok(true)
}

pub(crate) fn postgres_prepare_account_policy(
    client: &mut Client,
    incoming: &AccountPolicySpec,
) -> Result<PricingMutation> {
    let incoming = normalize_policy(incoming);
    if let Err(error) = validate_account_policy_shape(&incoming) {
        return Ok(invalid(error));
    }
    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL account policy prepare")?;
    advisory_lock(&mut transaction, &policy_lock_key(&incoming.account_id))?;
    let current_binding = policy_binding_locked(&mut transaction, &incoming.account_id)?;

    if let Some(binding) = &current_binding {
        if !binding_identity_matches(binding, &incoming) {
            return commit_mutation(transaction, version_conflict(), "account policy prepare");
        }
    }
    if let Some(existing) = policy_by_version(
        &mut transaction,
        &incoming.account_id,
        incoming.effective_version,
        true,
    )? {
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "account policy prepare");
    }

    let newest = newest_policy_lineage(&mut transaction, &incoming.account_id)?;
    if let Some(newest) = &newest {
        if incoming.effective_version < newest.target.version
            || incoming.policy_version < newest.policy_version
        {
            return commit_mutation(
                transaction,
                stale(Some(newest.target.clone())),
                "account policy prepare",
            );
        }
        if !policy_identity_matches(newest, &incoming)
            || (incoming.policy_version == newest.policy_version
                && incoming.source_policy_digest != newest.source_policy_digest)
        {
            return commit_mutation(transaction, version_conflict(), "account policy prepare");
        }
    }
    if locked_policy_lineage(&mut transaction, &incoming.account_id)?.is_some() {
        return commit_mutation(transaction, locked(), "account policy prepare");
    }
    match policy_dependencies(&mut transaction, &incoming)? {
        DependencyCheck::Ready(_) => {}
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "account policy prepare");
        }
    }
    if !insert_policy(&mut transaction, &incoming, now())? {
        let existing = policy_by_version(
            &mut transaction,
            &incoming.account_id,
            incoming.effective_version,
            true,
        )?
        .context("policy insert conflict did not expose the conflicting version")?;
        let outcome = if existing == incoming {
            PricingMutation::Unchanged
        } else {
            version_conflict()
        };
        return commit_mutation(transaction, outcome, "account policy prepare");
    }
    commit_mutation(
        transaction,
        PricingMutation::Stored,
        "account policy prepare",
    )
}

pub(crate) fn postgres_activate_account_policy(
    client: &mut Client,
    activation: &AccountPolicyActivationSpec,
    expectation: &PolicyActiveExpectation,
) -> Result<PricingMutation> {
    if let Err(error) = validate_account_policy_activation(activation)
        .and_then(|_| validate_policy_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }
    let target = VersionTarget::new(
        activation.effective_version,
        activation.content_digest.clone(),
    );

    let mut transaction = client
        .transaction()
        .context("begin PostgreSQL account policy activation")?;
    advisory_lock(&mut transaction, &policy_lock_key(&activation.account_id))?;
    let current_binding = policy_binding_locked(&mut transaction, &activation.account_id)?;
    let actual_state = current_binding
        .as_ref()
        .map(StoredPolicyBinding::state)
        .unwrap_or(PolicyBindingState::Unbound);

    let Some(policy) = policy_by_version(
        &mut transaction,
        &activation.account_id,
        activation.effective_version,
        true,
    )?
    else {
        return commit_mutation(
            transaction,
            missing(format!(
                "account policy {:?} effective version {}",
                activation.account_id, activation.effective_version
            )),
            "account policy activation",
        );
    };
    if policy.content_digest != activation.content_digest {
        return commit_mutation(transaction, version_conflict(), "account policy activation");
    }
    if let Some(binding) = &current_binding {
        if !binding_identity_matches(binding, &policy) {
            return commit_mutation(transaction, version_conflict(), "account policy activation");
        }
    }

    let desired = ActivePolicyTarget {
        target: target.clone(),
        binding: activation.binding.clone(),
    };
    if actual_state == PolicyBindingState::Active(desired) {
        return commit_mutation(
            transaction,
            PricingMutation::Unchanged,
            "account policy activation",
        );
    }
    if current_binding
        .as_ref()
        .and_then(|binding| binding.active_target.as_ref())
        .is_some_and(|current| target.version < current.version)
    {
        let actual = current_binding
            .as_ref()
            .and_then(|binding| binding.active_target.clone());
        return commit_mutation(transaction, stale(actual), "account policy activation");
    }
    if !policy_expectation_matches(expectation, &actual_state) {
        return commit_mutation(
            transaction,
            policy_cas_mismatch(actual_state),
            "account policy activation",
        );
    }
    if let Some(locked_history) = locked_policy_lineage(&mut transaction, &activation.account_id)? {
        if locked_history.target != target {
            return commit_mutation(transaction, locked(), "account policy activation");
        }
    }
    let dependencies = match policy_dependencies(&mut transaction, &policy)? {
        DependencyCheck::Ready(dependencies) => dependencies,
        DependencyCheck::Rejected(outcome) => {
            return commit_mutation(transaction, outcome, "account policy activation");
        }
    };
    if let Some(outcome) = active_policy_dependencies(&mut transaction, &dependencies)? {
        return commit_mutation(transaction, outcome, "account policy activation");
    }

    let updated_ts = now();
    let affected = match expectation {
        PolicyActiveExpectation::Unbound => transaction.execute(
            "INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT(account_id) DO NOTHING",
            &[
                &activation.account_id,
                &policy.product_id,
                &policy.account_class.as_str(),
                &activation.effective_version,
                &activation.binding.policy_enforcement.as_str(),
                &activation.binding.funding_enforcement.as_str(),
                &activation.binding.reconciliation_state.as_str(),
                &updated_ts,
            ],
        )?,
        PolicyActiveExpectation::Inactive(expected) => transaction.execute(
            "UPDATE account_policy_bindings
             SET active_effective_version=$2,
                 policy_enforcement=$3,
                 funding_enforcement=$4,
                 reconciliation_state=$5,
                 updated_ts=$6
             WHERE account_id=$1
               AND active_effective_version IS NULL
               AND policy_enforcement=$7
               AND funding_enforcement=$8
               AND reconciliation_state=$9
               AND product_id=$10
               AND account_class=$11",
            &[
                &activation.account_id,
                &activation.effective_version,
                &activation.binding.policy_enforcement.as_str(),
                &activation.binding.funding_enforcement.as_str(),
                &activation.binding.reconciliation_state.as_str(),
                &updated_ts,
                &expected.policy_enforcement.as_str(),
                &expected.funding_enforcement.as_str(),
                &expected.reconciliation_state.as_str(),
                &policy.product_id,
                &policy.account_class.as_str(),
            ],
        )?,
        PolicyActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE account_policy_bindings b
             SET active_effective_version=$2,
                 policy_enforcement=$3,
                 funding_enforcement=$4,
                 reconciliation_state=$5,
                 updated_ts=$6
             WHERE b.account_id=$1
               AND b.active_effective_version=$7
               AND b.policy_enforcement=$8
               AND b.funding_enforcement=$9
               AND b.reconciliation_state=$10
               AND EXISTS (
                   SELECT 1
                   FROM account_policy_versions v
                   WHERE v.account_id=b.account_id
                     AND v.effective_version=b.active_effective_version
                     AND v.product_id=b.product_id
                     AND v.content_digest=$11
               )
               AND b.product_id=$12
               AND b.account_class=$13",
            &[
                &activation.account_id,
                &activation.effective_version,
                &activation.binding.policy_enforcement.as_str(),
                &activation.binding.funding_enforcement.as_str(),
                &activation.binding.reconciliation_state.as_str(),
                &updated_ts,
                &expected.target.version,
                &expected.binding.policy_enforcement.as_str(),
                &expected.binding.funding_enforcement.as_str(),
                &expected.binding.reconciliation_state.as_str(),
                &expected.target.content_digest,
                &policy.product_id,
                &policy.account_class.as_str(),
            ],
        )?,
    };
    if affected != 1 {
        transaction
            .rollback()
            .context("rollback lost PostgreSQL account policy CAS")?;
        return Ok(policy_cas_mismatch(actual_state));
    }
    commit_mutation(
        transaction,
        PricingMutation::Applied,
        "account policy activation",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg::{PgStore, POSTGRES_DESTRUCTIVE_TEST_LOCK};
    use crate::pricing::{PolicySegment, PRICING_SCHEMA_VERSION};
    use std::sync::{Arc, Barrier};
    use tokio_postgres_rustls::MakeRustlsConnect;

    fn connect_client(url: &str) -> Client {
        let config: postgres::Config = url.parse().expect("parse PostgreSQL contract URL");
        let (connector, _certificate_errors) =
            MakeRustlsConnect::with_native_certs().expect("load PostgreSQL root certificates");
        config
            .connect(connector)
            .expect("connect PostgreSQL pricing contract client")
    }

    fn test_client() -> Option<(String, Client)> {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!(
                "skipping PostgreSQL pricing contract: CLAUDE_API_TEST_DATABASE_URL is unset"
            );
            return None;
        };

        let mut client = connect_client(&url);
        client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .expect("serialize destructive PostgreSQL registry tests");

        let mut store = PgStore::connect(&url).expect("connect PostgreSQL migration client");
        store
            .migrate()
            .expect("migrate isolated PostgreSQL contract database");
        drop(store);

        Some((url, client))
    }

    fn catalog(product_id: &str, generation: i64, digest: &str) -> PricingCatalogSpec {
        PricingCatalogSpec {
            product_id: product_id.to_owned(),
            generation,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".to_owned(),
            content_digest: digest.to_owned(),
            entries: vec![
                PricingCatalogEntrySpec {
                    provider_id: "openai".to_owned(),
                    canonical_model_id: "gpt-5".to_owned(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    canonical_model_id: "claude-sonnet-4".to_owned(),
                    enabled: true,
                },
            ],
        }
    }

    fn switches(generation: i64, digest: &str) -> ProviderSwitchSpec {
        switches_for_catalog(generation, 1, digest)
    }

    fn switches_for_catalog(
        generation: i64,
        catalog_generation: i64,
        digest: &str,
    ) -> ProviderSwitchSpec {
        let scoped = [
            (
                "anthropic",
                ProviderSwitchScope::Segment {
                    product_id: "main".to_owned(),
                    segment: PolicySegment::B2b,
                },
            ),
            (
                "openai",
                ProviderSwitchScope::Segment {
                    product_id: "main".to_owned(),
                    segment: PolicySegment::B2b,
                },
            ),
            (
                "anthropic",
                ProviderSwitchScope::Product {
                    product_id: "openkeys".to_owned(),
                },
            ),
            (
                "openai",
                ProviderSwitchScope::Product {
                    product_id: "openkeys".to_owned(),
                },
            ),
        ];
        let mut entries = vec![
            ProviderSwitchEntrySpec {
                provider_id: "anthropic".to_owned(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            },
            ProviderSwitchEntrySpec {
                provider_id: "openai".to_owned(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            },
        ];
        entries.extend(
            scoped
                .into_iter()
                .map(|(provider_id, scope)| ProviderSwitchEntrySpec {
                    provider_id: provider_id.to_owned(),
                    scope,
                    catalog_generation: Some(catalog_generation),
                    enabled: true,
                }),
        );
        ProviderSwitchSpec {
            generation,
            schema_version: PRICING_SCHEMA_VERSION,
            capability_generation: 17,
            capability_digest: "capability-17".to_owned(),
            content_digest: digest.to_owned(),
            entries,
        }
    }

    fn main_b2b_switches_for_catalog(
        generation: i64,
        catalog_generation: i64,
        digest: &str,
    ) -> ProviderSwitchSpec {
        let mut spec = switches_for_catalog(generation, catalog_generation, digest);
        spec.entries.retain(|entry| {
            matches!(entry.scope, ProviderSwitchScope::Master)
                || matches!(
                    &entry.scope,
                    ProviderSwitchScope::Segment {
                        product_id,
                        segment: PolicySegment::B2b,
                    } if product_id == "main"
                )
        });
        spec
    }

    fn b2b_policy(effective_version: i64, policy_version: i64, digest: &str) -> AccountPolicySpec {
        b2b_policy_for_lineage(
            "pricing-pg-contract-b2b",
            effective_version,
            policy_version,
            1,
            1,
            digest,
        )
    }

    fn b2b_policy_for_lineage(
        account_id: &str,
        effective_version: i64,
        policy_version: i64,
        catalog_generation: i64,
        switch_generation: i64,
        digest: &str,
    ) -> AccountPolicySpec {
        AccountPolicySpec {
            account_id: account_id.to_owned(),
            effective_version,
            policy_id: "contract-b2b-policy".to_owned(),
            policy_version,
            source_policy_digest: format!("contract-b2b-source-{policy_version}"),
            owner_type: PolicyOwnerType::B2bClient,
            owner_id: "contract-client".to_owned(),
            account_class: AccountClass::B2b,
            product_id: "main".to_owned(),
            schema_version: PRICING_SCHEMA_VERSION,
            catalog_generation,
            switch_generation,
            content_digest: digest.to_owned(),
            replacement_locked: false,
            rules: vec![AccountPolicyRuleSpec {
                rule_id: format!("contract-b2b-rule-{policy_version}"),
                rule_digest: format!("contract-b2b-rule-digest-{policy_version}"),
                scope: PolicyRuleScope::Provider {
                    provider_id: "anthropic".to_owned(),
                },
                pricing_mode: PricingMode::Discount,
                rule_origin: RuleOrigin::Managed,
                discount_bps: Some(2_000),
                payable_multiplier_bp: 8_000,
                track_eligible: false,
                retention_eligible: false,
                commission_eligible: false,
            }],
        }
    }

    fn openkeys_policy(account_id: &str, effective_version: i64) -> AccountPolicySpec {
        AccountPolicySpec {
            account_id: account_id.to_owned(),
            effective_version,
            policy_id: format!("contract-openkeys-policy-{account_id}"),
            policy_version: effective_version,
            source_policy_digest: format!("contract-openkeys-source-{effective_version}"),
            owner_type: PolicyOwnerType::OpenKeys,
            owner_id: account_id.to_owned(),
            account_class: AccountClass::OpenKeys,
            product_id: "openkeys".to_owned(),
            schema_version: PRICING_SCHEMA_VERSION,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: format!("contract-openkeys-policy-{effective_version}"),
            replacement_locked: true,
            rules: ["anthropic", "openai"]
                .into_iter()
                .map(|provider_id| AccountPolicyRuleSpec {
                    rule_id: format!("contract-openkeys-{provider_id}-{effective_version}"),
                    rule_digest: format!(
                        "contract-openkeys-{provider_id}-digest-{effective_version}"
                    ),
                    scope: PolicyRuleScope::Provider {
                        provider_id: provider_id.to_owned(),
                    },
                    pricing_mode: PricingMode::Discount,
                    rule_origin: RuleOrigin::Legacy,
                    discount_bps: None,
                    payable_multiplier_bp: 7_300,
                    track_eligible: false,
                    retention_eligible: false,
                    commission_eligible: false,
                })
                .collect(),
        }
    }

    fn binding(
        policy_enforcement: PolicyEnforcement,
        funding_enforcement: FundingEnforcement,
        reconciliation_state: ReconciliationState,
    ) -> AccountPolicyBindingSpec {
        AccountPolicyBindingSpec {
            policy_enforcement,
            funding_enforcement,
            reconciliation_state,
        }
    }

    fn activation(
        policy: &AccountPolicySpec,
        binding: AccountPolicyBindingSpec,
    ) -> AccountPolicyActivationSpec {
        AccountPolicyActivationSpec {
            account_id: policy.account_id.clone(),
            effective_version: policy.effective_version,
            content_digest: policy.content_digest.clone(),
            binding,
        }
    }

    fn is_missing(mutation: &PricingMutation) -> bool {
        matches!(
            mutation,
            PricingMutation::Rejected(PricingRejection::MissingDependency { .. })
        )
    }

    fn assert_active_bundle_lineages(
        client: &mut Client,
        account_id: &str,
        expected_policy: &AccountPolicySpec,
        expected_binding: &AccountPolicyBindingSpec,
        expected_policy_catalog: &PricingCatalogSpec,
        expected_policy_switches: &ProviderSwitchSpec,
        expected_admission_catalog: &PricingCatalogSpec,
        expected_admission_switches: &ProviderSwitchSpec,
    ) {
        let bundle = postgres_pricing_read_bundle(client, account_id)
            .expect("read PostgreSQL dual-lineage pricing bundle");
        assert_eq!(bundle.account_id, account_id);
        assert_eq!(bundle.account_multiplier_bp, 8_000);
        assert_eq!(
            bundle.policy,
            PricingPolicySnapshot::Active(ActiveAccountPolicy {
                policy: normalize_policy(expected_policy),
                binding: expected_binding.clone(),
            })
        );
        assert_eq!(
            bundle.policy_catalog,
            Some(normalize_catalog(expected_policy_catalog))
        );
        assert_eq!(
            bundle.policy_switches,
            Some(normalize_switches(expected_policy_switches))
        );
        assert_eq!(
            bundle.admission_catalog,
            Some(normalize_catalog(expected_admission_catalog))
        );
        assert_eq!(
            bundle.admission_switches,
            Some(normalize_switches(expected_admission_switches))
        );
    }

    fn run_postgres_dual_lineage_rollout_matrix(client: &mut Client) {
        const ACCOUNT_ID: &str = "pricing-pg-contract-dual-lineage";

        client
            .batch_execute(
                "TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;
                 INSERT INTO accounts(id,mult_bp,status,created_ts,created)
                 VALUES('pricing-pg-contract-dual-lineage',8000,'active',1,'')
                 ON CONFLICT(id) DO UPDATE SET mult_bp=EXCLUDED.mult_bp,status='active';",
            )
            .expect("reset PostgreSQL dual-lineage fixtures");

        let catalog_v1 = catalog("main", 1, "dual-lineage-catalog-1");
        let catalog_v2 = catalog("main", 2, "dual-lineage-catalog-2");
        let switches_v1 = main_b2b_switches_for_catalog(1, 1, "dual-lineage-switches-1");
        let switches_v2 = main_b2b_switches_for_catalog(2, 2, "dual-lineage-switches-2");
        let policy_v1 = b2b_policy_for_lineage(ACCOUNT_ID, 1, 1, 1, 1, "dual-lineage-policy-1");
        let policy_v2 = b2b_policy_for_lineage(ACCOUNT_ID, 2, 2, 2, 2, "dual-lineage-policy-2");
        let binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Pending,
        );

        for catalog in [&catalog_v1, &catalog_v2] {
            assert_eq!(
                postgres_prepare_pricing_catalog(client, catalog).unwrap(),
                PricingMutation::Stored
            );
        }
        for switches in [&switches_v1, &switches_v2] {
            assert_eq!(
                postgres_prepare_provider_switches(client, switches).unwrap(),
                PricingMutation::Stored
            );
        }
        for policy in [&policy_v1, &policy_v2] {
            assert_eq!(
                postgres_prepare_account_policy(client, policy).unwrap(),
                PricingMutation::Stored
            );
        }

        assert_eq!(
            postgres_activate_pricing_catalog(
                client,
                "main",
                &catalog_v1.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_activate_provider_switches(
                client,
                &switches_v1.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_pricing_read_bundle(client, ACCOUNT_ID).unwrap(),
            PricingReadBundle {
                account_id: ACCOUNT_ID.to_owned(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Unbound,
                policy_catalog: None,
                policy_switches: None,
                admission_catalog: None,
                admission_switches: None,
            },
            "an unbound account has no product context even when global heads exist"
        );
        assert_eq!(
            postgres_activate_account_policy(
                client,
                &activation(&policy_v1, binding.clone()),
                &PolicyActiveExpectation::Unbound,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        // C1/S1/P1: both lineages initially agree.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v1,
            &binding,
            &catalog_v1,
            &switches_v1,
            &catalog_v1,
            &switches_v1,
        );

        assert_eq!(
            postgres_activate_pricing_catalog(
                client,
                "main",
                &catalog_v2.target(),
                &ActiveExpectation::Exact(catalog_v1.target()),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // C2/S1/P1: admission sees C2 while policy resolution remains pinned to C1/S1.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v1,
            &binding,
            &catalog_v1,
            &switches_v1,
            &catalog_v2,
            &switches_v1,
        );

        assert_eq!(
            postgres_activate_provider_switches(
                client,
                &switches_v2.target(),
                &ActiveExpectation::Exact(switches_v1.target()),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // C2/S2/P1: admission has advanced fully; the old policy still resolves C1/S1.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v1,
            &binding,
            &catalog_v1,
            &switches_v1,
            &catalog_v2,
            &switches_v2,
        );

        assert_eq!(
            postgres_activate_account_policy(
                client,
                &activation(&policy_v2, binding.clone()),
                &PolicyActiveExpectation::Exact(ActivePolicyTarget {
                    target: policy_v1.target(),
                    binding: binding.clone(),
                }),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // C2/S2/P2: policy and admission lineages converge again.
        assert_active_bundle_lineages(
            client,
            ACCOUNT_ID,
            &policy_v2,
            &binding,
            &catalog_v2,
            &switches_v2,
            &catalog_v2,
            &switches_v2,
        );

        client
            .batch_execute(
                "TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;
                 DELETE FROM accounts WHERE id='pricing-pg-contract-dual-lineage';",
            )
            .expect("clean PostgreSQL dual-lineage fixtures");
    }

    /// Run against an isolated database:
    /// `CLAUDE_API_TEST_DATABASE_URL=postgresql://... cargo test -p registry \
    /// pricing::postgres::tests::postgres_pricing_contract_matrix -- --nocapture`
    #[test]
    fn postgres_pricing_contract_matrix() {
        let Some((url, mut client)) = test_client() else {
            return;
        };
        client
            .batch_execute(
                "SET statement_timeout='15s';
                 SET lock_timeout='5s';
                 TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;
                 INSERT INTO accounts(id,mult_bp,status,created_ts,created) VALUES
                     ('pricing-pg-contract-b2b',8000,'active',1,''),
                     ('pricing-pg-contract-openkeys',7300,'active',1,''),
                     ('pricing-pg-contract-openkeys-mismatch',7400,'active',1,'')
                 ON CONFLICT(id) DO UPDATE SET mult_bp=EXCLUDED.mult_bp,status='active';",
            )
            .expect("reset isolated PostgreSQL pricing fixtures");

        client.batch_execute("BEGIN").unwrap();
        client
            .query_one(
                "SELECT mult_bp FROM accounts
                 WHERE id='pricing-pg-contract-openkeys'
                 FOR SHARE",
                &[],
            )
            .unwrap();
        let mut scalar_writer = connect_client(&url);
        scalar_writer
            .batch_execute("SET lock_timeout='200ms'")
            .unwrap();
        assert!(scalar_writer
            .execute(
                "UPDATE accounts SET mult_bp=7400
                 WHERE id='pricing-pg-contract-openkeys'",
                &[],
            )
            .is_err());
        client.batch_execute("ROLLBACK").unwrap();
        assert_eq!(
            scalar_writer
                .execute(
                    "UPDATE accounts SET mult_bp=7300
                     WHERE id='pricing-pg-contract-openkeys'",
                    &[],
                )
                .unwrap(),
            1
        );

        client
            .batch_execute(
                "DROP TRIGGER IF EXISTS pricing_contract_reject_child
                     ON pricing_catalog_entries;
                 DROP FUNCTION IF EXISTS pricing_contract_reject_child();
                 CREATE FUNCTION pricing_contract_reject_child()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.provider_id = 'openai' THEN
                         RAISE EXCEPTION 'injected pricing child failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER pricing_contract_reject_child
                 BEFORE INSERT ON pricing_catalog_entries
                 FOR EACH ROW EXECUTE FUNCTION pricing_contract_reject_child();",
            )
            .unwrap();
        assert!(postgres_prepare_pricing_catalog(
            &mut client,
            &catalog("rollback", 1, "rollback-catalog")
        )
        .is_err());
        let rollback_counts: (i64, i64) = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*)::bigint FROM pricing_catalog_versions
                       WHERE product_id='rollback'),
                     (SELECT COUNT(*)::bigint FROM pricing_catalog_entries
                       WHERE product_id='rollback')",
                &[],
            )
            .map(|row| (row.get(0), row.get(1)))
            .unwrap();
        assert_eq!(rollback_counts, (0, 0));
        client
            .batch_execute(
                "DROP TRIGGER pricing_contract_reject_child ON pricing_catalog_entries;
                 DROP FUNCTION pricing_contract_reject_child();",
            )
            .unwrap();

        let main_catalog = catalog("main", 1, "main-catalog-1");
        let openkeys_catalog = catalog("openkeys", 1, "openkeys-catalog-1");
        let switch_v1 = switches(1, "switches-1");
        let b2b_v1 = b2b_policy(1, 1, "b2b-policy-1");
        let b2b_v3 = b2b_policy(3, 3, "b2b-policy-3");
        let openkeys_v1 = openkeys_policy("pricing-pg-contract-openkeys", 1);

        assert_eq!(
            postgres_pricing_read_bundle(&mut client, "pricing-pg-contract-openkeys-mismatch",)
                .unwrap(),
            PricingReadBundle {
                account_id: "pricing-pg-contract-openkeys-mismatch".to_owned(),
                account_multiplier_bp: 7_400,
                policy: PricingPolicySnapshot::Unbound,
                policy_catalog: None,
                policy_switches: None,
                admission_catalog: None,
                admission_switches: None,
            }
        );

        let mut malformed_policy = b2b_v1.clone();
        malformed_policy.effective_version = 0;
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &malformed_policy).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        let mut wrong_product_policy = b2b_v1.clone();
        wrong_product_policy.product_id = "other-product".to_owned();
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &wrong_product_policy).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &main_catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &openkeys_catalog).unwrap(),
            PricingMutation::Stored
        );

        client
            .batch_execute(
                "CREATE FUNCTION pricing_contract_reject_switch_child()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.provider_id = 'openai' THEN
                         RAISE EXCEPTION 'injected switch child failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER pricing_contract_reject_switch_child
                 BEFORE INSERT ON provider_switch_entries
                 FOR EACH ROW EXECUTE FUNCTION pricing_contract_reject_switch_child();",
            )
            .unwrap();
        assert!(postgres_prepare_provider_switches(&mut client, &switch_v1).is_err());
        let switch_rollback_counts: (i64, i64) = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*)::bigint FROM provider_switch_versions
                       WHERE generation=1),
                     (SELECT COUNT(*)::bigint FROM provider_switch_entries
                       WHERE generation=1)",
                &[],
            )
            .map(|row| (row.get(0), row.get(1)))
            .unwrap();
        assert_eq!(switch_rollback_counts, (0, 0));
        client
            .batch_execute(
                "DROP TRIGGER pricing_contract_reject_switch_child
                     ON provider_switch_entries;
                 DROP FUNCTION pricing_contract_reject_switch_child();",
            )
            .unwrap();
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v1).unwrap(),
            PricingMutation::Stored
        );

        client
            .batch_execute(
                "CREATE FUNCTION pricing_contract_reject_policy_child()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NEW.provider_id = 'openai' THEN
                         RAISE EXCEPTION 'injected policy child failure';
                     END IF;
                     RETURN NEW;
                 END;
                 $$;
                 CREATE TRIGGER pricing_contract_reject_policy_child
                 BEFORE INSERT ON account_policy_rules
                 FOR EACH ROW EXECUTE FUNCTION pricing_contract_reject_policy_child();",
            )
            .unwrap();
        assert!(postgres_prepare_account_policy(&mut client, &openkeys_v1).is_err());
        let policy_rollback_counts: (i64, i64) = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*)::bigint FROM account_policy_versions
                       WHERE account_id='pricing-pg-contract-openkeys'),
                     (SELECT COUNT(*)::bigint FROM account_policy_rules
                       WHERE account_id='pricing-pg-contract-openkeys')",
                &[],
            )
            .map(|row| (row.get(0), row.get(1)))
            .unwrap();
        assert_eq!(policy_rollback_counts, (0, 0));
        client
            .batch_execute(
                "DROP TRIGGER pricing_contract_reject_policy_child
                     ON account_policy_rules;
                 DROP FUNCTION pricing_contract_reject_policy_child();",
            )
            .unwrap();
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v1).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &openkeys_v1).unwrap(),
            PricingMutation::Stored
        );

        // Prepare persists complete immutable lineage but never creates or moves a head/binding.
        let head_and_binding_count: i64 = client
            .query_one(
                "SELECT
                     (SELECT COUNT(*) FROM pricing_catalog_heads)
                   + (SELECT COUNT(*) FROM provider_switch_head)
                   + (SELECT COUNT(*) FROM account_policy_bindings)",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(head_and_binding_count, 0);
        assert_eq!(
            postgres_pricing_catalog_by_generation(&mut client, "main", 1).unwrap(),
            Some(normalize_catalog(&main_catalog))
        );
        assert_eq!(
            postgres_provider_switches_by_generation(&mut client, 1).unwrap(),
            Some(normalize_switches(&switch_v1))
        );
        assert_eq!(
            postgres_account_policy_by_version(&mut client, "pricing-pg-contract-openkeys", 1)
                .unwrap(),
            Some(normalize_policy(&openkeys_v1))
        );
        let registry_timestamp: i64 = client
            .query_one(
                "SELECT created_ts FROM account_policy_versions
                 WHERE account_id='pricing-pg-contract-openkeys' AND effective_version=1",
                &[],
            )
            .unwrap()
            .get(0);
        assert!(registry_timestamp > 0);

        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &main_catalog).unwrap(),
            PricingMutation::Unchanged
        );
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v1).unwrap(),
            PricingMutation::Unchanged
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v1).unwrap(),
            PricingMutation::Unchanged
        );

        let race_v1 = catalog("race", 1, "race-catalog-1");
        let race_v2 = catalog("race", 2, "race-catalog-2");
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &race_v1).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &race_v2).unwrap(),
            PricingMutation::Stored
        );
        let barrier = Arc::new(Barrier::new(2));
        let racers = [race_v1.target(), race_v2.target()].map(|target| {
            let url = url.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut racer = connect_client(&url);
                barrier.wait();
                postgres_activate_pricing_catalog(
                    &mut racer,
                    "race",
                    &target,
                    &ActiveExpectation::Absent,
                )
                .unwrap()
            })
        });
        let race_outcomes = racers.map(|racer| racer.join().expect("pricing CAS racer"));
        assert_eq!(
            race_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(race_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(
                PricingRejection::CasMismatch { .. } | PricingRejection::Stale { .. }
            )
        )));

        let mut catalog_conflict = main_catalog.clone();
        catalog_conflict.content_digest = "main-catalog-conflict".to_owned();
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &catalog_conflict).unwrap(),
            version_conflict()
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &catalog("history", 1, "history-1"))
                .unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_pricing_catalog(&mut client, &catalog("history", 3, "history-3"))
                .unwrap(),
            PricingMutation::Stored
        );
        assert!(matches!(
            postgres_prepare_pricing_catalog(&mut client, &catalog("history", 2, "history-2"))
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));

        let switch_v3 = switches(3, "switches-3");
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v3).unwrap(),
            PricingMutation::Stored
        );
        assert!(matches!(
            postgres_prepare_provider_switches(&mut client, &switches(2, "switches-2")).unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));
        let mut switch_conflict = switch_v1.clone();
        switch_conflict.content_digest = "switches-conflict".to_owned();
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_conflict).unwrap(),
            version_conflict()
        );

        let mut b2b_conflict = b2b_v1.clone();
        b2b_conflict.content_digest = "b2b-policy-conflict".to_owned();
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_conflict).unwrap(),
            version_conflict()
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v3).unwrap(),
            PricingMutation::Stored
        );
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &b2b_policy(2, 2, "b2b-policy-2"))
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));

        let openkeys_v2 = openkeys_policy("pricing-pg-contract-openkeys", 2);
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &openkeys_v2).unwrap(),
            locked()
        );
        let mismatched_openkeys = openkeys_policy("pricing-pg-contract-openkeys-mismatch", 1);
        assert!(matches!(
            postgres_prepare_account_policy(&mut client, &mismatched_openkeys).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        let inactive = binding(
            PolicyEnforcement::LegacyScalar,
            FundingEnforcement::LegacySingle,
            ReconciliationState::Pending,
        );
        client
            .execute(
                "INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES($1,'main','b2b',NULL,$2,$3,$4,1)",
                &[
                    &b2b_v1.account_id,
                    &inactive.policy_enforcement.as_str(),
                    &inactive.funding_enforcement.as_str(),
                    &inactive.reconciliation_state.as_str(),
                ],
            )
            .unwrap();
        assert_eq!(
            postgres_active_account_policy(&mut client, &b2b_v1.account_id).unwrap(),
            None
        );
        assert_eq!(
            postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap(),
            PricingReadBundle {
                account_id: b2b_v1.account_id.clone(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Inactive {
                    product_id: "main".to_owned(),
                    account_class: AccountClass::B2b,
                    binding: inactive.clone(),
                },
                policy_catalog: None,
                policy_switches: None,
                admission_catalog: None,
                admission_switches: None,
            }
        );

        let active_v1_binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Pending,
        );
        let activate_v1 = activation(&b2b_v1, active_v1_binding.clone());
        let strict = activation(
            &b2b_v1,
            binding(
                PolicyEnforcement::Strict,
                FundingEnforcement::Strict,
                ReconciliationState::Verified,
            ),
        );
        assert!(matches!(
            postgres_activate_account_policy(
                &mut client,
                &strict,
                &PolicyActiveExpectation::Inactive(inactive.clone())
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));
        assert!(is_missing(
            &postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Inactive(inactive.clone())
            )
            .unwrap()
        ));
        let still_null: Option<i64> = client
            .query_one(
                "SELECT active_effective_version FROM account_policy_bindings
                 WHERE account_id=$1",
                &[&b2b_v1.account_id],
            )
            .unwrap()
            .get(0);
        assert_eq!(still_null, None);

        // A prepared switch is not activatable until every exact pinned catalog is active.
        assert!(is_missing(
            &postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Absent
            )
            .unwrap()
        ));
        assert_eq!(
            postgres_active_provider_switches(&mut client).unwrap(),
            None
        );

        assert_eq!(
            postgres_activate_pricing_catalog(
                &mut client,
                "main",
                &main_catalog.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Applied
        );
        // Lost ACK is idempotent even when the retry carries an obsolete expectation.
        assert_eq!(
            postgres_activate_pricing_catalog(
                &mut client,
                "main",
                &main_catalog.target(),
                &ActiveExpectation::Exact(VersionTarget::new(99, "obsolete"))
            )
            .unwrap(),
            PricingMutation::Unchanged
        );
        let catalog_only = postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap();
        assert_eq!(catalog_only.policy_catalog, None);
        assert_eq!(catalog_only.policy_switches, None);
        assert_eq!(
            catalog_only.admission_catalog,
            Some(normalize_catalog(&main_catalog))
        );
        assert_eq!(catalog_only.admission_switches, None);
        assert!(matches!(
            catalog_only.policy,
            PricingPolicySnapshot::Inactive { .. }
        ));
        assert!(is_missing(
            &postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Absent
            )
            .unwrap()
        ));
        assert_eq!(
            postgres_activate_pricing_catalog(
                &mut client,
                "openkeys",
                &openkeys_catalog.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Applied
        );
        let active_heads = postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap();
        assert_eq!(active_heads.policy_catalog, None);
        assert_eq!(active_heads.policy_switches, None);
        assert_eq!(
            active_heads.admission_catalog,
            Some(normalize_catalog(&main_catalog))
        );
        assert_eq!(
            active_heads.admission_switches,
            Some(normalize_switches(&switch_v1))
        );
        assert!(matches!(
            active_heads.policy,
            PricingPolicySnapshot::Inactive { .. }
        ));
        assert_eq!(
            postgres_activate_provider_switches(
                &mut client,
                &switch_v1.target(),
                &ActiveExpectation::Exact(VersionTarget::new(99, "obsolete"))
            )
            .unwrap(),
            PricingMutation::Unchanged
        );
        assert!(matches!(
            postgres_activate_provider_switches(
                &mut client,
                &switch_v3.target(),
                &ActiveExpectation::Absent
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::CasMismatch { .. })
        ));

        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Unbound
            )
            .unwrap(),
            policy_cas_mismatch(PolicyBindingState::Inactive(inactive.clone()))
        );
        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Inactive(inactive)
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap(),
            PricingReadBundle {
                account_id: b2b_v1.account_id.clone(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Active(ActiveAccountPolicy {
                    policy: normalize_policy(&b2b_v1),
                    binding: active_v1_binding.clone(),
                }),
                policy_catalog: Some(normalize_catalog(&main_catalog)),
                policy_switches: Some(normalize_switches(&switch_v1)),
                admission_catalog: Some(normalize_catalog(&main_catalog)),
                admission_switches: Some(normalize_switches(&switch_v1)),
            }
        );
        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Unbound
            )
            .unwrap(),
            PricingMutation::Unchanged
        );

        let active_v3_binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Verified,
        );
        let activate_v3 = activation(&b2b_v3, active_v3_binding.clone());
        let wrong_expected = PolicyActiveExpectation::Exact(ActivePolicyTarget {
            target: b2b_v1.target(),
            binding: binding(
                PolicyEnforcement::LegacyScalar,
                FundingEnforcement::LegacySingle,
                ReconciliationState::Pending,
            ),
        });
        assert!(matches!(
            postgres_activate_account_policy(&mut client, &activate_v3, &wrong_expected).unwrap(),
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { .. })
        ));
        let expected_v1 = PolicyActiveExpectation::Exact(ActivePolicyTarget {
            target: b2b_v1.target(),
            binding: active_v1_binding,
        });
        assert_eq!(
            postgres_activate_account_policy(&mut client, &activate_v3, &expected_v1).unwrap(),
            PricingMutation::Applied
        );
        assert!(matches!(
            postgres_activate_account_policy(
                &mut client,
                &activate_v1,
                &PolicyActiveExpectation::Exact(ActivePolicyTarget {
                    target: b2b_v3.target(),
                    binding: active_v3_binding.clone(),
                })
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale { .. })
        ));
        assert_eq!(
            postgres_active_account_policy(&mut client, &b2b_v1.account_id)
                .unwrap()
                .expect("active B2B policy"),
            ActiveAccountPolicy {
                policy: normalize_policy(&b2b_v3),
                binding: active_v3_binding.clone(),
            }
        );

        let openkeys_binding = binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Verified,
        );
        let activate_openkeys = activation(&openkeys_v1, openkeys_binding.clone());
        assert_eq!(
            postgres_activate_account_policy(
                &mut client,
                &activate_openkeys,
                &PolicyActiveExpectation::Unbound
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            postgres_active_account_policy(&mut client, &openkeys_v1.account_id)
                .unwrap()
                .expect("active OpenKeys policy"),
            ActiveAccountPolicy {
                policy: normalize_policy(&openkeys_v1),
                binding: openkeys_binding,
            }
        );

        let b2b_v4 = b2b_policy(4, 4, "b2b-policy-4");
        let b2b_v5 = b2b_policy(5, 5, "b2b-policy-5");
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v4).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            postgres_prepare_account_policy(&mut client, &b2b_v5).unwrap(),
            PricingMutation::Stored
        );
        let policy_race_barrier = Arc::new(Barrier::new(2));
        let expected_v3 = PolicyActiveExpectation::Exact(ActivePolicyTarget {
            target: b2b_v3.target(),
            binding: active_v3_binding.clone(),
        });
        let policy_racers = [b2b_v4, b2b_v5].map(|policy| {
            let url = url.clone();
            let barrier = Arc::clone(&policy_race_barrier);
            let expected = expected_v3.clone();
            let desired_binding = active_v3_binding.clone();
            std::thread::spawn(move || {
                let mut racer = connect_client(&url);
                let activation = activation(&policy, desired_binding);
                barrier.wait();
                postgres_activate_account_policy(&mut racer, &activation, &expected).unwrap()
            })
        });
        let policy_race_outcomes = policy_racers.map(|racer| racer.join().unwrap());
        assert_eq!(
            policy_race_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(policy_race_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { .. })
                | PricingMutation::Rejected(PricingRejection::Stale { .. })
        )));

        let switch_v4 = switches(4, "switches-4");
        assert_eq!(
            postgres_prepare_provider_switches(&mut client, &switch_v4).unwrap(),
            PricingMutation::Stored
        );
        let switch_race_barrier = Arc::new(Barrier::new(2));
        let switch_racers = [switch_v3.target(), switch_v4.target()].map(|target| {
            let url = url.clone();
            let barrier = Arc::clone(&switch_race_barrier);
            let expected = ActiveExpectation::Exact(switch_v1.target());
            std::thread::spawn(move || {
                let mut racer = connect_client(&url);
                barrier.wait();
                postgres_activate_provider_switches(&mut racer, &target, &expected).unwrap()
            })
        });
        let switch_race_outcomes = switch_racers.map(|racer| racer.join().unwrap());
        assert_eq!(
            switch_race_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(switch_race_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(PricingRejection::CasMismatch { .. })
                | PricingMutation::Rejected(PricingRejection::Stale { .. })
        )));
        let torn = postgres_pricing_read_bundle(&mut client, &b2b_v1.account_id).unwrap();
        let active_policy = match torn.policy {
            PricingPolicySnapshot::Active(active) => active,
            other => panic!("expected active PostgreSQL pricing policy, got {other:?}"),
        };
        assert_eq!(active_policy.policy.catalog_generation, 1);
        assert_eq!(active_policy.policy.switch_generation, 1);
        assert_eq!(torn.policy_catalog, Some(normalize_catalog(&main_catalog)));
        assert_eq!(torn.policy_switches, Some(normalize_switches(&switch_v1)));
        assert_eq!(
            torn.admission_catalog,
            Some(normalize_catalog(&main_catalog))
        );
        assert_ne!(
            torn.admission_switches
                .expect("current PostgreSQL switch head")
                .generation,
            active_policy.policy.switch_generation
        );

        client
            .batch_execute(
                "TRUNCATE
                     account_policy_bindings,
                     account_policy_rules,
                     account_policy_versions,
                     provider_switch_head,
                     provider_switch_entries,
                     provider_switch_versions,
                     pricing_catalog_heads,
                     pricing_catalog_entries,
                     pricing_catalog_versions
                 CASCADE;",
            )
            .expect("clean PostgreSQL pricing contract fixtures");
        client
            .execute(
                "DELETE FROM accounts WHERE id LIKE 'pricing-pg-contract-%'",
                &[],
            )
            .expect("clean PostgreSQL pricing contract accounts");

        run_postgres_dual_lineage_rollout_matrix(&mut client);

        client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .expect("unlock PostgreSQL pricing contract fixture");
    }
}
