import { Inject, Injectable } from "@nestjs/common";
import { multiplierForDiscount } from "@claude-api/contracts";
import {
  AccountStrictCutoverError,
  canonicalizeEmail,
  convertCustomerToBusiness,
  findAdminCreditByRef,
  getGrantedSignupBonusAmount,
  markSignupProfileAdminRevoked,
  getAdminDashboard,
  getAdminUserControlTarget,
  getEngineAccountMapping,
  listAdminAudit,
  listAdminAuditActions,
  listAdminBusinessInvites,
  listAdminTopups,
  recordAdminCredit,
  resetAdminUserTotp,
  revokeAdminUserSessions,
  setAdminUserStatus,
  stageAccountStrictCutoverJob,
  type AdminTopupsQuery,
  type AdminAuditQuery,
  type Database,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";

@Injectable()
export class AdminOperationsService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
  ) {}

  async dashboard(): Promise<Record<string, unknown>> {
    const value = await getAdminDashboard(this.database);
    return {
      generated_at: value.generatedAt.toISOString(),
      users: {
        total: value.users.total,
        active: value.users.active,
        disabled: value.users.disabled,
        registered_24h: value.users.registered24h,
        registered_30d: value.users.registered30d,
        active_7d: value.users.active7d,
        registered_oauth: value.users.registeredOauth,
        registered_password: value.users.registeredPassword,
        password_only: value.users.passwordOnly,
        oauth_only: value.users.oauthOnly,
        hybrid: value.users.hybrid,
        google: value.users.google,
        github: value.users.github,
        verified: value.users.verified,
        totp: value.users.totp,
      },
      topups: {
        paid_count: value.topups.paidCount,
        paid_users: value.topups.paidUsers,
        paid_usd: nanoToUsd(value.topups.paidNano),
        paid_30d_count: value.topups.paid30dCount,
        paid_30d_usd: nanoToUsd(value.topups.paid30dNano),
        pending_checkouts: value.topups.pendingCheckouts,
        failed_30d: value.topups.failed30d,
        refunded_count: value.topups.refundedCount,
        refunded_usd: nanoToUsd(value.topups.refundedNano),
        manual_count: value.topups.manualCount,
        manual_usd: nanoToUsd(value.topups.manualNano),
        manual_30d_count: value.topups.manual30dCount,
        manual_30d_usd: nanoToUsd(value.topups.manual30dNano),
      },
      platform: {
        b2c_users: value.platform.b2cUsers,
        b2b_users: value.platform.b2bUsers,
        active_api_keys: value.platform.activeApiKeys,
        total_api_keys: value.platform.totalApiKeys,
        active_sessions: value.platform.activeSessions,
        engine_active: value.platform.engineActive,
        engine_pending: value.platform.enginePending,
        engine_error: value.platform.engineError,
        engine_disabled: value.platform.engineDisabled,
      },
    };
  }

  /**
   * Два списка пополнений с общими фильтрами (семантика status — у AdminTopupsQuery) и
   * независимой пагинацией каждого: limit/offset применяются к обоим, а payments_total /
   * checkouts_total — COUNT с теми же фильтрами, чтобы панель рисовала пагинацию.
   * Без фильтров выдача байт-в-байт совпадает с исторической (paid-платежи + неоплаченные чекауты).
   */
  async topups(query: AdminTopupsQuery & { limit: number; offset: number }): Promise<Record<string, unknown>> {
    const value = await listAdminTopups(this.database, query);
    return {
      payments: value.payments.map((row) => ({
        id: row.id,
        user_id: row.userId,
        email: row.email,
        provider: row.provider,
        provider_payment_id: row.providerPaymentId,
        amount_usd: nanoToUsd(row.amountNano),
        currency: row.currency,
        status: row.status,
        paid_at: row.paidAt?.toISOString() ?? null,
        created_at: row.createdAt.toISOString(),
        credit_status: row.creditStatus,
      })),
      checkouts: value.checkouts.map((row) => ({
        id: row.id,
        user_id: row.userId,
        email: row.email,
        provider: row.provider,
        provider_payment_id: row.providerPaymentId,
        amount_usd: row.amountUsd,
        status: row.status,
        created_at: row.createdAt.toISOString(),
        completed_at: row.completedAt?.toISOString() ?? null,
        expires_at: row.expiresAt?.toISOString() ?? null,
      })),
      payments_total: value.paymentsTotal,
      checkouts_total: value.checkoutsTotal,
    };
  }

  async audit(query: AdminAuditQuery & { limit: number; offset: number }): Promise<Record<string, unknown>> {
    const page = await listAdminAudit(this.database, query);
    return {
      rows: page.rows.map((row) => ({
        id: row.id,
        actor_type: row.actorType,
        actor_id: row.actorId,
        action: row.action,
        target_type: row.targetType,
        target_id: row.targetId,
        metadata: row.metadata,
        created_at: row.createdAt.toISOString(),
      })),
      total: page.total,
      limit: query.limit,
      offset: query.offset,
    };
  }

  /** Distinct-список action для выпадающего фильтра аудита в панели. */
  async auditActions(): Promise<Record<string, unknown>> {
    return { actions: await listAdminAuditActions(this.database) };
  }

  async businessInvites(limit: number): Promise<Record<string, unknown>> {
    const rows = await listAdminBusinessInvites(this.database, limit);
    return { invites: rows.map((row) => ({
      id: row.id,
      email: row.email,
      discount_percent: row.policyVersion === null ? 100 - row.multiplierBp / 100 : null,
      policy_version: row.policyVersion,
      policy_digest: row.policyDigest,
      policy_rule_count: row.policyRuleCount,
      expires_at: row.expiresAt.toISOString(),
      consumed_at: row.consumedAt?.toISOString() ?? null,
      consumed_by_user_id: row.consumedByUserId,
      revoked_at: row.revokedAt?.toISOString() ?? null,
      superseded_by_invite_id: row.supersededByInviteId,
      created_by_actor: row.createdByActor,
      delivery_status: row.deliveryStatus,
      delivery_attempts: row.deliveryAttempts,
      delivery_error: row.deliveryError,
      delivery_sent_at: row.deliverySentAt?.toISOString() ?? null,
      created_at: row.createdAt.toISOString(),
    })) };
  }

  async creditUser(input: {
    userId: string;
    amountUsd: string;
    reason: string;
    idempotencyKey: string;
    actorId: string;
  }): Promise<Record<string, unknown>> {
    const amountNano = BigInt(input.amountUsd) * 1_000_000_000n;
    const ref = `admin-credit:${input.idempotencyKey}`;
    const existing = await findAdminCreditByRef(this.database, ref);
    if (existing) {
      if (existing.userId !== input.userId || existing.amountNano !== amountNano.toString()) {
        throw new AdminOperationError(409, "idempotency key was already used for another adjustment");
      }
      return {
        user_id: input.userId,
        credited_usd: input.amountUsd,
        balance_nano: existing.balanceAfterNano,
        balance_usd: nanoToUsd(existing.balanceAfterNano),
        idempotent_replay: true,
      };
    }

    const mapping = await getEngineAccountMapping(this.database, input.userId);
    if (!mapping) throw new AdminOperationError(404, "user has no engine account record");
    if (!mapping.engineAccountId || mapping.status !== "active") {
      throw new AdminOperationError(409, `engine account is not active (status: ${mapping.status})`);
    }
    const result = await this.engine.creditAccount(mapping.engineAccountId, amountNano, ref);
    await recordAdminCredit(this.database, {
      userId: input.userId,
      engineAccountId: mapping.engineAccountId,
      amountNano,
      ref,
      balanceAfterNano: result.balance_nano,
      reason: input.reason,
      actorId: input.actorId,
    });
    return {
      user_id: input.userId,
      credited_usd: input.amountUsd,
      balance_nano: result.balance_nano,
      balance_usd: nanoToUsd(result.balance_nano),
      idempotent_replay: false,
    };
  }

  /**
   * Ручной отзыв welcome-бонуса: идемпотентное списание ровно бонусной суммы с движка
   * (ref `bonus-revoke:<user_id>` — тот же неймспейс, что у ручных отзывов через Control API,
   * поэтому уже отозванный бонус не спишется второй раз) + пометка антифрод-профиля,
   * чтобы бонус не был заклеймлен повторно. Paid-баланс не разделяется от бонусного на
   * движке, поэтому при потраченном бонусе баланс может уйти в минус — это блокирует ключи.
   */
  async revokeSignupBonus(input: {
    userId: string;
    reason: string;
    actorId: string;
  }): Promise<Record<string, unknown>> {
    const ref = `bonus-revoke:${input.userId}`;
    const mapping = await getEngineAccountMapping(this.database, input.userId);
    if (!mapping?.engineAccountId) throw new AdminOperationError(404, "user has no engine account record");

    const email = await this.database.pool.query<{ email: string }>(
      "SELECT email FROM users WHERE id = $1",
      [input.userId],
    );
    if (!email.rows[0]) throw new AdminOperationError(404, "user not found");

    const existing = await findAdminCreditByRef(this.database, ref);
    if (existing) {
      const revokedAmountNano = -BigInt(existing.amountNano);
      if (revokedAmountNano <= 0n) {
        throw new AdminOperationError(409, "stored bonus revocation has an invalid amount");
      }
      await markSignupProfileAdminRevoked(this.database, {
        userId: input.userId,
        emailCanonical: canonicalizeEmail(email.rows[0].email),
      });
      return {
        user_id: input.userId,
        revoked_usd: nanoToUsd(revokedAmountNano.toString()),
        balance_nano: existing.balanceAfterNano,
        balance_usd: nanoToUsd(existing.balanceAfterNano),
        idempotent_replay: true,
      };
    }

    const bonusAmountNano = await getGrantedSignupBonusAmount(this.database, input.userId);
    if (bonusAmountNano === null) {
      throw new AdminOperationError(409, "user has no granted welcome bonus");
    }
    const result = await this.engine.debitAccount(mapping.engineAccountId, bonusAmountNano, ref);
    await recordAdminCredit(this.database, {
      userId: input.userId,
      engineAccountId: mapping.engineAccountId,
      amountNano: -bonusAmountNano,
      ref,
      balanceAfterNano: result.balance_nano,
      reason: input.reason,
      actorId: input.actorId,
    });
    await markSignupProfileAdminRevoked(this.database, {
      userId: input.userId,
      emailCanonical: canonicalizeEmail(email.rows[0].email),
    });
    return {
      user_id: input.userId,
      revoked_usd: nanoToUsd(bonusAmountNano.toString()),
      balance_nano: result.balance_nano,
      balance_usd: nanoToUsd(result.balance_nano),
      idempotent_replay: false,
    };
  }

  async setUserStatus(input: {
    userId: string;
    status: "active" | "disabled";
    reason: string;
    actorId: string;
  }): Promise<Record<string, unknown>> {
    const target = await getAdminUserControlTarget(this.database, input.userId);
    if (!target) throw new AdminOperationError(404, "user not found");
    if (target.engineAccountId && (input.status === "disabled" || target.engineAccountStatus === "disabled")) {
      await this.engine.setAccountStatus(target.engineAccountId, input.status);
    }
    const result = await setAdminUserStatus(this.database, input);
    return { user_id: input.userId, status: input.status, sessions_revoked: result.sessionsRevoked };
  }

  async convertToBusiness(input: {
    userId: string;
    reason: string;
    actorId: string;
    discountPercent: number;
  }): Promise<Record<string, unknown>> {
    const result = await convertCustomerToBusiness(this.database, {
      ...input,
      multiplierBp: multiplierForDiscount(input.discountPercent),
    });
    return {
      user_id: input.userId,
      customer_type: "b2b",
      discount_percent: 100 - result.multiplierBp / 100,
      multiplier_bp: result.multiplierBp,
      converted: result.converted,
      sync_status: result.jobId ? "pending" : "unchanged",
    };
  }

  /**
   * Per-account shadow→strict cutover for one converted B2B client. Sequence: normalize the
   * account's funding buckets (the strict trigger requires bucket/aggregate parity), stamp
   * every active key with the exact active-policy ACK (the cutover trigger requires it), then
   * stage the durable strict control job — the worker delivers the engine activation and
   * re-stamps keys on the new head. From the ACK on, the engine bills the materialized
   * per-provider policy instead of the legacy scalar, and the dashboard projection follows.
   * The engine enforces its side atomically (funding parity, key ACKs, drained legacy
   * reservations, policy-capable instances), so an unmet precondition fails the delivery job
   * loudly instead of producing a partial state.
   */
  async cutoverUserPolicyToStrict(input: {
    userId: string;
    reason: string;
    actorId: string;
  }): Promise<Record<string, unknown>> {
    const target = await getAdminUserControlTarget(this.database, input.userId);
    if (!target) throw new AdminOperationError(404, "user not found");
    if (!target.engineAccountId) throw new AdminOperationError(409, "user has no engine account");
    const accountId = target.engineAccountId;

    const plan = await this.engine.getFundingNormalizationPlanV2(accountId);
    let funding: "normalized" | "already_normalized" | "nothing_to_normalize";
    if (plan === null) {
      funding = "nothing_to_normalize";
    } else if (plan.status === "normalized") {
      funding = "already_normalized";
    } else if (plan.status === "blocked") {
      const blockers = plan.blockers.map((blocker) => `${blocker.code}: ${blocker.detail}`).join("; ");
      throw new AdminOperationError(409, `funding normalization is blocked: ${blockers}`);
    } else {
      if (plan.normalization_digest === null) {
        throw new AdminOperationError(409, "engine account has no funding normalization target");
      }
      const applied = await this.engine.applyFundingNormalizationV2(accountId, {
        expected_source_state_digest: plan.source_state_digest,
        expected_normalization_digest: plan.normalization_digest,
      });
      if (applied === null) throw new AdminOperationError(409, "funding normalization target vanished");
      funding = "normalized";
    }

    const state = await this.engine.getAccountPricingState(accountId);
    if (typeof state !== "object" || state === null || !("active" in state)) {
      throw new AdminOperationError(409, "account has no active engine policy to cut over");
    }
    if (state.active.binding.policy_enforcement !== "strict") {
      const ack = {
        effectivePolicyVersion: state.active.policy.effective_version,
        policyDigest: state.active.policy.content_digest,
      };
      const keys = await this.engine.listKeys(accountId);
      for (const key of keys) {
        if (key.status !== "active") continue;
        await this.engine.setKeyStatus(key.key_id, "active", ack);
      }
    }

    let staged;
    try {
      staged = await stageAccountStrictCutoverJob(this.database, { userId: input.userId });
    } catch (error) {
      if (error instanceof AccountStrictCutoverError) {
        throw new AdminOperationError(error.code === "no_binding" ? 404 : 409, error.message);
      }
      throw error;
    }
    return {
      user_id: input.userId,
      account_id: accountId,
      cutover: staged.status,
      job_id: staged.jobId,
      job_status: staged.jobStatus,
      effective_version: staged.effectiveVersion,
      funding,
    };
  }

  async revokeSessions(input: { userId: string; reason: string; actorId: string }): Promise<Record<string, unknown>> {
    const count = await revokeAdminUserSessions(this.database, input);
    return { user_id: input.userId, sessions_revoked: count };
  }

  async resetTotp(input: { userId: string; reason: string; actorId: string }): Promise<Record<string, unknown>> {
    const count = await resetAdminUserTotp(this.database, input);
    return { user_id: input.userId, totp_enabled: false, sessions_revoked: count };
  }
}

export class AdminOperationError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
    this.name = "AdminOperationError";
  }
}

/** Integer nano-USD string to an exact decimal USD string without floating-point conversion. */
export function nanoToUsd(nano: string): string {
  const negative = nano.startsWith("-");
  const digits = (negative ? nano.slice(1) : nano).padStart(10, "0");
  const whole = digits.slice(0, -9);
  const fraction = digits.slice(-9).replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole}${fraction ? `.${fraction}` : ""}`;
}
