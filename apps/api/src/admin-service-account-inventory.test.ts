import type { ConfigService } from "@nestjs/config";
import type { Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import { describe, expect, it, vi } from "vitest";
import {
  AdminService,
  AdminServiceAccountInventoryError,
} from "./admin.service.js";
import type { Environment } from "./config.js";

const inventoryAccount = (accountId: string) => ({
  account_id: accountId,
  status: "active" as const,
  multiplier_bp: 10_000,
  balance_nano: "0",
  reserved_nano: "0",
  spent_nano: "0",
  funding_generation: null,
  funding_head_version: null,
});

function serviceWithEngine(engine: Partial<EngineClient>): AdminService {
  return new AdminService(
    {} as Database,
    engine as EngineClient,
    {} as ConfigService<Environment, true>,
  );
}

const mutation = {
  expected_source_version: null,
  expected_content_digest: null,
  engine_account_id: "acct_service",
  purpose: "internal workload",
  responsible: "platform",
  reason: "register the engine-native service account",
};

describe("service-account inventory engine validation", () => {
  it("rejects an account absent from two stable exhaustive engine scans", async () => {
    const getPricingReleaseInventoryV2 = vi.fn().mockResolvedValue({
      accounts: [],
      next_after_account_id: null,
    });
    const getAccounts = vi.fn();
    const service = serviceWithEngine({ getPricingReleaseInventoryV2, getAccounts } as Partial<EngineClient>);

    await expect(service.upsertServiceAccountInventoryV2("service", mutation, "operator"))
      .rejects.toEqual(expect.objectContaining({
        code: "engine_account_missing",
      } satisfies Partial<AdminServiceAccountInventoryError>));
    expect(getPricingReleaseInventoryV2).toHaveBeenCalledTimes(2);
    expect(getAccounts).not.toHaveBeenCalled();
  });

  it("fails closed when the engine identity inventory changes between scans", async () => {
    const getPricingReleaseInventoryV2 = vi.fn()
      .mockResolvedValueOnce({ accounts: [inventoryAccount("acct_service")], next_after_account_id: null })
      .mockResolvedValueOnce({ accounts: [inventoryAccount("acct_other")], next_after_account_id: null });
    const service = serviceWithEngine({ getPricingReleaseInventoryV2 } as Partial<EngineClient>);

    await expect(service.upsertServiceAccountInventoryV2("service", mutation, "operator"))
      .rejects.toMatchObject({ code: "engine_inventory_unstable" });
  });

  it("never admits an OpenKeys handle into meter-only service authority", async () => {
    const getPricingReleaseInventoryV2 = vi.fn().mockResolvedValue({
      accounts: [inventoryAccount("acct_service")],
      next_after_account_id: null,
    });
    const getAccounts = vi.fn().mockResolvedValue([{
      account: "acct_service",
      balance_nano: "0",
      spent_nano: "0",
      reserved_nano: "0",
      balance: "0.000000000",
      mult_bp: 10_000,
      status: "active",
      handle: "openkeys-existing-key",
    }]);
    const service = serviceWithEngine({ getPricingReleaseInventoryV2, getAccounts } as Partial<EngineClient>);

    await expect(service.upsertServiceAccountInventoryV2("service", mutation, "operator"))
      .rejects.toMatchObject({ code: "account_owned_by_openkeys" });
    expect(getPricingReleaseInventoryV2).toHaveBeenCalledTimes(2);
    expect(getAccounts).toHaveBeenCalledWith(["acct_service"]);
  });

  it("rejects a status change after the stable inventory snapshot", async () => {
    const getPricingReleaseInventoryV2 = vi.fn().mockResolvedValue({
      accounts: [inventoryAccount("acct_service")],
      next_after_account_id: null,
    });
    const getAccounts = vi.fn().mockResolvedValue([{
      account: "acct_service",
      balance_nano: "0",
      spent_nano: "0",
      reserved_nano: "0",
      balance: "0.000000000",
      mult_bp: 10_000,
      status: "disabled",
      handle: "crm-parsing",
    }]);
    const service = serviceWithEngine({ getPricingReleaseInventoryV2, getAccounts } as Partial<EngineClient>);

    await expect(service.upsertServiceAccountInventoryV2("service", mutation, "operator"))
      .rejects.toMatchObject({ code: "engine_inventory_unstable" });
  });
});
