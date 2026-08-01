import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  MAIN_PRICING_PRODUCT_ID,
  OPENKEYS_PRICING_PRODUCT_ID,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";

const STAGE8_EVIDENCE_SCHEMA_VERSION = 1;
const REQUIRED_PRODUCTS = [MAIN_PRICING_PRODUCT_ID, OPENKEYS_PRICING_PRODUCT_ID] as const;
const BLOCKER_SAMPLE_LIMIT = 20;

interface IdentityRow {
  subject: string;
}

export interface Stage8CommerceBlocker {
  code: string;
  count: number;
  subject_digests: string[];
}

export interface Stage8CommerceEvidence {
  schema_version: 1;
  captured_at: string;
  passed: boolean;
  heads: {
    capability: { generation: string; content_digest: string } | null;
    catalogs: Array<{
      product_id: string;
      generation: string;
      schema_version: string;
      capability_generation: string;
      capability_digest: string;
      content_digest: string;
    }>;
    switches: {
      generation: string;
      schema_version: string;
      capability_generation: string;
      capability_digest: string;
      content_digest: string;
    } | null;
  };
  counts: {
    active_commerce_accounts: number;
    account_classes: Record<string, number>;
    active_service_bindings: number;
    active_invitations: number;
    catalog_jobs: Record<string, number>;
    switch_jobs: Record<string, number>;
    policy_jobs: Record<string, number>;
  };
  blockers: Stage8CommerceBlocker[];
  evidence_digest: string;
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, nested]) => [key, canonicalValue(nested)]),
    );
  }
  return value;
}

function evidenceDigest(value: unknown): string {
  const canonical = JSON.stringify(canonicalValue(value));
  return `sha256:v1:${createHash("sha256")
    .update("multi-discount-stage8:commerce-evidence\n", "utf8")
    .update(canonical, "utf8")
    .digest("hex")}`;
}

function subjectDigest(subject: string): string {
  return `sha256:v1:${createHash("sha256")
    .update("multi-discount-stage8:commerce-subject\n", "utf8")
    .update(subject, "utf8")
    .digest("hex")}`;
}

function sortedRecord(rows: Array<{ status: string; count: string }>): Record<string, number> {
  return Object.fromEntries(
    rows
      .sort((left, right) => compareUtf8(left.status, right.status))
      .map((row) => [row.status, Number(row.count)]),
  );
}

async function blocker(
  client: PoolClient,
  code: string,
  query: string,
  parameters: readonly unknown[] = [],
): Promise<Stage8CommerceBlocker | null> {
  const result = await client.query<IdentityRow>(query, [...parameters]);
  if (result.rowCount === 0) return null;
  const subjects = [...new Set(result.rows.map((row) => row.subject))].sort(compareUtf8);
  return {
    code,
    count: subjects.length,
    subject_digests: subjects.slice(0, BLOCKER_SAMPLE_LIMIT).map(subjectDigest),
  };
}

async function collectBlockers(client: PoolClient): Promise<Stage8CommerceBlocker[]> {
  const checks = [
    await blocker(client, "capability_head_missing_or_stale", `
      SELECT 'capability-head' AS subject
      WHERE NOT EXISTS (
        SELECT 1
        FROM provider_capability_head head
        WHERE head.singleton = 1
          AND head.active_generation = (SELECT max(generation) FROM provider_capability_versions)
      )
    `),
    await blocker(client, "catalog_head_missing_or_stale", `
      WITH required(product_id) AS (VALUES ($1::text), ($2::text)),
      latest AS (
        SELECT product_id, max(generation) AS generation
        FROM product_catalog_versions GROUP BY product_id
      )
      SELECT required.product_id AS subject
      FROM required
      LEFT JOIN latest USING (product_id)
      LEFT JOIN product_catalog_heads head
        ON head.product_id = required.product_id
       AND head.active_generation = latest.generation
      WHERE head.product_id IS NULL
      UNION ALL
      SELECT head.product_id AS subject
      FROM product_catalog_heads head
      WHERE head.product_id NOT IN ($1, $2)
    `, REQUIRED_PRODUCTS),
    await blocker(client, "switch_head_missing_or_stale", `
      SELECT 'switch-head' AS subject
      WHERE NOT EXISTS (
        SELECT 1 FROM provider_switch_head head
        WHERE head.singleton = 1
          AND head.active_generation = (SELECT max(generation) FROM provider_switch_versions)
      )
    `),
    await blocker(client, "active_product_graph_contains_gemini", `
      SELECT subject FROM (
        SELECT concat('catalog:', entry.product_id, ':', entry.canonical_model_id) AS subject
        FROM product_catalog_heads head
        JOIN product_catalog_entries entry
          ON entry.product_id = head.product_id AND entry.generation = head.active_generation
        WHERE entry.provider_id = 'gemini'
        UNION ALL
        SELECT concat('switch:', entry.scope_type, ':', entry.product_id, ':', entry.segment) AS subject
        FROM provider_switch_head head
        JOIN provider_switch_entries entry ON entry.generation = head.active_generation
        WHERE entry.provider_id = 'gemini'
        UNION ALL
        SELECT concat('policy:', rule.binding_id::text, ':', rule.rule_id) AS subject
        FROM account_policy_bindings binding
        JOIN account_policy_rules rule
          ON rule.binding_id = binding.id
         AND rule.effective_version = binding.desired_effective_version
        WHERE rule.provider_id = 'gemini'
      ) candidates
    `),
    await blocker(client, "active_product_graph_incomplete", `
      WITH required_catalog(product_id, provider_id) AS (
        VALUES
          ($1::text, 'anthropic'::text), ($1::text, 'openai'::text),
          ($2::text, 'anthropic'::text), ($2::text, 'openai'::text)
      ), missing_catalog AS (
        SELECT concat(product_id, ':', provider_id) AS subject
        FROM required_catalog required
        WHERE NOT EXISTS (
          SELECT 1
          FROM product_catalog_heads head
          JOIN product_catalog_entries entry
            ON entry.product_id = head.product_id
           AND entry.generation = head.active_generation
          WHERE entry.product_id = required.product_id
            AND entry.provider_id = required.provider_id
            AND entry.enabled
        )
      ), unexpected_catalog AS (
        SELECT concat(entry.product_id, ':', entry.provider_id) AS subject
        FROM product_catalog_heads head
        JOIN product_catalog_entries entry
          ON entry.product_id = head.product_id AND entry.generation = head.active_generation
        WHERE entry.provider_id NOT IN ('anthropic', 'openai')
      ), required_switch(provider_id, scope_type, product_id, segment) AS (
        VALUES
          ('anthropic'::text, 'master'::text, ''::text, ''::text),
          ('openai', 'master', '', ''),
          ('anthropic', 'product', $1, ''),
          ('openai', 'product', $1, ''),
          ('anthropic', 'product', $2, ''),
          ('openai', 'product', $2, ''),
          ('anthropic', 'segment', $1, 'b2c'),
          ('openai', 'segment', $1, 'b2c'),
          ('anthropic', 'segment', $1, 'b2b'),
          ('openai', 'segment', $1, 'b2b')
      ), missing_switch AS (
        SELECT concat(provider_id, ':', scope_type, ':', product_id, ':', segment) AS subject
        FROM required_switch required
        WHERE NOT EXISTS (
          SELECT 1
          FROM provider_switch_head head
          JOIN provider_switch_entries entry ON entry.generation = head.active_generation
          WHERE entry.provider_id = required.provider_id
            AND entry.scope_type = required.scope_type
            AND entry.product_id = required.product_id
            AND entry.segment = required.segment
            AND entry.enabled
        )
      )
      SELECT subject FROM missing_catalog
      UNION ALL SELECT subject FROM unexpected_catalog
      UNION ALL SELECT subject FROM missing_switch
    `, REQUIRED_PRODUCTS),
    await blocker(client, "active_commerce_account_unclassified", `
      SELECT account.id::text AS subject
      FROM engine_accounts account
      LEFT JOIN customer_profiles profile ON profile.user_id = account.user_id
      LEFT JOIN account_policy_bindings binding
        ON binding.engine_account_record_id = account.id
      WHERE account.status = 'active'
        AND (
          account.engine_account_id IS NULL
          OR profile.user_id IS NULL
          OR binding.id IS NULL
          OR binding.engine_account_id IS DISTINCT FROM account.engine_account_id
          OR binding.user_id IS DISTINCT FROM account.user_id
          OR binding.account_class IS DISTINCT FROM profile.customer_type::text
        )
    `),
    await blocker(client, "binding_not_fully_applied", `
      SELECT binding.id::text AS subject
      FROM account_policy_bindings binding
      WHERE binding.engine_account_id IS NOT NULL
        AND (
          binding.desired_effective_version IS NULL
          OR binding.desired_digest IS NULL
          OR binding.applied_effective_version IS DISTINCT FROM binding.desired_effective_version
          OR binding.applied_digest IS DISTINCT FROM binding.desired_digest
          OR binding.sync_state <> 'confirmed'
          OR binding.last_ack_at IS NULL
        )
    `),
    await blocker(client, "binding_targets_stale_policy_generation", `
      SELECT binding.id::text AS subject
      FROM account_policy_bindings binding
      LEFT JOIN LATERAL (
        SELECT max(candidate.effective_version) AS latest_version
        FROM account_policy_versions candidate
        WHERE candidate.binding_id = binding.id
      ) latest ON true
      WHERE binding.engine_account_id IS NOT NULL
        AND binding.desired_effective_version IS DISTINCT FROM latest.latest_version
    `),
    await blocker(client, "source_policy_head_stale", `
      SELECT policy.id AS subject
      FROM pricing_policies policy
      LEFT JOIN pricing_policy_heads head ON head.policy_id = policy.id
      LEFT JOIN LATERAL (
        SELECT max(candidate.version) AS latest_version
        FROM pricing_policy_versions candidate
        WHERE candidate.policy_id = policy.id
      ) latest ON true
      WHERE policy.status = 'active'
        AND (
          latest.latest_version IS NULL
          OR head.current_version IS DISTINCT FROM latest.latest_version
        )
    `),
    await blocker(client, "active_invitation_missing_policy", `
      SELECT invitation.id::text AS subject
      FROM business_invites invitation
      LEFT JOIN business_invite_policy_bindings binding ON binding.invite_id = invitation.id
      LEFT JOIN pricing_policy_heads head ON head.policy_id = binding.invitation_policy_id
      WHERE invitation.consumed_at IS NULL
        AND invitation.revoked_at IS NULL
        AND invitation.superseded_by_invite_id IS NULL
        AND invitation.expires_at > transaction_timestamp()
        AND (
          binding.invite_id IS NULL
          OR head.current_version IS DISTINCT FROM binding.current_policy_version
          OR head.current_digest IS DISTINCT FROM binding.current_policy_digest
        )
    `),
    await blocker(client, "pricing_control_job_backlog_or_failure", `
      SELECT concat(kind, ':', id::text) AS subject
      FROM (
        SELECT 'catalog' AS kind, id, status FROM engine_catalog_jobs
        UNION ALL SELECT 'switch', id, status FROM engine_switch_jobs
        UNION ALL SELECT 'policy', id, status FROM engine_policy_jobs
      ) job
      WHERE status IN ('pending', 'processing', 'retry', 'dead')
    `),
    await blocker(client, "active_catalog_ack_missing", `
      SELECT head.product_id AS subject
      FROM product_catalog_heads head
      JOIN product_catalog_versions version
        ON version.product_id = head.product_id AND version.generation = head.active_generation
      LEFT JOIN engine_catalog_jobs job
        ON job.product_id = version.product_id
       AND job.generation = version.generation
       AND job.schema_version = version.schema_version
       AND job.content_digest = version.content_digest
       AND job.status = 'confirmed'
       AND job.ack_generation = version.generation
       AND job.ack_schema_version = version.schema_version
       AND job.ack_content_digest = version.content_digest
      WHERE job.id IS NULL
    `),
    await blocker(client, "active_switch_ack_missing", `
      SELECT 'switch-head' AS subject
      FROM provider_switch_head head
      JOIN provider_switch_versions version ON version.generation = head.active_generation
      LEFT JOIN engine_switch_jobs job
        ON job.generation = version.generation
       AND job.schema_version = version.schema_version
       AND job.content_digest = version.content_digest
       AND job.status = 'confirmed'
       AND job.ack_generation = version.generation
       AND job.ack_schema_version = version.schema_version
       AND job.ack_content_digest = version.content_digest
      WHERE job.id IS NULL
    `),
    await blocker(client, "active_policy_ack_missing", `
      SELECT binding.id::text AS subject
      FROM account_policy_bindings binding
      LEFT JOIN engine_policy_jobs job
        ON job.binding_id = binding.id
       AND job.effective_version = binding.desired_effective_version
       AND job.content_digest = binding.desired_digest
       AND job.status = 'confirmed'
       AND job.ack_effective_version = binding.desired_effective_version
       AND job.ack_content_digest = binding.desired_digest
      WHERE binding.engine_account_id IS NOT NULL AND job.id IS NULL
    `),
  ];
  return checks
    .filter((candidate): candidate is Stage8CommerceBlocker => candidate !== null)
    .sort((left, right) => compareUtf8(left.code, right.code));
}

export async function collectStage8CommerceEvidence(
  database: Database,
): Promise<Stage8CommerceEvidence> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    await client.query("SET LOCAL statement_timeout = '30s'");
    await client.query("SET LOCAL lock_timeout = '5s'");

    const captured = await client.query<{ captured_at: string }>(
      "SELECT transaction_timestamp()::text AS captured_at",
    );
    const capability = await client.query<{ generation: string; content_digest: string }>(`
      SELECT version.generation::text, version.content_digest
      FROM provider_capability_head head
      JOIN provider_capability_versions version ON version.generation = head.active_generation
      WHERE head.singleton = 1
    `);
    const catalogs = await client.query<Stage8CommerceEvidence["heads"]["catalogs"][number]>(`
      SELECT version.product_id, version.generation::text, version.schema_version::text,
             version.capability_generation::text, version.capability_digest, version.content_digest
      FROM product_catalog_heads head
      JOIN product_catalog_versions version
        ON version.product_id = head.product_id AND version.generation = head.active_generation
      ORDER BY version.product_id COLLATE "C"
    `);
    const switches = await client.query<NonNullable<Stage8CommerceEvidence["heads"]["switches"]>>(`
      SELECT version.generation::text, version.schema_version::text,
             version.capability_generation::text, version.capability_digest, version.content_digest
      FROM provider_switch_head head
      JOIN provider_switch_versions version ON version.generation = head.active_generation
      WHERE head.singleton = 1
    `);
    const accountCounts = await client.query<{ account_class: string; count: string }>(`
      SELECT coalesce(profile.customer_type::text, 'unclassified') AS account_class, count(*)::text
      FROM engine_accounts account
      LEFT JOIN customer_profiles profile ON profile.user_id = account.user_id
      WHERE account.status = 'active'
      GROUP BY profile.customer_type::text
      ORDER BY profile.customer_type::text COLLATE "C"
    `);
    const scalarCounts = await client.query<{
      active_commerce_accounts: string;
      active_service_bindings: string;
      active_invitations: string;
    }>(`
      SELECT
        (SELECT count(*)::text FROM engine_accounts WHERE status = 'active') AS active_commerce_accounts,
        (SELECT count(*)::text FROM account_policy_bindings
          WHERE account_class = 'service' AND engine_account_id IS NOT NULL) AS active_service_bindings,
        (SELECT count(*)::text FROM business_invites
          WHERE consumed_at IS NULL AND revoked_at IS NULL
            AND superseded_by_invite_id IS NULL
            AND expires_at > transaction_timestamp()) AS active_invitations
    `);
    const jobCounts = async (table: string): Promise<Record<string, number>> => {
      if (!/^engine_(catalog|switch|policy)_jobs$/.test(table)) throw new Error("unsafe job table");
      const rows = await client.query<{ status: string; count: string }>(
        `SELECT status, count(*)::text FROM ${table} GROUP BY status`,
      );
      return sortedRecord(rows.rows);
    };
    const catalogJobs = await jobCounts("engine_catalog_jobs");
    const switchJobs = await jobCounts("engine_switch_jobs");
    const policyJobs = await jobCounts("engine_policy_jobs");
    const blockers = await collectBlockers(client);

    const scalar = scalarCounts.rows[0]!;
    const base = {
      schema_version: STAGE8_EVIDENCE_SCHEMA_VERSION as 1,
      captured_at: captured.rows[0]!.captured_at,
      passed: blockers.length === 0,
      heads: {
        capability: capability.rows[0] ?? null,
        catalogs: catalogs.rows,
        switches: switches.rows[0] ?? null,
      },
      counts: {
        active_commerce_accounts: Number(scalar.active_commerce_accounts),
        account_classes: Object.fromEntries(
          accountCounts.rows.map((row) => [row.account_class, Number(row.count)]),
        ),
        active_service_bindings: Number(scalar.active_service_bindings),
        active_invitations: Number(scalar.active_invitations),
        catalog_jobs: catalogJobs,
        switch_jobs: switchJobs,
        policy_jobs: policyJobs,
      },
      blockers,
    };
    const report: Stage8CommerceEvidence = {
      ...base,
      evidence_digest: evidenceDigest(base),
    };
    await client.query("COMMIT");
    return report;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
