use super::{
    expectation_matches, invalid, missing, normalize_catalog, normalize_policy, normalize_switches,
    policy_expectation_matches, require_id, required_catalog_generations, validate_account_policy,
    validate_account_policy_binding, validate_account_policy_shape, validate_active_expectation,
    validate_policy_active_expectation, validate_pricing_catalog, validate_provider_switches,
    validate_version_target, AccountClass, AccountPolicyActivationSpec, AccountPolicyBindingSpec,
    AccountPolicyRuleSpec, AccountPolicySpec, ActiveAccountPolicy, ActiveExpectation,
    ActivePolicyTarget, FundingEnforcement, PolicyActiveExpectation, PolicyBindingState,
    PolicyEnforcement, PolicyOwnerType, PolicyRuleScope, PricingCatalogEntrySpec,
    PricingCatalogSpec, PricingMode, PricingMutation, PricingPolicySnapshot, PricingReadBundle,
    PricingRejection, ProviderSwitchEntrySpec, ProviderSwitchScope, ProviderSwitchSpec,
    ReconciliationState, RuleOrigin, VersionTarget,
};
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn immediate(conn: &Connection) -> Result<Transaction<'_>> {
    Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .context("begin SQLite pricing transaction")
}

fn finish(transaction: Transaction<'_>, mutation: PricingMutation) -> Result<PricingMutation> {
    transaction
        .commit()
        .context("commit SQLite pricing transaction")?;
    Ok(mutation)
}

pub fn sqlite_pricing_catalog_by_generation(
    conn: &Connection,
    product_id: &str,
    generation: i64,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    if generation <= 0 {
        bail!("catalog generation must be positive");
    }
    let Some(mut catalog) = conn
        .query_row(
            "SELECT product_id, generation, schema_version, capability_generation,
                    capability_digest, content_digest
               FROM pricing_catalog_versions
              WHERE product_id=?1 AND generation=?2",
            params![product_id, generation],
            |row| {
                Ok(PricingCatalogSpec {
                    product_id: row.get(0)?,
                    generation: row.get(1)?,
                    schema_version: row.get(2)?,
                    capability_generation: row.get(3)?,
                    capability_digest: row.get(4)?,
                    content_digest: row.get(5)?,
                    entries: Vec::new(),
                })
            },
        )
        .optional()
        .context("read SQLite pricing catalog version")?
    else {
        return Ok(None);
    };

    let mut statement = conn
        .prepare(
            "SELECT provider_id, canonical_model_id, enabled
               FROM pricing_catalog_entries
              WHERE product_id=?1 AND generation=?2
              ORDER BY provider_id, canonical_model_id",
        )
        .context("prepare SQLite pricing catalog entries read")?;
    catalog.entries = statement
        .query_map(params![product_id, generation], |row| {
            Ok(PricingCatalogEntrySpec {
                provider_id: row.get(0)?,
                canonical_model_id: row.get(1)?,
                enabled: row.get(2)?,
            })
        })
        .context("read SQLite pricing catalog entries")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("decode SQLite pricing catalog entries")?;

    validate_pricing_catalog(&catalog).context("invalid stored SQLite pricing catalog")?;
    Ok(Some(normalize_catalog(&catalog)))
}

pub fn sqlite_active_pricing_catalog(
    conn: &Connection,
    product_id: &str,
) -> Result<Option<PricingCatalogSpec>> {
    require_id("product id", product_id)?;
    let generation = conn
        .query_row(
            "SELECT active_generation
               FROM pricing_catalog_heads
              WHERE product_id=?1",
            params![product_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .context("read SQLite active pricing catalog head")?;
    let Some(generation) = generation else {
        return Ok(None);
    };
    sqlite_pricing_catalog_by_generation(conn, product_id, generation)?
        .ok_or_else(|| {
            anyhow!(
            "SQLite pricing catalog head references missing version {product_id:?}/{generation}"
        )
        })
        .map(Some)
}

pub fn sqlite_prepare_pricing_catalog(
    conn: &Connection,
    incoming: &PricingCatalogSpec,
) -> Result<PricingMutation> {
    let incoming = normalize_catalog(incoming);
    if let Err(error) = validate_pricing_catalog(&incoming) {
        return Ok(invalid(error));
    }

    let transaction = immediate(conn)?;
    if let Some(stored) = sqlite_pricing_catalog_by_generation(
        &transaction,
        &incoming.product_id,
        incoming.generation,
    )? {
        let result = if stored == incoming {
            PricingMutation::Unchanged
        } else {
            PricingMutation::Rejected(PricingRejection::VersionConflict)
        };
        return finish(transaction, result);
    }

    if let Some(latest) = latest_catalog_target(&transaction, &incoming.product_id)? {
        if incoming.generation < latest.version {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::Stale {
                    actual: Some(latest),
                }),
            );
        }
    }

    transaction
        .execute(
            "INSERT INTO pricing_catalog_versions(
                 product_id, generation, schema_version, capability_generation,
                 capability_digest, content_digest, created_ts
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                &incoming.product_id,
                incoming.generation,
                incoming.schema_version,
                incoming.capability_generation,
                &incoming.capability_digest,
                &incoming.content_digest,
                now(),
            ],
        )
        .context("insert SQLite pricing catalog version")?;
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO pricing_catalog_entries(
                     product_id, generation, provider_id, canonical_model_id, enabled
                 ) VALUES(?1,?2,?3,?4,?5)",
            )
            .context("prepare SQLite pricing catalog entry insert")?;
        for entry in &incoming.entries {
            statement
                .execute(params![
                    &incoming.product_id,
                    incoming.generation,
                    &entry.provider_id,
                    &entry.canonical_model_id,
                    entry.enabled,
                ])
                .context("insert SQLite pricing catalog entry")?;
        }
    }
    finish(transaction, PricingMutation::Stored)
}

pub fn sqlite_activate_pricing_catalog(
    conn: &Connection,
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

    let transaction = immediate(conn)?;
    let Some(prepared) =
        sqlite_pricing_catalog_by_generation(&transaction, product_id, target.version)?
    else {
        return finish(
            transaction,
            missing(format!(
                "pricing catalog {product_id:?} generation {}",
                target.version
            )),
        );
    };
    if prepared.content_digest != target.content_digest {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::VersionConflict),
        );
    }

    let current = sqlite_active_pricing_catalog(&transaction, product_id)?;
    let actual = current.as_ref().map(PricingCatalogSpec::target);
    if actual.as_ref() == Some(target) {
        return finish(transaction, PricingMutation::Unchanged);
    }
    if let Some(active) = actual.as_ref() {
        if target.version < active.version {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::Stale {
                    actual: actual.clone(),
                }),
            );
        }
        if target.version == active.version {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::VersionConflict),
            );
        }
    }
    if !expectation_matches(expectation, actual.as_ref()) {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::CasMismatch { actual }),
        );
    }

    let changed = match expectation {
        ActiveExpectation::Absent => transaction.execute(
            "INSERT INTO pricing_catalog_heads(product_id, active_generation, updated_ts)
             VALUES(?1,?2,?3)",
            params![product_id, target.version, now()],
        )?,
        ActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE pricing_catalog_heads
                SET active_generation=?1, updated_ts=?2
              WHERE product_id=?3
                AND active_generation=?4
                AND EXISTS (
                    SELECT 1
                      FROM pricing_catalog_versions v
                     WHERE v.product_id=pricing_catalog_heads.product_id
                       AND v.generation=pricing_catalog_heads.active_generation
                       AND v.content_digest=?5
                )",
            params![
                target.version,
                now(),
                product_id,
                expected.version,
                &expected.content_digest,
            ],
        )?,
    };
    if changed != 1 {
        transaction
            .rollback()
            .context("rollback lost SQLite pricing catalog CAS")?;
        return Ok(PricingMutation::Rejected(PricingRejection::CasMismatch {
            actual,
        }));
    }
    finish(transaction, PricingMutation::Applied)
}

pub fn sqlite_provider_switches_by_generation(
    conn: &Connection,
    generation: i64,
) -> Result<Option<ProviderSwitchSpec>> {
    if generation <= 0 {
        bail!("provider switch generation must be positive");
    }
    let Some(mut switches) = conn
        .query_row(
            "SELECT generation, schema_version, capability_generation,
                    capability_digest, content_digest
               FROM provider_switch_versions
              WHERE generation=?1",
            params![generation],
            |row| {
                Ok(ProviderSwitchSpec {
                    generation: row.get(0)?,
                    schema_version: row.get(1)?,
                    capability_generation: row.get(2)?,
                    capability_digest: row.get(3)?,
                    content_digest: row.get(4)?,
                    entries: Vec::new(),
                })
            },
        )
        .optional()
        .context("read SQLite provider switch version")?
    else {
        return Ok(None);
    };

    let mut statement = conn
        .prepare(
            "SELECT provider_id, scope_type, product_id, segment,
                    catalog_generation, enabled
               FROM provider_switch_entries
              WHERE generation=?1
              ORDER BY provider_id, scope_type, product_id, segment",
        )
        .context("prepare SQLite provider switch entries read")?;
    let rows = statement
        .query_map(params![generation], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, bool>(5)?,
            ))
        })
        .context("read SQLite provider switch entries")?;
    let mut entries = Vec::new();
    for row in rows {
        let (provider_id, scope_type, product_id, segment, catalog_generation, enabled) =
            row.context("decode SQLite provider switch entry")?;
        entries.push(ProviderSwitchEntrySpec {
            provider_id,
            scope: ProviderSwitchScope::from_db(&scope_type, product_id, segment)
                .context("decode SQLite provider switch scope")?,
            catalog_generation,
            enabled,
        });
    }
    switches.entries = entries;
    validate_provider_switches(&switches)
        .context("invalid stored SQLite provider switch version")?;
    Ok(Some(normalize_switches(&switches)))
}

pub fn sqlite_active_provider_switches(conn: &Connection) -> Result<Option<ProviderSwitchSpec>> {
    let generation = conn
        .query_row(
            "SELECT active_generation FROM provider_switch_head WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .context("read SQLite active provider switch head")?;
    let Some(generation) = generation else {
        return Ok(None);
    };
    sqlite_provider_switches_by_generation(conn, generation)?
        .ok_or_else(|| {
            anyhow!("SQLite provider switch head references missing version {generation}")
        })
        .map(Some)
}

pub fn sqlite_prepare_provider_switches(
    conn: &Connection,
    incoming: &ProviderSwitchSpec,
) -> Result<PricingMutation> {
    let incoming = normalize_switches(incoming);
    if let Err(error) = validate_provider_switches(&incoming) {
        return Ok(invalid(error));
    }

    let transaction = immediate(conn)?;
    if let Some(stored) = sqlite_provider_switches_by_generation(&transaction, incoming.generation)?
    {
        let result = if stored == incoming {
            PricingMutation::Unchanged
        } else {
            PricingMutation::Rejected(PricingRejection::VersionConflict)
        };
        return finish(transaction, result);
    }
    if let Some(latest) = latest_switch_target(&transaction)? {
        if incoming.generation < latest.version {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::Stale {
                    actual: Some(latest),
                }),
            );
        }
    }

    for (product_id, generation) in required_catalog_generations(&incoming) {
        let Some(catalog) =
            sqlite_pricing_catalog_by_generation(&transaction, &product_id, generation)?
        else {
            return finish(
                transaction,
                missing(format!(
                    "pricing catalog {product_id:?} generation {generation}"
                )),
            );
        };
        if catalog.capability_generation != incoming.capability_generation
            || catalog.capability_digest != incoming.capability_digest
        {
            return finish(
                transaction,
                invalid(anyhow!(
                    "provider switches and catalog {product_id:?}/{generation} \
                     have different capability pins"
                )),
            );
        }
        for entry in incoming.entries.iter().filter(|entry| {
            matches!(
                &entry.scope,
                ProviderSwitchScope::Product {
                    product_id: entry_product
                } | ProviderSwitchScope::Segment {
                    product_id: entry_product,
                    ..
                } if entry_product == &product_id
            )
        }) {
            if !catalog
                .entries
                .iter()
                .any(|model| model.provider_id == entry.provider_id)
            {
                return finish(
                    transaction,
                    invalid(anyhow!(
                        "provider switch references provider {:?} absent from catalog {:?}/{}",
                        entry.provider_id,
                        product_id,
                        generation
                    )),
                );
            }
        }
    }

    transaction
        .execute(
            "INSERT INTO provider_switch_versions(
                 generation, schema_version, capability_generation,
                 capability_digest, content_digest, created_ts
             ) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                incoming.generation,
                incoming.schema_version,
                incoming.capability_generation,
                &incoming.capability_digest,
                &incoming.content_digest,
                now(),
            ],
        )
        .context("insert SQLite provider switch version")?;
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO provider_switch_entries(
                     generation, provider_id, scope_type, product_id, segment,
                     catalog_generation, enabled
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            )
            .context("prepare SQLite provider switch entry insert")?;
        for entry in &incoming.entries {
            let (scope_type, product_id, segment) = entry.scope.db_parts();
            statement
                .execute(params![
                    incoming.generation,
                    &entry.provider_id,
                    scope_type,
                    product_id,
                    segment,
                    entry.catalog_generation,
                    entry.enabled,
                ])
                .context("insert SQLite provider switch entry")?;
        }
    }
    finish(transaction, PricingMutation::Stored)
}

pub fn sqlite_activate_provider_switches(
    conn: &Connection,
    target: &VersionTarget,
    expectation: &ActiveExpectation,
) -> Result<PricingMutation> {
    if let Err(error) =
        validate_version_target(target).and_then(|_| validate_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }

    let transaction = immediate(conn)?;
    let Some(prepared) = sqlite_provider_switches_by_generation(&transaction, target.version)?
    else {
        return finish(
            transaction,
            missing(format!("provider switch generation {}", target.version)),
        );
    };
    if prepared.content_digest != target.content_digest {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::VersionConflict),
        );
    }
    let current = sqlite_active_provider_switches(&transaction)?;
    let actual = current.as_ref().map(ProviderSwitchSpec::target);
    if actual.as_ref() == Some(target) {
        return finish(transaction, PricingMutation::Unchanged);
    }
    if let Some(active) = actual.as_ref() {
        if target.version < active.version {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::Stale {
                    actual: actual.clone(),
                }),
            );
        }
        if target.version == active.version {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::VersionConflict),
            );
        }
    }
    if !expectation_matches(expectation, actual.as_ref()) {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::CasMismatch { actual }),
        );
    }
    for (product_id, generation) in required_catalog_generations(&prepared) {
        let Some(required_catalog) =
            sqlite_pricing_catalog_by_generation(&transaction, &product_id, generation)?
        else {
            return finish(
                transaction,
                missing(format!(
                    "pricing catalog {product_id:?} generation {generation}"
                )),
            );
        };
        let Some(active_catalog) = sqlite_active_pricing_catalog(&transaction, &product_id)? else {
            return finish(
                transaction,
                missing(format!(
                    "active pricing catalog {product_id:?} generation {generation}"
                )),
            );
        };
        if active_catalog.target() != required_catalog.target() {
            return finish(
                transaction,
                missing(format!(
                    "active pricing catalog {product_id:?} target {:?}",
                    required_catalog.target()
                )),
            );
        }
    }

    let changed = match expectation {
        ActiveExpectation::Absent => transaction.execute(
            "INSERT INTO provider_switch_head(singleton, active_generation, updated_ts)
             VALUES(1,?1,?2)",
            params![target.version, now()],
        )?,
        ActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE provider_switch_head
                SET active_generation=?1, updated_ts=?2
              WHERE singleton=1
                AND active_generation=?3
                AND EXISTS (
                    SELECT 1
                      FROM provider_switch_versions v
                     WHERE v.generation=provider_switch_head.active_generation
                       AND v.content_digest=?4
                )",
            params![
                target.version,
                now(),
                expected.version,
                &expected.content_digest,
            ],
        )?,
    };
    if changed != 1 {
        transaction
            .rollback()
            .context("rollback lost SQLite provider switch CAS")?;
        return Ok(PricingMutation::Rejected(PricingRejection::CasMismatch {
            actual,
        }));
    }
    finish(transaction, PricingMutation::Applied)
}

pub fn sqlite_account_policy_by_version(
    conn: &Connection,
    account_id: &str,
    effective_version: i64,
) -> Result<Option<AccountPolicySpec>> {
    require_id("account id", account_id)?;
    if effective_version <= 0 {
        bail!("effective policy version must be positive");
    }
    let Some(mut policy) = conn
        .query_row(
            "SELECT account_id, effective_version, policy_id, policy_version,
                    source_policy_digest, owner_type, owner_id, account_class,
                    product_id, schema_version, catalog_generation, switch_generation,
                    content_digest, replacement_locked
               FROM account_policy_versions
              WHERE account_id=?1 AND effective_version=?2",
            params![account_id, effective_version],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, bool>(13)?,
                ))
            },
        )
        .optional()
        .context("read SQLite account policy version")?
        .map(
            |(
                account_id,
                effective_version,
                policy_id,
                policy_version,
                source_policy_digest,
                owner_type,
                owner_id,
                account_class,
                product_id,
                schema_version,
                catalog_generation,
                switch_generation,
                content_digest,
                replacement_locked,
            )|
             -> Result<AccountPolicySpec> {
                Ok(AccountPolicySpec {
                    account_id,
                    effective_version,
                    policy_id,
                    policy_version,
                    source_policy_digest,
                    owner_type: PolicyOwnerType::from_db(&owner_type)?,
                    owner_id,
                    account_class: AccountClass::from_db(&account_class)?,
                    product_id,
                    schema_version,
                    catalog_generation,
                    switch_generation,
                    content_digest,
                    replacement_locked,
                    rules: Vec::new(),
                })
            },
        )
        .transpose()?
    else {
        return Ok(None);
    };

    let mut statement = conn
        .prepare(
            "SELECT rule_id, rule_digest, scope_type, provider_id, canonical_model_id,
                    pricing_mode, rule_origin, discount_bps, payable_multiplier_bp,
                    track_eligible, retention_eligible, commission_eligible
               FROM account_policy_rules
              WHERE account_id=?1 AND effective_version=?2
              ORDER BY provider_id, scope_type, COALESCE(canonical_model_id,''), rule_id",
        )
        .context("prepare SQLite account policy rules read")?;
    let rows = statement
        .query_map(params![account_id, effective_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, bool>(9)?,
                row.get::<_, bool>(10)?,
                row.get::<_, bool>(11)?,
            ))
        })
        .context("read SQLite account policy rules")?;
    let mut rules = Vec::new();
    for row in rows {
        let (
            rule_id,
            rule_digest,
            scope_type,
            provider_id,
            canonical_model_id,
            pricing_mode,
            rule_origin,
            discount_bps,
            payable_multiplier_bp,
            track_eligible,
            retention_eligible,
            commission_eligible,
        ) = row.context("decode SQLite account policy rule")?;
        rules.push(AccountPolicyRuleSpec {
            rule_id,
            rule_digest,
            scope: PolicyRuleScope::from_db(&scope_type, provider_id, canonical_model_id)?,
            pricing_mode: PricingMode::from_db(&pricing_mode)?,
            rule_origin: RuleOrigin::from_db(&rule_origin)?,
            discount_bps,
            payable_multiplier_bp,
            track_eligible,
            retention_eligible,
            commission_eligible,
        });
    }
    policy.rules = rules;
    Ok(Some(normalize_policy(&policy)))
}

pub fn sqlite_active_account_policy(
    conn: &Connection,
    account_id: &str,
) -> Result<Option<ActiveAccountPolicy>> {
    require_id("account id", account_id)?;
    let Some(stored_binding) = stored_policy_binding(conn, account_id)? else {
        return Ok(None);
    };
    let Some(target) = stored_binding.active_target.as_ref() else {
        return Ok(None);
    };
    let policy =
        sqlite_account_policy_by_version(conn, account_id, target.version)?.ok_or_else(|| {
            anyhow!(
                "SQLite account policy binding references missing version {:?}/{}",
                account_id,
                target.version
            )
        })?;
    if policy.content_digest != target.content_digest
        || policy.product_id != stored_binding.product_id
        || policy.account_class != stored_binding.account_class
    {
        return Err(anyhow!(
            "SQLite account policy binding identity does not match its active policy"
        ));
    }
    Ok(Some(ActiveAccountPolicy {
        policy,
        binding: stored_binding.binding,
    }))
}

/// Read the account scalar, policy binding and independently moving heads from one SQLite
/// snapshot.
///
/// The transaction is deferred because this path is strictly read-only. Its first query pins the
/// SQLite snapshot, while Stage 3A activations continue to use `BEGIN IMMEDIATE`. Consequently a
/// caller observes one historical state and never a mix assembled by separate autocommit reads.
pub fn sqlite_pricing_read_bundle(
    conn: &Connection,
    account_id: &str,
) -> Result<PricingReadBundle> {
    require_id("account id", account_id)?;
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)
        .context("begin SQLite pricing read snapshot")?;
    let account_multiplier_bp = transaction
        .query_row(
            "SELECT mult_bp FROM accounts WHERE id=?1",
            params![account_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .context("read SQLite pricing bundle account scalar")?
        .ok_or_else(|| anyhow!("SQLite pricing bundle account does not exist"))?;

    let Some(stored_binding) = stored_policy_binding(&transaction, account_id)? else {
        let bundle = PricingReadBundle {
            account_id: account_id.to_owned(),
            account_multiplier_bp,
            policy: PricingPolicySnapshot::Unbound,
            catalog: None,
            switches: None,
        };
        transaction
            .commit()
            .context("commit SQLite pricing read snapshot")?;
        return Ok(bundle);
    };

    let product_id = stored_binding.product_id.clone();
    let policy = match stored_binding.active_target.as_ref() {
        None => PricingPolicySnapshot::Inactive {
            product_id: product_id.clone(),
            account_class: stored_binding.account_class,
            binding: stored_binding.binding,
        },
        Some(target) => {
            let policy =
                sqlite_account_policy_by_version(&transaction, account_id, target.version)?
                    .ok_or_else(|| {
                        anyhow!(
                            "SQLite account policy binding references missing version {:?}/{}",
                            account_id,
                            target.version
                        )
                    })?;
            if policy.content_digest != target.content_digest
                || policy.product_id != product_id
                || policy.account_class != stored_binding.account_class
            {
                return Err(anyhow!(
                    "SQLite account policy binding identity does not match its active policy"
                ));
            }
            PricingPolicySnapshot::Active(ActiveAccountPolicy {
                policy,
                binding: stored_binding.binding,
            })
        }
    };
    let catalog = sqlite_active_pricing_catalog(&transaction, &product_id)?;
    let switches = sqlite_active_provider_switches(&transaction)?;
    let bundle = PricingReadBundle {
        account_id: account_id.to_owned(),
        account_multiplier_bp,
        policy,
        catalog,
        switches,
    };
    transaction
        .commit()
        .context("commit SQLite pricing read snapshot")?;
    Ok(bundle)
}

pub fn sqlite_prepare_account_policy(
    conn: &Connection,
    incoming: &AccountPolicySpec,
) -> Result<PricingMutation> {
    let incoming = normalize_policy(incoming);
    if let Err(error) = validate_account_policy_shape(&incoming) {
        return Ok(invalid(error));
    }

    let transaction = immediate(conn)?;
    let current_binding = stored_policy_binding(&transaction, &incoming.account_id)?;
    if let Some(stored) = sqlite_account_policy_by_version(
        &transaction,
        &incoming.account_id,
        incoming.effective_version,
    )? {
        let result = if current_binding
            .as_ref()
            .is_some_and(|binding| !binding.identity_matches(&incoming))
        {
            PricingMutation::Rejected(PricingRejection::VersionConflict)
        } else if stored == incoming {
            PricingMutation::Unchanged
        } else {
            PricingMutation::Rejected(PricingRejection::VersionConflict)
        };
        return finish(transaction, result);
    }

    if let Some(latest) = latest_account_policy(&transaction, &incoming.account_id)? {
        if incoming.effective_version < latest.effective_version
            || incoming.policy_version < latest.policy_version
        {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::Stale {
                    actual: Some(latest.target()),
                }),
            );
        }
        if incoming.policy_id != latest.policy_id
            || incoming.owner_type != latest.owner_type
            || incoming.owner_id != latest.owner_id
            || incoming.account_class != latest.account_class
            || incoming.product_id != latest.product_id
        {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::VersionConflict),
            );
        }
        if incoming.policy_version == latest.policy_version
            && incoming.source_policy_digest != latest.source_policy_digest
        {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::VersionConflict),
            );
        }
    }
    if current_binding
        .as_ref()
        .is_some_and(|binding| !binding.identity_matches(&incoming))
    {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::VersionConflict),
        );
    }
    if has_locked_policy(&transaction, &incoming.account_id)? {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::Locked),
        );
    }

    let Some(catalog) = sqlite_pricing_catalog_by_generation(
        &transaction,
        &incoming.product_id,
        incoming.catalog_generation,
    )?
    else {
        return finish(
            transaction,
            missing(format!(
                "pricing catalog {:?} generation {}",
                incoming.product_id, incoming.catalog_generation
            )),
        );
    };
    let Some(switches) =
        sqlite_provider_switches_by_generation(&transaction, incoming.switch_generation)?
    else {
        return finish(
            transaction,
            missing(format!(
                "provider switch generation {}",
                incoming.switch_generation
            )),
        );
    };
    let Some(multiplier_bp) = live_account_multiplier(&transaction, &incoming.account_id)? else {
        return finish(
            transaction,
            missing(format!("account {:?}", incoming.account_id)),
        );
    };
    if let Err(error) = validate_account_policy(&incoming, &catalog, &switches, Some(multiplier_bp))
    {
        return finish(transaction, invalid(error));
    }

    transaction
        .execute(
            "INSERT INTO account_policy_versions(
                 account_id, effective_version, policy_id, policy_version,
                 source_policy_digest, owner_type, owner_id, account_class,
                 product_id, schema_version, catalog_generation, switch_generation,
                 content_digest, replacement_locked, created_ts
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                &incoming.account_id,
                incoming.effective_version,
                &incoming.policy_id,
                incoming.policy_version,
                &incoming.source_policy_digest,
                incoming.owner_type.as_str(),
                &incoming.owner_id,
                incoming.account_class.as_str(),
                &incoming.product_id,
                incoming.schema_version,
                incoming.catalog_generation,
                incoming.switch_generation,
                &incoming.content_digest,
                incoming.replacement_locked,
                now(),
            ],
        )
        .context("insert SQLite account policy version")?;
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO account_policy_rules(
                     account_id, effective_version, rule_id, rule_digest, scope_type,
                     provider_id, canonical_model_id, pricing_mode, rule_origin,
                     discount_bps, payable_multiplier_bp, track_eligible,
                     retention_eligible, commission_eligible
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            )
            .context("prepare SQLite account policy rule insert")?;
        for rule in &incoming.rules {
            let (scope_type, provider_id, canonical_model_id) = rule.scope.db_parts();
            statement
                .execute(params![
                    &incoming.account_id,
                    incoming.effective_version,
                    &rule.rule_id,
                    &rule.rule_digest,
                    scope_type,
                    provider_id,
                    canonical_model_id,
                    rule.pricing_mode.as_str(),
                    rule.rule_origin.as_str(),
                    rule.discount_bps,
                    rule.payable_multiplier_bp,
                    rule.track_eligible,
                    rule.retention_eligible,
                    rule.commission_eligible,
                ])
                .context("insert SQLite account policy rule")?;
        }
    }
    finish(transaction, PricingMutation::Stored)
}

pub fn sqlite_activate_account_policy(
    conn: &Connection,
    activation: &AccountPolicyActivationSpec,
    expectation: &PolicyActiveExpectation,
) -> Result<PricingMutation> {
    let target = VersionTarget::new(
        activation.effective_version,
        activation.content_digest.clone(),
    );
    if let Err(error) = require_id("account id", &activation.account_id)
        .and_then(|_| validate_version_target(&target))
        .and_then(|_| validate_account_policy_binding(&activation.binding))
        .and_then(|_| validate_policy_active_expectation(expectation))
    {
        return Ok(invalid(error));
    }

    let transaction = immediate(conn)?;
    let current_binding = stored_policy_binding(&transaction, &activation.account_id)?;
    let current = current_binding
        .as_ref()
        .map(StoredPolicyBinding::state)
        .unwrap_or(PolicyBindingState::Unbound);
    let Some(policy) = sqlite_account_policy_by_version(
        &transaction,
        &activation.account_id,
        activation.effective_version,
    )?
    else {
        return finish(
            transaction,
            missing(format!(
                "account policy {:?} effective version {}",
                activation.account_id, activation.effective_version
            )),
        );
    };
    if policy.content_digest != activation.content_digest {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::VersionConflict),
        );
    }
    if current_binding
        .as_ref()
        .is_some_and(|binding| !binding.identity_matches(&policy))
    {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::VersionConflict),
        );
    }

    let desired = ActivePolicyTarget {
        target: target.clone(),
        binding: activation.binding.clone(),
    };
    if current == PolicyBindingState::Active(desired) {
        return finish(transaction, PricingMutation::Unchanged);
    }
    if let PolicyBindingState::Active(active) = &current {
        if target.version < active.target.version {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::Stale {
                    actual: Some(active.target.clone()),
                }),
            );
        }
        if target.version == active.target.version
            && target.content_digest != active.target.content_digest
        {
            return finish(
                transaction,
                PricingMutation::Rejected(PricingRejection::VersionConflict),
            );
        }
    }
    if !policy_expectation_matches(expectation, &current) {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { actual: current }),
        );
    }
    if has_different_locked_policy(&transaction, &activation.account_id, &target)? {
        return finish(
            transaction,
            PricingMutation::Rejected(PricingRejection::Locked),
        );
    }

    let Some(catalog) = sqlite_pricing_catalog_by_generation(
        &transaction,
        &policy.product_id,
        policy.catalog_generation,
    )?
    else {
        return finish(
            transaction,
            missing(format!(
                "pricing catalog {:?} generation {}",
                policy.product_id, policy.catalog_generation
            )),
        );
    };
    let Some(switches) =
        sqlite_provider_switches_by_generation(&transaction, policy.switch_generation)?
    else {
        return finish(
            transaction,
            missing(format!(
                "provider switch generation {}",
                policy.switch_generation
            )),
        );
    };
    let Some(multiplier_bp) = live_account_multiplier(&transaction, &activation.account_id)? else {
        return finish(
            transaction,
            missing(format!("account {:?}", activation.account_id)),
        );
    };
    if let Err(error) = validate_account_policy(&policy, &catalog, &switches, Some(multiplier_bp)) {
        return finish(transaction, invalid(error));
    }

    let active_switches = sqlite_active_provider_switches(&transaction)?;
    if active_switches
        .as_ref()
        .is_none_or(|active| active.target() != switches.target())
    {
        return finish(
            transaction,
            missing(format!(
                "active provider switch target {:?}",
                switches.target()
            )),
        );
    }
    let active_catalog = sqlite_active_pricing_catalog(&transaction, &policy.product_id)?;
    if active_catalog
        .as_ref()
        .is_none_or(|active| active.target() != catalog.target())
    {
        return finish(
            transaction,
            missing(format!(
                "active pricing catalog {:?} target {:?}",
                policy.product_id,
                catalog.target()
            )),
        );
    }

    let changed = match expectation {
        PolicyActiveExpectation::Unbound => transaction.execute(
            "INSERT INTO account_policy_bindings(
                 account_id, product_id, account_class, active_effective_version,
                 policy_enforcement, funding_enforcement, reconciliation_state, updated_ts
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                &activation.account_id,
                &policy.product_id,
                policy.account_class.as_str(),
                activation.effective_version,
                activation.binding.policy_enforcement.as_str(),
                activation.binding.funding_enforcement.as_str(),
                activation.binding.reconciliation_state.as_str(),
                now(),
            ],
        )?,
        PolicyActiveExpectation::Inactive(expected) => transaction.execute(
            "UPDATE account_policy_bindings
                SET active_effective_version=?1, policy_enforcement=?2,
                    funding_enforcement=?3, reconciliation_state=?4, updated_ts=?5
              WHERE account_id=?6
                AND product_id=?7
                AND account_class=?8
                AND active_effective_version IS NULL
                AND policy_enforcement=?9
                AND funding_enforcement=?10
                AND reconciliation_state=?11",
            params![
                activation.effective_version,
                activation.binding.policy_enforcement.as_str(),
                activation.binding.funding_enforcement.as_str(),
                activation.binding.reconciliation_state.as_str(),
                now(),
                &activation.account_id,
                &policy.product_id,
                policy.account_class.as_str(),
                expected.policy_enforcement.as_str(),
                expected.funding_enforcement.as_str(),
                expected.reconciliation_state.as_str(),
            ],
        )?,
        PolicyActiveExpectation::Exact(expected) => transaction.execute(
            "UPDATE account_policy_bindings
                SET active_effective_version=?1, policy_enforcement=?2,
                    funding_enforcement=?3, reconciliation_state=?4, updated_ts=?5
              WHERE account_id=?6
                AND product_id=?7
                AND account_class=?8
                AND active_effective_version=?9
                AND policy_enforcement=?10
                AND funding_enforcement=?11
                AND reconciliation_state=?12
                AND EXISTS (
                    SELECT 1
                      FROM account_policy_versions v
                     WHERE v.account_id=account_policy_bindings.account_id
                       AND v.effective_version=account_policy_bindings.active_effective_version
                       AND v.product_id=account_policy_bindings.product_id
                       AND v.content_digest=?13
                )",
            params![
                activation.effective_version,
                activation.binding.policy_enforcement.as_str(),
                activation.binding.funding_enforcement.as_str(),
                activation.binding.reconciliation_state.as_str(),
                now(),
                &activation.account_id,
                &policy.product_id,
                policy.account_class.as_str(),
                expected.target.version,
                expected.binding.policy_enforcement.as_str(),
                expected.binding.funding_enforcement.as_str(),
                expected.binding.reconciliation_state.as_str(),
                &expected.target.content_digest,
            ],
        )?,
    };
    if changed != 1 {
        transaction
            .rollback()
            .context("rollback lost SQLite account policy CAS")?;
        return Ok(PricingMutation::Rejected(
            PricingRejection::PolicyCasMismatch { actual: current },
        ));
    }
    finish(transaction, PricingMutation::Applied)
}

fn latest_catalog_target(conn: &Connection, product_id: &str) -> Result<Option<VersionTarget>> {
    conn.query_row(
        "SELECT generation, content_digest
           FROM pricing_catalog_versions
          WHERE product_id=?1
          ORDER BY generation DESC
          LIMIT 1",
        params![product_id],
        |row| Ok(VersionTarget::new(row.get(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .context("read latest SQLite pricing catalog target")
}

fn latest_switch_target(conn: &Connection) -> Result<Option<VersionTarget>> {
    conn.query_row(
        "SELECT generation, content_digest
           FROM provider_switch_versions
          ORDER BY generation DESC
          LIMIT 1",
        [],
        |row| Ok(VersionTarget::new(row.get(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .context("read latest SQLite provider switch target")
}

fn latest_account_policy(conn: &Connection, account_id: &str) -> Result<Option<AccountPolicySpec>> {
    let version = conn
        .query_row(
            "SELECT effective_version
               FROM account_policy_versions
              WHERE account_id=?1
              ORDER BY effective_version DESC
              LIMIT 1",
            params![account_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .context("read latest SQLite account policy version")?;
    let Some(version) = version else {
        return Ok(None);
    };
    sqlite_account_policy_by_version(conn, account_id, version)?
        .ok_or_else(|| {
            anyhow!("latest SQLite account policy version disappeared during locked read")
        })
        .map(Some)
}

fn live_account_multiplier(conn: &Connection, account_id: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT mult_bp FROM accounts WHERE id=?1",
        params![account_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .context("read live SQLite account multiplier")
}

fn has_locked_policy(conn: &Connection, account_id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1
               FROM account_policy_versions
              WHERE account_id=?1 AND replacement_locked=1
         )",
        params![account_id],
        |row| row.get(0),
    )
    .context("read SQLite account policy replacement lock")
}

fn has_different_locked_policy(
    conn: &Connection,
    account_id: &str,
    target: &VersionTarget,
) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1
               FROM account_policy_versions
              WHERE account_id=?1
                AND replacement_locked=1
                AND (effective_version<>?2 OR content_digest<>?3)
         )",
        params![account_id, target.version, &target.content_digest],
        |row| row.get(0),
    )
    .context("read SQLite account policy replacement lock target")
}

#[derive(Clone, Debug)]
struct StoredPolicyBinding {
    product_id: String,
    account_class: AccountClass,
    active_target: Option<VersionTarget>,
    binding: AccountPolicyBindingSpec,
}

impl StoredPolicyBinding {
    fn state(&self) -> PolicyBindingState {
        match &self.active_target {
            Some(target) => PolicyBindingState::Active(ActivePolicyTarget {
                target: target.clone(),
                binding: self.binding.clone(),
            }),
            None => PolicyBindingState::Inactive(self.binding.clone()),
        }
    }

    fn identity_matches(&self, policy: &AccountPolicySpec) -> bool {
        self.product_id == policy.product_id && self.account_class == policy.account_class
    }
}

fn stored_policy_binding(
    conn: &Connection,
    account_id: &str,
) -> Result<Option<StoredPolicyBinding>> {
    let row = conn
        .query_row(
            "SELECT b.product_id, b.account_class, b.active_effective_version,
                    v.content_digest, b.policy_enforcement,
                    b.funding_enforcement, b.reconciliation_state
               FROM account_policy_bindings b
               LEFT JOIN account_policy_versions v
                 ON v.account_id=b.account_id
                AND v.effective_version=b.active_effective_version
                AND v.product_id=b.product_id
              WHERE b.account_id=?1",
            params![account_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .context("read SQLite account policy binding")?;
    let Some((
        product_id,
        account_class,
        effective_version,
        content_digest,
        policy_enforcement,
        funding_enforcement,
        reconciliation_state,
    )) = row
    else {
        return Ok(None);
    };
    let binding = AccountPolicyBindingSpec {
        policy_enforcement: PolicyEnforcement::from_db(&policy_enforcement)?,
        funding_enforcement: FundingEnforcement::from_db(&funding_enforcement)?,
        reconciliation_state: ReconciliationState::from_db(&reconciliation_state)?,
    };
    validate_account_policy_binding(&binding)
        .context("invalid stored SQLite account policy binding")?;
    let active_target = match (effective_version, content_digest) {
        (None, None) => None,
        (Some(version), Some(digest)) => Some(VersionTarget::new(version, digest)),
        _ => bail!("SQLite account policy binding references a missing effective version"),
    };
    Ok(Some(StoredPolicyBinding {
        product_id,
        account_class: AccountClass::from_db(&account_class)?,
        active_target,
        binding,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{account_create, account_set_mult_bp};
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    use std::thread;

    const CAPABILITY_GENERATION: i64 = 17;
    const CAPABILITY_DIGEST: &str = "capability-17";

    fn catalog(product_id: &str, generation: i64, digest: &str) -> PricingCatalogSpec {
        PricingCatalogSpec {
            product_id: product_id.to_owned(),
            generation,
            schema_version: 1,
            capability_generation: CAPABILITY_GENERATION,
            capability_digest: CAPABILITY_DIGEST.to_owned(),
            content_digest: digest.to_owned(),
            entries: vec![
                PricingCatalogEntrySpec {
                    provider_id: "openai".to_owned(),
                    canonical_model_id: "gpt-test".to_owned(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    canonical_model_id: "claude-test".to_owned(),
                    enabled: true,
                },
            ],
        }
    }

    fn b2b_switches(generation: i64, catalog_generation: i64, digest: &str) -> ProviderSwitchSpec {
        ProviderSwitchSpec {
            generation,
            schema_version: 1,
            capability_generation: CAPABILITY_GENERATION,
            capability_digest: CAPABILITY_DIGEST.to_owned(),
            content_digest: digest.to_owned(),
            entries: vec![
                ProviderSwitchEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    scope: ProviderSwitchScope::Segment {
                        product_id: "main".to_owned(),
                        segment: super::super::PolicySegment::B2b,
                    },
                    catalog_generation: Some(catalog_generation),
                    enabled: true,
                },
                ProviderSwitchEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    scope: ProviderSwitchScope::Master,
                    catalog_generation: None,
                    enabled: true,
                },
            ],
        }
    }

    fn openkeys_switches(
        generation: i64,
        catalog_generation: i64,
        digest: &str,
    ) -> ProviderSwitchSpec {
        let mut entries = Vec::new();
        for provider_id in ["anthropic", "openai"] {
            entries.push(ProviderSwitchEntrySpec {
                provider_id: provider_id.to_owned(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            });
            entries.push(ProviderSwitchEntrySpec {
                provider_id: provider_id.to_owned(),
                scope: ProviderSwitchScope::Product {
                    product_id: "openkeys".to_owned(),
                },
                catalog_generation: Some(catalog_generation),
                enabled: true,
            });
        }
        ProviderSwitchSpec {
            generation,
            schema_version: 1,
            capability_generation: CAPABILITY_GENERATION,
            capability_digest: CAPABILITY_DIGEST.to_owned(),
            content_digest: digest.to_owned(),
            entries,
        }
    }

    fn discount_rule(
        rule_id: &str,
        provider_id: &str,
        origin: RuleOrigin,
        multiplier_bp: i64,
    ) -> AccountPolicyRuleSpec {
        AccountPolicyRuleSpec {
            rule_id: rule_id.to_owned(),
            rule_digest: format!("{rule_id}-digest"),
            scope: PolicyRuleScope::Provider {
                provider_id: provider_id.to_owned(),
            },
            pricing_mode: PricingMode::Discount,
            rule_origin: origin,
            discount_bps: (origin == RuleOrigin::Managed).then_some(10_000 - multiplier_bp),
            payable_multiplier_bp: multiplier_bp,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
        }
    }

    fn b2b_policy(account_id: &str, effective_version: i64, digest: &str) -> AccountPolicySpec {
        AccountPolicySpec {
            account_id: account_id.to_owned(),
            effective_version,
            policy_id: format!("b2b:{account_id}"),
            policy_version: effective_version,
            source_policy_digest: format!("source-{effective_version}"),
            owner_type: PolicyOwnerType::B2bClient,
            owner_id: account_id.to_owned(),
            account_class: AccountClass::B2b,
            product_id: "main".to_owned(),
            schema_version: 1,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: digest.to_owned(),
            replacement_locked: false,
            rules: vec![discount_rule(
                "anthropic-discount",
                "anthropic",
                RuleOrigin::Managed,
                9_000,
            )],
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

    fn shadow_binding() -> AccountPolicyBindingSpec {
        binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Shadow,
            ReconciliationState::Pending,
        )
    }

    fn legacy_binding() -> AccountPolicyBindingSpec {
        binding(
            PolicyEnforcement::LegacyScalar,
            FundingEnforcement::LegacySingle,
            ReconciliationState::Pending,
        )
    }

    fn prepare_active_b2b_dependencies(conn: &Connection) {
        let catalog = catalog("main", 1, "main-catalog-1");
        assert_eq!(
            sqlite_prepare_pricing_catalog(conn, &catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                conn,
                "main",
                &catalog.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        let switches = b2b_switches(1, 1, "b2b-switches-1");
        assert_eq!(
            sqlite_prepare_provider_switches(conn, &switches).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_provider_switches(
                conn,
                &switches.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
    }

    #[test]
    fn catalog_and_switch_snapshots_use_semantic_equality_and_monotonic_cas() {
        let conn = crate::open(":memory:").unwrap();
        let catalog_one = catalog("main", 1, "catalog-one");
        assert_eq!(
            sqlite_prepare_pricing_catalog(&conn, &catalog_one).unwrap(),
            PricingMutation::Stored
        );

        let mut reordered = catalog_one.clone();
        reordered.entries.reverse();
        assert_eq!(
            sqlite_prepare_pricing_catalog(&conn, &reordered).unwrap(),
            PricingMutation::Unchanged
        );
        let mut conflicting = catalog_one.clone();
        conflicting.entries[0].enabled = false;
        assert_eq!(
            sqlite_prepare_pricing_catalog(&conn, &conflicting).unwrap(),
            PricingMutation::Rejected(PricingRejection::VersionConflict)
        );
        assert_eq!(
            sqlite_pricing_catalog_by_generation(&conn, "main", 1)
                .unwrap()
                .unwrap(),
            normalize_catalog(&catalog_one)
        );

        let catalog_two = catalog("main", 2, "catalog-two");
        assert_eq!(
            sqlite_prepare_pricing_catalog(&conn, &catalog_two).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &catalog_one.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &catalog_two.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::CasMismatch {
                actual: Some(catalog_one.target()),
            })
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &catalog_two.target(),
                &ActiveExpectation::Exact(catalog_one.target()),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &catalog_one.target(),
                &ActiveExpectation::Exact(catalog_two.target()),
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::Stale {
                actual: Some(catalog_two.target()),
            })
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &catalog_two.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Unchanged
        );

        let switches = b2b_switches(1, 2, "switches-one");
        assert_eq!(
            sqlite_prepare_provider_switches(&conn, &switches).unwrap(),
            PricingMutation::Stored
        );
        let mut reordered_switches = switches.clone();
        reordered_switches.entries.reverse();
        assert_eq!(
            sqlite_prepare_provider_switches(&conn, &reordered_switches).unwrap(),
            PricingMutation::Unchanged
        );
        assert_eq!(
            sqlite_activate_provider_switches(
                &conn,
                &switches.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            sqlite_active_provider_switches(&conn).unwrap(),
            Some(normalize_switches(&switches))
        );
    }

    #[test]
    fn failed_child_insert_rolls_back_the_whole_snapshot() {
        let conn = crate::open(":memory:").unwrap();
        conn.execute_batch(
            "CREATE TRIGGER test_reject_second_catalog_child
             BEFORE INSERT ON pricing_catalog_entries
             WHEN NEW.provider_id='openai'
             BEGIN
                 SELECT RAISE(ABORT, 'injected child failure');
             END;",
        )
        .unwrap();

        assert!(sqlite_prepare_pricing_catalog(&conn, &catalog("main", 1, "rollback")).is_err());
        assert!(sqlite_pricing_catalog_by_generation(&conn, "main", 1)
            .unwrap()
            .is_none());
        let counts: (i64, i64) = conn
            .query_row(
                "SELECT
                     (SELECT COUNT(*) FROM pricing_catalog_versions),
                     (SELECT COUNT(*) FROM pricing_catalog_entries)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0));

        conn.execute_batch("DROP TRIGGER test_reject_second_catalog_child")
            .unwrap();
        let main_catalog = catalog("main", 1, "rollback-main-catalog");
        assert_eq!(
            sqlite_prepare_pricing_catalog(&conn, &main_catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &main_catalog.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        conn.execute_batch(
            "CREATE TRIGGER test_reject_second_switch_child
             BEFORE INSERT ON provider_switch_entries
             WHEN NEW.scope_type='segment'
             BEGIN
                 SELECT RAISE(ABORT, 'injected switch child failure');
             END;",
        )
        .unwrap();
        let switches = b2b_switches(1, 1, "rollback-switches");
        assert!(sqlite_prepare_provider_switches(&conn, &switches).is_err());
        let switch_counts: (i64, i64) = conn
            .query_row(
                "SELECT
                     (SELECT COUNT(*) FROM provider_switch_versions),
                     (SELECT COUNT(*) FROM provider_switch_entries)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(switch_counts, (0, 0));
        conn.execute_batch("DROP TRIGGER test_reject_second_switch_child")
            .unwrap();
        assert_eq!(
            sqlite_prepare_provider_switches(&conn, &switches).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_provider_switches(
                &conn,
                &switches.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        account_create(&conn, "rollback-account", None, 8_000).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER test_reject_policy_child
             BEFORE INSERT ON account_policy_rules
             BEGIN
                 SELECT RAISE(ABORT, 'injected policy child failure');
             END;",
        )
        .unwrap();
        assert!(sqlite_prepare_account_policy(
            &conn,
            &b2b_policy("rollback-account", 1, "rollback-policy")
        )
        .is_err());
        let policy_counts: (i64, i64) = conn
            .query_row(
                "SELECT
                     (SELECT COUNT(*) FROM account_policy_versions
                       WHERE account_id='rollback-account'),
                     (SELECT COUNT(*) FROM account_policy_rules
                       WHERE account_id='rollback-account')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(policy_counts, (0, 0));
    }

    #[test]
    fn null_binding_is_not_unbound_and_full_binding_participates_in_cas() {
        let conn = crate::open(":memory:").unwrap();
        account_create(&conn, "acct", None, 8_000).unwrap();
        prepare_active_b2b_dependencies(&conn);
        let policy = b2b_policy("acct", 1, "policy-one");
        assert_eq!(
            sqlite_prepare_account_policy(&conn, &policy).unwrap(),
            PricingMutation::Stored
        );
        let mut rule_conflict = policy.clone();
        rule_conflict.rules[0].rule_digest = "different-rule-digest".to_owned();
        assert_eq!(
            sqlite_prepare_account_policy(&conn, &rule_conflict).unwrap(),
            PricingMutation::Rejected(PricingRejection::VersionConflict)
        );
        assert_eq!(
            sqlite_account_policy_by_version(&conn, "acct", 1)
                .unwrap()
                .unwrap(),
            normalize_policy(&policy)
        );

        let inactive = legacy_binding();
        conn.execute(
            "INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES(?1,'main','b2b',NULL,?2,?3,?4,1)",
            params![
                "acct",
                inactive.policy_enforcement.as_str(),
                inactive.funding_enforcement.as_str(),
                inactive.reconciliation_state.as_str(),
            ],
        )
        .unwrap();
        assert!(sqlite_active_account_policy(&conn, "acct")
            .unwrap()
            .is_none());

        let activation = AccountPolicyActivationSpec {
            account_id: "acct".to_owned(),
            effective_version: 1,
            content_digest: policy.content_digest.clone(),
            binding: shadow_binding(),
        };
        conn.execute(
            "UPDATE account_policy_bindings SET product_id='wrong-product' WHERE account_id='acct'",
            [],
        )
        .unwrap();
        assert_eq!(
            sqlite_activate_account_policy(
                &conn,
                &activation,
                &PolicyActiveExpectation::Inactive(inactive.clone()),
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::VersionConflict)
        );
        assert_eq!(
            conn.query_row(
                "SELECT product_id FROM account_policy_bindings WHERE account_id='acct'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "wrong-product"
        );
        conn.execute(
            "UPDATE account_policy_bindings SET product_id='main' WHERE account_id='acct'",
            [],
        )
        .unwrap();
        assert_eq!(
            sqlite_activate_account_policy(&conn, &activation, &PolicyActiveExpectation::Unbound,)
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch {
                actual: PolicyBindingState::Inactive(inactive.clone()),
            })
        );
        let wrong_inactive = binding(
            PolicyEnforcement::LegacyScalar,
            FundingEnforcement::LegacySingle,
            ReconciliationState::Verified,
        );
        assert_eq!(
            sqlite_activate_account_policy(
                &conn,
                &activation,
                &PolicyActiveExpectation::Inactive(wrong_inactive),
            )
            .unwrap(),
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch {
                actual: PolicyBindingState::Inactive(inactive.clone()),
            })
        );
        assert_eq!(
            sqlite_activate_account_policy(
                &conn,
                &activation,
                &PolicyActiveExpectation::Inactive(inactive),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            sqlite_active_account_policy(&conn, "acct").unwrap(),
            Some(ActiveAccountPolicy {
                policy: normalize_policy(&policy),
                binding: shadow_binding(),
            })
        );

        conn.execute("DELETE FROM provider_switch_head", [])
            .unwrap();
        conn.execute("DELETE FROM pricing_catalog_heads", [])
            .unwrap();
        assert_eq!(
            sqlite_activate_account_policy(&conn, &activation, &PolicyActiveExpectation::Unbound,)
                .unwrap(),
            PricingMutation::Unchanged
        );
    }

    #[test]
    fn pricing_read_bundle_preserves_policy_state_and_exposes_current_heads() {
        let conn = crate::open(":memory:").unwrap();
        assert!(sqlite_pricing_read_bundle(&conn, "missing-account").is_err());
        account_create(&conn, "bundle-unbound", None, 8_000).unwrap();
        account_create(&conn, "bundle-active", None, 8_000).unwrap();
        prepare_active_b2b_dependencies(&conn);

        assert_eq!(
            sqlite_pricing_read_bundle(&conn, "bundle-unbound").unwrap(),
            PricingReadBundle {
                account_id: "bundle-unbound".to_owned(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Unbound,
                catalog: None,
                switches: None,
            }
        );

        let policy = b2b_policy("bundle-active", 1, "bundle-policy-1");
        assert_eq!(
            sqlite_prepare_account_policy(&conn, &policy).unwrap(),
            PricingMutation::Stored
        );
        let inactive = legacy_binding();
        conn.execute(
            "INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES(?1,'main','b2b',NULL,?2,?3,?4,1)",
            params![
                "bundle-active",
                inactive.policy_enforcement.as_str(),
                inactive.funding_enforcement.as_str(),
                inactive.reconciliation_state.as_str(),
            ],
        )
        .unwrap();

        let catalog_v1 = normalize_catalog(&catalog("main", 1, "main-catalog-1"));
        let switches_v1 = normalize_switches(&b2b_switches(1, 1, "b2b-switches-1"));
        assert_eq!(
            sqlite_pricing_read_bundle(&conn, "bundle-active").unwrap(),
            PricingReadBundle {
                account_id: "bundle-active".to_owned(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Inactive {
                    product_id: "main".to_owned(),
                    account_class: AccountClass::B2b,
                    binding: inactive.clone(),
                },
                catalog: Some(catalog_v1.clone()),
                switches: Some(switches_v1.clone()),
            }
        );

        let active_binding = shadow_binding();
        let activation = AccountPolicyActivationSpec {
            account_id: policy.account_id.clone(),
            effective_version: policy.effective_version,
            content_digest: policy.content_digest.clone(),
            binding: active_binding.clone(),
        };
        assert_eq!(
            sqlite_activate_account_policy(
                &conn,
                &activation,
                &PolicyActiveExpectation::Inactive(inactive),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        assert_eq!(
            sqlite_pricing_read_bundle(&conn, "bundle-active").unwrap(),
            PricingReadBundle {
                account_id: "bundle-active".to_owned(),
                account_multiplier_bp: 8_000,
                policy: PricingPolicySnapshot::Active(ActiveAccountPolicy {
                    policy: normalize_policy(&policy),
                    binding: active_binding,
                }),
                catalog: Some(catalog_v1.clone()),
                switches: Some(switches_v1),
            }
        );

        let catalog_v2 = catalog("main", 2, "main-catalog-2");
        assert_eq!(
            sqlite_prepare_pricing_catalog(&conn, &catalog_v2).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &catalog_v2.target(),
                &ActiveExpectation::Exact(catalog_v1.target()),
            )
            .unwrap(),
            PricingMutation::Applied
        );
        let torn = sqlite_pricing_read_bundle(&conn, "bundle-active").unwrap();
        assert_eq!(torn.account_id, "bundle-active");
        assert!(matches!(
            torn.policy,
            PricingPolicySnapshot::Active(ActiveAccountPolicy {
                policy: AccountPolicySpec {
                    catalog_generation: 1,
                    switch_generation: 1,
                    ..
                },
                ..
            })
        ));
        assert_eq!(torn.catalog, Some(normalize_catalog(&catalog_v2)));
        assert_eq!(
            torn.switches,
            Some(normalize_switches(&b2b_switches(1, 1, "b2b-switches-1",)))
        );
    }

    #[test]
    fn legacy_openkeys_uses_live_multiplier_and_locks_history() {
        let conn = crate::open(":memory:").unwrap();
        account_create(&conn, "legacy-openkeys", None, 7_300).unwrap();
        let catalog = catalog("openkeys", 1, "openkeys-catalog");
        assert_eq!(
            sqlite_prepare_pricing_catalog(&conn, &catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_pricing_catalog(
                &conn,
                "openkeys",
                &catalog.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        let switches = openkeys_switches(1, 1, "openkeys-switches");
        assert_eq!(
            sqlite_prepare_provider_switches(&conn, &switches).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_activate_provider_switches(
                &conn,
                &switches.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        let legacy_policy = AccountPolicySpec {
            account_id: "legacy-openkeys".to_owned(),
            effective_version: 1,
            policy_id: "openkeys:legacy-openkeys".to_owned(),
            policy_version: 1,
            source_policy_digest: "legacy-source".to_owned(),
            owner_type: PolicyOwnerType::OpenKeys,
            owner_id: "legacy-openkeys".to_owned(),
            account_class: AccountClass::OpenKeys,
            product_id: "openkeys".to_owned(),
            schema_version: 1,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: "legacy-policy".to_owned(),
            replacement_locked: true,
            rules: vec![
                discount_rule("anthropic-legacy", "anthropic", RuleOrigin::Legacy, 7_300),
                discount_rule("openai-legacy", "openai", RuleOrigin::Legacy, 7_300),
            ],
        };
        assert_eq!(
            sqlite_prepare_account_policy(&conn, &legacy_policy).unwrap(),
            PricingMutation::Stored
        );

        account_set_mult_bp(&conn, "legacy-openkeys", 7_400).unwrap();
        let activation = AccountPolicyActivationSpec {
            account_id: "legacy-openkeys".to_owned(),
            effective_version: 1,
            content_digest: legacy_policy.content_digest.clone(),
            binding: legacy_binding(),
        };
        assert!(matches!(
            sqlite_activate_account_policy(&conn, &activation, &PolicyActiveExpectation::Unbound,)
                .unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));
        account_set_mult_bp(&conn, "legacy-openkeys", 7_300).unwrap();
        assert_eq!(
            sqlite_activate_account_policy(&conn, &activation, &PolicyActiveExpectation::Unbound,)
                .unwrap(),
            PricingMutation::Applied
        );

        let mut replacement = legacy_policy.clone();
        replacement.effective_version = 2;
        replacement.policy_version = 2;
        replacement.source_policy_digest = "current-source".to_owned();
        replacement.content_digest = "current-policy".to_owned();
        replacement.replacement_locked = false;
        replacement.rules = vec![
            discount_rule(
                "anthropic-current",
                "anthropic",
                RuleOrigin::Managed,
                10_000,
            ),
            discount_rule("openai-current", "openai", RuleOrigin::Managed, 10_000),
        ];
        assert_eq!(
            sqlite_prepare_account_policy(&conn, &replacement).unwrap(),
            PricingMutation::Rejected(PricingRejection::Locked)
        );
    }

    #[test]
    fn openkeys_and_stage3a_validation_fail_closed() {
        let conn = crate::open(":memory:").unwrap();
        let catalog = catalog("openkeys", 1, "openkeys-validation-catalog");
        let switches = openkeys_switches(1, 1, "openkeys-validation-switches");
        let mut policy = AccountPolicySpec {
            account_id: "current-openkeys".to_owned(),
            effective_version: 1,
            policy_id: "openkeys:current-openkeys".to_owned(),
            policy_version: 1,
            source_policy_digest: "current-openkeys-source".to_owned(),
            owner_type: PolicyOwnerType::OpenKeys,
            owner_id: "current-openkeys".to_owned(),
            account_class: AccountClass::OpenKeys,
            product_id: "openkeys".to_owned(),
            schema_version: super::super::PRICING_SCHEMA_VERSION,
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: "current-openkeys-policy".to_owned(),
            replacement_locked: false,
            rules: vec![
                discount_rule(
                    "anthropic-current",
                    "anthropic",
                    RuleOrigin::Managed,
                    10_000,
                ),
                discount_rule("openai-current", "openai", RuleOrigin::Managed, 10_000),
            ],
        };
        assert!(validate_account_policy(&policy, &catalog, &switches, Some(10_000)).is_ok());

        let mut malformed = policy.clone();
        malformed.effective_version = 0;
        assert!(matches!(
            sqlite_prepare_account_policy(&conn, &malformed).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        let mut wrong_product_policy = b2b_policy("wrong-product-b2b", 1, "wrong-product-policy");
        wrong_product_policy.product_id = "other-product".to_owned();
        assert!(matches!(
            sqlite_prepare_account_policy(&conn, &wrong_product_policy).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        let mut wrong_b2c_product = b2b_policy("wrong-product-b2c", 1, "wrong-product-b2c-policy");
        wrong_b2c_product.owner_type = PolicyOwnerType::GlobalB2c;
        wrong_b2c_product.account_class = AccountClass::B2c;
        wrong_b2c_product.product_id = "other-product".to_owned();
        assert!(matches!(
            sqlite_prepare_account_policy(&conn, &wrong_b2c_product).unwrap(),
            PricingMutation::Rejected(PricingRejection::Invalid { .. })
        ));

        let valid_rules = policy.rules.clone();
        policy.rules.clear();
        assert!(validate_account_policy(&policy, &catalog, &switches, Some(10_000)).is_err());

        policy.rules = valid_rules.clone();
        policy.rules[0] = discount_rule(
            "anthropic-discounted",
            "anthropic",
            RuleOrigin::Managed,
            9_000,
        );
        assert!(validate_account_policy(&policy, &catalog, &switches, Some(10_000)).is_err());

        policy.rules = valid_rules;
        policy.rules[0].scope = PolicyRuleScope::Model {
            provider_id: "anthropic".to_owned(),
            canonical_model_id: "claude-test".to_owned(),
        };
        assert!(validate_account_policy(&policy, &catalog, &switches, Some(10_000)).is_err());

        assert!(validate_account_policy_binding(&binding(
            PolicyEnforcement::Strict,
            FundingEnforcement::Shadow,
            ReconciliationState::Verified,
        ))
        .is_err());
        assert!(validate_account_policy_binding(&binding(
            PolicyEnforcement::Shadow,
            FundingEnforcement::Strict,
            ReconciliationState::Verified,
        ))
        .is_err());
    }

    fn unique_test_db() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "registry-pricing-cas-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    #[test]
    fn two_connections_cannot_both_win_an_absent_head_cas() {
        let path = unique_test_db();
        let path_string = path.to_string_lossy().into_owned();
        let first = crate::open(&path_string).unwrap();
        let second = crate::open(&path_string).unwrap();
        let catalog_one = catalog("main", 1, "race-one");
        let catalog_two = catalog("main", 2, "race-two");
        assert_eq!(
            sqlite_prepare_pricing_catalog(&first, &catalog_one).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_prepare_pricing_catalog(&first, &catalog_two).unwrap(),
            PricingMutation::Stored
        );

        let barrier = Arc::new(Barrier::new(2));
        let first_barrier = Arc::clone(&barrier);
        let first_target = catalog_one.target();
        let first_writer = thread::spawn(move || {
            first_barrier.wait();
            sqlite_activate_pricing_catalog(
                &first,
                "main",
                &first_target,
                &ActiveExpectation::Absent,
            )
            .unwrap()
        });
        let second_barrier = Arc::clone(&barrier);
        let second_target = catalog_two.target();
        let second_writer = thread::spawn(move || {
            second_barrier.wait();
            sqlite_activate_pricing_catalog(
                &second,
                "main",
                &second_target,
                &ActiveExpectation::Absent,
            )
            .unwrap()
        });
        let outcomes = [first_writer.join().unwrap(), second_writer.join().unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(PricingRejection::CasMismatch { .. })
                | PricingMutation::Rejected(PricingRejection::Stale { .. })
        )));

        let verification = crate::open(&path_string).unwrap();
        let active_catalog = sqlite_active_pricing_catalog(&verification, "main")
            .unwrap()
            .unwrap();

        let switch_one = b2b_switches(1, active_catalog.generation, "race-switch-one");
        let switch_two = b2b_switches(2, active_catalog.generation, "race-switch-two");
        assert_eq!(
            sqlite_prepare_provider_switches(&verification, &switch_one).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_prepare_provider_switches(&verification, &switch_two).unwrap(),
            PricingMutation::Stored
        );
        let switch_barrier = Arc::new(Barrier::new(2));
        let switch_writers = [switch_one.target(), switch_two.target()].map(|target| {
            let conn = crate::open(&path_string).unwrap();
            let barrier = Arc::clone(&switch_barrier);
            thread::spawn(move || {
                barrier.wait();
                sqlite_activate_provider_switches(&conn, &target, &ActiveExpectation::Absent)
                    .unwrap()
            })
        });
        let switch_outcomes = switch_writers.map(|writer| writer.join().unwrap());
        assert_eq!(
            switch_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(switch_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(PricingRejection::CasMismatch { .. })
                | PricingMutation::Rejected(PricingRejection::Stale { .. })
        )));

        let active_switch = sqlite_active_provider_switches(&verification)
            .unwrap()
            .unwrap();
        account_create(&verification, "race-account", None, 8_000).unwrap();
        let mut policy_one = b2b_policy("race-account", 1, "race-policy-one");
        policy_one.catalog_generation = active_catalog.generation;
        policy_one.switch_generation = active_switch.generation;
        let mut policy_two = b2b_policy("race-account", 2, "race-policy-two");
        policy_two.catalog_generation = active_catalog.generation;
        policy_two.switch_generation = active_switch.generation;
        assert_eq!(
            sqlite_prepare_account_policy(&verification, &policy_one).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            sqlite_prepare_account_policy(&verification, &policy_two).unwrap(),
            PricingMutation::Stored
        );
        let policy_barrier = Arc::new(Barrier::new(2));
        let policy_writers = [policy_one, policy_two].map(|policy| {
            let conn = crate::open(&path_string).unwrap();
            let barrier = Arc::clone(&policy_barrier);
            thread::spawn(move || {
                let activation = AccountPolicyActivationSpec {
                    account_id: policy.account_id,
                    effective_version: policy.effective_version,
                    content_digest: policy.content_digest,
                    binding: shadow_binding(),
                };
                barrier.wait();
                sqlite_activate_account_policy(
                    &conn,
                    &activation,
                    &PolicyActiveExpectation::Unbound,
                )
                .unwrap()
            })
        });
        let policy_outcomes = policy_writers.map(|writer| writer.join().unwrap());
        assert_eq!(
            policy_outcomes
                .iter()
                .filter(|outcome| **outcome == PricingMutation::Applied)
                .count(),
            1
        );
        assert!(policy_outcomes.iter().any(|outcome| matches!(
            outcome,
            PricingMutation::Rejected(PricingRejection::PolicyCasMismatch { .. })
                | PricingMutation::Rejected(PricingRejection::Stale { .. })
        )));

        drop(verification);
        for candidate in [
            path_string.clone(),
            format!("{path_string}-wal"),
            format!("{path_string}-shm"),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }
}
