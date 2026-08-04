import { ServiceUnavailableException, UnauthorizedException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import type { AuthUserView } from "@claude-api/contracts";
import { EngineClientError } from "@claude-api/engine-client";
import { AccountController } from "./account.controller.js";
import { AccountService, EngineAccountUnavailableError } from "./account.service.js";
import type { RequestAuth } from "./auth.guard.js";
import type { TotpService } from "./totp.service.js";

function currentAuth(): RequestAuth {
  const user: AuthUserView = {
    id: "alice-id",
    email: "alice@example.com",
    displayName: "Alice",
    emailVerified: true,
    passwordEnabled: false,
    engineAccountStatus: "active",
    customerType: "b2c",
    totpEnabled: true,
  };
  return { sessionId: "session-id", user };
}

describe("API key policy controller", () => {
  it("requires valid TOTP before replacing nullable policy fields", async () => {
    const updateApiKeyPolicy = vi.fn().mockResolvedValue({ id: "key-view" });
    const verify = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    const controller = new AccountController(
      { updateApiKeyPolicy } as unknown as AccountService,
      { verify } as unknown as TotpService,
    );
    const id = "9d8ac711-43c0-47f1-95af-a4a8ad6a89fe";
    const policy = { spendLimitUsd: null, expiresAt: "2099-01-01T00:00:00.000Z" };

    await expect(controller.updateApiKeyPolicy(currentAuth(), id, policy))
      .rejects.toBeInstanceOf(UnauthorizedException);
    expect(verify).not.toHaveBeenCalled();
    expect(updateApiKeyPolicy).not.toHaveBeenCalled();

    await expect(controller.updateApiKeyPolicy(currentAuth(), id, { ...policy, totpCode: "111111" }))
      .rejects.toBeInstanceOf(UnauthorizedException);
    expect(updateApiKeyPolicy).not.toHaveBeenCalled();

    await expect(controller.updateApiKeyPolicy(currentAuth(), id, { ...policy, totpCode: "222222" }))
      .resolves.toEqual({ id: "key-view" });
    expect(updateApiKeyPolicy).toHaveBeenCalledWith("alice-id", id, {
      ...policy,
      totpCode: "222222",
    });
  });
});

describe("engine failure mapping", () => {
  function controllerWithWarning(accounts: unknown) {
    const controller = new AccountController(
      accounts as AccountService,
      { verify: vi.fn() } as unknown as TotpService,
    );
    const logger = (controller as unknown as { logger: { warn: (message: string) => void } }).logger;
    const warn = vi.spyOn(logger, "warn");
    return { controller, warn };
  }

  it("keeps the public 503 text for a retryable engine error and logs the original failure", async () => {
    const failure = new EngineClientError("engine returned HTTP 503", 503, true);
    const { controller, warn } = controllerWithWarning({ getAccount: vi.fn().mockRejectedValue(failure) });

    const caught = await controller.getAccount(currentAuth()).then(
      () => null,
      (error: unknown) => error,
    );
    expect(caught).toBeInstanceOf(ServiceUnavailableException);
    expect((caught as ServiceUnavailableException).message).toBe("engine is temporarily unavailable");
    expect(warn).toHaveBeenCalledTimes(1);
    const logged = warn.mock.calls[0]?.[0] ?? "";
    expect(logged).toContain("engine returned HTTP 503");
    expect(logged).toContain("status: 503");
    expect(logged).toContain("retryable: true");
  });

  it("logs the provisioning cause behind an EngineAccountUnavailableError", async () => {
    const cause = new EngineClientError("engine request timed out", undefined, true);
    const failure = new EngineAccountUnavailableError("engine account is temporarily unavailable", { cause });
    const { controller, warn } = controllerWithWarning({ getAccount: vi.fn().mockRejectedValue(failure) });

    await expect(controller.getAccount(currentAuth())).rejects.toMatchObject({
      message: "engine is temporarily unavailable",
    });
    expect(warn).toHaveBeenCalledTimes(1);
    const logged = warn.mock.calls[0]?.[0] ?? "";
    expect(logged).toContain("engine account is temporarily unavailable");
    expect(logged).toContain("engine request timed out");
  });
});
