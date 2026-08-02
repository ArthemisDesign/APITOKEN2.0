import { randomUUID } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  buildOpenKeysPricingInventoryPageV2,
  type OpenKeysPricingInventorySourceRowV2,
} from "./pricing-inventory";

function row(
  accountId: string,
  overrides: Partial<OpenKeysPricingInventorySourceRowV2> = {},
): OpenKeysPricingInventorySourceRowV2 {
  return {
    sourceId: randomUUID(),
    engineAccountId: accountId,
    status: "active",
    removed: false,
    pricingContract: "official_1_to_1",
    sourceMultiplierBp: 10_000,
    ...overrides,
  };
}

describe("OpenKeys pricing inventory v2", () => {
  it("paginates every account in byte order with one stable full-manifest digest", () => {
    const source = [
      row("acct_c", { status: "disabled" }),
      row("acct_a", { pricingContract: "legacy", sourceMultiplierBp: 4000 }),
      row("acct_b", { removed: true, status: "disabled" }),
    ];
    const first = buildOpenKeysPricingInventoryPageV2(source, { limit: 2 });
    const second = buildOpenKeysPricingInventoryPageV2(source, {
      afterAccountId: first.next_after_account_id!,
      limit: 2,
    });

    expect(first.accounts.map((account) => account.account_id)).toEqual(["acct_a", "acct_b"]);
    expect(first.accounts.map((account) => account.lifecycle)).toEqual(["active", "removed"]);
    expect(first.next_after_account_id).toBe("acct_b");
    expect(second.accounts.map((account) => account.account_id)).toEqual(["acct_c"]);
    expect(second.next_after_account_id).toBeNull();
    expect(second.inventory_digest).toBe(first.inventory_digest);
    expect(first.inventory_digest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
  });

  it("changes identities on source pricing drift and rejects duplicate engine ownership", () => {
    const original = row("acct_exact", { pricingContract: "legacy", sourceMultiplierBp: 4000 });
    const before = buildOpenKeysPricingInventoryPageV2([original]);
    const after = buildOpenKeysPricingInventoryPageV2([{
      ...original,
      pricingContract: "official_1_to_1",
      sourceMultiplierBp: 10_000,
    }]);
    expect(after.inventory_digest).not.toBe(before.inventory_digest);
    expect(after.accounts[0]!.content_digest).not.toBe(before.accounts[0]!.content_digest);
    expect(() => buildOpenKeysPricingInventoryPageV2([
      original,
      row("acct_exact"),
    ])).toThrow(/duplicate OpenKeys engine account/);
  });
});
