/**
 * Replay test for tools/pricing-next-generation.mjs.
 *
 * Feeds the generator the exact GEN5 → GEN6 transition (the gpt-image-2
 * admission) and asserts that every produced constant and digest is
 * byte-identical to the frozen GEN6 artifacts currently in the repository:
 *
 *   - `MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST` in packages/contracts/src/index.ts;
 *   - the GEN6 constants block in packages/contracts/src/index.ts (full text,
 *     doc comment excluded — prose stays human);
 *   - the runtime-manifest tuple in crates/forward/src/pricing.rs;
 *   - the manifest test tuple in crates/server/src/config.rs;
 *   - the Stage 5 v2 materializer generation/policy constants.
 *
 * The catalog and switch digests are not frozen in any source file (the
 * materializer computes them at runtime); their expected values below were
 * verified byte-for-byte against the compiled production builder
 * `packages/db/dist/pricing-stage5-materializer-v2.js`
 * (`buildStage5V2Capability` + `buildStage5V2CatalogsAndSwitches`).
 *
 * Run: node --test tools/
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  buildNextGeneration,
  renderReport,
  SpecError,
} from "./pricing-next-generation.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

function readRepo(path) {
  return readFileSync(join(repoRoot, path), "utf8");
}

/** Extracts a frozen `const NAME = [ "a", "b" ] as const;` string list. */
function extractStringList(source, name) {
  const match = source.match(new RegExp(`export const ${name} = \\[([^\\]]*)\\] as const;`));
  assert.ok(match, `constant ${name} not found in contracts`);
  return [...match[1].matchAll(/"([^"]+)"/g)].map((entry) => entry[1]);
}

function extractStringConstant(source, name) {
  const match = source.match(new RegExp(`export const ${name} =\\s*"([^"]+)";`));
  assert.ok(match, `constant ${name} not found in contracts`);
  return match[1];
}

const contracts = readRepo("packages/contracts/src/index.ts");
const forwardPricing = readRepo("crates/forward/src/pricing.rs");
const serverConfig = readRepo("crates/server/src/config.rs");
const materializer = readRepo("packages/db/src/pricing-stage5-materializer-v2.ts");

const GEN2_ANTHROPIC = extractStringList(contracts, "MULTI_DISCOUNT_GEN2_ANTHROPIC_CANONICAL_MODELS");
const OPENAI_MODELS = extractStringList(contracts, "CURRENT_OPENAI_CANONICAL_MODELS");
const GEN5_GEMINI = extractStringList(contracts, "MULTI_DISCOUNT_GEN5_GEMINI_CANONICAL_MODELS");
const GPT_IMAGE_2 = extractStringConstant(contracts, "GPT_IMAGE_2_CANONICAL_MODEL");
const GEN5_DIGEST = extractStringConstant(contracts, "MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST");
const GEN6_DIGEST = extractStringConstant(contracts, "MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST");

const toEntries = (provider, ids) => ids.map((id) => ({
  provider_id: provider,
  canonical_model_id: id,
  enabled: true,
}));

const GEN2_ENTRIES = [...toEntries("anthropic", GEN2_ANTHROPIC), ...toEntries("openai", OPENAI_MODELS)];
const GEN5_MAIN_ENTRIES = [...GEN2_ENTRIES, ...toEntries("google", GEN5_GEMINI)];

/** The exact GEN5 → GEN6 admission spec. */
function gen6Spec() {
  return {
    currentGeneration: 5,
    base: {
      mainEntries: GEN5_MAIN_ENTRIES,
      openkeysEntries: GEN2_ENTRIES,
      aliases: [{
        provider_id: "openai",
        alias_model_id: "gpt-5.6",
        canonical_model_id: "gpt-5.6-sol",
      }],
    },
    add: [{
      provider_id: "openai",
      canonical_model_id: GPT_IMAGE_2,
      constant_name: "GPT_IMAGE_2_CANONICAL_MODEL",
      entry_const_suffix: "IMAGE",
      alias: "gpt-image-2",
    }],
  };
}

test("replay: base generation 5 recomputes to the frozen GEN5 digest", () => {
  // Validates the extracted base lists and the digest primitive before the
  // transition itself: a wrong base would pass silently otherwise.
  // Entry order is part of the digest, so the appended model must be the
  // frozen list's last element: base = GEN5 without gemini-3.6-flash, add it back.
  const baseOnly = buildNextGeneration({
    currentGeneration: 4,
    base: {
      mainEntries: [
        ...GEN2_ENTRIES,
        ...toEntries("google", GEN5_GEMINI.slice(0, -1)),
      ],
      openkeysEntries: GEN2_ENTRIES,
      aliases: gen6Spec().base.aliases,
    },
    add: [{
      provider_id: "google",
      canonical_model_id: GEN5_GEMINI[GEN5_GEMINI.length - 1],
      constant_name: "GEMINI_3_6_FLASH_CANONICAL_MODEL",
      entry_const_suffix: "GEMINI_3_6_FLASH",
      products: ["main"],
    }],
  });
  assert.equal(baseOnly.capabilityDigest, GEN5_DIGEST);
});

test("replay: GEN5 + gpt-image-2 yields the frozen GEN6 capability digest", () => {
  const plan = buildNextGeneration(gen6Spec());
  assert.equal(plan.generation, 6);
  assert.equal(plan.capabilityDigest, GEN6_DIGEST);
  assert.notEqual(plan.capabilityDigest, GEN5_DIGEST);
});

test("replay: catalog and switch digests match the production materializer", () => {
  // Expected values verified byte-for-byte against the compiled
  // packages/db/dist/pricing-stage5-materializer-v2.js on the GEN6 SHA.
  const plan = buildNextGeneration(gen6Spec());
  assert.equal(
    plan.mainCatalogDigest,
    "sha256:v1:16c8a38996dd874d501ef22d31fdf77dfb0eed3d2f1535b7976ff96fc281cbad",
  );
  assert.equal(
    plan.openkeysCatalogDigest,
    "sha256:v1:2aa7dc90a90c479184e67cd2e986e468a6de2a79e7a7ae569596eab177f6313c",
  );
  assert.equal(
    plan.switchDigest,
    "sha256:v1:1bd98f8e3624e1573d144946de7c7f3d05a51f6eb06d1a2f19ba3cb92a174567",
  );
  assert.equal(plan.policyVersion, 3);
});

test("replay: generated contracts block is byte-identical to the frozen GEN6 block", () => {
  const plan = buildNextGeneration(gen6Spec());
  const report = renderReport(plan);
  const blockStart = report.indexOf("export const MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION");
  assert.ok(blockStart !== -1, "report does not contain the contracts block");
  const generated = report.slice(blockStart, report.indexOf("]);", report.indexOf(
    "MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES = Object.freeze([",
  )) + 3);

  const frozenStart = contracts.indexOf("export const MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION");
  assert.ok(frozenStart !== -1, "frozen GEN6 block not found in contracts");
  const frozen = contracts.slice(frozenStart, contracts.indexOf("]);", contracts.indexOf(
    "MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES = Object.freeze([",
  )) + 3);

  assert.equal(generated, frozen);
});

test("replay: generated Rust mirror tuples appear verbatim in the engine sources", () => {
  const plan = buildNextGeneration(gen6Spec());
  const report = renderReport(plan);

  const forwardTuple = [
    "        (",
    "            6,",
    `            "${GEN6_DIGEST}",`,
    "        ),",
  ].join("\n");
  assert.ok(report.includes(forwardTuple), "report lacks the forward manifest tuple");
  assert.ok(forwardPricing.includes(forwardTuple), "forward manifest tuple not in pricing.rs");
  assert.ok(
    forwardPricing.includes("PricingRuntimeManifestEvidence::new(6, capabilities)"),
    "forward manifest generation pin not in pricing.rs",
  );
  assert.ok(
    report.includes("PricingRuntimeManifestEvidence::new(6, capabilities)"),
    "report lacks the forward manifest generation pin",
  );

  const configTuple = [
    "            (",
    "                5,",
    "                6,",
    `                "${GEN6_DIGEST}",`,
    "            ),",
  ].join("\n");
  assert.ok(report.includes(configTuple), "report lacks the server config test tuple");
  assert.ok(serverConfig.includes(configTuple), "server config test tuple not in config.rs");
  assert.ok(
    serverConfig.includes("assert_eq!(manifest.manifest_generation(), 6);"),
    "server config manifest generation assertion not in config.rs",
  );
});

test("replay: materializer constants match the frozen GEN6 materializer", () => {
  const plan = buildNextGeneration(gen6Spec());
  const report = renderReport(plan);
  for (const line of [
    "export const STAGE5_V2_CATALOG_GENERATION = 6;",
    "export const STAGE5_V2_SWITCH_GENERATION = 6;",
    "export const STAGE5_V2_POLICY_VERSION = 3;",
  ]) {
    assert.ok(report.includes(line), `report lacks materializer line: ${line}`);
    assert.ok(materializer.includes(line), `materializer lacks line: ${line}`);
  }
  const alias = [
    "      {",
    "        provider_id: \"openai\",",
    "        alias_model_id: \"gpt-image-2\",",
    `        canonical_model_id: "${GPT_IMAGE_2}",`,
    "      },",
  ].join("\n");
  assert.ok(report.includes(alias), "report lacks the materializer alias entry");
  assert.ok(materializer.includes(alias), "materializer alias entry not in the materializer");
});

test("replay: engine-client blocks carry the GEN6 policy version and OpenKeys row", () => {
  const plan = buildNextGeneration(gen6Spec());
  const report = renderReport(plan);
  assert.ok(
    report.includes("if (release.capability_generation === 6) return 3;"),
    "report lacks the externalOwnerPolicyVersion line",
  );
  const provisioning = readRepo("packages/engine-client/src/release-provisioning.ts");
  assert.ok(
    provisioning.includes("if (release.capability_generation === 6) return 3;"),
    "externalOwnerPolicyVersion line not in release-provisioning.ts",
  );
  assert.ok(
    report.includes("entries: MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES,"),
    "report lacks the REVIEWED_OPENKEYS_CATALOGS row",
  );
});

test("spec validation fails loudly", () => {
  assert.throws(() => buildNextGeneration({ currentGeneration: 5 }), SpecError);
  assert.throws(
    () => buildNextGeneration({ ...gen6Spec(), add: [] }),
    /at least one added model/,
  );
  assert.throws(
    () => buildNextGeneration({
      ...gen6Spec(),
      add: [{ provider_id: "openai", canonical_model_id: "x" }],
    }),
    /constant_name/,
  );
});
