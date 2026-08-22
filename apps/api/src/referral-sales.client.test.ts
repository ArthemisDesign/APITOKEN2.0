import { ServiceUnavailableException } from "@nestjs/common";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { z } from "zod";
import { ReferralSalesClient, ReferralSalesError } from "./referral-sales.client.js";

const SALES_URL = "https://sales.example.test";
const SALES_KEY = "s".repeat(32);
const responseSchema = z.object({ ok: z.literal(true) }).strict();

function config(overrides: Record<string, unknown> = {}) {
  const values: Record<string, unknown> = {
    SALES_API_URL: SALES_URL,
    SALES_CONTROL_KEY: SALES_KEY,
    ...overrides,
  };
  return { get: vi.fn((key: string) => values[key]) } as never;
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("Referral Sales client boundary", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("sends mutations once with the protected internal URL, credentials, actor, and idempotency key", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse({ ok: true }));
    const timeoutSignal = new AbortController().signal;
    const timeout = vi.spyOn(AbortSignal, "timeout").mockReturnValue(timeoutSignal);
    const client = new ReferralSalesClient(config());

    await expect(client.call("partner/user%40example.test/requests/commission", responseSchema, {
      method: "POST",
      body: { requestedCommissionBps: 1_500 },
      idempotencyKey: "request-0001",
      actor: "operator@example.test",
    })).resolves.toEqual({ ok: true });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(timeout).toHaveBeenCalledOnce();
    expect(timeout).toHaveBeenCalledWith(6_000);
    const [url, request] = fetchMock.mock.calls[0]!;
    expect(String(url)).toBe(
      "https://sales.example.test/v1/internal/referral/partner/user%40example.test/requests/commission",
    );
    expect(request?.method).toBe("POST");
    expect(request?.signal).toBe(timeoutSignal);
    expect(request?.body).toBe(JSON.stringify({ requestedCommissionBps: 1_500 }));
    const headers = new Headers(request?.headers);
    expect(headers.get("x-api-key")).toBe(SALES_KEY);
    expect(headers.get("content-type")).toBe("application/json");
    expect(headers.get("idempotency-key")).toBe("request-0001");
    expect(headers.get("x-admin-actor")).toBe("operator@example.test");
  });

  it("does not retry a failed mutation", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("offline"));
    const client = new ReferralSalesClient(config());

    await expect(client.call("partner/account/wallet", responseSchema, {
      method: "PATCH",
      body: { address: "0x0000000000000000000000000000000000000000" },
    })).rejects.toBeInstanceOf(ServiceUnavailableException);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("fails closed when Sales returns malformed success data", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse({ ok: true, unexpected: "field" }));
    const client = new ReferralSalesClient(config());

    await expect(client.call("partner/account", responseSchema)).rejects.toMatchObject({ status: 503 });
  });

  it.each([400, 403, 404, 409, 422, 429])("preserves actionable Sales status %s", async (status) => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse({ message: "action required" }, status));
    const client = new ReferralSalesClient(config());

    const error = await client.call("partner/account", responseSchema).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ReferralSalesError);
    expect(error).toMatchObject({ status, salesStatus: status });
  });

  it.each([401, 500, 502])("does not expose Sales status %s as a Commerce auth or server error", async (status) => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse({ message: "upstream details" }, status));
    const client = new ReferralSalesClient(config());

    await expect(client.call("partner/account", responseSchema)).rejects.toMatchObject({
      status: 503,
      salesStatus: status,
      message: "partner program is temporarily unavailable",
    });
  });

  it("fails before network access when the internal Sales pair is incomplete", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    const client = new ReferralSalesClient(config({ SALES_CONTROL_KEY: undefined }));

    await expect(client.call("partner/account", responseSchema)).rejects.toBeInstanceOf(
      ServiceUnavailableException,
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
