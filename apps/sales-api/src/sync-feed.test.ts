import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { z } from "zod";
import { parseFeedPage, SyncService, usageEventSchema } from "./sync.service.js";
import {
  advanceSyncCursor,
  recordReferredSpend,
  recordReferredSpendV2,
} from "@claude-api/sales-db";

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...actual,
    getSyncCursor: vi.fn(async () => 0n),
    advanceSyncCursor: vi.fn(async () => undefined),
    recordReferredSpend: vi.fn(async () => "recorded"),
    recordReferredSpendV2: vi.fn(async () => "recorded"),
  };
});

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

  it("accepts the live scalar row: provider set, attribution absent", () => {
    // Регрессия 2026-08-10: producer после отката прайсинга шлёт providerId + amountNano
    // (= real_funded_nano), а attribution больше не существует. Consumer отверг такую страницу
    // и продакшн-синк комиссий встал на пять часов. Эта форма обязана проходить.
    const row = usageEventSchema.parse({ ...base, providerId: "anthropic" });
    expect(row).toMatchObject({
      amountNano: 600n,
      providerId: "anthropic",
      accountClass: null,
      paidFundedNano: null,
      form: "legacy",
    });
  });

  it("accepts the exact wire row the commerce feed serializes", () => {
    // Тот же файл читает продюсерский тест apps/api/src/sales-feed.controller.test.ts.
    // Раздельно-зелёные сюиты не ловят несовместимость — общий golden ловит.
    const golden = JSON.parse(readFileSync(
      new URL("../../../tests/contracts/sales-usage-feed.golden.json", import.meta.url),
      "utf8",
    )) as { row: unknown };
    expect(usageEventSchema.parse(golden.row)).toMatchObject({
      providerId: "anthropic",
      amountNano: 9_007_199_254_740_993n,
      form: "legacy",
    });
  });

  it("rejects partial, ineligible, and amount-divergent attributed payloads", () => {
    expect(() => usageEventSchema.parse({ ...base, providerId: "anthropic", accountClass: "b2c" }))
      .toThrow("usage attribution must be entirely null or complete");
    expect(() => usageEventSchema.parse({
      ...base,
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: "600",
      commissionEligible: true,
      snapshotDigest: "snapshot-17",
    })).toThrow("attributed usage must carry its provider");
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

  it("classifies the legacy and v1 forms", () => {
    expect(usageEventSchema.parse(base).form).toBe("legacy");
    expect(usageEventSchema.parse({
      ...base,
      providerId: "anthropic",
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: "600",
      commissionEligible: true,
      snapshotDigest: "snapshot-17",
    }).form).toBe("v1");
  });
});

describe("release-v2 usage event parser", () => {
  const v2 = {
    id: "18",
    userId: "00000000-0000-4000-8000-000000000018",
    amountNano: "10000",
    providerId: "google",
    accountClass: "b2c",
    pricingMode: null,
    paidFundedNano: "10000",
    commissionEligible: true,
    snapshotDigest: "snapshot-18",
    officialNano: "20000",
    chargedNano: "12000",
    bonusFundedNano: "2000",
    otherFundedNano: "0",
    releaseGeneration: "7",
    releaseDigest: "release-7",
    occurredAt: "2026-08-01T12:00:00.000Z",
  };

  it("parses a complete release-v2 row and classifies it as v2", () => {
    const parsed = usageEventSchema.parse({ ...v2, futureExpandOnlyField: "ignored" });
    expect(parsed).toMatchObject({
      form: "v2",
      id: 18n,
      amountNano: 10_000n,
      paidFundedNano: 10_000n,
      officialNano: 20_000n,
      chargedNano: 12_000n,
      bonusFundedNano: 2_000n,
      otherFundedNano: 0n,
      releaseGeneration: 7n,
      releaseDigest: "release-7",
    });
  });

  it("fails closed on an incomplete v2 lineage instead of falling back to v1", () => {
    const { releaseDigest, ...missingDigest } = v2;
    expect(() => usageEventSchema.parse(missingDigest))
      .toThrow("release-v2 usage lineage must be entirely null or complete");
    expect(() => usageEventSchema.parse({ ...v2, officialNano: null }))
      .toThrow("release-v2 usage lineage must be entirely null or complete");
  });

  it("rejects a v2 row dressed as track and a divergent funding split", () => {
    expect(() => usageEventSchema.parse({ ...v2, pricingMode: "track" }))
      .toThrow("release-v2 lineage is incompatible with track pricing mode");
    expect(() => usageEventSchema.parse({ ...v2, chargedNano: "12001" }))
      .toThrow("release-v2 funding buckets must sum to the charged amount");
    expect(() => usageEventSchema.parse({ ...v2, amountNano: "9999" }))
      .toThrow("usage amount must equal positive attributed paid funding");
    expect(() => usageEventSchema.parse({ ...v2, paidFundedNano: "0", amountNano: "0", chargedNano: "2000" }))
      .toThrow("usage amount must equal positive attributed paid funding");
  });
});

describe("usage feed routing (dual consumer)", () => {
  const v1Row = {
    id: "17",
    userId: "00000000-0000-4000-8000-000000000017",
    amountNano: "600",
    providerId: "anthropic",
    accountClass: "b2c",
    pricingMode: "track",
    paidFundedNano: "600",
    commissionEligible: true,
    snapshotDigest: "snapshot-17",
    officialNano: null,
    chargedNano: null,
    bonusFundedNano: null,
    otherFundedNano: null,
    releaseGeneration: null,
    releaseDigest: null,
    occurredAt: "2026-08-01T12:00:00.000Z",
  };
  const v2Row = {
    id: "18",
    userId: "00000000-0000-4000-8000-000000000018",
    amountNano: "10000",
    providerId: "google",
    accountClass: "b2c",
    pricingMode: null,
    paidFundedNano: "10000",
    commissionEligible: true,
    snapshotDigest: "snapshot-18",
    officialNano: "20000",
    chargedNano: "12000",
    bonusFundedNano: "2000",
    otherFundedNano: "0",
    releaseGeneration: "7",
    releaseDigest: "release-7",
    occurredAt: "2026-08-01T12:00:00.000Z",
  };

  function service(): SyncService {
    const config = {
      get: (key: string) => ({
        COMMERCE_BASE_URL: "http://127.0.0.1:8791",
        SALES_CONTROL_KEY: "test-key",
        SYNC_INTERVAL_MS: 60_000,
      })[key],
    };
    return new SyncService({ pool: {} } as never, config as never);
  }

  function stubFeed(body: unknown, status = 200): void {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(body), { status })));
  }

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("routes each event to exactly one writer and advances the cursor", async () => {
    stubFeed({ items: [v1Row, v2Row], nextCursor: "18" });
    const sync = service();
    await (sync as never as { syncUsageEvents(): Promise<void> }).syncUsageEvents();

    expect(vi.mocked(recordReferredSpendV2)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(recordReferredSpendV2)).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      commerceEventId: 18n,
      paidFundedNano: 10_000n,
      bonusFundedNano: 2_000n,
      releaseGeneration: 7n,
    }));
    expect(vi.mocked(recordReferredSpend)).toHaveBeenCalledTimes(1);
    expect(vi.mocked(recordReferredSpend)).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      commerceEventId: 17n,
      amountNano: 600n,
    }));
    expect(vi.mocked(advanceSyncCursor)).toHaveBeenCalledWith(expect.anything(), "usage_events", 18n);
  });

  it("keeps the cursor behind when a writer fails (at-least-once)", async () => {
    stubFeed({ items: [v1Row, v2Row], nextCursor: "18" });
    vi.mocked(recordReferredSpendV2).mockRejectedValueOnce(new Error("db down"));
    const sync = service();
    await expect((sync as never as { syncUsageEvents(): Promise<void> }).syncUsageEvents())
      .rejects.toThrow("db down");
    expect(vi.mocked(advanceSyncCursor)).not.toHaveBeenCalled();
  });

  it("treats a 404 feed as a pending deploy, not an error", async () => {
    stubFeed({}, 404);
    const sync = service();
    await expect((sync as never as { syncUsageEvents(): Promise<void> }).syncUsageEvents())
      .resolves.toBeUndefined();
    expect(vi.mocked(recordReferredSpend)).not.toHaveBeenCalled();
    expect(vi.mocked(recordReferredSpendV2)).not.toHaveBeenCalled();
    expect(vi.mocked(advanceSyncCursor)).not.toHaveBeenCalled();
  });
});
