//! Read-only Stage 8 synchronization and pricing-shadow evidence.
//!
//! The report is deliberately PostgreSQL-only: production authority, immutable admission
//! snapshots and shadow evaluations must be observed in one `REPEATABLE READ READ ONLY` snapshot.
//! It never changes a head, binding, funding bucket, reservation or charge.

use crate::pricing::PricingRuntimeManifestEvidence;
use anyhow::{bail, Context, Result};
use postgres::types::ToSql;
use postgres::{Client, GenericClient, IsolationLevel};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const STAGE8_ENGINE_EVIDENCE_SCHEMA_VERSION: i64 = 1;
const BLOCKER_SUBJECT_LIMIT: usize = 20;

#[derive(Clone, Debug)]
pub struct Stage8EngineEvidenceRequest {
    pub window_start_ts: i64,
    pub window_end_ts: i64,
    pub min_samples_per_provider: i64,
    pub financial_sample_size: usize,
    pub gemini_client_admissions: i64,
    pub runtime_manifest: PricingRuntimeManifestEvidence,
}

impl Stage8EngineEvidenceRequest {
    fn validate(&self, captured_ts: i64) -> Result<()> {
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
    pub runtime_manifest: Stage8RuntimeManifestEvidence,
    pub catalogs: Vec<Stage8CatalogHeadEvidence>,
    pub switches: Option<Stage8SwitchHeadEvidence>,
    pub counts: Stage8EngineEvidenceCounts,
    pub financial_samples: Vec<Stage8FinancialSample>,
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
    Ok(digest(
        b"claude-api/multi-discount-stage8/engine-evidence/v1\0",
        &encoded,
    ))
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
           WHERE entry.provider_id NOT IN('anthropic','openai')), \
         required_switch(provider_id,scope_type,product_id,segment) AS (VALUES \
           ('anthropic'::text,'master'::text,''::text,''::text),('openai','master','',''), \
           ('anthropic','product','main',''),('openai','product','main',''), \
           ('anthropic','product','openkeys',''),('openai','product','openkeys',''), \
           ('anthropic','segment','main','b2c'),('openai','segment','main','b2c'), \
           ('anthropic','segment','main','b2b'),('openai','segment','main','b2b')), \
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
        "active_product_graph_contains_gemini",
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
        "active_account_unclassified_or_unreconciled",
        "SELECT account.id FROM accounts account \
         LEFT JOIN account_policy_bindings binding ON binding.account_id=account.id \
         LEFT JOIN account_policy_versions policy ON policy.account_id=binding.account_id \
          AND policy.effective_version=binding.active_effective_version \
         WHERE account.status='active' AND( \
           binding.account_id IS NULL OR binding.active_effective_version IS NULL \
           OR policy.account_id IS NULL OR binding.account_class<>policy.account_class \
           OR binding.product_id<>policy.product_id OR binding.policy_enforcement<>'shadow' \
           OR binding.reconciliation_state<>'verified')",
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
    query_blocker(
        &mut transaction,
        &mut blockers,
        "funding_bucket_reconciliation_mismatch",
        "SELECT account.id FROM accounts account \
         LEFT JOIN LATERAL(SELECT COUNT(*)::bigint bucket_count, \
             COALESCE(SUM(balance_nano::numeric),0) balance_nano, \
             COALESCE(SUM(reserved_nano::numeric),0) reserved_nano \
           FROM funding_buckets bucket WHERE bucket.account_id=account.id) funding ON true \
         WHERE account.status='active' AND( funding.bucket_count=0 \
           OR funding.balance_nano<>account.balance_nano::numeric \
           OR funding.reserved_nano<>account.reserved_nano::numeric)",
        &[],
    )?;
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

    let providers = ["anthropic", "openai"];
    let mut insufficient_providers = Vec::new();
    for provider in providers {
        let count = query_count(
            &mut transaction,
            "SELECT COUNT(*)::bigint FROM pricing_admission_snapshots \
             WHERE snapshot_kind='legacy_scalar' AND provider_id=$1 \
              AND admission_ts >= $2 AND admission_ts < $3",
            &[&provider, &request.window_start_ts, &request.window_end_ts],
        )?;
        if count < request.min_samples_per_provider {
            insufficient_providers.push(format!("{provider}:{count}"));
        }
    }
    push_subjects(
        &mut blockers,
        "insufficient_shadow_provider_coverage",
        insufficient_providers,
    );
    query_blocker(
        &mut transaction,
        &mut blockers,
        "shadow_evaluation_missing_or_late",
        "SELECT snapshot.request_id FROM pricing_admission_snapshots snapshot \
         LEFT JOIN pricing_shadow_admission_evaluations evaluation \
          ON evaluation.request_id=snapshot.request_id \
         WHERE snapshot.snapshot_kind='legacy_scalar' \
          AND snapshot.provider_id IN('anthropic','openai') \
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
          AND snapshot.provider_id IN('anthropic','openai') \
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
          AND snapshot.provider_id IN('anthropic','openai') \
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
        "shadow_nano_usd_mismatch",
        "WITH candidate AS( \
           SELECT evaluation.*, \
             div(evaluation.official_hold_nano::numeric*evaluation.authorized_multiplier_bp+5000,10000) scalar_uncapped, \
             div(evaluation.official_hold_nano::numeric*evaluation.payable_multiplier_bp+5000,10000) policy_uncapped \
           FROM pricing_shadow_admission_evaluations evaluation \
           JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
           WHERE snapshot.snapshot_kind='legacy_scalar' \
            AND snapshot.provider_id IN('anthropic','openai') \
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
    if request.gemini_client_admissions != 0 || gemini_usage_rows != 0 || gemini_outbox_rows != 0 {
        push_subjects(
            &mut blockers,
            "gemini_client_admissions_nonzero",
            vec![format!(
                "observed:{}:usage:{}:outbox:{}",
                request.gemini_client_admissions, gemini_usage_rows, gemini_outbox_rows
            )],
        );
    }

    let account_classes = grouped_counts(
        &mut transaction,
        "SELECT binding.account_class,COUNT(*)::bigint FROM accounts account \
         JOIN account_policy_bindings binding ON binding.account_id=account.id \
         WHERE account.status='active' GROUP BY binding.account_class ORDER BY binding.account_class",
        &[],
    )?;
    let snapshots_by_provider = grouped_counts(
        &mut transaction,
        "SELECT provider_id,COUNT(*)::bigint FROM pricing_admission_snapshots \
         WHERE snapshot_kind='legacy_scalar' AND provider_id IN('anthropic','openai') \
          AND admission_ts >= $1 AND admission_ts < $2 GROUP BY provider_id ORDER BY provider_id",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    let evaluations_by_outcome = grouped_counts(
        &mut transaction,
        "SELECT evaluation.outcome,COUNT(*)::bigint FROM pricing_shadow_admission_evaluations evaluation \
         JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
         WHERE snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
          AND snapshot.provider_id IN('anthropic','openai') GROUP BY evaluation.outcome ORDER BY evaluation.outcome",
        &[&request.window_start_ts, &request.window_end_ts],
    )?;
    let comparisons = grouped_counts(
        &mut transaction,
        "SELECT evaluation.comparison_result,COUNT(*)::bigint \
         FROM pricing_shadow_admission_evaluations evaluation \
         JOIN pricing_admission_snapshots snapshot ON snapshot.request_id=evaluation.request_id \
         WHERE snapshot.admission_ts >= $1 AND snapshot.admission_ts < $2 \
          AND snapshot.provider_id IN('anthropic','openai') \
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
              AND snapshot.provider_id IN('anthropic','openai') AND evaluation.outcome='resolved'",
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
              AND snapshot.provider_id IN('anthropic','openai') AND evaluation.outcome='resolved' \
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

    blockers.sort_by(|left, right| left.code.cmp(&right.code));
    let active_accounts = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM accounts WHERE status='active'",
        &[],
    )?;
    let reconciled_accounts = query_count(
        &mut transaction,
        "SELECT COUNT(*)::bigint FROM accounts account JOIN account_policy_bindings binding \
         ON binding.account_id=account.id WHERE account.status='active' \
          AND binding.reconciliation_state='verified'",
        &[],
    )?;
    let mut report = Stage8EngineEvidenceReport {
        schema_version: STAGE8_ENGINE_EVIDENCE_SCHEMA_VERSION,
        captured_ts,
        window_start_ts: request.window_start_ts,
        window_end_ts: request.window_end_ts,
        min_samples_per_provider: request.min_samples_per_provider,
        gemini_client_admissions: request.gemini_client_admissions,
        passed: blockers.is_empty(),
        runtime_manifest,
        catalogs,
        switches,
        counts: Stage8EngineEvidenceCounts {
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
        },
        financial_samples,
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
}
