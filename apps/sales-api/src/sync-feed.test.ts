import { describe, expect, it } from "vitest";
import { z } from "zod";
import { parseFeedPage, usageEventSchema } from "./sync.service.js";

const rowSchema = z.object({
  id: z.string().regex(/^\d+$/).transform(BigInt),
  value: z.string(),
});

describe("sales feed page cursor", () => {
  it("advances across a canonical page containing only filtered source rows", () => {
    expect(parseFeedPage({ items: [], nextCursor: "42" }, rowSchema, "usage_events"))
      .toEqual({ items: [], nextCursor: 42n });
  });

  it("keeps compatibility with the legacy array response during a rolling deploy", () => {
    expect(parseFeedPage([
      { id: "7", value: "first" },
      { id: "9", value: "second" },
    ], rowSchema, "usage_events")).toEqual({
      items: [
        { id: 7n, value: "first" },
        { id: 9n, value: "second" },
      ],
      nextCursor: 9n,
    });
  });

  it("rejects a producer cursor that would skip a returned item", () => {
    expect(() => parseFeedPage({
      items: [{ id: "10", value: "event" }],
      nextCursor: "9",
    }, rowSchema, "usage_events")).toThrow("cursor behind its items");
  });
});

describe("immutable usage attribution parser", () => {
  const base = {
    id: "17",
    userId: "00000000-0000-4000-8000-000000000017",
    amountNano: "600",
    occurredAt: "2026-08-01T12:00:00.000Z",
  };

  it("normalizes an old producer payload to the all-null legacy shape", () => {
    expect(usageEventSchema.parse(base)).toMatchObject({
      id: 17n,
      amountNano: 600n,
      providerId: null,
      accountClass: null,
      pricingMode: null,
      paidFundedNano: null,
      commissionEligible: null,
      snapshotDigest: null,
    });
  });

  it("parses and preserves complete B2C track paid authority", () => {
    expect(usageEventSchema.parse({
      ...base,
      providerId: "anthropic",
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: "600",
      commissionEligible: true,
      snapshotDigest: "snapshot-17",
    })).toMatchObject({
      providerId: "anthropic",
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: 600n,
      commissionEligible: true,
      snapshotDigest: "snapshot-17",
    });
  });

  it("rejects partial, ineligible, and amount-divergent attributed payloads", () => {
    expect(() => usageEventSchema.parse({ ...base, providerId: "anthropic" }))
      .toThrow("usage attribution must be entirely null or complete");
    expect(() => usageEventSchema.parse({
      ...base,
      providerId: "anthropic",
      accountClass: "service",
      pricingMode: "track",
      paidFundedNano: "600",
      commissionEligible: true,
      snapshotDigest: "snapshot-service",
    })).toThrow();
    expect(() => usageEventSchema.parse({
      ...base,
      providerId: "anthropic",
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: "599",
      commissionEligible: true,
      snapshotDigest: "snapshot-mismatch",
    })).toThrow("usage amount must equal positive attributed paid funding");
  });
});
