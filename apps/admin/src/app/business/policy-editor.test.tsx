import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import {
  canonicalizePricingRules,
  PolicyRuleEditor,
  pricingRulesSignature,
  type PricingCatalogView,
  type PricingRule,
} from "./policy-editor";

const catalog: PricingCatalogView = {
  productId: "main",
  catalogGeneration: 3,
  switchGeneration: 5,
  switchDigest: "sha256:v1:switches",
  switchSyncState: "confirmed",
  switchLastError: null,
  providers: [
    {
      providerId: "anthropic",
      masterEnabled: true,
      productEnabled: true,
      b2cEnabled: true,
      b2bEnabled: true,
      models: ["claude-sonnet-4-5"],
    },
    {
      providerId: "openai",
      masterEnabled: true,
      productEnabled: true,
      b2cEnabled: true,
      b2bEnabled: false,
      models: ["gpt-5"],
    },
  ],
};

describe("multi-provider policy editor", () => {
  it("canonicalizes full replacement rules independently from selection order", () => {
    const provider: PricingRule = {
      scope: { provider: { providerId: "anthropic" } },
      pricingMode: "discount",
      discountBps: 6_000,
    };
    const model: PricingRule = {
      scope: { model: { providerId: "anthropic", canonicalModelId: "claude-sonnet-4-5" } },
      pricingMode: "discount",
      discountBps: 5_000,
    };

    expect(pricingRulesSignature([provider, model])).toBe(pricingRulesSignature([model, provider]));
    expect(canonicalizePricingRules([model, provider])).toEqual([provider, model]);
  });

  it("renders only catalog providers/models and exposes independent provider/model rules", () => {
    const html = renderToString(
      <PolicyRuleEditor catalog={catalog} rules={[]} onChange={vi.fn()} segment="b2b" />,
    );
    expect(html).toContain("anthropic");
    expect(html).toContain("claude-sonnet-4-5");
    expect(html).toContain("openai");
    expect(html).toContain("gpt-5");
    expect(html).not.toContain("gemini");
    expect(html).toContain("Нужно выбрать хотя бы одно правило");
  });

  it("does not expose track mode for B2B/service editors", () => {
    const html = renderToString(
      <PolicyRuleEditor
        catalog={catalog}
        rules={[{
          scope: { provider: { providerId: "anthropic" } },
          pricingMode: "discount",
          discountBps: 6_000,
        }]}
        onChange={vi.fn()}
        segment="b2b"
      />,
    );
    expect(html).not.toContain("прогрессивный тариф");
    expect(html).toContain("фиксированная скидка");
  });
});
