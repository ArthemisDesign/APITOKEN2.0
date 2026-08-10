import { describe, expect, it } from "vitest";
import type { AccountView, ApiKeyView, AuthUser, CheckoutView, LedgerEntry, UsageView } from "./api";
import { previewRequest } from "./preview-fixtures";

describe("web/v2 preview fixtures", () => {
  it("serves an authenticated user without any network", async () => {
    const { user } = await previewRequest<{ user: AuthUser }>("/auth/me");
    expect(user.email).toBe("v2.preview@apitoken.sale");
    expect(user.engineAccountStatus).toBe("active");
  });

  it("serves account, ledger and usage shaped like the real backend", async () => {
    const account = await previewRequest<AccountView>("/account");
    expect(account.pricing?.customerType).toBe("b2c");
    expect(account.pricing?.discountPercent).toBe(50);
    expect(account.markupBasisPoints).toBe(5000);
    expect(BigInt(account.balanceNano)).toBeGreaterThan(0n);
    const { entries } = await previewRequest<{ entries: LedgerEntry[] }>("/account/ledger?limit=50");
    expect(entries.some((entry) => entry.kind === "topup")).toBe(true);
    expect(entries[0]!.amountUsd.startsWith("$")).toBe(true);
    const usage = await previewRequest<UsageView>("/account/usage?window=7d");
    expect(usage.window).toBe("7d");
    expect(usage.models.length).toBeGreaterThan(0);
    expect(usage.models.every((model) => typeof model.provider === "string")).toBe(true);
    expect(usage.dailyProviders?.length).toBeGreaterThan(0);
  });

  it("keeps API-key mutations stateful: create → rename → revoke", async () => {
    const created = await previewRequest<ApiKeyView>("/api-keys", {
      method: "POST", body: JSON.stringify({ label: "From test", spendLimitUsd: "5" }),
    });
    expect(created.key).toMatch(/^sk-pool-preview-/);
    expect(created.spendLimitNano).toBe("5000000000");

    const renamed = await previewRequest<ApiKeyView>(`/api-keys/${created.id}`, {
      method: "PATCH", body: JSON.stringify({ label: "Renamed" }),
    });
    expect(renamed.label).toBe("Renamed");

    await previewRequest<void>(`/api-keys/${created.id}`, { method: "DELETE" });
    const { keys } = await previewRequest<{ keys: ApiKeyView[] }>("/api-keys");
    expect(keys.find((key) => key.id === created.id)).toBeUndefined();
  });

  it("pays a checkout on the second status poll and credits the balance", async () => {
    const before = await previewRequest<AccountView>("/account");
    const checkout = await previewRequest<CheckoutView>("/checkouts", {
      method: "POST", body: JSON.stringify({ amountUsd: "25", provider: "platega" }),
    });
    expect(checkout.status).toBe("pending");
    await previewRequest<CheckoutView>(`/checkouts/${checkout.id}`);
    const paid = await previewRequest<CheckoutView>(`/checkouts/${checkout.id}`);
    expect(paid.status).toBe("paid");
    const after = await previewRequest<AccountView>("/account");
    expect(BigInt(after.balanceNano) - BigInt(before.balanceNano)).toBe(25_000_000_000n);
  });

  it("rejects unknown routes with a 404 ApiError", async () => {
    await expect(previewRequest("/nope")).rejects.toMatchObject({ status: 404 });
  });
});
