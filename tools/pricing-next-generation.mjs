#!/usr/bin/env node
/**
 * pricing-next-generation — generates the exact constants for the next pricing
 * capability generation.
 *
 * Post-cutover, admitting a model requires a new frozen "capability generation":
 * constants in `packages/contracts/src/index.ts`, the Rust runtime-manifest mirrors
 * in `crates/forward/src/pricing.rs` and `crates/server/src/config.rs`, and the
 * Stage 5 v2 materializer constants in `packages/db/src/pricing-stage5-materializer-v2.ts`.
 * Every frozen digest must reproduce exactly or the drift guard
 * `target_capability_digest_drift` fires in production. This tool computes all
 * digests with the canonical algorithm and prints copy-pastable blocks; it NEVER
 * edits the target files — a human/agent applies and reviews the blocks.
 *
 * Canonical digest algorithm (the only authority, mirrored from
 * `packages/db/src/pricing-release-digest.ts` `stage5Digest` and
 * `packages/db/src/pricing-stage5-materializer-v2.ts` `legacyStage5Digest`):
 *
 *   digest(label, value) = "sha256:v1:" + sha256hex(
 *     "multi-discount-stage5:" + label + "\n" + canonicalJson(value))
 *
 * where canonicalJson is JSON.stringify over objects whose keys are sorted by
 * UTF-8 byte order with `undefined` values dropped (arrays keep their order).
 *
 * Usage:
 *   node tools/pricing-next-generation.mjs <spec.json>
 *   node tools/pricing-next-generation.mjs --example
 *
 * Spec JSON:
 * {
 *   "currentGeneration": 6,                  // the latest frozen generation
 *   "schemaVersion": 1,                      // MULTI_DISCOUNT_SCHEMA_VERSION
 *   "policyVersion": 4,                      // optional; default = next generation - 3
 *   "base": {
 *     "mainEntries":     [ {"provider_id": "...", "canonical_model_id": "...", "enabled": true}, ... ],
 *     "openkeysEntries": [ ... ],            // current OpenKeys catalog entries
 *     "aliases":         [ {"provider_id": "...", "alias_model_id": "...", "canonical_model_id": "..."} ]
 *   },
 *   "add": [
 *     {
 *       "provider_id": "openai",
 *       "canonical_model_id": "gpt-image-2-2026-04-21",
 *       "constant_name": "GPT_IMAGE_2_CANONICAL_MODEL",   // exported model-id const
 *       "entry_const_suffix": "IMAGE",                    // MULTI_DISCOUNT_GEN{n}_IMAGE_CATALOG_ENTRY
 *       "alias": "gpt-image-2",                           // optional public alias
 *       "products": ["main", "openkeys"]                  // optional; default both
 *     }
 *   ],
 *   "addAliases": [                                       // optional standalone capability aliases
 *     { "provider_id": "anthropic", "alias_model_id": "claude-haiku-4-5-20251001",
 *       "canonical_model_id": "claude-haiku-4-5" }        // may target an EXISTING model
 *   ],
 *   "summary": "one-line review note for the generated doc comments"  // optional
 * }
 *
 * The capability alias array is digested in canonical (provider_id, alias_model_id) UTF-8
 * byte order — the generator sorts the union of base aliases, addition aliases and
 * addAliases, so the frozen arrays in the materializer must stay in that order.
 */

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

// ---------------------------------------------------------------------------
// Canonical digest primitives (exact mirror of packages/db stage5Digest)
// ---------------------------------------------------------------------------

function compareUtf8(left, right) {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalValue(value) {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([, child]) => child !== undefined)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, child]) => [key, canonicalValue(child)]),
    );
  }
  return value;
}

export function canonicalJson(value) {
  return JSON.stringify(canonicalValue(value));
}

/** Exact mirror of `stage5Digest` in packages/db/src/pricing-release-digest.ts. */
export function stage5Digest(label, value) {
  const hex = createHash("sha256")
    .update(`multi-discount-stage5:${label}\n`, "utf8")
    .update(canonicalJson(value), "utf8")
    .digest("hex");
  return `sha256:v1:${hex}`;
}

// ---------------------------------------------------------------------------
// Capability / catalog / switch builders (exact mirror of the Stage 5 v2
// materializer semantics in packages/db/src/pricing-stage5-materializer-v2.ts)
// ---------------------------------------------------------------------------

function capabilityEntry(entry) {
  const capabilityData = { pricing_supported: true };
  return {
    provider_id: entry.provider_id,
    canonical_model_id: entry.canonical_model_id,
    enabled: true,
    entry_digest: stage5Digest("capability-entry", {
      provider_id: entry.provider_id,
      canonical_model_id: entry.canonical_model_id,
      enabled: entry.enabled,
      capability_data: capabilityData,
    }),
    capability_data: capabilityData,
  };
}

function buildCapabilityDigest(generation, schemaVersion, mainEntries, aliases) {
  const base = {
    generation,
    schema_version: schemaVersion,
    entries: mainEntries.map(capabilityEntry),
    aliases,
  };
  return stage5Digest("capability", base);
}

function buildCatalog(productId, generation, schemaVersion, capabilityGeneration, capabilityDigest, entries) {
  const normalizedEntries = [...entries].sort((left, right) =>
    compareUtf8(left.provider_id, right.provider_id)
    || compareUtf8(left.canonical_model_id, right.canonical_model_id));
  const base = {
    product_id: productId,
    generation,
    schema_version: schemaVersion,
    capability_generation: capabilityGeneration,
    capability_digest: capabilityDigest,
    entries: normalizedEntries.map((entry) => ({ ...entry })),
  };
  return { ...base, content_digest: stage5Digest("catalog", base) };
}

function switchParts(scope) {
  if (scope === "master") return ["master", "", ""];
  if ("product" in scope) return ["product", scope.product.product_id, ""];
  return ["segment", scope.segment.product_id, scope.segment.segment];
}

function sortSwitches(entries) {
  return [...entries].sort((left, right) => {
    const leftParts = [left.provider_id, ...switchParts(left.scope)];
    const rightParts = [right.provider_id, ...switchParts(right.scope)];
    for (let index = 0; index < leftParts.length; index += 1) {
      const order = compareUtf8(leftParts[index], rightParts[index]);
      if (order !== 0) return order;
    }
    return 0;
  });
}

function buildSwitches(generation, schemaVersion, capabilityGeneration, capabilityDigest, catalogGeneration) {
  const entries = [];
  for (const providerId of ["anthropic", "openai", "google"]) {
    entries.push(
      { provider_id: providerId, scope: "master", catalog_generation: null, enabled: true },
      {
        provider_id: providerId,
        scope: { product: { product_id: "main" } },
        catalog_generation: catalogGeneration,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: "main", segment: "b2c" } },
        catalog_generation: catalogGeneration,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: "main", segment: "b2b" } },
        catalog_generation: catalogGeneration,
        enabled: true,
      },
    );
    if (providerId !== "google") {
      entries.push({
        provider_id: providerId,
        scope: { product: { product_id: "openkeys" } },
        catalog_generation: catalogGeneration,
        enabled: true,
      });
    }
  }
  const base = {
    generation,
    schema_version: schemaVersion,
    capability_generation: capabilityGeneration,
    capability_digest: capabilityDigest,
    entries: sortSwitches(entries),
  };
  return { ...base, content_digest: stage5Digest("switches", base) };
}

// ---------------------------------------------------------------------------
// Generation plan
// ---------------------------------------------------------------------------

const GEN_PREFIX = "MULTI_DISCOUNT_GEN";

export class SpecError extends Error {}

function requireString(value, path) {
  if (typeof value !== "string" || value.length === 0) {
    throw new SpecError(`spec field ${path} must be a non-empty string`);
  }
  return value;
}

function normalizeAlias(alias, path) {
  if (alias === null || typeof alias !== "object") throw new SpecError(`${path} must be an object`);
  return {
    provider_id: requireString(alias.provider_id, `${path}.provider_id`),
    alias_model_id: requireString(alias.alias_model_id, `${path}.alias_model_id`),
    canonical_model_id: requireString(alias.canonical_model_id, `${path}.canonical_model_id`),
  };
}

/** Canonical capability-alias order: (provider_id, alias_model_id) in UTF-8 byte order. */
function sortAliases(aliases) {
  return [...aliases].sort((left, right) =>
    compareUtf8(left.provider_id, right.provider_id)
    || compareUtf8(left.alias_model_id, right.alias_model_id));
}

export function buildNextGeneration(spec) {
  if (spec === null || typeof spec !== "object") throw new SpecError("spec must be an object");
  const currentGeneration = spec.currentGeneration;
  if (!Number.isSafeInteger(currentGeneration) || currentGeneration <= 0) {
    throw new SpecError("spec.currentGeneration must be a positive integer");
  }
  const generation = currentGeneration + 1;
  const schemaVersion = spec.schemaVersion ?? 1;
  if (!Number.isSafeInteger(schemaVersion) || schemaVersion <= 0) {
    throw new SpecError("spec.schemaVersion must be a positive integer");
  }
  const policyVersion = spec.policyVersion ?? generation - 3;
  if (!Number.isSafeInteger(policyVersion) || policyVersion <= 0) {
    throw new SpecError("spec.policyVersion must be a positive integer");
  }
  const base = spec.base;
  if (base === null || typeof base !== "object") throw new SpecError("spec.base is required");
  for (const key of ["mainEntries", "openkeysEntries", "aliases"]) {
    if (!Array.isArray(base[key])) throw new SpecError(`spec.base.${key} must be an array`);
  }
  const additions = spec.add;
  if (!Array.isArray(additions) || additions.length === 0) {
    throw new SpecError("spec.add must list at least one added model");
  }
  const seenSuffixes = new Set();
  const normalizedAdditions = additions.map((addition, index) => {
    const path = `spec.add[${index}]`;
    const providerId = requireString(addition?.provider_id, `${path}.provider_id`);
    const canonicalModelId = requireString(addition?.canonical_model_id, `${path}.canonical_model_id`);
    const constantName = requireString(addition?.constant_name, `${path}.constant_name`);
    const entryConstSuffix = requireString(addition?.entry_const_suffix, `${path}.entry_const_suffix`);
    if (!/^[A-Z0-9_]+$/.test(entryConstSuffix)) {
      throw new SpecError(`${path}.entry_const_suffix must be an UPPER_SNAKE identifier fragment`);
    }
    if (seenSuffixes.has(entryConstSuffix)) {
      throw new SpecError(`${path}.entry_const_suffix ${entryConstSuffix} is duplicated`);
    }
    seenSuffixes.add(entryConstSuffix);
    const products = addition.products ?? ["main", "openkeys"];
    if (!Array.isArray(products) || products.length === 0
        || products.some((product) => product !== "main" && product !== "openkeys")) {
      throw new SpecError(`${path}.products must be a non-empty subset of ["main", "openkeys"]`);
    }
    return {
      provider_id: providerId,
      canonical_model_id: canonicalModelId,
      constant_name: constantName,
      entry_const_suffix: entryConstSuffix,
      alias: addition.alias ?? null,
      products,
      entry: { provider_id: providerId, canonical_model_id: canonicalModelId, enabled: true },
    };
  });

  const mainEntries = [
    ...base.mainEntries,
    ...normalizedAdditions.filter((addition) => addition.products.includes("main"))
      .map((addition) => addition.entry),
  ];
  const openkeysEntries = [
    ...base.openkeysEntries,
    ...normalizedAdditions.filter((addition) => addition.products.includes("openkeys"))
      .map((addition) => addition.entry),
  ];
  const addedAliases = [
    ...normalizedAdditions.filter((addition) => addition.alias !== null).map((addition) => ({
      provider_id: addition.provider_id,
      alias_model_id: addition.alias,
      canonical_model_id: addition.canonical_model_id,
    })),
    ...(spec.addAliases ?? []).map((alias, index) => normalizeAlias(alias, `spec.addAliases[${index}]`)),
  ];
  const aliases = sortAliases([
    ...base.aliases.map((alias, index) => normalizeAlias(alias, `spec.base.aliases[${index}]`)),
    ...addedAliases,
  ]);

  const capabilityDigest = buildCapabilityDigest(generation, schemaVersion, mainEntries, aliases);
  const mainCatalog = buildCatalog(
    "main", generation, schemaVersion, generation, capabilityDigest, mainEntries);
  const openkeysCatalog = buildCatalog(
    "openkeys", generation, schemaVersion, generation, capabilityDigest, openkeysEntries);
  const switches = buildSwitches(generation, schemaVersion, generation, capabilityDigest, generation);

  return {
    generation,
    currentGeneration,
    schemaVersion,
    policyVersion,
    additions: normalizedAdditions,
    mainEntries,
    openkeysEntries,
    aliases,
    addedAliases,
    openkeysChanged: normalizedAdditions.some((addition) => addition.products.includes("openkeys")),
    capabilityDigest,
    mainCatalogDigest: mainCatalog.content_digest,
    openkeysCatalogDigest: openkeysCatalog.content_digest,
    switchDigest: switches.content_digest,
    summary: spec.summary ?? null,
  };
}

// ---------------------------------------------------------------------------
// Rendering: copy-pastable blocks per target file. Nothing is written to disk.
// ---------------------------------------------------------------------------

function renderContractsBlock(plan) {
  const gen = `${GEN_PREFIX}${plan.generation}`;
  const prev = `${GEN_PREFIX}${plan.currentGeneration}`;
  const lines = [];
  const addedIds = plan.additions.map((addition) => addition.canonical_model_id).join(", ");
  lines.push("/**");
  lines.push(` * Additive pricing capability generation ${plan.generation}. Generation ${plan.currentGeneration} remains`);
  lines.push(` * byte-identical because the active production release may still pin it; generation`);
  lines.push(` * ${plan.generation} adds only: ${addedIds}.${plan.summary ? ` ${plan.summary}` : ""}`);
  lines.push(" */");
  lines.push(`export const ${gen}_CAPABILITY_GENERATION = ${plan.generation};`);
  lines.push(`export const ${gen}_CAPABILITY_DIGEST =`);
  lines.push(`  "${plan.capabilityDigest}";`);
  for (const addition of plan.additions) {
    lines.push(`export const ${addition.constant_name} = "${addition.canonical_model_id}";`);
  }
  lines.push("");
  for (const addition of plan.additions) {
    lines.push(`const ${gen}_${addition.entry_const_suffix}_CATALOG_ENTRY = Object.freeze({`);
    lines.push(`  provider_id: "${addition.provider_id}" as const,`);
    lines.push(`  canonical_model_id: ${addition.constant_name},`);
    lines.push("  enabled: true as const,");
    lines.push("});");
    lines.push("");
  }
  const entryRefs = (product) => plan.additions
    .filter((addition) => addition.products.includes(product))
    .map((addition) => `  ${gen}_${addition.entry_const_suffix}_CATALOG_ENTRY,`);
  lines.push(`export const ${gen}_MAIN_CATALOG_ENTRIES = Object.freeze([`);
  lines.push(`  ...${prev}_MAIN_CATALOG_ENTRIES,`);
  lines.push(...entryRefs("main"));
  lines.push("]);");
  lines.push("");
  lines.push(`export const ${gen}_OPENKEYS_CATALOG_ENTRIES = Object.freeze([`);
  lines.push(`  ...${prev}_OPENKEYS_CATALOG_ENTRIES,`);
  lines.push(...entryRefs("openkeys"));
  lines.push("]);");
  return lines.join("\n");
}

function renderForwardManifestBlock(plan) {
  return [
    "// crates/forward/src/pricing.rs — builtin_pricing_runtime_manifest():",
    "// 1) append to the capabilities array:",
    "        (",
    `            ${plan.generation},`,
    `            "${plan.capabilityDigest}",`,
    "        ),",
    "// 2) bump the manifest generation:",
    `    PricingRuntimeManifestEvidence::new(${plan.generation}, capabilities)`,
    "// 3) extend the doc comment with one line describing the new generation.",
  ].join("\n");
}

function renderServerConfigTestBlock(plan) {
  return [
    "// crates/server/src/config.rs — pricing_shadow_manifest_is_fixed_registry_canonical_evidence:",
    `        assert_eq!(manifest.manifest_generation(), ${plan.generation});`,
    `        assert_eq!(manifest.capabilities().len(), ${plan.generation});`,
    "// and append to the (index, generation, digest) table:",
    "            (",
    `                ${plan.currentGeneration},`,
    `                ${plan.generation},`,
    `                "${plan.capabilityDigest}",`,
    "            ),",
  ].join("\n");
}

function renderMaterializerBlock(plan) {
  const gen = `${GEN_PREFIX}${plan.generation}`;
  const prev = `${GEN_PREFIX}${plan.currentGeneration}`;
  const lines = [
    "// packages/db/src/pricing-stage5-materializer-v2.ts:",
    "// 1) imports: replace the four " + prev + "_* imports with:",
    `  ${gen}_CAPABILITY_DIGEST,`,
    `  ${gen}_CAPABILITY_GENERATION,`,
    `  ${gen}_MAIN_CATALOG_ENTRIES,`,
    `  ${gen}_OPENKEYS_CATALOG_ENTRIES,`,
    "// 2) generation constants:",
    `export const STAGE5_V2_CATALOG_GENERATION = ${plan.generation};`,
    `export const STAGE5_V2_SWITCH_GENERATION = ${plan.generation};`,
    `export const STAGE5_V2_POLICY_VERSION = ${plan.policyVersion};`,
    "// 3) replace every remaining " + prev + "_ reference in the file with " + gen + "_",
  ];
  const newAliases = plan.addedAliases;
  if (newAliases.length > 0) {
    lines.push("// 4) merge into the aliases array in buildStage5V2Capability(), keeping canonical");
    lines.push("//    (provider_id, alias_model_id) UTF-8 byte order — the digest is order-sensitive:");
    for (const alias of newAliases) {
      lines.push(
        "      {",
        `        provider_id: "${alias.provider_id}",`,
        `        alias_model_id: "${alias.alias_model_id}",`,
        `        canonical_model_id: "${alias.canonical_model_id}",`,
        "      },",
      );
    }
  }
  return lines.join("\n");
}

function renderEngineClientBlock(plan) {
  const gen = `${GEN_PREFIX}${plan.generation}`;
  const lines = ["// packages/engine-client/src/release-provisioning.ts — externalOwnerPolicyVersion():"];
  lines.push(`  if (release.capability_generation === ${plan.generation}) return ${plan.policyVersion};`);
  if (plan.openkeysChanged) {
    lines.push("// packages/engine-client/src/openkeys-policy.ts — append to REVIEWED_OPENKEYS_CATALOGS:");
    lines.push(
      "  {",
      `    generation: ${plan.generation},`,
      `    capability_generation: ${gen}_CAPABILITY_GENERATION,`,
      `    capability_digest: ${gen}_CAPABILITY_DIGEST,`,
      `    entries: ${gen}_OPENKEYS_CATALOG_ENTRIES,`,
      "  },",
    );
  } else {
    lines.push("// packages/engine-client/src/openkeys-policy.ts: OpenKeys entries unchanged —");
    lines.push("// no new REVIEWED_OPENKEYS_CATALOGS row is needed.");
  }
  return lines.join("\n");
}

export function renderReport(plan) {
  return [
    `=== Pricing capability generation ${plan.generation} (extends frozen generation ${plan.currentGeneration}) ===`,
    "",
    "Computed digests (canonical stage5 algorithm, domain `multi-discount-stage5`):",
    `  capability digest:        ${plan.capabilityDigest}`,
    `  catalog digest (main):    ${plan.mainCatalogDigest}`,
    `  catalog digest (openkeys): ${plan.openkeysCatalogDigest}`,
    `  switch digest:            ${plan.switchDigest}`,
    `  release policy version:   ${plan.policyVersion}`,
    "",
    "--- BLOCK A: packages/contracts/src/index.ts (insert after the generation-"
      + `${plan.currentGeneration} block) ---`,
    renderContractsBlock(plan),
    "",
    "--- BLOCK B: crates/forward/src/pricing.rs ---",
    renderForwardManifestBlock(plan),
    "",
    "--- BLOCK C: crates/server/src/config.rs ---",
    renderServerConfigTestBlock(plan),
    "",
    "--- BLOCK D: packages/db/src/pricing-stage5-materializer-v2.ts ---",
    renderMaterializerBlock(plan),
    "",
    "--- BLOCK E: packages/engine-client ---",
    renderEngineClientBlock(plan),
    "",
    "NOT covered (review and edit by hand): doc-comment prose in every file above,",
    "unit-test expectation updates that pin the catalog/switch digests (use the values",
    "at the top of this report), MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES",
    "publication (a separate evidence-gated checkpoint), and activation/materialization",
    "itself. This tool never edits the target files.",
  ].join("\n");
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

const EXAMPLE_SPEC = {
  currentGeneration: 6,
  schemaVersion: 1,
  base: {
    mainEntries: [
      { provider_id: "anthropic", canonical_model_id: "claude-opus-5", enabled: true },
      "// ... the complete frozen generation-6 main entry list, in order",
    ],
    openkeysEntries: ["// ... the complete frozen generation-6 OpenKeys entry list, in order"],
    aliases: [
      { provider_id: "openai", alias_model_id: "gpt-5.6", canonical_model_id: "gpt-5.6-sol" },
      { provider_id: "openai", alias_model_id: "gpt-image-2", canonical_model_id: "gpt-image-2-2026-04-21" },
    ],
  },
  add: [
    {
      provider_id: "openai",
      canonical_model_id: "gpt-example-1-2026-01-01",
      constant_name: "GPT_EXAMPLE_1_CANONICAL_MODEL",
      entry_const_suffix: "EXAMPLE",
      alias: "gpt-example-1",
      products: ["main", "openkeys"],
    },
  ],
  summary: "optional one-line review note for the doc comments",
};

function main(argv) {
  const args = argv.slice(2);
  if (args.length === 0 || args.includes("--help")) {
    console.log("usage: node tools/pricing-next-generation.mjs <spec.json>\n" +
      "       node tools/pricing-next-generation.mjs --example");
    return args.length === 0 ? 1 : 0;
  }
  if (args[0] === "--example") {
    console.log(JSON.stringify(EXAMPLE_SPEC, null, 2));
    return 0;
  }
  const spec = JSON.parse(readFileSync(args[0], "utf8"));
  console.log(renderReport(buildNextGeneration(spec)));
  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    process.exitCode = main(process.argv);
  } catch (error) {
    console.error(error instanceof SpecError ? `spec error: ${error.message}` : error);
    process.exitCode = 1;
  }
}
