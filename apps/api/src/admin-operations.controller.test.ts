import { BadRequestException, HttpException, UnauthorizedException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import { AdminGuard } from "./admin.guard.js";
import { AdminOperationsController } from "./admin-operations.controller.js";
import { AdminOperationError, type AdminOperationsService } from "./admin-operations.service.js";

const userId = "11111111-1111-4111-8111-111111111111";

describe("admin operations HTTP contract", () => {
  it("rejects malformed IDs, amounts, reasons, and limits before service calls", async () => {
    const operations = fakeOperations();
    const controller = new AdminOperationsController(operations.service);

    await expect(controller.creditUser("not-a-uuid", {})).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.creditUser(userId, {
      amount_usd: "01",
      reason: "customer support credit",
      idempotency_key: crypto.randomUUID(),
    })).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.creditUser(userId, {
      amount_usd: "7",
      reason: "x",
      idempotency_key: crypto.randomUUID(),
    })).rejects.toBeInstanceOf(BadRequestException);
    expect(() => controller.audit("501")).toThrow(BadRequestException);
    expect(operations.creditUser).not.toHaveBeenCalled();
  });

  it("forwards a validated, idempotent balance adjustment", async () => {
    const operations = fakeOperations();
    const controller = new AdminOperationsController(operations.service);
    const idempotencyKey = crypto.randomUUID();
    operations.creditUser.mockResolvedValue({ balance_usd: "12" });

    await expect(controller.creditUser(userId, {
      amount_usd: "7",
      reason: "customer support credit",
      idempotency_key: idempotencyKey,
    })).resolves.toEqual({ balance_usd: "12" });
    expect(operations.creditUser).toHaveBeenCalledWith({
      userId,
      amountUsd: "7",
      reason: "customer support credit",
      idempotencyKey,
      actorId: "unknown",
    });
  });

  it("maps safe operation errors to their intended HTTP status", async () => {
    const operations = fakeOperations();
    const controller = new AdminOperationsController(operations.service);
    operations.setUserStatus.mockRejectedValue(new AdminOperationError(409, "engine account conflict"));

    const failure = controller.setUserStatus(userId, { status: "disabled", reason: "security review" }, "admin-q");
    await expect(failure).rejects.toBeInstanceOf(HttpException);
    await expect(failure).rejects.toMatchObject({ status: 409 });
  });
});

describe("admin key guard", () => {
  it("accepts the current and temporary previous admin keys", () => {
    const guard = new AdminGuard({
      get: (name: string) => name === "COMMERCIAL_ADMIN_KEY" ? "a".repeat(32) : "b".repeat(32),
    } as never);
    expect(guard.canActivate(contextWithKey("a".repeat(32)))).toBe(true);
    expect(guard.canActivate(contextWithKey("b".repeat(32)))).toBe(true);
    expect(() => guard.canActivate(contextWithKey("wrong"))).toThrow(UnauthorizedException);
    expect(() => guard.canActivate(contextWithKey(undefined))).toThrow(UnauthorizedException);
  });
});

function fakeOperations() {
  const operations = {
    dashboard: vi.fn(),
    topups: vi.fn(),
    audit: vi.fn(),
    businessInvites: vi.fn(),
    creditUser: vi.fn(),
    setUserStatus: vi.fn(),
    revokeSessions: vi.fn(),
    resetTotp: vi.fn(),
  };
  return { ...operations, service: operations as unknown as AdminOperationsService };
}

function contextWithKey(key: string | undefined) {
  return {
    switchToHttp: () => ({
      getRequest: () => ({ headers: { "x-admin-key": key } }),
    }),
  } as never;
}
