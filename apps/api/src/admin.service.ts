import { createHash, randomBytes } from "node:crypto";
import { Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { multiplierForDiscount } from "@claude-api/contracts";
import {
  createBusinessInvite,
  setBusinessPricing,
  type Database,
} from "@claude-api/db";
import type { Environment } from "./config.js";
import { DATABASE } from "./infrastructure.module.js";

@Injectable()
export class AdminService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async createBusinessInvite(input: {
    email: string;
    discountPercent: number;
    expiresInDays: number;
  }): Promise<Record<string, unknown>> {
    const token = randomBytes(32).toString("base64url");
    const expiresAt = new Date(Date.now() + input.expiresInDays * 86_400_000);
    const inviteId = await createBusinessInvite(this.database, {
      email: input.email,
      tokenHash: hashToken(token),
      multiplierBp: multiplierForDiscount(input.discountPercent),
      expiresAt,
    });
    const inviteUrl = new URL("/register", this.config.get("PUBLIC_APP_BASE_URL", { infer: true }));
    inviteUrl.searchParams.set("invite", token);
    return {
      id: inviteId,
      email: input.email,
      discountPercent: input.discountPercent,
      expiresAt: expiresAt.toISOString(),
      inviteUrl: inviteUrl.toString(),
    };
  }

  async setBusinessPricing(userId: string, discountPercent: number): Promise<Record<string, unknown>> {
    await setBusinessPricing(this.database, {
      userId,
      multiplierBp: multiplierForDiscount(discountPercent),
      actorId: "commercial-admin",
    });
    return { userId, discountPercent, syncStatus: "pending" };
  }
}

function hashToken(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}
