import { UnauthorizedException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import type { AuthUserView } from "@claude-api/contracts";
import { AccountController } from "./account.controller.js";
import type { AccountService } from "./account.service.js";
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
