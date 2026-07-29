import { createHash, randomBytes } from "node:crypto";
import { Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { multiplierForDiscount } from "@claude-api/contracts";
import {
  BusinessInvitationConflictError,
  BusinessInvitationNotFoundError,
  createBusinessInvite,
  decodeAuthEncryptionKey,
  decryptAuthToken,
  encryptAuthToken,
  evaluateRefundEligibility,
  getBusinessInviteToken,
  getEngineAccountMapping,
  listAdminUserOverview,
  recordAdminCredit,
  revokeBusinessInvite,
  rotateBusinessInvite,
  setBusinessPricing,
  type AdminUserOverviewRow,
  type AdminUserOverviewQuery,
  type Database,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";

@Injectable()
export class AdminService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  /** Обзор всех пользователей для панели: агрегаты commerce БД + live-деньги движка. */
  async listUsers(query: AdminUserOverviewQuery = {}): Promise<{
    users: Array<Record<string, unknown>>;
    total: number;
    limit: number;
    offset: number;
  }> {
    const page = await listAdminUserOverview(this.database, query);
    // Live-баланс/расход — из движка (он авторитет денег); недоступность движка не валит список.
    const live = new Map<string, { balance: string; spent: string; reserved: string; status: string }>();
    const accountIds = page.rows.flatMap((row) => row.engineAccountId ? [row.engineAccountId] : []);
    if (accountIds.length > 0) {
      try {
        const accounts = await this.engine.getAccounts(accountIds);
        for (const account of accounts) {
          live.set(account.account, {
            balance: account.balance_nano,
            spent: account.spent_nano,
            reserved: account.reserved_nano,
            status: account.status,
          });
        }
      } catch {
        // движок недоступен → bounded commerce page remains available with null live fields.
      }
    }
    return {
      users: page.rows.map((row) => serializeUser(
        row,
        row.engineAccountId ? live.get(row.engineAccountId) ?? null : null,
      )),
      total: page.total,
      limit: page.limit,
      offset: page.offset,
    };
  }

  /**
   * Админское начисление баланса. Сумма — целые USD строкой цифр (правило проекта: без float,
   * точек и ведущих нулей). Деньги кредитует движок (авторитет live-баланса) идемпотентно по ref;
   * commerce пишет только след в audit_log. НЕ считается пополнением для prepay-тира
   * (мимо payments/engine_credits — подарок не двигает тир).
   */
  async creditUser(userId: string, amountUsd: string): Promise<Record<string, unknown>> {
    if (!/^[1-9][0-9]{0,4}$/.test(amountUsd)) {
      throw new AdminCreditError(400, "amount_usd must be an integer USD string from 1 to 99999");
    }
    const mapping = await getEngineAccountMapping(this.database, userId);
    if (!mapping) throw new AdminCreditError(404, "user has no engine account record");
    if (!mapping.engineAccountId || mapping.status !== "active") {
      throw new AdminCreditError(409, `engine account is not active (status: ${mapping.status})`);
    }
    const amountNano = BigInt(amountUsd) * 1_000_000_000n;
    const ref = `admin-credit:${randomBytes(16).toString("hex")}`;
    const result = await this.engine.creditAccount(mapping.engineAccountId, amountNano, ref);
    await recordAdminCredit(this.database, {
      userId,
      engineAccountId: mapping.engineAccountId,
      amountNano,
      ref,
      balanceAfterNano: result.balance_nano,
    });
    return {
      user_id: userId,
      credited_usd: amountUsd,
      balance: result.balance,
      balance_nano: result.balance_nano,
    };
  }

  async createBusinessInvite(input: {
    email?: string;
    discountPercent: number;
    expiresInDays: number;
    reason: string;
    idempotencyKey: string;
    actorId: string;
  }): Promise<Record<string, unknown>> {
    const token = randomBytes(32).toString("base64url");
    const expiresAt = new Date(Date.now() + input.expiresInDays * 86_400_000);
    const key = decodeAuthEncryptionKey(this.config.get("AUTH_TOKEN_ENCRYPTION_KEY", { infer: true }));
    const invite = await createBusinessInvite(this.database, {
      ...(input.email ? { email: input.email } : {}),
      tokenHash: hashToken(token),
      encryptedToken: encryptAuthToken(token, key),
      multiplierBp: multiplierForDiscount(input.discountPercent),
      expiresAt,
      idempotencyKey: input.idempotencyKey,
      actorId: input.actorId,
      reason: input.reason,
    });
    const storedToken = invite.idempotentReplay
      ? decryptAuthToken(invite.encryptedToken, key)
      : token;
    return {
      id: invite.id,
      email: invite.email,
      discountPercent: 100 - invite.multiplierBp / 100,
      expiresAt: invite.expiresAt.toISOString(),
      inviteUrl: this.inviteUrl(storedToken),
      deliveryStatus: invite.deliveryStatus,
      idempotentReplay: invite.idempotentReplay,
    };
  }

  async getBusinessInviteLink(inviteId: string): Promise<Record<string, unknown>> {
    const invite = await getBusinessInviteToken(this.database, inviteId);
    const key = decodeAuthEncryptionKey(this.config.get("AUTH_TOKEN_ENCRYPTION_KEY", { infer: true }));
    return {
      id: inviteId,
      email: invite.email,
      expiresAt: invite.expiresAt.toISOString(),
      inviteUrl: this.inviteUrl(decryptAuthToken(invite.encryptedToken, key)),
    };
  }

  async revokeBusinessInvite(inviteId: string, actorId: string, reason: string): Promise<Record<string, unknown>> {
    await revokeBusinessInvite(this.database, { inviteId, actorId, reason });
    return { id: inviteId, status: "revoked" };
  }

  async resendBusinessInvite(
    inviteId: string,
    actorId: string,
    reason: string,
    expiresInDays: number,
    idempotencyKey: string,
  ): Promise<Record<string, unknown>> {
    const token = randomBytes(32).toString("base64url");
    const expiresAt = new Date(Date.now() + expiresInDays * 86_400_000);
    const key = decodeAuthEncryptionKey(this.config.get("AUTH_TOKEN_ENCRYPTION_KEY", { infer: true }));
    const invite = await rotateBusinessInvite(this.database, {
      inviteId,
      tokenHash: hashToken(token),
      encryptedToken: encryptAuthToken(token, key),
      expiresAt,
      idempotencyKey,
      actorId,
      reason,
    });
    return {
      id: invite.id,
      email: invite.email,
      discountPercent: 100 - invite.multiplierBp / 100,
      expiresAt: invite.expiresAt.toISOString(),
      inviteUrl: this.inviteUrl(invite.idempotentReplay
        ? decryptAuthToken(invite.encryptedToken, key)
        : token),
      deliveryStatus: invite.deliveryStatus,
      supersedesInviteId: inviteId,
      idempotentReplay: invite.idempotentReplay,
    };
  }

  /**
   * Право на возврат по правилу соглашения (≤5 дней с оплаты И реальные деньги не тратились).
   * Отдаёт вердикт панели/поддержке, чтобы решение об оформлении возврата принималось кодом, а не на
   * глаз. Само оформление возврата (дебет движка) — отдельный шаг.
   */
  async refundEligibility(checkoutId: string): Promise<Record<string, unknown>> {
    const verdict = await evaluateRefundEligibility(this.database, checkoutId);
    return {
      checkout_id: checkoutId,
      eligible: verdict.eligible,
      reason: verdict.reason,
      payment_id: verdict.paymentId,
      paid_at: verdict.paidAt,
      window_days: verdict.windowDays,
      age_days: verdict.ageDays === null ? null : Math.round(verdict.ageDays * 100) / 100,
      real_spent_since_usd: nanoToUsd(verdict.realSpentSinceNano),
    };
  }

  async setBusinessPricing(
    userId: string,
    discountPercent: number,
    actorId: string,
    reason: string,
  ): Promise<Record<string, unknown>> {
    const result = await setBusinessPricing(this.database, {
      userId,
      multiplierBp: multiplierForDiscount(discountPercent),
      actorId,
      reason,
    });
    return { userId, discountPercent, syncStatus: "pending", pricingJobId: result.jobId };
  }

  private inviteUrl(token: string): string {
    const inviteUrl = new URL("/register", this.config.get("PUBLIC_APP_BASE_URL", { infer: true }));
    inviteUrl.searchParams.set("invite", token);
    return inviteUrl.toString();
  }
}

export class AdminCreditError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
    this.name = "AdminCreditError";
  }
}

function hashToken(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

function serializeUser(
  row: AdminUserOverviewRow,
  engine: { balance: string; spent: string; reserved: string; status: string } | null,
): Record<string, unknown> {
  const methods = [...row.providers];
  if (row.hasPassword) methods.push("password");
  return {
    id: row.id,
    email: row.email,
    display_name: row.displayName,
    email_verified: row.emailVerified,
    status: row.status,
    created_at: row.createdAt.toISOString(),
    auth_methods: methods,
    totp_enabled: row.totpEnabled,
    customer_type: row.customerType,
    tier: row.currentTier,
    multiplier_bp: row.multiplierBp,
    cumulative_topup_usd: nanoToUsd(row.cumulativeTopupNano),
    tier_window_spent_usd: nanoToUsd(row.tierWindowSpentNano),
    engine_account_id: row.engineAccountId,
    engine_account_status: row.engineAccountStatus,
    pricing_sync_status: row.pricingSyncStatus,
    pricing_sync_attempts: row.pricingSyncAttempts,
    pricing_sync_error: row.pricingSyncError,
    pricing_sync_confirmed_at: row.pricingSyncConfirmedAt?.toISOString() ?? null,
    balance_usd: engine ? nanoToUsd(engine.balance) : null,
    spent_usd: engine ? nanoToUsd(engine.spent) : null,
    reserved_usd: engine ? nanoToUsd(engine.reserved) : null,
    engine_live_status: engine?.status ?? null,
    spent_30d_usd: nanoToUsd(row.spent30dNano),
    payments: {
      paid_count: row.paidPaymentsCount,
      paid_total_usd: nanoToUsd(row.paidTotalNano),
      last_paid_at: row.lastPaidAt?.toISOString() ?? null,
      pending_checkouts: row.pendingCheckoutsCount,
    },
    api_keys: { active: row.apiKeysActive, total: row.apiKeysTotal },
    last_seen_at: row.lastSeenAt?.toISOString() ?? null,
  };
}

/** nano-USD (строка целого) → десятичная USD-строка с 4 знаками; без float. */
function nanoToUsd(nano: string): string {
  const negative = nano.startsWith("-");
  const digits = (negative ? nano.slice(1) : nano).padStart(10, "0");
  const whole = digits.slice(0, -9);
  const frac = digits.slice(-9, -5); // 4 знака после точки достаточно для панели
  return `${negative ? "-" : ""}${whole}.${frac}`;
}
