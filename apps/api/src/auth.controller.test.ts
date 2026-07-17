import { BadRequestException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { describe, expect, it, vi } from "vitest";
import type { AuthUserView } from "@claude-api/contracts";
import { AuthController } from "./auth.controller.js";
import type { RequestAuth } from "./auth.guard.js";
import type { AuthService } from "./auth.service.js";
import type { Environment } from "./config.js";

function currentAuth(userId = "alice-id"): RequestAuth {
  const user: AuthUserView = {
    id: userId,
    email: "alice@example.com",
    displayName: "Alice",
    emailVerified: true,
    passwordEnabled: false,
    engineAccountStatus: "active", totpEnabled: false,
    customerType: "b2c",
  };
  return { sessionId: "session-id", user };
}

describe("authenticated profile updates", () => {
  it("derives the target user from the authenticated session and trims the name", async () => {
    const updateProfile = vi.fn().mockResolvedValue({ ...currentAuth().user, displayName: "Alice Studio" });
    const controller = new AuthController(
      { updateProfile } as unknown as AuthService,
      new ConfigService<Environment, true>({} as Environment),
    );

    await expect(controller.updateProfile({ displayName: "  Alice Studio  " }, currentAuth())).resolves.toEqual({
      user: expect.objectContaining({ id: "alice-id", displayName: "Alice Studio" }),
    });
    expect(updateProfile).toHaveBeenCalledWith("alice-id", "Alice Studio");
  });

  it("rejects attempts to submit another user id", async () => {
    const updateProfile = vi.fn();
    const controller = new AuthController(
      { updateProfile } as unknown as AuthService,
      new ConfigService<Environment, true>({} as Environment),
    );

    await expect(controller.updateProfile(
      { displayName: "Mallory", userId: "bob-id" },
      currentAuth("alice-id"),
    )).rejects.toBeInstanceOf(BadRequestException);
    expect(updateProfile).not.toHaveBeenCalled();
  });

  it("rejects names outside the public profile bounds", async () => {
    const updateProfile = vi.fn();
    const controller = new AuthController(
      { updateProfile } as unknown as AuthService,
      new ConfigService<Environment, true>({} as Environment),
    );

    await expect(controller.updateProfile({ displayName: "A".repeat(81) }, currentAuth())).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.updateProfile({ displayName: "Bad\nName" }, currentAuth())).rejects.toBeInstanceOf(BadRequestException);
    expect(updateProfile).not.toHaveBeenCalled();
  });
});

describe("authentication provider status", () => {
  it("advertises password verification as disabled", () => {
    const controller = new AuthController(
      { providerStatus: () => ({ google: true, github: false }) } as unknown as AuthService,
      new ConfigService<Environment, true>({ EMAIL_VERIFICATION_REQUIRED: false } as Environment),
    );

    expect(controller.providers()).toEqual({
      email: { password: true, verificationRequired: false },
      google: { configured: true, enabled: true },
      github: { configured: false, enabled: false, emailScope: "user:email" },
    });
  });
});
