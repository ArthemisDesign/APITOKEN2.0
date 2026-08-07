import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { buildStage5V2Capability } from "./pricing-stage5-materializer-v2.js";

/**
 * Every model the router advertises must be admitted to the catalog the materializer publishes.
 *
 * Publishing a model and pricing it are separate steps, and nothing forced the second one. A model
 * reaches `routing-presets.json`, the provider config and `crates/metering`, starts serving, and
 * only then — when a customer asks for it — does admission discover it cannot be quoted. That gap
 * cost 953 customer-visible `529`s in one day, and it stayed open for days because the Rust gate
 * `advertised_models_all_have_an_exact_tariff` checks the tariff half only. This is the other half:
 * the tariff exists, but the catalog entry does not.
 *
 * The check runs against `buildStage5V2Capability()` rather than a copied list, so it follows the
 * generation automatically and cannot drift from what is actually published.
 */
describe("advertised models are admitted to the published catalog", () => {
  const manifestPath = path.resolve(
    __dirname,
    "../../../crates/router/routing-presets.json",
  );

  /**
   * The gap this gate was written to expose, recorded so the gate can land before it is closed.
   *
   * These four are advertised and unpriced today; the first three refuse live customer requests,
   * and the fourth survives only because `crates/metering` normalises the dated snapshot to
   * `claude-haiku-4-5` before admission looks anything up. Generation 7 admits them.
   *
   * The list is self-expiring: admitting a model here without deleting its line fails the second
   * test below. An exemption that outlives its reason is how a temporary allowance becomes
   * permanent, and this one cannot.
   */
  const KNOWN_UNADMITTED = [
    "anthropic/claude-opus-4-6",
    "anthropic/claude-opus-4-5-20251101",
    "anthropic/claude-sonnet-4-5-20250929",
    "anthropic/claude-haiku-4-5-20251001",
  ];

  function advertisedModels(): { provider: string; model: string }[] {
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as {
      models: { id: string }[];
    };
    return manifest.models.map((entry) => {
      const separator = entry.id.indexOf("/");
      if (separator < 0) {
        throw new Error(`advertised model id is not namespaced: ${entry.id}`);
      }
      return {
        provider: entry.id.slice(0, separator),
        model: entry.id.slice(separator + 1),
      };
    });
  }

  function admittedIds(): Set<string> {
    const capability = buildStage5V2Capability();
    const admitted = new Set(
      capability.entries.map(
        (entry: { provider_id: string; canonical_model_id: string }) =>
          `${entry.provider_id}/${entry.canonical_model_id}`,
      ),
    );
    // A dated snapshot is admitted through its alias, exactly as `gpt-5.6` and `gpt-image-2` are:
    // the alias resolves to a canonical id that must itself carry an entry.
    for (const alias of capability.aliases) {
      if (admitted.has(`${alias.provider_id}/${alias.canonical_model_id}`)) {
        admitted.add(`${alias.provider_id}/${alias.alias_model_id}`);
      }
    }
    return admitted;
  }

  it("has a catalog entry or a canonical alias for every advertised model", () => {
    const admitted = admittedIds();
    const unpriced = advertisedModels()
      .map(({ provider, model }) => `${provider}/${model}`)
      .filter((id) => !admitted.has(id) && !KNOWN_UNADMITTED.includes(id));

    expect(
      unpriced,
      `these models are advertised by the router but absent from the published catalog, so admission ` +
        `cannot quote them and every request for them is refused: ${unpriced.join(", ")}. Admit them in a ` +
        `capability generation before advertising them, or drop them from crates/router/routing-presets.json.`,
    ).toEqual([]);
  });

  it("keeps no exemption for a model that is already admitted", () => {
    const admitted = admittedIds();
    const stale = KNOWN_UNADMITTED.filter((id) => admitted.has(id));

    expect(
      stale,
      `these models are now in the catalog, so their exemption in KNOWN_UNADMITTED is obsolete and ` +
        `must be deleted: ${stale.join(", ")}. Leaving it would hide the next gap behind a stale allowance.`,
    ).toEqual([]);
  });

  it("reads a manifest that actually contains models", () => {
    // A renamed or moved manifest would otherwise turn the gate above into a silent pass.
    expect(advertisedModels().length).toBeGreaterThan(0);
  });
});
