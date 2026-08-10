import { BadRequestException, HttpException, NotFoundException } from "@nestjs/common";
import {
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
