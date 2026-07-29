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

describe("customer password length", () => {
  it("accepts eight characters and rejects seven for registration and reset", async () => {
    const register = vi.fn().mockResolvedValue({ user: currentAuth().user, session: null });
    const resetPassword = vi.fn().mockResolvedValue(undefined);
    const controller = new AuthController(
      { register, resetPassword } as unknown as AuthService,
      new ConfigService<Environment, true>({} as Environment),
    );
    const request = { headers: {} };
    const reply = { header: vi.fn() };
    const token = "A".repeat(43);

    await expect(controller.register({
      email: "alice@example.com",
      password: "12345678",
    }, request, reply)).resolves.toEqual({
      user: currentAuth().user,
      verificationRequired: true,
    });
    await expect(controller.register({
      email: "alice@example.com",
      password: "1234567",
    }, request, reply)).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.resetPassword({ token, password: "12345678" })).resolves.toBeUndefined();
    await expect(controller.resetPassword({ token, password: "1234567" })).rejects.toBeInstanceOf(BadRequestException);

    expect(register).toHaveBeenCalledTimes(1);
    expect(resetPassword).toHaveBeenCalledWith(token, "12345678");
  });
});

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

describe("business invitation preview", () => {
  it("uses a POST body and returns a generic invalid result for malformed tokens", async () => {
    const businessInvitePreview = vi.fn().mockResolvedValue({
      valid: true,
      discountPercent: 75,
      expiresAt: "2026-08-01T12:00:00.000Z",
    });
    const controller = new AuthController(
      { businessInvitePreview } as unknown as AuthService,
      new ConfigService<Environment, true>({} as Environment),
    );

    await expect(controller.businessInvitePreview({ token: "a".repeat(43) }))
      .resolves.toMatchObject({ valid: true, discountPercent: 75 });
    await expect(controller.businessInvitePreview({ token: "short" }))
      .resolves.toEqual({ valid: false });
    expect(businessInvitePreview).toHaveBeenCalledTimes(1);
  });
});
