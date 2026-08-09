import { describe, expect, it } from "vitest";
import { INTEGRATION_MODELS, type IntegrationProvider } from "./integration-builder-data";
import { API_REFERENCE_PROVIDER_TABS } from "./api-reference";
import { INTEGRATION_PROVIDER_TABS } from "./integration-builder";

/**
 * Every docs surface must offer every provider we serve.
 *
 * The data layer and each tab strip are separate arrays, so adding a provider to
 * `INTEGRATION_MODELS` silently leaves it unreachable in a UI that hardcodes its own list. That
 * is exactly how KIMI shipped into the API reference data while its tab strip still showed three
 * providers — the guide existed and no one could select it.
 */
describe("provider tab strips", () => {
  const served = Object.keys(INTEGRATION_MODELS).sort() as IntegrationProvider[];

  for (const [name, tabs] of [
    ["integration builder", INTEGRATION_PROVIDER_TABS],
    ["API reference", API_REFERENCE_PROVIDER_TABS],
  ] as const) {
    it(`offers every served provider in the ${name}`, () => {
      expect(tabs.map((tab) => tab.id).sort()).toEqual(served);
      // A tab without a label reads as a blank row rather than a provider.
      expect(tabs.every((tab) => tab.name.length > 0 && tab.en.length > 0 && tab.ru.length > 0)).toBe(true);
    });
  }
});
