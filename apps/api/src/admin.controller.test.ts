import { BadRequestException, HttpException, NotFoundException } from "@nestjs/common";
import {
  PricingPolicyDeliveryRepairError,
  PricingPolicyWriteError,
} from "@claude-api/db";
import { describe, expect, it, vi } from "vitest";
import { AdminController } from "./admin.controller.js";
import type { AdminService } from "./admin.service.js";

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
