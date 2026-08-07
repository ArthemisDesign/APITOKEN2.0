import { BadRequestException, HttpException, NotFoundException } from "@nestjs/common";
import {
  PricingPolicyDeliveryRepairError,
  PricingPolicyWriteError,
  ServiceAccountInventoryV2Error,
} from "@claude-api/db";
import { describe, expect, it, vi } from "vitest";
import { AdminController } from "./admin.controller.js";
import { AdminServiceAccountInventoryError, type AdminService } from "./admin.service.js";

describe("admin user list HTTP contract", () => {
  it("passes bounded pagination and filters to the service", async () => {
    const listUsers = vi.fn().mockResolvedValue({ users: [], total: 0, limit: 25, offset: 50 });
    const controller = new AdminController({ listUsers } as unknown as AdminService);

    await expect(controller.listUsers("25", "50", "alice", "active", "google", "b2b"))
      .resolves.toMatchObject({ total: 0 });
    expect(listUsers).toHaveBeenCalledWith({
      limit: 25,
      offset: 50,
      sort: "created_at",
      dir: "desc",
      search: "alice",
      status: "active",
      auth: "google",
      customerType: "b2b",
    });
  });

  it("passes a whitelisted sort and direction to the service", async () => {
    const listUsers = vi.fn().mockResolvedValue({ users: [], total: 0, limit: 50, offset: 0 });
    const controller = new AdminController({ listUsers } as unknown as AdminService);

    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "paid_total", "asc"))
      .resolves.toMatchObject({ total: 0 });
    expect(listUsers).toHaveBeenCalledWith({ limit: 50, offset: 0, sort: "paid_total", dir: "asc" });
  });

  it("rejects unbounded or unknown filters and non-whitelisted sorts", async () => {
    const listUsers = vi.fn();
    const controller = new AdminController({ listUsers } as unknown as AdminService);
    await expect(controller.listUsers("500", "0")).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.listUsers("50", "0", "", "unknown")).rejects.toBeInstanceOf(BadRequestException);
    // sort интерполируется в ORDER BY — принимаются только значения белого списка.
    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "balance_usd"))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "created_at; DROP TABLE users;--"))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.listUsers(undefined, undefined, undefined, undefined, undefined, undefined, "spent_30d", "sideways"))
      .rejects.toBeInstanceOf(BadRequestException);
    expect(listUsers).not.toHaveBeenCalled();
  });

  it("accepts copy-only invitations and forwards the verified operator identity", async () => {
    const createBusinessInvite = vi.fn().mockResolvedValue({
      id: "invite-id",
      email: null,
      deliveryStatus: "copy_only",
    });
    const controller = new AdminController({ createBusinessInvite } as unknown as AdminService);
    const body = {
      discountPercent: 75,
      expiresInDays: 7,
      reason: "negotiated business terms",
      idempotencyKey: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    };

    await expect(controller.createBusinessInvite(body, "owner@example.com"))
      .resolves.toMatchObject({ deliveryStatus: "copy_only" });
    expect(createBusinessInvite).toHaveBeenCalledWith({
      ...body,
      actorId: "owner@example.com",
    });
  });

  it("accepts a full provider/model invitation policy without a scalar discount", async () => {
    const createBusinessInvite = vi.fn().mockResolvedValue({ id: "invite-id", policy: { currentVersion: 1 } });
    const controller = new AdminController({ createBusinessInvite } as unknown as AdminService);
    const body = {
      policy: {
        rules: [{
          scope: { provider: { providerId: "anthropic" } },
          pricingMode: "discount",
          discountBps: 6_000,
        }],
      },
      expiresInDays: 7,
      reason: "negotiated provider policy",
      idempotencyKey: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    };

    await expect(controller.createBusinessInvite(body, "owner@example.com"))
      .resolves.toMatchObject({ policy: { currentVersion: 1 } });
    expect(createBusinessInvite).toHaveBeenCalledWith({ ...body, actorId: "owner@example.com" });
  });

  it("rejects ambiguous mutations that combine full policy and scalar compatibility fields", async () => {
    const createBusinessInvite = vi.fn();
    const setBusinessPricing = vi.fn();
    const controller = new AdminController({ createBusinessInvite, setBusinessPricing } as unknown as AdminService);
    const policy = {
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 6_000,
      }],
    };
    await expect(controller.createBusinessInvite({
      discountPercent: 60,
      policy,
      expiresInDays: 7,
      reason: "ambiguous rolling payload",
      idempotencyKey: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    })).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.setBusinessPricing(
      "4f53639f-ced1-472f-998e-50e426bd5734",
      {
        discountPercent: 60,
        policy: { expectedVersion: 1, rules: policy.rules },
        reason: "ambiguous rolling payload",
      },
    )).rejects.toBeInstanceOf(BadRequestException);
    expect(createBusinessInvite).not.toHaveBeenCalled();
    expect(setBusinessPricing).not.toHaveBeenCalled();
  });
});

describe("managed pricing HTTP contract", () => {
  const rule = {
    scope: { provider: { providerId: "anthropic" } },
    pricingMode: "discount" as const,
    discountBps: 6_000,
  };

  it("forwards switch CAS and the bounded admin actor", async () => {
    const updateManagedProviderSwitches = vi.fn().mockResolvedValue({ switchGeneration: 3 });
    const controller = new AdminController({ updateManagedProviderSwitches } as unknown as AdminService);
    const body = {
      expectedGeneration: 2,
      reason: "disable only the B2B segment",
      providers: [{
        providerId: "anthropic",
        masterEnabled: true,
        productEnabled: true,
        b2cEnabled: true,
        b2bEnabled: false,
      }],
    };

    await expect(controller.updateManagedProviderSwitches(body, "operator@example.com"))
      .resolves.toMatchObject({ switchGeneration: 3 });
    expect(updateManagedProviderSwitches).toHaveBeenCalledWith(body, "operator@example.com");
  });

  it("wraps a B2B replacement policy in the existing rolling-compatible endpoint", async () => {
    const setBusinessPricing = vi.fn().mockResolvedValue({ policy: { currentVersion: 4 } });
    const controller = new AdminController({ setBusinessPricing } as unknown as AdminService);
    const userId = "4f53639f-ced1-472f-998e-50e426bd5734";
    const body = {
      policy: { expectedVersion: 3, rules: [rule] },
      reason: "replace the complete negotiated policy",
    };

    await expect(controller.setBusinessPricing(userId, body, "operator@example.com"))
      .resolves.toMatchObject({ policy: { currentVersion: 4 } });
    expect(setBusinessPricing).toHaveBeenCalledWith(
      userId,
      { policy: body.policy },
      "operator@example.com",
      body.reason,
    );
  });

  it("lists service policies without accepting inferred owner input", async () => {
    const listManagedServicePricingPolicies = vi.fn().mockResolvedValue({ policies: [{ ownerId: "crm" }] });
    const controller = new AdminController({ listManagedServicePricingPolicies } as unknown as AdminService);

    await expect(controller.listServicePricingPolicies()).resolves.toEqual({ policies: [{ ownerId: "crm" }] });
    expect(listManagedServicePricingPolicies).toHaveBeenCalledOnce();
  });

  it("stages catalog and switch convergence jobs with attributed intent", async () => {
    const stagePricingCatalogJobV2 = vi.fn().mockResolvedValue({
      status: "staged",
      job_id: "11111111-2222-3333-4444-555555555555",
    });
    const stagePricingSwitchJobV2 = vi.fn().mockResolvedValue({
      status: "staged",
      job_id: "66666666-7777-4888-8999-000000000000",
    });
    const controller = new AdminController({
      stagePricingCatalogJobV2,
      stagePricingSwitchJobV2,
    } as unknown as AdminService);

    const catalogBody = {
      product_id: "main",
      generation: 3,
      reason: "converge commerce catalog head with the engine gen 3",
    };
    await expect(controller.stagePricingCatalogJobV2(catalogBody, "operator@example.test"))
      .resolves.toMatchObject({ status: "staged" });
    expect(stagePricingCatalogJobV2).toHaveBeenCalledWith(catalogBody, "operator@example.test");

    const switchBody = { generation: 3, reason: "converge provider switch head with the engine" };
    await expect(controller.stagePricingSwitchJobV2(switchBody, "operator@example.test"))
      .resolves.toMatchObject({ status: "staged" });
    expect(stagePricingSwitchJobV2).toHaveBeenCalledWith(switchBody, "operator@example.test");

    await expect(controller.stagePricingCatalogJobV2(catalogBody, undefined))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.stagePricingCatalogJobV2({ ...catalogBody, payload: {} }, "operator"))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.stagePricingCatalogJobV2(
      { ...catalogBody, product_id: "staging" },
      "operator",
    )).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.stagePricingSwitchJobV2({ generation: 0, reason: "x" }, "operator"))
      .rejects.toBeInstanceOf(BadRequestException);
  });

  it("writes service inventory only through strict CAS metadata and a verified actor", async () => {
    const upsertServiceAccountInventoryV2 = vi.fn().mockResolvedValue({
      status: "stored",
      account: { service_id: "crm-parsing", source_version: 1 },
    });
    const controller = new AdminController({ upsertServiceAccountInventoryV2 } as unknown as AdminService);
    const body = {
      expected_source_version: null,
      expected_content_digest: null,
      engine_account_id: "acct_service_crm",
      purpose: "CRM ingestion and parsing",
      responsible: "platform",
      reason: "register the existing engine-native service account",
    };

    await expect(controller.upsertServiceAccountInventoryV2("crm-parsing", body, "owner@example.test"))
      .resolves.toMatchObject({ status: "stored" });
    expect(upsertServiceAccountInventoryV2).toHaveBeenCalledWith(
      "crm-parsing",
      body,
      "owner@example.test",
    );

    await expect(controller.upsertServiceAccountInventoryV2("", body))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.upsertServiceAccountInventoryV2("crm-parsing", {
      ...body,
      expected_content_digest: `sha256:v2:${"a".repeat(64)}`,
    })).rejects.toBeInstanceOf(BadRequestException);
  });

  it("queues only an exact audited policy-delivery repair with a verified actor", async () => {
    const repairPricingPolicyDeliveryV2 = vi.fn().mockResolvedValue({
      status: "queued",
      replacement_job_id: "22222222-2222-4222-8222-222222222222",
    });
    const controller = new AdminController({ repairPricingPolicyDeliveryV2 } as unknown as AdminService);
    const body = {
      job_id: "11111111-1111-4111-8111-111111111111",
      expected_effective_version: 1,
      expected_content_digest: `sha256:v1:${"a".repeat(64)}`,
      reason: "repair the reviewed historical pre-cutover delivery",
    };

    await expect(controller.repairPricingPolicyDeliveryV2(body, "owner@example.test"))
      .resolves.toMatchObject({ status: "queued" });
    expect(repairPricingPolicyDeliveryV2).toHaveBeenCalledWith(body, "owner@example.test");
    await expect(controller.repairPricingPolicyDeliveryV2(body))
      .rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.repairPricingPolicyDeliveryV2({
      ...body,
      expected_content_digest: `sha256:v2:${"a".repeat(64)}`,
    }, "owner@example.test")).rejects.toBeInstanceOf(BadRequestException);

    const missing = new AdminController({
      repairPricingPolicyDeliveryV2: vi.fn().mockRejectedValue(
        new PricingPolicyDeliveryRepairError("repair_job_not_found", "missing"),
      ),
    } as unknown as AdminService);
    const conflict = new AdminController({
      repairPricingPolicyDeliveryV2: vi.fn().mockRejectedValue(
        new PricingPolicyDeliveryRepairError("repair_precondition_changed", "changed"),
      ),
    } as unknown as AdminService);
    await expect(missing.repairPricingPolicyDeliveryV2(body, "owner@example.test"))
      .rejects.toBeInstanceOf(NotFoundException);
    await expect(conflict.repairPricingPolicyDeliveryV2(body, "owner@example.test"))
      .rejects.toMatchObject({ status: 409 });
  });

  it("maps missing engine accounts to 404 and all ownership/CAS races to 409", async () => {
    const missing = new AdminController({
      upsertServiceAccountInventoryV2: vi.fn().mockRejectedValue(
        new AdminServiceAccountInventoryError("engine_account_missing", "missing"),
      ),
    } as unknown as AdminService);
    const conflict = new AdminController({
      upsertServiceAccountInventoryV2: vi.fn().mockRejectedValue(
        new ServiceAccountInventoryV2Error("version_conflict", "stale"),
      ),
    } as unknown as AdminService);
    const body = {
      expected_source_version: null,
      expected_content_digest: null,
      engine_account_id: "acct_service_crm",
      purpose: "CRM ingestion and parsing",
      responsible: "platform",
      reason: "register the existing engine-native service account",
    };

    await expect(missing.upsertServiceAccountInventoryV2("crm-parsing", body))
      .rejects.toBeInstanceOf(NotFoundException);
    const rejected = conflict.upsertServiceAccountInventoryV2("crm-parsing", body);
    await expect(rejected).rejects.toBeInstanceOf(HttpException);
    await expect(rejected).rejects.toMatchObject({ status: 409 });
  });

  it("maps catalog/rule errors to 400, missing policies to 404, and CAS conflicts to 409", async () => {
    const invalid = new AdminController({
      updateManagedProviderSwitches: vi.fn().mockRejectedValue(
        new PricingPolicyWriteError("rule_outside_catalog", "outside catalog"),
      ),
    } as unknown as AdminService);
    await expect(invalid.updateManagedProviderSwitches({
      expectedGeneration: 1,
      reason: "invalid provider test",
      providers: [{ providerId: "unknown", masterEnabled: true, productEnabled: true, b2cEnabled: true, b2bEnabled: true }],
    })).rejects.toBeInstanceOf(BadRequestException);

    const missing = new AdminController({
      getManagedPricingPolicy: vi.fn().mockRejectedValue(
        new PricingPolicyWriteError("policy_not_found", "missing"),
      ),
    } as unknown as AdminService);
    await expect(missing.getGlobalB2cPricingPolicy()).rejects.toBeInstanceOf(NotFoundException);

    const conflict = new AdminController({
      updateManagedPricingPolicy: vi.fn().mockRejectedValue(
        new PricingPolicyWriteError("version_conflict", "stale"),
      ),
    } as unknown as AdminService);
    const rejected = conflict.updateGlobalB2cPricingPolicy({
      expectedVersion: 1,
      reason: "stale replacement test",
      rules: [rule],
    });
    await expect(rejected).rejects.toBeInstanceOf(HttpException);
    await expect(rejected).rejects.toMatchObject({ status: 409 });
  });
});
