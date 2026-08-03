import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import {
  buildPricingStage8CapturePayloadV2,
  PricingStage8CaptureControl,
  pricingStage8CaptureActiveCount,
  pricingStage8CaptureConfirmationPhrase,
  type PricingStage8CaptureControlV2,
  type PricingStage8CaptureDraftV2,
} from "./stage8-capture-control";

const OBSERVED_AT = "2026-08-03T00:00:00.000Z";

function draft(overrides: Partial<PricingStage8CaptureDraftV2> = {}): PricingStage8CaptureDraftV2 {
  return {
    idempotencyKey: "44444444-4444-4444-8444-444444444444",
    targetGeneration: "41",
    recoveryGeneration: "42",
    windowStartTs: "1785714900",
    windowEndTs: "1785715190",
    minSamplesPerProvider: "100",
    financialSampleSize: "100",
    geminiClientAdmissions: "27",
    reason: " reviewed full-inventory Stage 8 peak window ",
    ...overrides,
  };
}

function control(
  counts: Partial<PricingStage8CaptureControlV2["counts_by_status"]> = {},
): PricingStage8CaptureControlV2 {
  return {
    database_observed_at: OBSERVED_AT,
    counts_by_status: {
      pending: 0,
      processing: 0,
      retry: 0,
      passed: 0,
      blocked: 0,
      dead: 0,
      ...counts,
    },
    jobs: [],
    artifacts: [],
  };
}

describe("managed Stage 8 capture control", () => {
  it("renders its loading shell without issuing fetch during server render", () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const html = renderToString(createElement(PricingStage8CaptureControl));
    expect(html).toContain("Managed Stage 8 capture");
    expect(html).toContain("loading-grid");
    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("builds the exact numeric wire payload without float or inferred values", () => {
    const result = buildPricingStage8CapturePayloadV2(draft(), OBSERVED_AT);
    expect(result.error).toBeUndefined();
    expect(result.payload).toEqual({
      idempotency_key: "44444444-4444-4444-8444-444444444444",
      target_generation: 41,
      recovery_generation: 42,
      window_start_ts: 1785714900,
      window_end_ts: 1785715190,
      min_samples_per_provider: 100,
      financial_sample_size: 100,
      gemini_client_admissions: 27,
      reason: "reviewed full-inventory Stage 8 peak window",
    });
    expect(pricingStage8CaptureConfirmationPhrase(result.payload!)).toBe("CAPTURE 41->42 1785715190");
  });

  it("fails closed on non-canonical, unsafe or out-of-order capture identities", () => {
    expect(buildPricingStage8CapturePayloadV2(draft({ targetGeneration: "041" }), OBSERVED_AT).error)
      .toContain("ведущих нулей");
    expect(buildPricingStage8CapturePayloadV2(draft({ recoveryGeneration: "41" }), OBSERVED_AT).error)
      .toContain("новее target");
    expect(buildPricingStage8CapturePayloadV2(draft({ financialSampleSize: "1001" }), OBSERVED_AT).error)
      .toContain("1…1000");
    expect(buildPricingStage8CapturePayloadV2(draft({ geminiClientAdmissions: "9007199254740992" }), OBSERVED_AT).error)
      .toContain(String(Number.MAX_SAFE_INTEGER));
    expect(buildPricingStage8CapturePayloadV2(draft({ windowStartTs: "1785715190" }), OBSERVED_AT).error)
      .toContain("непустым");
  });

  it("uses commerce database time and never permits an unclosed window", () => {
    expect(buildPricingStage8CapturePayloadV2(
      draft({ windowEndTs: "1785715201" }),
      OBSERVED_AT,
    ).error).toContain("ещё не закрыт");
    expect(buildPricingStage8CapturePayloadV2(draft(), "unavailable").error)
      .toContain("database time недоступно");
  });

  it("blocks a second staging action while any durable capture remains active", () => {
    expect(pricingStage8CaptureActiveCount(control())).toBe(0);
    expect(pricingStage8CaptureActiveCount(control({ pending: 1, processing: 2, retry: 3, blocked: 9 })))
      .toBe(6);
  });
});
