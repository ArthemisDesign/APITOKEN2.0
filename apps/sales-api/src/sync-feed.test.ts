import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { z } from "zod";
import {
  parseCanonicalFeedPage,
  parseFeedPage,
  paymentReversalSchema,
  SyncService,
  topupV2Schema,
  usageEventSchema,
} from "./sync.service.js";
import {
  advanceSyncCursor,
  getSyncCursor,
  recordReferredDeposit,
  recordReferredSpend,
  recordReferredSpendV2,
  recordPaidFundingLot,
  recordPaymentReversalPage,
  hasIncompletePartnerFundingEvidence,
  reconcilePartnerFundingEvidence,
} from "@claude-api/sales-db";

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...actual,
    getSyncCursor: vi.fn(async () => 0n),
    advanceSyncCursor: vi.fn(async () => undefined),
    recordReferredDeposit: vi.fn(async () => "recorded"),
    recordReferredSpend: vi.fn(async () => "recorded"),
    recordReferredSpendV2: vi.fn(async () => "recorded"),
    recordPaidFundingLot: vi.fn(async () => "recorded"),
    recordPaymentReversalPage: vi.fn(async () => undefined),
    hasIncompletePartnerFundingEvidence: vi.fn(async () => false),
    reconcilePartnerFundingEvidence: vi.fn(async () => ({ examined: 0, completed: 0 })),
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

  it("accepts the PostgreSQL bigint maximum and rejects overflow or non-canonical cursors", () => {
    expect(parseFeedPage({ items: [], nextCursor: "9223372036854775807" }, rowSchema, "usage_events"))
      .toEqual({ items: [], nextCursor: 9_223_372_036_854_775_807n });
    expect(() => parseFeedPage(
      { items: [], nextCursor: "9223372036854775808" },
      rowSchema,
      "usage_events",
    )).toThrow();
    expect(() => parseFeedPage(
      { items: [], nextCursor: "01" },
      rowSchema,
      "usage_events",
    )).toThrow();
    expect(() => parseFeedPage(
      { items: [], nextCursor: Number.MAX_SAFE_INTEGER + 1 },
      rowSchema,
      "usage_events",
    )).toThrow();
  });
});

describe("commit-ordered topups-v2 feed", () => {
  const golden = JSON.parse(readFileSync(
    new URL("../../../tests/contracts/sales-topups-v2-feed.golden.json", import.meta.url),
    "utf8",
  )) as { row: unknown; nextCursor: string };

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

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("parses the same golden row as the deployed Commerce producer", () => {
    expect(topupV2Schema.parse(golden.row)).toMatchObject({
      id: 12n,
      amountNano: 9_007_199_254_740_993n,
    });
    expect(parseCanonicalFeedPage({ items: [golden.row], nextCursor: golden.nextCursor }, topupV2Schema, "topups_v2"))
      .toMatchObject({ nextCursor: 13n });
  });

  it("rejects legacy pages and non-canonical identifiers, money, or timestamps", () => {
    expect(() => parseCanonicalFeedPage([golden.row], topupV2Schema, "topups_v2"))
      .toThrow("unexpected body shape");
    expect(() => parseCanonicalFeedPage({ items: [golden.row] }, topupV2Schema, "topups_v2"))
      .toThrow("unexpected body shape");
    expect(() => topupV2Schema.parse({ ...golden.row as object, id: 12 })).toThrow();
    expect(() => topupV2Schema.parse({ ...golden.row as object, id: "0" }))
      .toThrow("topup id must be positive");
    expect(() => topupV2Schema.parse({ ...golden.row as object, id: "012" })).toThrow();
    expect(() => topupV2Schema.parse({ ...golden.row as object, amountNano: "0" }))
      .toThrow("topup amount must be positive");
    expect(() => topupV2Schema.parse({ ...golden.row as object, paymentId: "not-a-uuid" }))
      .toThrow();
    expect(() => topupV2Schema.parse({ ...golden.row as object, paidAt: 1_786_464_896_789 }))
      .toThrow();
  });

  it("rejects replayed, duplicated, out-of-order, or regressing sequence pages", () => {
    const row = golden.row as Record<string, unknown>;
    expect(() => parseCanonicalFeedPage({
      items: [row],
      nextCursor: golden.nextCursor,
    }, topupV2Schema, "topups_v2", 12n)).toThrow("non-monotonic items");
    expect(() => parseCanonicalFeedPage({
      items: [{ ...row, id: "12" }, { ...row, id: "12" }],
      nextCursor: golden.nextCursor,
    }, topupV2Schema, "topups_v2", 0n)).toThrow("non-monotonic items");
    expect(() => parseCanonicalFeedPage({
      items: [{ ...row, id: "12" }, { ...row, id: "11" }],
      nextCursor: golden.nextCursor,
    }, topupV2Schema, "topups_v2", 0n)).toThrow("non-monotonic items");
    expect(() => parseCanonicalFeedPage({
      items: [],
      nextCursor: "11",
    }, topupV2Schema, "topups_v2", 12n)).toThrow("cursor behind its items");
  });

  it("uses only the new cursor and endpoint, then advances across the full source page", async () => {
    vi.mocked(getSyncCursor).mockResolvedValueOnce(0n);
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      items: [golden.row],
      nextCursor: golden.nextCursor,
    }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    await (service() as never as { syncTopups(): Promise<void> }).syncTopups();

    expect(getSyncCursor).toHaveBeenCalledWith(expect.anything(), "topups_v2");
    expect(fetchMock).toHaveBeenCalledWith(
      expect.objectContaining({ pathname: "/v1/internal/sales/topups-v2", search: "?after_id=0&limit=500" }),
      expect.anything(),
    );
    expect(recordReferredDeposit).toHaveBeenCalledTimes(1);
    expect(recordReferredDeposit).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      amountNano: 9_007_199_254_740_993n,
    }));
    expect(advanceSyncCursor).toHaveBeenCalledWith(expect.anything(), "topups_v2", 13n);
    expect(advanceSyncCursor).not.toHaveBeenCalledWith(expect.anything(), "topups", expect.anything());
  });

  it("advances an empty filtered page and keeps the cursor behind a failed write", async () => {
    vi.mocked(getSyncCursor).mockResolvedValue(0n);
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      items: [],
      nextCursor: "13",
    }), { status: 200 })));
    await (service() as never as { syncTopups(): Promise<void> }).syncTopups();
    expect(advanceSyncCursor).toHaveBeenCalledWith(expect.anything(), "topups_v2", 13n);

    vi.clearAllMocks();
    vi.mocked(getSyncCursor).mockResolvedValue(0n);
    vi.mocked(recordReferredDeposit).mockRejectedValueOnce(new Error("db down"));
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      items: [golden.row],
      nextCursor: golden.nextCursor,
    }), { status: 200 })));
    await expect((service() as never as { syncTopups(): Promise<void> }).syncTopups())
      .rejects.toThrow("db down");
    expect(advanceSyncCursor).not.toHaveBeenCalled();
  });
});

describe("payment reversal feed", () => {
  const golden = JSON.parse(readFileSync(
    new URL("../../../tests/contracts/sales-payment-reversals-feed.golden.json", import.meta.url),
    "utf8",
  )) as { row: Record<string, unknown>; nextCursor: string };

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

  function serveReversalWithCausalPages(input: {
    usageCursor?: string;
    fundingCursor?: string;
  } = {}): ReturnType<typeof vi.fn> {
    return vi.fn(async (request: Parameters<typeof fetch>[0]) => {
      const url = request instanceof URL ? request : new URL(String(request));
      if (url.pathname.endsWith("/payment-reversals")) {
        return new Response(JSON.stringify({
          items: [golden.row],
          nextCursor: golden.nextCursor,
        }), { status: 200 });
      }
      return new Response(JSON.stringify({
        items: [],
        nextCursor: url.pathname.endsWith("/usage-events")
          ? (input.usageCursor ?? "0")
          : (input.fundingCursor ?? "0"),
      }), { status: 200 });
    });
  }

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("parses the exact producer golden and rejects unsafe variants", () => {
    expect(paymentReversalSchema.parse(golden.row)).toMatchObject({
      id: 77n,
      kind: "refund",
      amountNano: 9_007_199_254_740_993n,
    });
    expect(() => paymentReversalSchema.parse({ ...golden.row, id: 77 })).toThrow();
    expect(() => paymentReversalSchema.parse({ ...golden.row, id: "077" })).toThrow();
    expect(() => paymentReversalSchema.parse({ ...golden.row, kind: "cancel" })).toThrow();
    expect(() => paymentReversalSchema.parse({ ...golden.row, amountNano: "0" })).toThrow();
    expect(() => paymentReversalSchema.parse({ ...golden.row, reversedAt: 1_786_464_896_789 })).toThrow();
  });

  it("passes the canonical page to the atomic writer without a separate cursor advance", async () => {
    vi.mocked(getSyncCursor).mockResolvedValueOnce(0n);
    vi.stubGlobal("fetch", serveReversalWithCausalPages());

    await (service() as never as { syncPaymentReversals(): Promise<void> }).syncPaymentReversals();

    expect(recordPaymentReversalPage).toHaveBeenCalledWith(expect.anything(), [expect.objectContaining({
      commerceReversalId: 77n,
      amountNano: 9_007_199_254_740_993n,
    })], 78n);
    expect(advanceSyncCursor).not.toHaveBeenCalledWith(
      expect.anything(),
      "payment_reversals",
      expect.anything(),
    );
  });

  it("keeps the reversal cursor behind while funding evidence is incomplete", async () => {
    vi.mocked(getSyncCursor).mockResolvedValueOnce(0n);
    vi.mocked(hasIncompletePartnerFundingEvidence).mockResolvedValueOnce(true);
    vi.stubGlobal("fetch", serveReversalWithCausalPages());

    await (service() as never as { syncPaymentReversals(): Promise<void> }).syncPaymentReversals();

    expect(recordPaymentReversalPage).not.toHaveBeenCalled();
    expect(advanceSyncCursor).not.toHaveBeenCalled();
  });
});

describe("immutable usage attribution parser", () => {
  const base = {
    id: "17",
    userId: "00000000-0000-4000-8000-000000000017",
    amountNano: "600",
    occurredAt: "2026-08-01T12:00:00.000Z",
  };

  it("rejects unsafe numeric ids and out-of-range or non-canonical money strings", () => {
    expect(usageEventSchema.safeParse({ ...base, id: "not-an-id" }).success).toBe(false);
    expect(() => usageEventSchema.parse({ ...base, id: Number.MAX_SAFE_INTEGER + 1 })).toThrow();
    expect(() => usageEventSchema.parse({ ...base, id: "9223372036854775808" })).toThrow();
    expect(() => usageEventSchema.parse({ ...base, id: "017" })).toThrow();
    expect(() => usageEventSchema.parse({ ...base, amountNano: "9223372036854775808" })).toThrow();
    expect(() => usageEventSchema.parse({ ...base, amountNano: "0600" })).toThrow();
    expect(usageEventSchema.parse({ ...base, id: Number.MAX_SAFE_INTEGER }).id)
      .toBe(BigInt(Number.MAX_SAFE_INTEGER));
  });

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
