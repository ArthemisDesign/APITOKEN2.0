import { BadRequestException } from "@nestjs/common";
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
      search: "alice",
      status: "active",
      auth: "google",
      customerType: "b2b",
    });
  });

  it("rejects unbounded or unknown filters", async () => {
    const controller = new AdminController({ listUsers: vi.fn() } as unknown as AdminService);
    await expect(controller.listUsers("500", "0")).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.listUsers("50", "0", "", "unknown")).rejects.toBeInstanceOf(BadRequestException);
  });
});
