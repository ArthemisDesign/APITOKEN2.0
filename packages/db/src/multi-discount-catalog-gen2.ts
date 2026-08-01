import { Buffer } from "node:buffer";
import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import type { PricingCatalogSpec, ProviderSwitchSpec } from "@claude-api/contracts";
import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MAIN_PRICING_PRODUCT_ID,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  pricingCatalogSpecSchema,
  providerSwitchSpecSchema,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import { stage5Digest } from "./multi-discount-backfill.js";

/**
 * Catalog generation 2: the frozen generation-1 Anthropic/OpenAI product
 * catalogs plus `claude-opus-5` and `claude-fable-5`. Everything here is
 * additive and deterministic from the reviewed constants in
 * `@claude-api/contracts`; generation 1 keeps its Stage 5 planner untouched.
 *
 * The plan materializes three durable artifacts in one serializable
 * transaction, mirroring the Stage 5 ensure* semantics:
 *
 * - provider capability generation 2 (12 entries, the same single
 *   `gpt-5.6 -> gpt-5.6-sol` alias), whose content digest is the pinned
 *   `MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST`;
 * - `main` and `openkeys` product catalog generation 2 with all 12 models
 *   enabled, plus their `engine_catalog_jobs` rows;
 * - provider-switch generation 2, identical to generation 1 except every
 *   scoped entry re-pins `catalog_generation` to 2, plus its
 *   `engine_switch_jobs` row.
 *
 * The pricing worker then delivers catalog jobs before the switch job (its
 * claim query already requires confirmed catalog jobs), so the engine
 * authority walks the supported catalog → switches order. No account policy
 * is rebuilt: existing bindings keep pinning generation 1 and resolve through
 * the dual-lineage choreography, while the two new models stay gated by the
 * policy catalog until a later policy generation.
 */

export const GEN2_SCHEMA_VERSION = MULTI_DISCOUNT_SCHEMA_VERSION;
export const GEN2_CAPABILITY_GENERATION = MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION;
export const GEN2_CAPABILITY_DIGEST = MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST;
export const GEN2_CATALOG_GENERATION = 2;
export const GEN2_SWITCH_GENERATION = 2;

const GEN2_ACTOR_TYPE = "migration";
const GEN2_ACTOR_ID = "multi-discount-catalog-gen2";

export type CatalogGen2Mode = "dry_run" | "apply";

export class CatalogGen2Error extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "CatalogGen2Error";
  }
}

export interface Gen2CapabilityProjection {
  generation: typeof GEN2_CAPABILITY_GENERATION;
  schema_version: typeof GEN2_SCHEMA_VERSION;
  content_digest: string;
  entries: Array<{
    provider_id: string;
    canonical_model_id: string;
    entry_digest: string;
    capability_data: Record<string, unknown>;
  }>;
  aliases: Array<{
    provider_id: string;
    alias_model_id: string;
    canonical_model_id: string;
  }>;
}

export interface CatalogGen2Plan {
  schema_version: typeof GEN2_SCHEMA_VERSION;
  capability: Gen2CapabilityProjection;
  catalogs: PricingCatalogSpec[];
  switches: ProviderSwitchSpec;
  plan_digest: string;
}

export interface CatalogGen2Foundation {
  capability: { generation: number; content_digest: string } | null;
  catalogs: Array<{ product_id: string; generation: number; content_digest: string }>;
  switches: { generation: number; content_digest: string } | null;
  matches_reviewed_generation_1: boolean;
  already_materialized: boolean;
}

export interface CatalogGen2Result {
  mode: CatalogGen2Mode;
  plan: CatalogGen2Plan;
  foundation: CatalogGen2Foundation;
  writes_committed: boolean;
}

function capabilityEntry(
  entry: { provider_id: string; canonical_model_id: string; enabled: boolean },
): Gen2CapabilityProjection["entries"][number] {
  const capabilityData = { pricing_supported: true };
  return {
    provider_id: entry.provider_id,
    canonical_model_id: entry.canonical_model_id,
    entry_digest: stage5Digest("capability-entry", { ...entry, capability_data: capabilityData }),
    capability_data: capabilityData,
  };
}

/**
 * The generation-2 capability digest is reproducible: the Stage 5 digest
 * domain over the canonical JSON of this exact projection. The computed value
 * must always equal the reviewed constant; a drift here means either the
 * builder or the constant was edited without re-review.
 */
export function buildGen2CapabilityProjection(): Gen2CapabilityProjection {
  const base: Omit<Gen2CapabilityProjection, "content_digest"> = {
    generation: GEN2_CAPABILITY_GENERATION,
    schema_version: GEN2_SCHEMA_VERSION,
    entries: MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES.map(capabilityEntry),
    aliases: [{
      provider_id: "openai",
      alias_model_id: "gpt-5.6",
      canonical_model_id: "gpt-5.6-sol",
    }],
  };
  const contentDigest = stage5Digest("capability", base);
  if (contentDigest !== GEN2_CAPABILITY_DIGEST) {
    throw new CatalogGen2Error(
      "capability_digest_drift",
      "computed generation-2 capability digest differs from the reviewed constant",
    );
  }
  return { ...base, content_digest: contentDigest };
}

export function buildGen2Catalog(productId: string): PricingCatalogSpec {
  const base = {
    product_id: productId,
    generation: GEN2_CATALOG_GENERATION,
    schema_version: GEN2_SCHEMA_VERSION,
    capability_generation: GEN2_CAPABILITY_GENERATION,
    capability_digest: GEN2_CAPABILITY_DIGEST,
    entries: MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
  };
  return pricingCatalogSpecSchema.parse({ ...base, content_digest: stage5Digest("catalog", base) });
}

function switchScopeParts(scope: ProviderSwitchSpec["entries"][number]["scope"]): {
  scopeType: "master" | "product" | "segment";
  productId: string;
  segment: string;
} {
  if (scope === "master") return { scopeType: "master", productId: "", segment: "" };
  if ("product" in scope) {
    return { scopeType: "product", productId: scope.product.product_id, segment: "" };
  }
  return {
    scopeType: "segment",
    productId: scope.segment.product_id,
    segment: scope.segment.segment,
  };
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function sortedSwitchEntries(
  entries: ProviderSwitchSpec["entries"],
): ProviderSwitchSpec["entries"] {
  return [...entries].sort((left, right) => {
    const leftScope = switchScopeParts(left.scope);
    const rightScope = switchScopeParts(right.scope);
    return compareUtf8(left.provider_id, right.provider_id) ||
      compareUtf8(leftScope.scopeType, rightScope.scopeType) ||
      compareUtf8(leftScope.productId, rightScope.productId) ||
      compareUtf8(leftScope.segment, rightScope.segment);
  });
}

function buildSwitchEntries(catalogGeneration: number): ProviderSwitchSpec["entries"] {
  const entries: ProviderSwitchSpec["entries"] = [];
  for (const providerId of ["anthropic", "openai"] as const) {
    entries.push(
      { provider_id: providerId, scope: "master", catalog_generation: null, enabled: true },
      {
        provider_id: providerId,
        scope: { product: { product_id: MAIN_PRICING_PRODUCT_ID } },
        catalog_generation: catalogGeneration,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { product: { product_id: OPENKEYS_PRICING_PRODUCT_ID } },
        catalog_generation: catalogGeneration,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: MAIN_PRICING_PRODUCT_ID, segment: "b2c" } },
        catalog_generation: catalogGeneration,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: MAIN_PRICING_PRODUCT_ID, segment: "b2b" } },
        catalog_generation: catalogGeneration,
        enabled: true,
      },
    );
  }
  return sortedSwitchEntries(entries);
}

/**
 * Switch generation 2 flips every scoped pin to catalog generation 2 while
 * keeping the master switches unpinned and every provider enabled, exactly
 * like generation 1.
 */
export function buildGen2Switches(): ProviderSwitchSpec {
  const base = {
    generation: GEN2_SWITCH_GENERATION,
    schema_version: GEN2_SCHEMA_VERSION,
    capability_generation: GEN2_CAPABILITY_GENERATION,
    capability_digest: GEN2_CAPABILITY_DIGEST,
    entries: buildSwitchEntries(GEN2_CATALOG_GENERATION),
  };
  return providerSwitchSpecSchema.parse({ ...base, content_digest: stage5Digest("switches", base) });
}

/** Reviewed generation-1 identities, rebuilt from the frozen constants to
 * verify that the durable foundation is exactly what generation 2 extends. */
function buildGen1Catalog(productId: string): PricingCatalogSpec {
  const base = {
    product_id: productId,
    generation: 1,
    schema_version: GEN2_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    entries: CURRENT_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry })),
  };
  return pricingCatalogSpecSchema.parse({ ...base, content_digest: stage5Digest("catalog", base) });
}

function buildGen1Switches(): ProviderSwitchSpec {
  const base = {
    generation: 1,
    schema_version: GEN2_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    entries: buildSwitchEntries(1),
  };
  return providerSwitchSpecSchema.parse({ ...base, content_digest: stage5Digest("switches", base) });
}

export function buildCatalogGen2Plan(): CatalogGen2Plan {
  const capability = buildGen2CapabilityProjection();
  const catalogs = [
    buildGen2Catalog(MAIN_PRICING_PRODUCT_ID),
    buildGen2Catalog(OPENKEYS_PRICING_PRODUCT_ID),
  ];
  const switches = buildGen2Switches();
  const base: Omit<CatalogGen2Plan, "plan_digest"> = {
    schema_version: GEN2_SCHEMA_VERSION,
    capability,
    catalogs,
    switches,
  };
  return { ...base, plan_digest: stage5Digest("catalog-gen2-plan", base) };
}

function versionNumber(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new CatalogGen2Error("malformed_stored_version", `${label} is not a positive safe integer`);
  }
  return parsed;
}

function sameJson(left: unknown, right: unknown): boolean {
  // Job payloads round-trip through jsonb, which normalizes key order; the
  // comparison must be structural, not textual.
  return isDeepStrictEqual(left, right);
}

function assertStored(kind: string, expected: unknown, actual: unknown): void {
  if (!sameJson(expected, actual)) {
    throw new CatalogGen2Error(
      "immutable_version_conflict",
      `${kind} already exists with different immutable content`,
    );
  }
}

async function readFoundation(client: PoolClient): Promise<CatalogGen2Foundation> {
  const capability = await client.query<{ generation: string; content_digest: string }>(`
    SELECT version.generation::text, version.content_digest
    FROM provider_capability_head head
    JOIN provider_capability_versions version ON version.generation = head.active_generation
    WHERE head.singleton = 1
  `);
  const catalogs = await client.query<{
    product_id: string;
    generation: string;
    content_digest: string;
  }>(`
    SELECT version.product_id, version.generation::text, version.content_digest
    FROM product_catalog_heads head
    JOIN product_catalog_versions version
      ON version.product_id = head.product_id AND version.generation = head.active_generation
    ORDER BY version.product_id
  `);
  const switches = await client.query<{ generation: string; content_digest: string }>(`
    SELECT version.generation::text, version.content_digest
    FROM provider_switch_head head
    JOIN provider_switch_versions version ON version.generation = head.active_generation
    WHERE head.singleton = 1
  `);

  const capabilityRow = capability.rows[0] ?? null;
  const switchRow = switches.rows[0] ?? null;
  const plan = buildCatalogGen2Plan();
  const gen1Main = buildGen1Catalog(MAIN_PRICING_PRODUCT_ID);
  const gen1Openkeys = buildGen1Catalog(OPENKEYS_PRICING_PRODUCT_ID);
  const gen1Switches = buildGen1Switches();

  const activeCatalog = (productId: string) =>
    catalogs.rows.find((row) => row.product_id === productId) ?? null;
  const mainRow = activeCatalog(MAIN_PRICING_PRODUCT_ID);
  const openkeysRow = activeCatalog(OPENKEYS_PRICING_PRODUCT_ID);

  const matchesGen1 = capabilityRow !== null &&
    versionNumber(capabilityRow.generation, "capability head") === 1 &&
    capabilityRow.content_digest === MULTI_DISCOUNT_CAPABILITY_DIGEST &&
    mainRow !== null && openkeysRow !== null &&
    versionNumber(mainRow.generation, "main catalog head") === 1 &&
    mainRow.content_digest === gen1Main.content_digest &&
    versionNumber(openkeysRow.generation, "openkeys catalog head") === 1 &&
    openkeysRow.content_digest === gen1Openkeys.content_digest &&
    switchRow !== null &&
    versionNumber(switchRow.generation, "switch head") === 1 &&
    switchRow.content_digest === gen1Switches.content_digest;

  const alreadyMaterialized = capabilityRow !== null &&
    versionNumber(capabilityRow.generation, "capability head") === GEN2_CAPABILITY_GENERATION &&
    capabilityRow.content_digest === GEN2_CAPABILITY_DIGEST &&
    mainRow !== null && openkeysRow !== null &&
    versionNumber(mainRow.generation, "main catalog head") === GEN2_CATALOG_GENERATION &&
    mainRow.content_digest === plan.catalogs[0]!.content_digest &&
    versionNumber(openkeysRow.generation, "openkeys catalog head") === GEN2_CATALOG_GENERATION &&
    openkeysRow.content_digest === plan.catalogs[1]!.content_digest &&
    switchRow !== null &&
    versionNumber(switchRow.generation, "switch head") === GEN2_SWITCH_GENERATION &&
    switchRow.content_digest === plan.switches.content_digest;

  return {
    capability: capabilityRow === null ? null : {
      generation: versionNumber(capabilityRow.generation, "capability head"),
      content_digest: capabilityRow.content_digest,
    },
    catalogs: catalogs.rows.map((row) => ({
      product_id: row.product_id,
      generation: versionNumber(row.generation, "catalog head"),
      content_digest: row.content_digest,
    })),
    switches: switchRow === null ? null : {
      generation: versionNumber(switchRow.generation, "switch head"),
      content_digest: switchRow.content_digest,
    },
    matches_reviewed_generation_1: matchesGen1,
    already_materialized: alreadyMaterialized,
  };
}

/** The gen-1 entries stored in the database must be the reviewed set exactly;
 * a hand-edited generation-1 row cannot hide behind an untouched digest. */
async function assertGen1Entries(client: PoolClient): Promise<void> {
  const expectedCatalog = CURRENT_PRODUCT_CATALOG_ENTRIES.map((entry) => ({ ...entry }));
  for (const productId of [MAIN_PRICING_PRODUCT_ID, OPENKEYS_PRICING_PRODUCT_ID]) {
    const entries = await client.query<{
      provider_id: string;
      canonical_model_id: string;
      enabled: boolean;
    }>(`
      SELECT provider_id, canonical_model_id, enabled
      FROM product_catalog_entries
      WHERE product_id = $1 AND generation = 1
      ORDER BY provider_id COLLATE "C", canonical_model_id COLLATE "C"
    `, [productId]);
    const expected = [...expectedCatalog].sort((left, right) =>
      compareUtf8(left.provider_id, right.provider_id) ||
      compareUtf8(left.canonical_model_id, right.canonical_model_id));
    assertStored(`generation-1 catalog entries ${productId}`, expected, entries.rows);
  }

  const switchEntries = await client.query<{
    provider_id: string;
    scope_type: "master" | "product" | "segment";
    product_id: string;
    segment: "" | "b2c" | "b2b";
    catalog_generation: string | null;
    enabled: boolean;
  }>(`
    SELECT provider_id, scope_type, product_id, segment,
           catalog_generation::text, enabled
    FROM provider_switch_entries WHERE generation = 1
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C", product_id COLLATE "C", segment COLLATE "C"
  `);
  const expectedSwitches = buildGen1Switches().entries.map((entry) => {
    const scope = switchScopeParts(entry.scope);
    return {
      provider_id: entry.provider_id,
      scope_type: scope.scopeType,
      product_id: scope.productId,
      segment: scope.segment,
      catalog_generation: entry.catalog_generation === null ? null : String(entry.catalog_generation),
      enabled: entry.enabled,
    };
  });
  assertStored("generation-1 switch entries", expectedSwitches, switchEntries.rows.map((row) => ({
    provider_id: row.provider_id,
    scope_type: row.scope_type,
    product_id: row.scope_type === "master" ? "" : row.product_id,
    segment: row.scope_type === "master" ? "" : row.segment,
    catalog_generation: row.catalog_generation,
    enabled: row.enabled,
  })));
}

async function ensureGen2Capability(
  client: PoolClient,
  capability: Gen2CapabilityProjection,
): Promise<void> {
  await client.query(`
    INSERT INTO provider_capability_versions (
      generation, schema_version, content_digest, source_runtime, source_revision, observed_at
    ) VALUES ($1, $2, $3, 'claude-api', $3, now())
    ON CONFLICT (generation) DO NOTHING
  `, [capability.generation, capability.schema_version, capability.content_digest]);
  const header = await client.query<{
    generation: string;
    schema_version: string;
    content_digest: string;
    source_runtime: string | null;
    source_revision: string | null;
  }>(`
    SELECT generation::text, schema_version::text, content_digest, source_runtime, source_revision
    FROM provider_capability_versions WHERE generation = $1
  `, [capability.generation]);
  const row = header.rows[0];
  if (!row) throw new CatalogGen2Error("capability_insert_lost", "capability projection was not stored");
  assertStored("capability projection", {
    generation: capability.generation,
    schema_version: capability.schema_version,
    content_digest: capability.content_digest,
    source_runtime: "claude-api",
    source_revision: capability.content_digest,
  }, {
    generation: versionNumber(row.generation, "capability generation"),
    schema_version: versionNumber(row.schema_version, "capability schema version"),
    content_digest: row.content_digest,
    source_runtime: row.source_runtime,
    source_revision: row.source_revision,
  });

  for (const entry of capability.entries) {
    await client.query(`
      INSERT INTO provider_capability_entries (
        generation, provider_id, canonical_model_id, entry_digest, capability_data
      ) VALUES ($1, $2, $3, $4, $5::jsonb)
      ON CONFLICT (generation, provider_id, canonical_model_id) DO NOTHING
    `, [
      capability.generation,
      entry.provider_id,
      entry.canonical_model_id,
      entry.entry_digest,
      JSON.stringify(entry.capability_data),
    ]);
  }
  const entries = await client.query<{
    provider_id: string;
    canonical_model_id: string;
    entry_digest: string;
    capability_data: Record<string, unknown>;
  }>(`
    SELECT provider_id, canonical_model_id, entry_digest, capability_data
    FROM provider_capability_entries
    WHERE generation = $1
    ORDER BY provider_id COLLATE "C", canonical_model_id COLLATE "C"
  `, [capability.generation]);
  const expectedEntries = [...capability.entries].sort((left, right) =>
    compareUtf8(left.provider_id, right.provider_id) ||
    compareUtf8(left.canonical_model_id, right.canonical_model_id));
  assertStored("capability entries", expectedEntries, entries.rows);

  for (const alias of capability.aliases) {
    await client.query(`
      INSERT INTO provider_capability_aliases (
        generation, provider_id, alias_model_id, canonical_model_id
      ) VALUES ($1, $2, $3, $4)
      ON CONFLICT (generation, provider_id, alias_model_id) DO NOTHING
    `, [capability.generation, alias.provider_id, alias.alias_model_id, alias.canonical_model_id]);
  }
  const aliases = await client.query<{
    provider_id: string;
    alias_model_id: string;
    canonical_model_id: string;
  }>(`
    SELECT provider_id, alias_model_id, canonical_model_id
    FROM provider_capability_aliases
    WHERE generation = $1
    ORDER BY provider_id COLLATE "C", alias_model_id COLLATE "C"
  `, [capability.generation]);
  assertStored("capability aliases", capability.aliases, aliases.rows);

  const head = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text FROM provider_capability_head WHERE singleton = 1 FOR UPDATE
  `);
  if (!head.rows[0]) {
    throw new CatalogGen2Error("foundation_missing", "provider capability head is not materialized");
  }
  if (versionNumber(head.rows[0].active_generation, "capability head") < capability.generation) {
    await client.query(`
      UPDATE provider_capability_head SET active_generation = $1, updated_at = now() WHERE singleton = 1
    `, [capability.generation]);
  }
}

async function ensureGen2Catalog(client: PoolClient, catalog: PricingCatalogSpec): Promise<void> {
  await client.query(`
    INSERT INTO product_catalog_versions (
      product_id, generation, schema_version, capability_generation, capability_digest,
      content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
    ON CONFLICT (product_id, generation) DO NOTHING
  `, [
    catalog.product_id,
    catalog.generation,
    catalog.schema_version,
    catalog.capability_generation,
    catalog.capability_digest,
    catalog.content_digest,
    GEN2_ACTOR_TYPE,
    GEN2_ACTOR_ID,
    "Catalog generation 2: enable claude-opus-5 and claude-fable-5",
  ]);
  for (const entry of catalog.entries) {
    await client.query(`
      INSERT INTO product_catalog_entries (
        product_id, generation, capability_generation, provider_id, canonical_model_id, enabled
      ) VALUES ($1, $2, $3, $4, $5, $6)
      ON CONFLICT (product_id, generation, provider_id, canonical_model_id) DO NOTHING
    `, [
      catalog.product_id,
      catalog.generation,
      catalog.capability_generation,
      entry.provider_id,
      entry.canonical_model_id,
      entry.enabled,
    ]);
  }
  const header = await client.query<{
    product_id: string;
    generation: string;
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    content_digest: string;
  }>(`
    SELECT product_id, generation::text, schema_version::text,
           capability_generation::text, capability_digest, content_digest
    FROM product_catalog_versions WHERE product_id = $1 AND generation = $2
  `, [catalog.product_id, catalog.generation]);
  const row = header.rows[0];
  if (!row) throw new CatalogGen2Error("catalog_insert_lost", `catalog ${catalog.product_id} was not stored`);
  const entries = await client.query<{
    provider_id: string;
    canonical_model_id: string;
    enabled: boolean;
  }>(`
    SELECT provider_id, canonical_model_id, enabled
    FROM product_catalog_entries
    WHERE product_id = $1 AND generation = $2
    ORDER BY provider_id COLLATE "C", canonical_model_id COLLATE "C"
  `, [catalog.product_id, catalog.generation]);
  const expectedCatalog = {
    ...catalog,
    entries: [...catalog.entries].sort((left, right) =>
      compareUtf8(left.provider_id, right.provider_id) ||
      compareUtf8(left.canonical_model_id, right.canonical_model_id)),
  };
  assertStored(`catalog ${catalog.product_id}`, expectedCatalog, {
    product_id: row.product_id,
    generation: versionNumber(row.generation, "catalog generation"),
    schema_version: versionNumber(row.schema_version, "catalog schema version"),
    capability_generation: versionNumber(row.capability_generation, "catalog capability generation"),
    capability_digest: row.capability_digest,
    content_digest: row.content_digest,
    entries: entries.rows,
  });

  const head = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text FROM product_catalog_heads WHERE product_id = $1 FOR UPDATE
  `, [catalog.product_id]);
  if (!head.rows[0]) {
    throw new CatalogGen2Error("foundation_missing", `catalog head ${catalog.product_id} is not materialized`);
  }
  if (versionNumber(head.rows[0].active_generation, "catalog head") < catalog.generation) {
    await client.query(`
      UPDATE product_catalog_heads SET active_generation = $2, updated_at = now() WHERE product_id = $1
    `, [catalog.product_id, catalog.generation]);
  }

  await client.query(`
    INSERT INTO engine_catalog_jobs (
      id, product_id, generation, schema_version, content_digest, payload
    ) VALUES ($1, $2, $3, $4, $5, $6::jsonb)
    ON CONFLICT (product_id, generation) DO NOTHING
  `, [
    randomUUID(),
    catalog.product_id,
    catalog.generation,
    catalog.schema_version,
    catalog.content_digest,
    JSON.stringify(catalog),
  ]);
  const job = await client.query<{ schema_version: string; content_digest: string; payload: unknown }>(`
    SELECT schema_version::text, content_digest, payload
    FROM engine_catalog_jobs WHERE product_id = $1 AND generation = $2
  `, [catalog.product_id, catalog.generation]);
  const jobRow = job.rows[0];
  if (!jobRow) throw new CatalogGen2Error("catalog_job_insert_lost", "catalog control job was not stored");
  assertStored(`catalog job ${catalog.product_id}`, {
    schema_version: catalog.schema_version,
    content_digest: catalog.content_digest,
    payload: catalog,
  }, {
    schema_version: versionNumber(jobRow.schema_version, "catalog job schema version"),
    content_digest: jobRow.content_digest,
    payload: jobRow.payload,
  });
}

async function ensureGen2Switches(client: PoolClient, switches: ProviderSwitchSpec): Promise<void> {
  await client.query(`
    INSERT INTO provider_switch_versions (
      generation, schema_version, capability_generation, capability_digest,
      content_digest, actor_type, actor_id, reason
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    ON CONFLICT (generation) DO NOTHING
  `, [
    switches.generation,
    switches.schema_version,
    switches.capability_generation,
    switches.capability_digest,
    switches.content_digest,
    GEN2_ACTOR_TYPE,
    GEN2_ACTOR_ID,
    "Provider switches re-pinned to catalog generation 2",
  ]);
  for (const entry of switches.entries) {
    const scope = switchScopeParts(entry.scope);
    await client.query(`
      INSERT INTO provider_switch_entries (
        generation, provider_id, scope_type, product_id, segment, catalog_generation, enabled
      ) VALUES ($1, $2, $3, $4, $5, $6, $7)
      ON CONFLICT (generation, provider_id, scope_type, product_id, segment) DO NOTHING
    `, [
      switches.generation,
      entry.provider_id,
      scope.scopeType,
      scope.productId,
      scope.segment,
      entry.catalog_generation,
      entry.enabled,
    ]);
  }
  const header = await client.query<{
    generation: string;
    schema_version: string;
    capability_generation: string;
    capability_digest: string;
    content_digest: string;
  }>(`
    SELECT generation::text, schema_version::text, capability_generation::text,
           capability_digest, content_digest
    FROM provider_switch_versions WHERE generation = $1
  `, [switches.generation]);
  const row = header.rows[0];
  if (!row) throw new CatalogGen2Error("switch_insert_lost", "provider switches were not stored");
  const entries = await client.query<{
    provider_id: string;
    scope_type: "master" | "product" | "segment";
    product_id: string;
    segment: "" | "b2c" | "b2b";
    catalog_generation: string | null;
    enabled: boolean;
  }>(`
    SELECT provider_id, scope_type, product_id, segment,
           catalog_generation::text, enabled
    FROM provider_switch_entries WHERE generation = $1
    ORDER BY provider_id COLLATE "C", scope_type COLLATE "C", product_id COLLATE "C", segment COLLATE "C"
  `, [switches.generation]);
  const storedEntries: ProviderSwitchSpec["entries"] = entries.rows.map((entry) => ({
    provider_id: entry.provider_id,
    scope: entry.scope_type === "master"
      ? "master"
      : entry.scope_type === "product"
        ? { product: { product_id: entry.product_id } }
        : { segment: { product_id: entry.product_id, segment: entry.segment as "b2c" | "b2b" } },
    catalog_generation: entry.catalog_generation === null
      ? null
      : versionNumber(entry.catalog_generation, "switch catalog generation"),
    enabled: entry.enabled,
  }));
  assertStored("provider switches", switches, {
    generation: versionNumber(row.generation, "switch generation"),
    schema_version: versionNumber(row.schema_version, "switch schema version"),
    capability_generation: versionNumber(row.capability_generation, "switch capability generation"),
    capability_digest: row.capability_digest,
    content_digest: row.content_digest,
    entries: storedEntries,
  });

  const head = await client.query<{ active_generation: string }>(`
    SELECT active_generation::text FROM provider_switch_head WHERE singleton = 1 FOR UPDATE
  `);
  if (!head.rows[0]) {
    throw new CatalogGen2Error("foundation_missing", "provider switch head is not materialized");
  }
  if (versionNumber(head.rows[0].active_generation, "switch head") < switches.generation) {
    await client.query(`
      UPDATE provider_switch_head SET active_generation = $1, updated_at = now() WHERE singleton = 1
    `, [switches.generation]);
  }

  await client.query(`
    INSERT INTO engine_switch_jobs (
      id, generation, schema_version, content_digest, payload
    ) VALUES ($1, $2, $3, $4, $5::jsonb)
    ON CONFLICT (generation) DO NOTHING
  `, [randomUUID(), switches.generation, switches.schema_version, switches.content_digest, JSON.stringify(switches)]);
  const job = await client.query<{ schema_version: string; content_digest: string; payload: unknown }>(`
    SELECT schema_version::text, content_digest, payload FROM engine_switch_jobs WHERE generation = $1
  `, [switches.generation]);
  const jobRow = job.rows[0];
  if (!jobRow) throw new CatalogGen2Error("switch_job_insert_lost", "switch control job was not stored");
  assertStored("switch job", {
    schema_version: switches.schema_version,
    content_digest: switches.content_digest,
    payload: switches,
  }, {
    schema_version: versionNumber(jobRow.schema_version, "switch job schema version"),
    content_digest: jobRow.content_digest,
    payload: jobRow.payload,
  });
}

/**
 * Plan (dry_run) or materialize (apply) catalog generation 2.
 *
 * Apply is fail-closed on the foundation: the durable heads must still equal
 * the reviewed generation-1 identities, or already equal this exact plan
 * (idempotent replay). Any other state — an admin-advanced switch generation,
 * a hand-edited generation-1 entry, a partial generation-2 write with
 * different content — stops the transaction before any write.
 */
export async function runCatalogGen2(
  database: Database,
  options: { mode: CatalogGen2Mode },
): Promise<CatalogGen2Result> {
  const plan = buildCatalogGen2Plan();
  if (options.mode === "dry_run") {
    const client = await database.pool.connect();
    try {
      await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
      const foundation = await readFoundation(client);
      await client.query("ROLLBACK");
      return { mode: options.mode, plan, foundation, writes_committed: false };
    } catch (error) {
      await client.query("ROLLBACK").catch(() => undefined);
      throw error;
    } finally {
      client.release();
    }
  }

  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    const foundation = await readFoundation(client);
    if (!foundation.matches_reviewed_generation_1 && !foundation.already_materialized) {
      throw new CatalogGen2Error(
        "foundation_mismatch",
        "durable heads are neither the reviewed generation 1 nor this exact generation-2 plan",
      );
    }
    if (foundation.matches_reviewed_generation_1) {
      await assertGen1Entries(client);
    }

    await ensureGen2Capability(client, plan.capability);
    for (const catalog of plan.catalogs) await ensureGen2Catalog(client, catalog);
    await ensureGen2Switches(client, plan.switches);

    await client.query("COMMIT");
    return { mode: options.mode, plan, foundation, writes_committed: true };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
