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

  it("does not register an account when post-cutover release provisioning is incomplete", async () => {
    const digest = (seed: string): string => `sha256:v2:${seed.repeat(64)}`;
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
      handle: "crm-parsing",
    }]);
    const release = {
      generation: 10,
      release_kind: "target" as const,
      schema_version: 2 as const,
      capability_generation: 3,
      capability_digest: digest("a"),
      main_catalog_generation: 3,
      main_catalog_digest: digest("b"),
      openkeys_catalog_generation: 3,
      openkeys_catalog_digest: digest("c"),
      switch_generation: 3,
      switch_digest: digest("d"),
      inventory_digest: digest("e"),
      funding_manifest_digest: digest("f"),
      minimum_runtime_schema_version: 2,
      content_digest: digest("1"),
    };
    const getPricingReleaseProvisioningContextV2 = vi.fn().mockResolvedValue({
      head: {
        active_generation: 10,
        active_digest: release.content_digest,
        head_version: 1,
        updated_ts: 1,
      },
      activation: {
        activation_id: "1",
        activation_kind: "cutover",
        evidence_digest: digest("2"),
        activated_ts: 1,
      },
      active_release: release,
      paired_recovery: {
        release: { ...release, generation: 11, release_kind: "recovery", content_digest: digest("3") },
        recovery_link: {
          target_generation: 10,
          target_digest: release.content_digest,
          recovery_generation: 11,
          recovery_digest: digest("3"),
          link_digest: digest("4"),
        },
      },
    });
    const getPricingReleaseV2 = vi.fn().mockResolvedValue(null);
    const inventoryQuery = vi.fn().mockResolvedValue({ rows: [] });
    const releaseClient = vi.fn();
    const database = {
      pool: {
        connect: vi.fn(async () => ({ query: inventoryQuery, release: releaseClient })),
      },
    } as unknown as Database;
    const service = new AdminService(
      database,
      {
        getPricingReleaseInventoryV2,
        getAccounts,
        getPricingReleaseProvisioningContextV2,
        getPricingReleaseV2,
      } as unknown as EngineClient,
      {} as ConfigService<Environment, true>,
    );

    await expect(service.upsertServiceAccountInventoryV2("service", mutation, "operator"))
      .rejects.toMatchObject({ code: "pricing_release_not_ready" });
    expect(inventoryQuery).toHaveBeenCalledOnce();
    expect(releaseClient).toHaveBeenCalledOnce();
  });
});
