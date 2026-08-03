//! Read-only Stage 8 synchronization and pricing-shadow evidence.
//!
//! The report is deliberately PostgreSQL-only: production authority, immutable admission
//! snapshots and shadow evaluations must be observed in one `REPEATABLE READ READ ONLY` snapshot.
//! It never changes a head, binding, funding authority, reservation or charge.

use crate::pricing::{
    BillingModeV2, PricingReleaseKindV2, PricingReleaseV2, PricingRuntimeManifestEvidence,
    PRICING_RELEASE_SCHEMA_VERSION, PRICING_SCHEMA_VERSION,
};
use anyhow::{bail, Context, Result};
use postgres::types::ToSql;
use postgres::{Client, GenericClient, IsolationLevel};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const STAGE8_ENGINE_EVIDENCE_SCHEMA_VERSION: i64 = 2;
const BLOCKER_SUBJECT_LIMIT: usize = 20;
const STAGE8_SHADOW_PROVIDERS: [&str; 3] = ["anthropic", "openai", "google"];

#[derive(Clone, Debug)]
pub struct Stage8EngineEvidenceRequest {
    pub target_generation: i64,
    pub recovery_generation: i64,
    pub window_start_ts: i64,
    pub window_end_ts: i64,
    pub min_samples_per_provider: i64,
    pub financial_sample_size: usize,
    pub gemini_client_admissions: i64,
    pub runtime_manifest: PricingRuntimeManifestEvidence,
}

impl Stage8EngineEvidenceRequest {
    fn validate(&self, captured_ts: i64) -> Result<()> {
        if self.target_generation <= 0 || self.recovery_generation <= self.target_generation {
            bail!("Stage 8 requires a positive target and a newer recovery generation");
        }
        if self.window_start_ts <= 0 || self.window_end_ts <= self.window_start_ts {
            bail!("Stage 8 evidence window must be a positive non-empty half-open interval");
        }
        if self.window_end_ts > captured_ts {
            bail!("Stage 8 evidence window cannot end in the future");
        }
        if !(1..=1_000_000).contains(&self.min_samples_per_provider) {
            bail!("Stage 8 minimum provider sample must be between 1 and 1000000");
        }
        if !(1..=1_000).contains(&self.financial_sample_size) {
            bail!("Stage 8 financial sample size must be between 1 and 1000");
        }
        if self.gemini_client_admissions < 0 {
            bail!("Stage 8 Gemini admission observation cannot be negative");
        }
        if self.runtime_manifest.capabilities().is_empty() {
            bail!("Stage 8 runtime manifest must contain at least one capability");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8EvidenceBlocker {
    pub code: String,
    pub count: i64,
    pub subject_digests: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8CatalogHeadEvidence {
    pub product_id: String,
    pub generation: i64,
    pub schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
    pub content_digest: String,
    pub enabled_entries: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8SwitchHeadEvidence {
    pub generation: i64,
    pub schema_version: i64,
    pub capability_generation: i64,
    pub capability_digest: String,
    pub content_digest: String,
    pub entries: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8RuntimeManifestEvidence {
    pub generation: i64,
    pub digest: String,
    pub capabilities: Vec<Stage8RuntimeCapabilityEvidence>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8RuntimeCapabilityEvidence {
    pub schema_version: i64,
    pub generation: i64,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8ReleaseHeadEvidence {
    pub active_generation: i64,
    pub active_digest: String,
    pub head_version: i64,
    pub updated_ts: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8ReleasePairEvidence {
    pub target_generation: i64,
    pub target_digest: Option<String>,
    pub recovery_generation: i64,
    pub recovery_digest: Option<String>,
    pub recovery_link_digest: Option<String>,
    pub inventory_digest: Option<String>,
    pub funding_digest: Option<String>,
    pub target_assignment_count: i64,
    pub recovery_assignment_count: i64,
    pub active_head: Option<Stage8ReleaseHeadEvidence>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8FinancialSample {
    pub subject_digest: String,
    pub evaluation_digest: String,
    pub provider_id: String,
    pub account_class: String,
    pub authorized_multiplier_bp: i64,
    pub payable_multiplier_bp: i64,
    pub official_hold_nano: i64,
    pub legacy_hold_nano: i64,
    pub policy_hold_nano: i64,
    pub comparison_result: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8EngineEvidenceCounts {
    pub total_accounts: i64,
    pub active_accounts: i64,
    pub account_classes: BTreeMap<String, i64>,
    pub reconciled_accounts: i64,
    pub snapshots_by_provider: BTreeMap<String, i64>,
    pub evaluations_by_outcome: BTreeMap<String, i64>,
    pub comparisons: BTreeMap<String, i64>,
    pub scalar_parity_rows: i64,
    pub policy_divergence_rows: i64,
    pub gemini_usage_rows: i64,
    pub gemini_outbox_rows: i64,
    pub live_runtime_instances: i64,
    pub release_capable_runtime_instances: i64,
    pub legacy_inflight_reservations: i64,
    pub legacy_inflight_outbox_rows: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Stage8EngineEvidenceReport {
    pub schema_version: i64,
    pub captured_ts: i64,
    pub window_start_ts: i64,
    pub window_end_ts: i64,
    pub min_samples_per_provider: i64,
    pub gemini_client_admissions: i64,
    pub passed: bool,
    pub release: Stage8ReleasePairEvidence,
    pub runtime_manifest: Stage8RuntimeManifestEvidence,
    pub catalogs: Vec<Stage8CatalogHeadEvidence>,
    pub switches: Option<Stage8SwitchHeadEvidence>,
    pub counts: Stage8EngineEvidenceCounts,
    pub financial_samples: Vec<Stage8FinancialSample>,
    pub engine_inventory_digest: String,
    pub funding_digest: String,
    pub shadow_digest: String,
    pub runtime_floor_digest: String,
    pub legacy_inflight_count: i64,
    pub blockers: Vec<Stage8EvidenceBlocker>,
    pub evidence_digest: String,
}

#[derive(Clone, Debug)]
struct RawFinancialSample {
    request_id: String,
    account_id: String,
    evaluation_digest: String,
    provider_id: String,
    account_class: String,
    authorized_multiplier_bp: i64,
    payable_multiplier_bp: i64,
    official_hold_nano: i64,
    legacy_hold_nano: i64,
    policy_hold_nano: i64,
    comparison_result: String,
}

fn digest(domain: &[u8], payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(payload);
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("sha256:v1:{hex}")
}

fn subject_digest(subject: &str) -> String {
    digest(
        b"claude-api/multi-discount-stage8/engine-subject/v1\0",
        subject.as_bytes(),
    )
}

fn report_digest(report: &Stage8EngineEvidenceReport) -> Result<String> {
    let encoded = serde_json::to_vec(report).context("encode Stage 8 engine evidence")?;
    Ok(digest_parts(
        b"claude-api/multi-discount-stage8/engine-evidence/v2\0",
        std::iter::once(encoded),
    ))
}

fn digest_parts<I, P>(domain: &[u8], parts: I) -> String
where
    I: IntoIterator<Item = P>,
    P: AsRef<[u8]>,
{
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        let part = part.as_ref();
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("sha256:v2:{hex}")
}

fn query_count<C: GenericClient>(
    client: &mut C,
    sql: &str,
    parameters: &[&(dyn ToSql + Sync)],
) -> Result<i64> {
    Ok(client.query_one(sql, parameters)?.get(0))
}

fn grouped_counts<C: GenericClient>(
    client: &mut C,
    sql: &str,
    parameters: &[&(dyn ToSql + Sync)],
) -> Result<BTreeMap<String, i64>> {
    client
        .query(sql, parameters)?
        .into_iter()
        .map(|row| Ok((row.get(0), row.get(1))))
        .collect()
}

fn push_subjects(blockers: &mut Vec<Stage8EvidenceBlocker>, code: &str, mut subjects: Vec<String>) {
    if subjects.is_empty() {
        return;
    }
    subjects.sort();
    subjects.dedup();
    blockers.push(Stage8EvidenceBlocker {
        code: code.to_owned(),
        count: subjects.len() as i64,
        subject_digests: subjects
            .iter()
            .take(BLOCKER_SUBJECT_LIMIT)
            .map(|subject| subject_digest(subject))
            .collect(),
    });
}

fn query_blocker<C: GenericClient>(
    client: &mut C,
    blockers: &mut Vec<Stage8EvidenceBlocker>,
    code: &str,
    sql: &str,
    parameters: &[&(dyn ToSql + Sync)],
) -> Result<()> {
    let subjects = client
        .query(sql, parameters)?
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect();
    push_subjects(blockers, code, subjects);
    Ok(())
}

fn manifest_evidence(manifest: &PricingRuntimeManifestEvidence) -> Stage8RuntimeManifestEvidence {
    Stage8RuntimeManifestEvidence {
        generation: manifest.manifest_generation(),
        digest: manifest.manifest_digest().to_owned(),
        capabilities: manifest
            .capabilities()
            .iter()
            .map(|capability| Stage8RuntimeCapabilityEvidence {
                schema_version: capability.pricing_schema_version(),
                generation: capability.capability_generation(),
                digest: capability.capability_digest().to_owned(),
            })
            .collect(),
    }
}

fn supports_capability(
    manifest: &Stage8RuntimeManifestEvidence,
    schema_version: i64,
    generation: i64,
    digest: &str,
) -> bool {
    manifest.capabilities.iter().any(|capability| {
        capability.schema_version == schema_version
            && capability.generation == generation
            && capability.digest == digest
    })
}

#[derive(Serialize)]
struct EngineInventoryIdentity<'a> {
    account_id: &'a str,
    multiplier_bp: i64,
    status: &'a str,
}

#[derive(Serialize)]
struct FundingManifestIdentity<'a> {
    account_id: &'a str,
    funding_digest: &'a str,
    funding_generation: String,
}

#[derive(Serialize)]
struct CanonicalScope<'a, T: Serialize> {
    scope: &'a str,
    value: T,
}

fn sha256_v2_json(domain: &[u8], value: &impl Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(value).context("encode Stage 8 canonical identity")?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(encoded);
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(format!("sha256:v2:{hex}"))
}

pub(crate) fn engine_inventory_digest<C: GenericClient>(client: &mut C) -> Result<String> {
    let rows = client.query(
        "SELECT id,status,mult_bp FROM accounts ORDER BY id COLLATE \"C\"",
        &[],
    )?;
    let identities = rows
        .iter()
        .map(|row| EngineInventoryIdentity {
            account_id: row.get(0),
            multiplier_bp: row.get(2),
            status: row.get(1),
        })
        .collect::<Vec<_>>();
    sha256_v2_json(
        b"pricing-stage5-v2:engine-identity-inventory\n",
        &identities,
    )
}

pub(crate) fn release_base_inventory_digest_v2<C: GenericClient>(
    client: &mut C,
    release_generation: i64,
) -> Result<String> {
    let rows = client.query(
        "SELECT account.id,account.status,account.mult_bp \
         FROM pricing_release_assignments assignment \
         JOIN accounts account ON account.id=assignment.account_id \
         WHERE assignment.release_generation=$1 \
         ORDER BY account.id COLLATE \"C\"",
        &[&release_generation],
    )?;
    let identities = rows
        .iter()
        .map(|row| EngineInventoryIdentity {
            account_id: row.get(0),
            multiplier_bp: row.get(2),
            status: row.get(1),
        })
        .collect::<Vec<_>>();
    sha256_v2_json(
        b"pricing-stage5-v2:engine-identity-inventory\n",
        &identities,
    )
}

pub(crate) fn funding_manifest_digest<C: GenericClient>(
    client: &mut C,
    release: Option<&PricingReleaseV2>,
) -> Result<String> {
    let Some(release) = release else {
        return Ok(digest_parts(
            b"claude-api/stage8-v2/missing-funding-manifest\0",
            std::iter::empty::<&[u8]>(),
        ));
    };
    let mut rows = Vec::new();
    for assignment in &release.assignments {
        if assignment.billing_mode != BillingModeV2::Balance {
            continue;
        }
        let generation = assignment
            .funding_generation
            .context("balance release assignment lacks funding generation")?;
        let row = client.query_opt(
            "SELECT normalization_digest FROM account_funding_generations_v2 \
             WHERE account_id=$1 AND generation=$2",
            &[&assignment.account_id, &generation],
        )?;
        let digest = row
            .as_ref()
            .map(|row| row.get::<_, String>(0))
            .unwrap_or_default();
        rows.push((assignment.account_id.as_str(), digest, generation));
    }
    rows.sort_by(|left, right| left.0.cmp(right.0));
    let identities = rows
        .iter()
        .map(|(account_id, digest, generation)| FundingManifestIdentity {
            account_id,
            funding_digest: digest,
            funding_generation: generation.to_string(),
        })
        .collect::<Vec<_>>();
    sha256_v2_json(
        b"",
        &CanonicalScope {
            scope: "pricing-funding-normalization-manifest-v2",
            value: identities,
        },
    )
}

pub(crate) struct RuntimeFloorCheckV2 {
    pub digest: String,
    pub live_instances: i64,
    pub compatible_instances: i64,
    pub incompatible_instance_ids: Vec<String>,
}

pub(crate) fn runtime_floor_check_v2<C: GenericClient>(
    client: &mut C,
    captured_ts: i64,
    minimum_runtime_schema_version: i64,
    runtime_manifest: &PricingRuntimeManifestEvidence,
) -> Result<RuntimeFloorCheckV2> {
    let rows = client.query(
        "SELECT instance_id,pricing_release_schema_version,funding_schema_version, \
                pricing_release_runtime_digest,owner_epoch,pricing_release_claim_epoch \
         FROM engine_instances WHERE lease_until >= $1 \
         ORDER BY instance_id COLLATE \"C\"",
        &[&captured_ts],
    )?;
    let incompatible_instance_ids = rows
        .iter()
        .filter_map(|row| {
            let release_schema = row.get::<_, Option<i64>>(1);
            let funding_schema = row.get::<_, Option<i64>>(2);
            let runtime_digest = row.get::<_, Option<String>>(3);
            let owner_epoch = row.get::<_, i64>(4);
            let claim_epoch = row.get::<_, Option<i64>>(5);
            (release_schema.is_none_or(|version| version < minimum_runtime_schema_version)
                || funding_schema.is_none_or(|version| version < PRICING_RELEASE_SCHEMA_VERSION)
                || runtime_digest.as_deref()
                    != Some(crate::pricing::PRICING_RELEASE_RUNTIME_DIGEST_V2)
                || claim_epoch != Some(owner_epoch))
            .then(|| row.get::<_, String>(0))
        })
        .collect::<Vec<_>>();
    let runtime_manifest = manifest_evidence(runtime_manifest);
    let runtime_floor_parts = rows
        .iter()
        .map(|row| {
            format!(
                "{}\0{}\0{}\0{}\0{}\0{}",
                row.get::<_, String>(0),
                row.get::<_, Option<i64>>(1)
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.get::<_, Option<i64>>(2)
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                row.get::<_, Option<String>>(3).unwrap_or_default(),
                row.get::<_, i64>(4),
                row.get::<_, Option<i64>>(5)
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            )
        })
        .chain(std::iter::once(format!(
            "manifest\0{}\0{}",
            runtime_manifest.generation, runtime_manifest.digest
        )))
        .collect::<Vec<_>>();
    let live_instances = rows.len() as i64;
    Ok(RuntimeFloorCheckV2 {
        digest: digest_parts(
            b"claude-api/stage8-v2/runtime-floor\0",
            runtime_floor_parts.iter(),
        ),
        live_instances,
        compatible_instances: live_instances - incompatible_instance_ids.len() as i64,
        incompatible_instance_ids,
    })
}

fn insufficient_provider_coverage(counts: &BTreeMap<String, i64>, minimum: i64) -> Vec<String> {
    STAGE8_SHADOW_PROVIDERS
        .iter()
        .filter_map(|provider| {
            let count = counts.get(*provider).copied().unwrap_or(0);
            (count < minimum).then(|| format!("{provider}:{count}"))
        })
        .collect()
}

pub(crate) fn postgres_stage8_engine_evidence(
    client: &mut Client,
    request: &Stage8EngineEvidenceRequest,
) -> Result<Stage8EngineEvidenceReport> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .context("begin Stage 8 engine evidence transaction")?;
    transaction.batch_execute("SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='5s'")?;
    let captured_ts: i64 = transaction
        .query_one(
            "SELECT floor(extract(epoch FROM transaction_timestamp()))::bigint",
            &[],
        )?
        .get(0);
    request.validate(captured_ts)?;

    let catalogs = transaction
        .query(
            "SELECT version.product_id,version.generation,version.schema_version, \
                    version.capability_generation,version.capability_digest,version.content_digest, \
                    COUNT(entry.canonical_model_id) FILTER (WHERE entry.enabled)::bigint \
             FROM pricing_catalog_heads head \
             JOIN pricing_catalog_versions version \
               ON version.product_id=head.product_id AND version.generation=head.active_generation \
             LEFT JOIN pricing_catalog_entries entry \
               ON entry.product_id=version.product_id AND entry.generation=version.generation \
             GROUP BY version.product_id,version.generation,version.schema_version, \
                      version.capability_generation,version.capability_digest,version.content_digest \
             ORDER BY version.product_id COLLATE \"C\"",
            &[],
        )?
        .into_iter()
        .map(|row| Stage8CatalogHeadEvidence {
            product_id: row.get(0),
            generation: row.get(1),
            schema_version: row.get(2),
            capability_generation: row.get(3),
            capability_digest: row.get(4),
            content_digest: row.get(5),
            enabled_entries: row.get(6),
        })
        .collect::<Vec<_>>();
    let switches = transaction
        .query_opt(
            "SELECT version.generation,version.schema_version,version.capability_generation, \
                    version.capability_digest,version.content_digest,COUNT(entry.provider_id)::bigint \
             FROM provider_switch_head head \
             JOIN provider_switch_versions version ON version.generation=head.active_generation \
             LEFT JOIN provider_switch_entries entry ON entry.generation=version.generation \
             WHERE head.singleton=1 \
             GROUP BY version.generation,version.schema_version,version.capability_generation, \
                      version.capability_digest,version.content_digest",
            &[],
        )?
        .map(|row| Stage8SwitchHeadEvidence {
            generation: row.get(0),
            schema_version: row.get(1),
            capability_generation: row.get(2),
            capability_digest: row.get(3),
            content_digest: row.get(4),
            entries: row.get(5),
        });
    let runtime_manifest = manifest_evidence(&request.runtime_manifest);
    let mut blockers = Vec::new();

    let target = crate::pricing::postgres::pricing_release_v2_in_transaction(
        &mut transaction,
        request.target_generation,
    )?;
    let recovery = crate::pricing::postgres::pricing_release_v2_in_transaction(
        &mut transaction,
        request.recovery_generation,
    )?;
    let recovery_link = transaction.query_opt(
        "SELECT target_digest,recovery_digest,link_digest \
         FROM pricing_release_recovery_links \
         WHERE target_generation=$1 AND recovery_generation=$2",
        &[&request.target_generation, &request.recovery_generation],
    )?;
    let active_release_head = transaction
        .query_opt(
            "SELECT active_generation,active_digest,head_version,updated_ts \
         FROM pricing_release_head_v2 WHERE singleton=1",
            &[],
        )?
        .map(|row| Stage8ReleaseHeadEvidence {
            active_generation: row.get(0),
            active_digest: row.get(1),
            head_version: row.get(2),
            updated_ts: row.get(3),
        });
    let recovery_evidence_mode = active_release_head.as_ref().is_some_and(|head| {
        head.active_generation == request.target_generation
            && target.as_ref().is_some_and(|release| {
                release.release_kind == PricingReleaseKindV2::Target
                    && head.active_digest == release.content_digest
            })
    });
    let current_inventory_digest = if recovery_evidence_mode {
        release_base_inventory_digest_v2(&mut transaction, request.target_generation)?
    } else {
        engine_inventory_digest(&mut transaction)?
    };
    let current_funding_digest = funding_manifest_digest(&mut transaction, target.as_ref())?;

    if active_release_head.is_some() && !recovery_evidence_mode {
        push_subjects(
            &mut blockers,
            "active_release_head_outside_requested_target",
            vec![active_release_head
                .as_ref()
                .map(|head| format!("{}:{}", head.active_generation, head.head_version))
                .unwrap_or_default()],
        );
    }

    if target.is_none() {
        push_subjects(
            &mut blockers,
            "target_release_missing",
            vec![format!("target:{}", request.target_generation)],
        );
    }
    if recovery.is_none() {
        push_subjects(
            &mut blockers,
            "recovery_release_missing",
            vec![format!("recovery:{}", request.recovery_generation)],
        );
    }
    if recovery_link.is_none() {
        push_subjects(
            &mut blockers,
            "target_recovery_link_missing",
            vec![format!(
                "{}:{}",
                request.target_generation, request.recovery_generation
            )],
        );
    }

    if let Some(target) = target.as_ref() {
        let mut release_failures = Vec::new();
        if target.release_kind != PricingReleaseKindV2::Target {
            release_failures.push("kind".to_owned());
        }
        if target.inventory_digest != current_inventory_digest {
            release_failures.push("inventory".to_owned());
        }
        if target.funding_manifest_digest != current_funding_digest {
            release_failures.push("funding".to_owned());
        }
        if target.minimum_runtime_schema_version > PRICING_RELEASE_SCHEMA_VERSION
            || !supports_capability(
                &runtime_manifest,
                PRICING_SCHEMA_VERSION,
                target.capability_generation,
                &target.capability_digest,
            )
        {
            release_failures.push("runtime".to_owned());
        }
        push_subjects(
            &mut blockers,
            "target_release_identity_drift",
            release_failures,
        );
    }
    if let Some(recovery) = recovery.as_ref() {
        let mut release_failures = Vec::new();
        if recovery.release_kind != PricingReleaseKindV2::Recovery {
            release_failures.push("kind".to_owned());
        }
        if recovery.inventory_digest != current_inventory_digest {
            release_failures.push("inventory".to_owned());
        }
        if recovery.funding_manifest_digest != current_funding_digest {
            release_failures.push("funding".to_owned());
        }
        if recovery.minimum_runtime_schema_version > PRICING_RELEASE_SCHEMA_VERSION
            || !supports_capability(
                &runtime_manifest,
                PRICING_SCHEMA_VERSION,
                recovery.capability_generation,
                &recovery.capability_digest,
            )
        {
            release_failures.push("runtime".to_owned());
        }
        push_subjects(
            &mut blockers,
            "recovery_release_identity_drift",
            release_failures,
        );
    }
    if let (Some(target), Some(recovery)) = (target.as_ref(), recovery.as_ref()) {
        let same_runtime_lineage = target.schema_version == recovery.schema_version
            && target.capability_generation == recovery.capability_generation
            && target.capability_digest == recovery.capability_digest
            && target.main_catalog_generation == recovery.main_catalog_generation
            && target.main_catalog_digest == recovery.main_catalog_digest
            && target.openkeys_catalog_generation == recovery.openkeys_catalog_generation
            && target.openkeys_catalog_digest == recovery.openkeys_catalog_digest
            && target.switch_generation == recovery.switch_generation
            && target.switch_digest == recovery.switch_digest
            && target.inventory_digest == recovery.inventory_digest
            && target.funding_manifest_digest == recovery.funding_manifest_digest
            && target.minimum_runtime_schema_version == recovery.minimum_runtime_schema_version;
        if !same_runtime_lineage {
            push_subjects(
                &mut blockers,
                "target_recovery_lineage_mismatch",
                vec![format!("{}:{}", target.generation, recovery.generation)],
            );
        }
        let target_assignments = target
            .assignments
            .iter()
            .map(|assignment| {
                (
                    assignment.account_id.as_str(),
                    assignment.billing_mode,
                    assignment.funding_generation,
                )
            })
            .collect::<Vec<_>>();
        let recovery_assignments = recovery
            .assignments
            .iter()
            .map(|assignment| {
                (
                    assignment.account_id.as_str(),
                    assignment.billing_mode,
                    assignment.funding_generation,
                )
            })
            .collect::<Vec<_>>();
        if target_assignments != recovery_assignments {
            push_subjects(
                &mut blockers,
                "target_recovery_funding_assignment_mismatch",
                vec![format!("{}:{}", target.generation, recovery.generation)],
            );
        }
    }

    if let Some(link) = recovery_link.as_ref() {
        let target_digest = link.get::<_, String>(0);
        let recovery_digest = link.get::<_, String>(1);
        if target
            .as_ref()
            .map(|release| release.content_digest.as_str())
            != Some(target_digest.as_str())
            || recovery
                .as_ref()
                .map(|release| release.content_digest.as_str())
                != Some(recovery_digest.as_str())
        {
            push_subjects(
                &mut blockers,
                "target_recovery_link_digest_mismatch",
                vec![format!(
                    "{}:{}",
                    request.target_generation, request.recovery_generation
                )],
            );
        }
    }

    for (generation, label) in [
        (request.target_generation, "target"),
        (request.recovery_generation, "recovery"),
    ] {
        query_blocker(
            &mut transaction,
            &mut blockers,
            &format!("{label}_release_assignment_inventory_drift"),
            "SELECT subject FROM( \
               SELECT 'account:'||account.id subject FROM accounts account \
               WHERE NOT EXISTS( \
                   SELECT 1 FROM pricing_release_assignments assignment \
                   WHERE assignment.release_generation=$1 AND assignment.account_id=account.id) \
                 AND NOT($2 AND EXISTS( \
                   SELECT 1 FROM pricing_release_assignment_extensions_v2 extension \
                   WHERE extension.release_generation=$1 AND extension.account_id=account.id \
                     AND extension.provisioning_head_generation=$3 \
                     AND extension.provisioning_head_digest=$4 \
                     AND extension.provisioning_head_version=$5 \
                     AND extension.paired_recovery_generation=$6 \
                     AND extension.paired_recovery_digest=$7)) \
               UNION ALL \
               SELECT 'assignment:'||assignment.account_id FROM pricing_release_assignments assignment \
               LEFT JOIN accounts account ON account.id=assignment.account_id \
               WHERE assignment.release_generation=$1 AND account.id IS NULL \
             ) drift",
            &[
                &generation,
                &recovery_evidence_mode,
                &request.target_generation,
                &target
                    .as_ref()
                    .map(|release| release.content_digest.as_str())
                    .unwrap_or_default(),
                &active_release_head
                    .as_ref()
                    .map(|head| head.head_version)
                    .unwrap_or(0),
                &request.recovery_generation,
                &recovery
                    .as_ref()
                    .map(|release| release.content_digest.as_str())
                    .unwrap_or_default(),
            ],
        )?;
        query_blocker(
            &mut transaction,
            &mut blockers,
            &format!("{label}_release_funding_head_drift"),
            "WITH assignment AS( \
               SELECT account_id,billing_mode,funding_generation \
                 FROM pricing_release_assignments WHERE release_generation=$1 \
               UNION \
               SELECT account_id,billing_mode,funding_generation \
                 FROM pricing_release_assignment_extensions_v2 \
                 WHERE $2 AND release_generation=$1 \
                   AND provisioning_head_generation=$3 AND provisioning_head_digest=$4 \
                   AND provisioning_head_version=$5 \
                   AND paired_recovery_generation=$6 AND paired_recovery_digest=$7 \
             ) SELECT assignment.account_id FROM assignment \
             LEFT JOIN account_funding_head_v2 head ON head.account_id=assignment.account_id \
             WHERE assignment.billing_mode='balance' \
               AND head.active_generation IS DISTINCT FROM assignment.funding_generation",
            &[
                &generation,
                &recovery_evidence_mode,
                &request.target_generation,
                &target
                    .as_ref()
                    .map(|release| release.content_digest.as_str())
                    .unwrap_or_default(),
                &active_release_head
                    .as_ref()
                    .map(|head| head.head_version)
                    .unwrap_or(0),
                &request.recovery_generation,
                &recovery
                    .as_ref()
                    .map(|release| release.content_digest.as_str())
                    .unwrap_or_default(),
            ],
        )?;
    }
    query_blocker(
        &mut transaction,
        &mut blockers,
        "target_release_funding_v2_aggregate_mismatch",
        "WITH assignment AS( \
           SELECT account_id,billing_mode,funding_generation \
             FROM pricing_release_assignments WHERE release_generation=$1 \
           UNION \
           SELECT account_id,billing_mode,funding_generation \
             FROM pricing_release_assignment_extensions_v2 \
             WHERE $2 AND release_generation=$1 \
               AND provisioning_head_generation=$1 AND provisioning_head_digest=$3 \
               AND provisioning_head_version=$4 \
               AND paired_recovery_generation=$5 AND paired_recovery_digest=$6 \
         ) SELECT assignment.account_id \
         FROM assignment \
         JOIN accounts account ON account.id=assignment.account_id \
         LEFT JOIN account_funding_head_v2 head ON head.account_id=assignment.account_id \
         LEFT JOIN account_funding_generations_v2 generation \
           ON generation.account_id=assignment.account_id \
          AND generation.generation=assignment.funding_generation \
         LEFT JOIN LATERAL( \
           SELECT COUNT(*)::bigint lot_count, \
                  COUNT(*) FILTER(WHERE lot.source_type='paid')::bigint paid_count, \
                  COALESCE(SUM(lot.balance_nano::numeric),0) balance_nano, \
                  COALESCE(SUM(lot.reserved_nano::numeric),0) reserved_nano, \
                  COALESCE(SUM(lot.spent_nano::numeric),0) spent_nano \
           FROM funding_lots_v2 lot \
           WHERE lot.account_id=assignment.account_id \
             AND lot.funding_generation=assignment.funding_generation \
         ) lots ON true \
         WHERE assignment.billing_mode='balance' \
           AND( head.active_generation IS DISTINCT FROM assignment.funding_generation \
             OR generation.account_id IS NULL OR lots.lot_count=0 OR lots.paid_count=0 \
             OR generation.balance_nano::numeric<>account.balance_nano::numeric \
             OR generation.reserved_nano::numeric<>account.reserved_nano::numeric \
             OR generation.spent_nano::numeric<>account.spent_nano::numeric \
             OR lots.balance_nano<>generation.balance_nano::numeric \
             OR lots.reserved_nano<>generation.reserved_nano::numeric \
             OR lots.spent_nano<>generation.spent_nano::numeric)",
        &[
            &request.target_generation,
            &recovery_evidence_mode,
            &target
                .as_ref()
                .map(|release| release.content_digest.as_str())
                .unwrap_or_default(),
            &active_release_head
                .as_ref()
                .map(|head| head.head_version)
                .unwrap_or(0),
            &request.recovery_generation,
            &recovery
                .as_ref()
                .map(|release| release.content_digest.as_str())
                .unwrap_or_default(),
        ],
    )?;

    query_blocker(
        &mut transaction,
        &mut blockers,
        "catalog_head_missing_or_stale",
        "WITH required(product_id) AS (VALUES('main'::text),('openkeys'::text)), \
         latest AS (SELECT product_id,max(generation) generation FROM pricing_catalog_versions GROUP BY product_id) \
         SELECT required.product_id FROM required \
         LEFT JOIN latest USING(product_id) \
         LEFT JOIN pricing_catalog_heads head \
           ON head.product_id=required.product_id AND head.active_generation=latest.generation \
         WHERE head.product_id IS NULL \
         UNION ALL SELECT product_id FROM pricing_catalog_heads WHERE product_id NOT IN('main','openkeys')",
        &[],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "switch_head_missing_or_stale",
        "SELECT 'switch-head' WHERE NOT EXISTS( \
           SELECT 1 FROM provider_switch_head \
           WHERE singleton=1 AND active_generation=(SELECT max(generation) FROM provider_switch_versions))",
        &[],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "active_product_graph_incomplete",
        "WITH required_catalog(product_id,provider_id) AS (VALUES \
             ('main'::text,'anthropic'::text),('main','openai'), \
             ('main','google'), \
             ('openkeys','anthropic'),('openkeys','openai')), \
         missing_catalog AS ( \
           SELECT product_id||':'||provider_id subject FROM required_catalog required \
           WHERE NOT EXISTS(SELECT 1 FROM pricing_catalog_heads head \
             JOIN pricing_catalog_entries entry ON entry.product_id=head.product_id \
              AND entry.generation=head.active_generation \
             WHERE entry.product_id=required.product_id AND entry.provider_id=required.provider_id \
              AND entry.enabled)), \
         unexpected_catalog AS ( \
           SELECT entry.product_id||':'||entry.provider_id FROM pricing_catalog_heads head \
           JOIN pricing_catalog_entries entry ON entry.product_id=head.product_id \
            AND entry.generation=head.active_generation \
           WHERE entry.provider_id NOT IN('anthropic','openai','google') \
              OR(entry.product_id='openkeys' AND entry.provider_id='google')), \
         required_switch(provider_id,scope_type,product_id,segment) AS (VALUES \
           ('anthropic'::text,'master'::text,''::text,''::text),('openai','master','',''),('google','master','',''), \
           ('anthropic','product','main',''),('openai','product','main',''), \
           ('google','product','main',''), \
           ('anthropic','product','openkeys',''),('openai','product','openkeys',''), \
           ('anthropic','segment','main','b2c'),('openai','segment','main','b2c'), \
           ('google','segment','main','b2c'), \
           ('anthropic','segment','main','b2b'),('openai','segment','main','b2b'), \
           ('google','segment','main','b2b')), \
         missing_switch AS ( \
           SELECT provider_id||':'||scope_type||':'||product_id||':'||segment FROM required_switch required \
           WHERE NOT EXISTS(SELECT 1 FROM provider_switch_head head \
             JOIN provider_switch_entries entry ON entry.generation=head.active_generation \
             WHERE entry.provider_id=required.provider_id AND entry.scope_type=required.scope_type \
              AND entry.product_id=required.product_id AND entry.segment=required.segment \
              AND entry.enabled)) \
         SELECT subject FROM missing_catalog UNION ALL SELECT * FROM unexpected_catalog \
         UNION ALL SELECT * FROM missing_switch",
        &[],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "active_product_graph_contains_deprecated_gemini_provider_id",
        "SELECT subject FROM( \
           SELECT 'catalog:'||entry.product_id||':'||entry.canonical_model_id subject \
           FROM pricing_catalog_heads head JOIN pricing_catalog_entries entry \
            ON entry.product_id=head.product_id AND entry.generation=head.active_generation \
           WHERE entry.provider_id='gemini' \
           UNION ALL SELECT 'switch:'||entry.scope_type||':'||entry.product_id||':'||entry.segment \
           FROM provider_switch_head head JOIN provider_switch_entries entry \
            ON entry.generation=head.active_generation WHERE entry.provider_id='gemini' \
           UNION ALL SELECT 'policy:'||rule.account_id||':'||rule.rule_id \
           FROM account_policy_bindings binding JOIN account_policy_rules rule \
            ON rule.account_id=binding.account_id AND rule.effective_version=binding.active_effective_version \
           WHERE rule.provider_id='gemini') candidates",
        &[],
    )?;

    let unsupported_catalogs = catalogs
        .iter()
        .filter(|catalog| {
            !supports_capability(
                &runtime_manifest,
                catalog.schema_version,
                catalog.capability_generation,
                &catalog.capability_digest,
            )
        })
        .map(|catalog| format!("catalog:{}:{}", catalog.product_id, catalog.generation))
        .collect();
    push_subjects(
        &mut blockers,
        "active_catalog_unsupported_by_runtime",
        unsupported_catalogs,
    );
    let unsupported_switch = switches
        .iter()
        .filter(|switches| {
            !supports_capability(
                &runtime_manifest,
                switches.schema_version,
                switches.capability_generation,
                &switches.capability_digest,
            )
        })
        .map(|switches| format!("switches:{}", switches.generation))
        .collect();
    push_subjects(
        &mut blockers,
        "active_switches_unsupported_by_runtime",
        unsupported_switch,
    );

    query_blocker(
        &mut transaction,
        &mut blockers,
        "active_account_shadow_binding_drift",
        "SELECT account.id FROM accounts account \
         LEFT JOIN account_policy_bindings binding ON binding.account_id=account.id \
         LEFT JOIN account_policy_versions policy ON policy.account_id=binding.account_id \
          AND policy.effective_version=binding.active_effective_version \
         WHERE account.status='active' AND( \
           binding.account_id IS NULL OR binding.active_effective_version IS NULL \
           OR policy.account_id IS NULL OR binding.account_class<>policy.account_class \
           OR binding.product_id IS DISTINCT FROM policy.product_id \
           OR binding.policy_enforcement<>'shadow')",
        &[],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "active_policy_or_dependency_stale",
        "SELECT binding.account_id FROM account_policy_bindings binding \
         JOIN accounts account ON account.id=binding.account_id AND account.status='active' \
         JOIN account_policy_versions policy ON policy.account_id=binding.account_id \
          AND policy.effective_version=binding.active_effective_version \
         LEFT JOIN pricing_catalog_heads catalog ON catalog.product_id=binding.product_id \
         LEFT JOIN provider_switch_head switches ON switches.singleton=1 \
         WHERE binding.active_effective_version IS DISTINCT FROM( \
             SELECT max(candidate.effective_version) FROM account_policy_versions candidate \
             WHERE candidate.account_id=binding.account_id) \
           OR policy.catalog_generation IS DISTINCT FROM catalog.active_generation \
           OR policy.switch_generation IS DISTINCT FROM switches.active_generation",
        &[],
    )?;
    // The target release funding-generation/head/lot aggregate checks above are the Stage 8
    // authority. Legacy `funding_buckets` are intentionally absent after Stage 6 normalization
    // and must not be reintroduced as a second, contradictory activation precondition.
    query_blocker(
        &mut transaction,
        &mut blockers,
        "authority_changed_during_validation_window",
        "SELECT subject FROM( \
           SELECT 'catalog:'||product_id subject FROM pricing_catalog_heads WHERE updated_ts >= $1 \
           UNION ALL SELECT 'switch-head' FROM provider_switch_head WHERE updated_ts >= $1 \
           UNION ALL SELECT 'binding:'||account_id FROM account_policy_bindings WHERE updated_ts >= $1 \
           UNION ALL SELECT 'account:'||id FROM accounts WHERE status='active' AND created_ts >= $1 \
           UNION ALL SELECT 'catalog-version:'||product_id||':'||generation::text \
             FROM pricing_catalog_versions WHERE created_ts >= $1 \
           UNION ALL SELECT 'switch-version:'||generation::text \
             FROM provider_switch_versions WHERE created_ts >= $1 \
           UNION ALL SELECT 'policy-version:'||account_id||':'||effective_version::text \
             FROM account_policy_versions WHERE created_ts >= $1) changed",
        &[&request.window_start_ts],
    )?;

    let mut shadow_provider_counts = BTreeMap::new();
    for provider in STAGE8_SHADOW_PROVIDERS {
        let count = query_count(
            &mut transaction,
            "SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
             WHERE snapshot_kind='legacy_scalar' AND provider_id=$1 \
              AND admission_ts >= $2 AND admission_ts < $3",
            &[&provider, &request.window_start_ts, &request.window_end_ts],
        )?;
        shadow_provider_counts.insert(provider.to_owned(), count);
    }
    push_subjects(
        &mut blockers,
        "insufficient_shadow_provider_coverage",
        insufficient_provider_coverage(&shadow_provider_counts, request.min_samples_per_provider),
    );
    query_blocker(
        &mut transaction,
        &mut blockers,
        "shadow_evaluation_missing_or_late",
        "SELECT snapshot.request_id FROM pricing_admission_snapshots snapshot \
         LEFT JOIN pricing_shadow_admission_evaluations evaluation \
          ON evaluation.request_id=snapshot.request_id \
         WHERE snapshot.snapshot_kind='legacy_scalar' \
          AND snapshot.provider_id IN('anthropic','openai','google') \
          AND snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
          AND(evaluation.request_id IS NULL OR evaluation.enqueued_ts < snapshot.admission_ts \
           OR evaluation.evaluated_ts < evaluation.enqueued_ts OR evaluation.evaluated_ts >= $2)",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "shadow_evaluation_not_resolved_by_expected_runtime",
        "SELECT evaluation.request_id FROM pricing_shadow_admission_evaluations evaluation \
         JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
         WHERE snapshot.snapshot_kind='legacy_scalar' \
          AND snapshot.provider_id IN('anthropic','openai','google') \
          AND snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
          AND(evaluation.outcome<>'resolved' \
           OR evaluation.runtime_manifest_generation<>$3 \
           OR evaluation.runtime_manifest_digest<>$4 \
           OR evaluation.observed_multiplier_bp IS DISTINCT FROM evaluation.authorized_multiplier_bp)",
        &[
            &request.window_start_ts,
            &request.window_end_ts,
            &runtime_manifest.generation,
            &runtime_manifest.digest,
        ],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "shadow_evaluation_lineage_drift",
        "SELECT evaluation.request_id FROM pricing_shadow_admission_evaluations evaluation \
         JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
         LEFT JOIN account_policy_bindings binding ON binding.account_id=evaluation.account_id \
         LEFT JOIN account_policy_versions policy ON policy.account_id=binding.account_id \
          AND policy.effective_version=binding.active_effective_version \
         LEFT JOIN pricing_catalog_heads catalog ON catalog.product_id=binding.product_id \
         LEFT JOIN pricing_catalog_versions catalog_version ON catalog_version.product_id=catalog.product_id \
          AND catalog_version.generation=catalog.active_generation \
         LEFT JOIN provider_switch_head switch_head ON switch_head.singleton=1 \
         LEFT JOIN provider_switch_versions switch_version \
          ON switch_version.generation=switch_head.active_generation \
         WHERE snapshot.snapshot_kind='legacy_scalar' \
          AND snapshot.provider_id IN('anthropic','openai','google') \
          AND snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
          AND(evaluation.outcome='resolved') AND( \
           evaluation.effective_policy_version IS DISTINCT FROM policy.effective_version \
           OR evaluation.policy_id IS DISTINCT FROM policy.policy_id \
           OR evaluation.policy_version IS DISTINCT FROM policy.policy_version \
           OR evaluation.source_policy_digest IS DISTINCT FROM policy.source_policy_digest \
           OR evaluation.policy_digest IS DISTINCT FROM policy.content_digest \
           OR evaluation.account_class IS DISTINCT FROM policy.account_class \
           OR evaluation.product_id IS DISTINCT FROM policy.product_id \
           OR evaluation.policy_catalog_generation IS DISTINCT FROM catalog.active_generation \
           OR evaluation.admission_catalog_generation IS DISTINCT FROM catalog.active_generation \
           OR evaluation.policy_catalog_digest IS DISTINCT FROM catalog_version.content_digest \
           OR evaluation.admission_catalog_digest IS DISTINCT FROM catalog_version.content_digest \
           OR evaluation.policy_switch_generation IS DISTINCT FROM switch_head.active_generation \
           OR evaluation.admission_switch_generation IS DISTINCT FROM switch_head.active_generation \
           OR evaluation.policy_switch_digest IS DISTINCT FROM switch_version.content_digest \
           OR evaluation.admission_switch_digest IS DISTINCT FROM switch_version.content_digest)",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "shadow_evaluation_differs_from_target_release",
        "SELECT evaluation.request_id \
         FROM pricing_shadow_admission_evaluations evaluation \
         JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
         LEFT JOIN LATERAL( \
           SELECT candidate.account_id,candidate.policy_id,candidate.policy_version, \
                  candidate.billing_mode \
           FROM( \
             SELECT assignment.account_id,assignment.policy_id,assignment.policy_version, \
                    assignment.billing_mode,0 priority \
             FROM pricing_release_assignments assignment \
             WHERE assignment.release_generation=$3 \
               AND assignment.account_id=evaluation.account_id \
             UNION ALL \
             SELECT extension.account_id,extension.policy_id,extension.policy_version, \
                    extension.billing_mode,1 priority \
             FROM pricing_release_assignment_extensions_v2 extension \
             WHERE $4 AND extension.release_generation=$3 \
               AND extension.account_id=evaluation.account_id \
               AND extension.provisioning_head_generation=$3 \
               AND extension.provisioning_head_digest=$5 \
               AND extension.provisioning_head_version=$6 \
               AND extension.paired_recovery_generation=$7 \
               AND extension.paired_recovery_digest=$8 \
           ) candidate ORDER BY candidate.priority LIMIT 1 \
         ) assignment ON true \
         LEFT JOIN LATERAL( \
           SELECT rule.payable_multiplier_bp \
           FROM pricing_release_policy_rules rule \
           WHERE rule.policy_id=assignment.policy_id \
             AND rule.policy_version=assignment.policy_version \
             AND( rule.scope_type='global' \
               OR(rule.scope_type='provider' AND rule.provider_id=snapshot.provider_id) \
               OR(rule.scope_type='model' AND rule.provider_id=snapshot.provider_id \
                  AND rule.canonical_model_id=snapshot.canonical_model_id)) \
           ORDER BY CASE rule.scope_type WHEN 'model' THEN 0 WHEN 'provider' THEN 1 ELSE 2 END \
           LIMIT 1 \
         ) target_rule ON true \
         WHERE snapshot.snapshot_kind='legacy_scalar' \
           AND snapshot.provider_id IN('anthropic','openai','google') \
           AND snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
           AND(assignment.account_id IS NULL OR assignment.billing_mode<>'balance' \
             OR target_rule.payable_multiplier_bp IS NULL \
             OR evaluation.payable_multiplier_bp IS DISTINCT FROM target_rule.payable_multiplier_bp)",
        &[
            &request.window_start_ts,
            &request.window_end_ts,
            &request.target_generation,
            &recovery_evidence_mode,
            &target
                .as_ref()
                .map(|release| release.content_digest.as_str())
                .unwrap_or_default(),
            &active_release_head
                .as_ref()
                .map(|head| head.head_version)
                .unwrap_or(0),
            &request.recovery_generation,
            &recovery
                .as_ref()
                .map(|release| release.content_digest.as_str())
                .unwrap_or_default(),
        ],
    )?;
    query_blocker(
        &mut transaction,
        &mut blockers,
        "shadow_nano_usd_mismatch",
        "WITH candidate AS( \
           SELECT evaluation.*, \
             div(evaluation.official_hold_nano::numeric*evaluation.authorized_multiplier_bp+5000,10000) scalar_uncapped, \
             div(evaluation.official_hold_nano::numeric*evaluation.payable_multiplier_bp+5000,10000) policy_uncapped \
           FROM pricing_shadow_admission_evaluations evaluation \
           JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
           WHERE snapshot.snapshot_kind='legacy_scalar' \
            AND snapshot.provider_id IN('anthropic','openai','google') \
            AND snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
            AND evaluation.outcome='resolved'), expected AS( \
           SELECT candidate.*,CASE WHEN legacy_hold_nano::numeric<scalar_uncapped \
             THEN LEAST(policy_uncapped,legacy_hold_nano::numeric) ELSE policy_uncapped END expected_policy \
           FROM candidate) \
         SELECT request_id FROM expected WHERE legacy_hold_nano::numeric>scalar_uncapped \
          OR policy_hold_nano::numeric<>expected_policy \
          OR comparison_result<>CASE WHEN expected_policy=legacy_hold_nano::numeric \
             THEN 'equal' ELSE 'different' END \
          OR(payable_multiplier_bp=authorized_multiplier_bp \
             AND(policy_hold_nano<>legacy_hold_nano OR comparison_result<>'equal'))",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;

    let gemini_usage_rows = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM usage_events WHERE provider='google' AND ts >= $1 AND ts < $2",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    let gemini_outbox_rows = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM settlement_outbox \
         WHERE provider='google' AND created_ts >= $1 AND created_ts < $2",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    let account_classes = grouped_counts(
        &mut transaction,
        "SELECT binding.account_class,COUNT(*)::bigint FROM accounts account \
         JOIN account_policy_bindings binding ON binding.account_id=account.id \
         WHERE account.status='active' GROUP BY binding.account_class ORDER BY binding.account_class",
        &[],
    )?;
    let snapshots_by_provider = shadow_provider_counts;
    let evaluations_by_outcome = grouped_counts(
        &mut transaction,
        "SELECT evaluation.outcome,COUNT(*)::bigint FROM pricing_shadow_admission_evaluations evaluation \
         JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
         WHERE snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
          AND snapshot.provider_id IN('anthropic','openai','google') GROUP BY evaluation.outcome ORDER BY evaluation.outcome",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    let comparisons = grouped_counts(
        &mut transaction,
        "SELECT evaluation.comparison_result,COUNT(*)::bigint \
         FROM pricing_shadow_admission_evaluations evaluation \
         JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
         WHERE snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
          AND snapshot.provider_id IN('anthropic','openai','google') \
         GROUP BY evaluation.comparison_result ORDER BY evaluation.comparison_result",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    let parity: (i64, i64) = {
        let row = transaction.query_one(
            "SELECT COUNT(*) FILTER(WHERE evaluation.payable_multiplier_bp=evaluation.authorized_multiplier_bp)::bigint, \
                    COUNT(*) FILTER(WHERE evaluation.payable_multiplier_bp<>evaluation.authorized_multiplier_bp)::bigint \
             FROM pricing_shadow_admission_evaluations evaluation \
             JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
             WHERE snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
              AND snapshot.provider_id IN('anthropic','openai','google') AND evaluation.outcome='resolved'",
            &[&request.window_start_ts, &request.window_end_ts],
        )?;
        (row.get(0), row.get(1))
    };

    let raw_samples = transaction
        .query(
            "SELECT evaluation.request_id,evaluation.account_id,evaluation.evaluation_digest, \
                    evaluation.provider_id,evaluation.account_class,evaluation.authorized_multiplier_bp, \
                    evaluation.payable_multiplier_bp,evaluation.official_hold_nano, \
                    evaluation.legacy_hold_nano,evaluation.policy_hold_nano,evaluation.comparison_result \
             FROM pricing_shadow_admission_evaluations evaluation \
             JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
             WHERE snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
              AND snapshot.provider_id IN('anthropic','openai','google') AND evaluation.outcome='resolved' \
             ORDER BY evaluation.evaluation_digest COLLATE \"C\" LIMIT $3",
            &[
                &request.window_start_ts,
                &request.window_end_ts,
                &(request.financial_sample_size as i64),
            ],
        )?
        .into_iter()
        .map(|row| RawFinancialSample {
            request_id: row.get(0),
            account_id: row.get(1),
            evaluation_digest: row.get(2),
            provider_id: row.get(3),
            account_class: row.get(4),
            authorized_multiplier_bp: row.get(5),
            payable_multiplier_bp: row.get(6),
            official_hold_nano: row.get(7),
            legacy_hold_nano: row.get(8),
            policy_hold_nano: row.get(9),
            comparison_result: row.get(10),
        })
        .collect::<Vec<_>>();
    let mut sample_validation_failures = Vec::new();
    for sample in &raw_samples {
        match crate::pricing::postgres::postgres_shadow_evaluation_in_transaction(
            &mut transaction,
            &sample.request_id,
            false,
        ) {
            Ok(Some(evaluation))
                if evaluation.evaluation_digest().as_str() == sample.evaluation_digest => {}
            Ok(_) | Err(_) => sample_validation_failures
                .push(format!("{}\0{}", sample.request_id, sample.account_id)),
        }
    }
    push_subjects(
        &mut blockers,
        "financial_sample_canonical_validation_failed",
        sample_validation_failures,
    );
    let financial_samples = raw_samples
        .into_iter()
        .map(|sample| Stage8FinancialSample {
            subject_digest: subject_digest(&format!(
                "{}\0{}",
                sample.request_id, sample.account_id
            )),
            evaluation_digest: sample.evaluation_digest,
            provider_id: sample.provider_id,
            account_class: sample.account_class,
            authorized_multiplier_bp: sample.authorized_multiplier_bp,
            payable_multiplier_bp: sample.payable_multiplier_bp,
            official_hold_nano: sample.official_hold_nano,
            legacy_hold_nano: sample.legacy_hold_nano,
            policy_hold_nano: sample.policy_hold_nano,
            comparison_result: sample.comparison_result,
        })
        .collect();

    let shadow_evaluation_digests = transaction
        .query(
            "SELECT evaluation.evaluation_digest \
             FROM pricing_shadow_admission_evaluations evaluation \
             JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
             WHERE snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
               AND snapshot.provider_id IN('anthropic','openai','google') \
             ORDER BY evaluation.evaluation_digest COLLATE \"C\"",
            &[&request.window_start_ts, &request.window_end_ts],
        )?
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>();
    let shadow_digest = digest_parts(
        b"claude-api/stage8-v2/shadow-evaluations\0",
        shadow_evaluation_digests.iter(),
    );

    let minimum_runtime_schema_version = target
        .as_ref()
        .map(|release| release.minimum_runtime_schema_version)
        .unwrap_or(PRICING_RELEASE_SCHEMA_VERSION);
    let runtime_floor = runtime_floor_check_v2(
        &mut transaction,
        captured_ts,
        minimum_runtime_schema_version,
        &request.runtime_manifest,
    )?;
    if !runtime_floor.incompatible_instance_ids.is_empty() {
        push_subjects(
            &mut blockers,
            "live_runtime_below_release_v2_floor",
            runtime_floor.incompatible_instance_ids.clone(),
        );
    }
    if runtime_floor.live_instances == 0 {
        push_subjects(
            &mut blockers,
            "live_runtime_floor_unobserved",
            vec!["engine-instances".to_owned()],
        );
    }
    let runtime_floor_digest = runtime_floor.digest;

    let legacy_inflight_reservations = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM reservations reservation \
         LEFT JOIN pricing_request_snapshots_v2 snapshot \
           ON snapshot.request_id=reservation.request_id \
         WHERE reservation.state NOT IN('settled','canceled') AND snapshot.request_id IS NULL",
        &[],
    )?;
    let legacy_inflight_outbox_rows = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM settlement_outbox \
         WHERE state<>'done' AND release_schema_version IS NULL",
        &[],
    )?;
    let legacy_inflight_count = legacy_inflight_reservations
        .checked_add(legacy_inflight_outbox_rows)
        .context("Stage 8 legacy inflight count overflow")?;
    blockers.sort_by(|left, right| left.code.cmp(&right.code));
    let total_accounts = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM accounts",
        &[],
    )?;
    let active_accounts = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM accounts WHERE status='active'",
        &[],
    )?;
    let reconciled_accounts = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM accounts account \
         JOIN account_policy_bindings binding ON binding.account_id=account.id \
         JOIN account_policy_versions policy ON policy.account_id=binding.account_id \
          AND policy.effective_version=binding.active_effective_version \
         WHERE account.status='active' AND binding.account_class=policy.account_class \
          AND binding.product_id IS NOT DISTINCT FROM policy.product_id \
          AND binding.policy_enforcement='shadow'",
        &[],
    )?;
    let release = Stage8ReleasePairEvidence {
        target_generation: request.target_generation,
        target_digest: target
            .as_ref()
            .map(|release| release.content_digest.clone()),
        recovery_generation: request.recovery_generation,
        recovery_digest: recovery
            .as_ref()
            .map(|release| release.content_digest.clone()),
        recovery_link_digest: recovery_link.as_ref().map(|row| row.get::<_, String>(2)),
        inventory_digest: target
            .as_ref()
            .map(|release| release.inventory_digest.clone()),
        funding_digest: target
            .as_ref()
            .map(|release| release.funding_manifest_digest.clone()),
        target_assignment_count: target
            .as_ref()
            .map(|release| release.assignments.len() as i64)
            .unwrap_or(0),
        recovery_assignment_count: recovery
            .as_ref()
            .map(|release| release.assignments.len() as i64)
            .unwrap_or(0),
        active_head: active_release_head,
    };
    let mut report = Stage8EngineEvidenceReport {
        schema_version: STAGE8_ENGINE_EVIDENCE_SCHEMA_VERSION,
        captured_ts,
        window_start_ts: request.window_start_ts,
        window_end_ts: request.window_end_ts,
        min_samples_per_provider: request.min_samples_per_provider,
        gemini_client_admissions: request.gemini_client_admissions,
        passed: blockers.is_empty(),
        release,
        runtime_manifest,
        catalogs,
        switches,
        counts: Stage8EngineEvidenceCounts {
            total_accounts,
            active_accounts,
            account_classes,
            reconciled_accounts,
            snapshots_by_provider,
            evaluations_by_outcome,
            comparisons,
            scalar_parity_rows: parity.0,
            policy_divergence_rows: parity.1,
            gemini_usage_rows,
            gemini_outbox_rows,
            live_runtime_instances: runtime_floor.live_instances,
            release_capable_runtime_instances: runtime_floor.compatible_instances,
            legacy_inflight_reservations,
            legacy_inflight_outbox_rows,
        },
        financial_samples,
        engine_inventory_digest: current_inventory_digest,
        funding_digest: current_funding_digest,
        shadow_digest,
        runtime_floor_digest,
        legacy_inflight_count,
        blockers,
        evidence_digest: String::new(),
    };
    report.evidence_digest = report_digest(&report)?;
    transaction
        .commit()
        .context("commit Stage 8 engine evidence transaction")?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::{PricingRuntimeCapabilityEvidence, PRICING_SCHEMA_VERSION};

    fn manifest() -> PricingRuntimeManifestEvidence {
        PricingRuntimeManifestEvidence::new(
            1,
            vec![
                PricingRuntimeCapabilityEvidence::new(PRICING_SCHEMA_VERSION, 1, "capability")
                    .unwrap(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn request_bounds_are_fail_closed() {
        let valid = Stage8EngineEvidenceRequest {
            target_generation: 1,
            recovery_generation: 2,
            window_start_ts: 100,
            window_end_ts: 200,
            min_samples_per_provider: 1,
            financial_sample_size: 1,
            gemini_client_admissions: 0,
            runtime_manifest: manifest(),
        };
        assert!(valid.validate(200).is_ok());
        let mut invalid = valid.clone();
        invalid.window_end_ts = 99;
        assert!(invalid.validate(200).is_err());
        invalid = valid.clone();
        invalid.min_samples_per_provider = 0;
        assert!(invalid.validate(200).is_err());
        invalid = valid.clone();
        invalid.financial_sample_size = 1_001;
        assert!(invalid.validate(200).is_err());
        invalid = valid;
        invalid.gemini_client_admissions = -1;
        assert!(invalid.validate(200).is_err());
    }

    #[test]
    fn provider_coverage_requires_google_and_accepts_all_three_authorities() {
        let mut counts =
            BTreeMap::from([("anthropic".to_owned(), 100), ("openai".to_owned(), 100)]);
        assert_eq!(
            insufficient_provider_coverage(&counts, 100),
            vec!["google:0"]
        );
        counts.insert("google".to_owned(), 100);
        assert!(insufficient_provider_coverage(&counts, 100).is_empty());
    }

    #[test]
    fn release_inventory_and_funding_digests_match_the_typescript_canonical_contract() {
        let inventory = vec![
            EngineInventoryIdentity {
                account_id: "a",
                multiplier_bp: 5_000,
                status: "active",
            },
            EngineInventoryIdentity {
                account_id: "b",
                multiplier_bp: 10_000,
                status: "disabled",
            },
        ];
        assert_eq!(
            sha256_v2_json(b"pricing-stage5-v2:engine-identity-inventory\n", &inventory,).unwrap(),
            "sha256:v2:a8ed9afc4feeaf0e4f648ad55533ea87dd99852f796ee035713636b0e99258b0"
        );

        let funding = CanonicalScope {
            scope: "pricing-funding-normalization-manifest-v2",
            value: vec![FundingManifestIdentity {
                account_id: "a",
                funding_digest:
                    "sha256:v2:1111111111111111111111111111111111111111111111111111111111111111",
                funding_generation: "1".to_owned(),
            }],
        };
        assert_eq!(
            sha256_v2_json(b"", &funding).unwrap(),
            "sha256:v2:96886f1dc94223e2ef37fbbc1bad307ad1cf84ec22f21b586253072f48307fef"
        );
    }
}
