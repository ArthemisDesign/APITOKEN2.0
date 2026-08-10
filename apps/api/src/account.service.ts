import { BadRequestException, ConflictException, Inject, Injectable, Logger, NotFoundException } from "@nestjs/common";
import {
  findOwnedApiKey,
  getPricingView,
  markEngineAccountMissing,
  markOwnedApiKeyDisabled,
  saveIssuedApiKey,
  syncEngineApiKey,
  type Database,
  type StoredApiKey,
} from "@claude-api/db";
import { EngineClient, EngineClientError } from "@claude-api/engine-client";
import {
  B2C_LEGACY_SIGNUP_BONUS_BALANCE_NANO,
  type CreateApiKey,
  type EngineApiKey,
  type UpdateApiKeyPolicy,
} from "@claude-api/contracts";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";
import { createFundedEngineAccount } from "./engine-provisioning.js";
import { settleSignupBonus } from "./signup-bonus.js";

export class EngineAccountUnavailableError extends Error {}

@Injectable()
export class AccountService {
  private readonly logger = new Logger(AccountService.name);

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
  ) {}

  async ensureEngineAccount(userId: string): Promise<string> {
    const client = await this.database.pool.connect();
    let transactionOpen = false;
    try {
      await client.query("BEGIN");
      transactionOpen = true;
      // AUDIT(C82): serialize provisioning across API instances without keeping lock state in process memory.
      await client.query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", [userId]);

      const mappingResult = await client.query<EngineAccountMappingRow>(`
        SELECT ea.engine_account_id, ea.status, ea.mult_bp, u.email,
               COALESCE(cp.customer_type, 'b2c') AS customer_type,
               COALESCE(sp.bonus_granted, false) AS bonus_granted,
               sp.flagged_reason,
               EXISTS (SELECT 1 FROM auth_identities ai WHERE ai.user_id = ea.user_id) AS has_oauth_identity,
               CASE WHEN COALESCE(sp.bonus_granted, false)
                 THEN COALESCE(sp.bonus_amount_nano, $2::bigint)
                 ELSE NULL
               END AS welcome_bonus_amount_nano
        FROM engine_accounts ea
        JOIN users u ON u.id = ea.user_id
        LEFT JOIN customer_profiles cp ON cp.user_id = ea.user_id
        LEFT JOIN signup_profiles sp ON sp.user_id = ea.user_id
        WHERE ea.user_id = $1
        FOR UPDATE OF ea
      `, [userId, B2C_LEGACY_SIGNUP_BONUS_BALANCE_NANO.toString()]);
      const row = mappingResult.rows[0];
      const mapping = row ? {
        engineAccountId: row.engine_account_id,
        status: row.status,
        multBp: row.mult_bp,
        email: row.email,
        customerType: row.customer_type,
        bonusGranted: row.bonus_granted,
        flaggedReason: row.flagged_reason,
        hasOAuthIdentity: row.has_oauth_identity,
        welcomeBonusAmountNano: row.welcome_bonus_amount_nano === null
          ? null
          : BigInt(row.welcome_bonus_amount_nano),
      } : null;
      if (!mapping) throw new EngineAccountUnavailableError("engine account mapping is missing");
      if (mapping.status === "disabled") throw new EngineAccountUnavailableError("engine account is disabled");
      if (mapping.status === "active" && mapping.engineAccountId) {
        await client.query("COMMIT");
        transactionOpen = false;
        // Отложенный welcome-бонус: профиль чист, но клейм ещё не прошёл — например, аккаунт
        // активировался worker'ом уже после OAuth-регистрации, когда гейт видел pending.
        // Только OAuth-регистрации: password-аккаунт без auth_identities бонус не получает
        // (settleSignupBonus дублирует этот гейт как defense in depth).
        // Best-effort: ошибка зачисления не ломает доступ к аккаунту — клейм освобождён,
        // следующий вызов повторит.
        if (mapping.customerType === "b2c" && mapping.hasOAuthIdentity
          && !mapping.bonusGranted && mapping.flaggedReason === null) {
          try {
            await settleSignupBonus(this.database, this.engine, {
              userId,
              email: mapping.email,
              customerType: mapping.customerType,
            });
          } catch (error) {
            this.logger.warn(`deferred signup bonus settlement failed for user ${userId}: ${error instanceof Error ? error.message : String(error)}`);
          }
        }
        return mapping.engineAccountId;
      }
      if (mapping.status !== "pending" && mapping.status !== "error") {
        throw new EngineAccountUnavailableError("engine account mapping is inconsistent");
      }

      let account: Awaited<ReturnType<typeof createFundedEngineAccount>>;
      try {
        account = await createFundedEngineAccount(this.engine, {
          userId,
          customerType: mapping.customerType,
          handle: `user:${userId}`,
          multBp: mapping.multBp,
          welcomeBonusAmountNano: mapping.welcomeBonusAmountNano,
        });
      } catch (error) {
        // AUDIT(C65/C82): a failed attempt may only fail the exact state it observed.
        await client.query(`
          UPDATE engine_accounts
          SET status = 'error', last_error = $4, updated_at = now()
          WHERE user_id = $1
            AND status = $2
            AND engine_account_id IS NOT DISTINCT FROM $3
        `, [
          userId,
          mapping.status,
          mapping.engineAccountId,
          (error instanceof Error ? error.message : "engine account provisioning failed").slice(0, 1000),
        ]);
        await client.query("COMMIT");
        transactionOpen = false;
        throw new EngineAccountUnavailableError("engine account is temporarily unavailable", { cause: error });
      }

      // AUDIT(C65): never let an in-flight request overwrite a concurrent administrative disable.
      const completed = await client.query<{ engine_account_id: string }>(`
        UPDATE engine_accounts
        SET engine_account_id = $3, status = 'pending', last_error = NULL, updated_at = now()
        WHERE user_id = $1
          AND status = $2
          AND engine_account_id IS NOT DISTINCT FROM $4
        RETURNING engine_account_id
      `, [userId, mapping.status, account.account, mapping.engineAccountId]);
      await client.query("COMMIT");
      transactionOpen = false;

      if (!completed.rows[0]) {
        // AUDIT-TODO(C65): add an engine-client account-disable compensation call for a newly
        // provisioned account when this compare-and-set loses to an administrative disable.
        throw new EngineAccountUnavailableError("engine account state changed during provisioning");
      }
      return completed.rows[0].engine_account_id;
    } catch (error) {
      if (transactionOpen) await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }
  }


  async getAccount(userId: string): Promise<unknown> {
    const [account, pricing] = await Promise.all([
      this.withEngineAccount(userId, (accountId) => this.engine.getAccount(accountId)),
      getPricingView(this.database, userId),
    ]);
    return {
      balanceNano: account.balance_nano,
      reservedNano: account.reserved_nano,
      spentNano: account.spent_nano,
      balanceUsd: account.balance,
      markupBasisPoints: account.mult_bp,
      status: account.status,
      pricing,
    };
  }

  async getLedger(userId: string, limit: number): Promise<unknown> {
    const entries = await this.withEngineAccount(userId, (accountId) => this.engine.getLedger(accountId, limit));
    return {
      entries: entries.map((entry) => ({
        id: entry.id,
        kind: entry.kind,
        amountNano: entry.amount_nano,
        amountUsd: entry.amount,
        keyMasked: entry.key_masked,
        reference: entry.ref,
        model: entry.model ?? null,
        requestId: entry.request_id ?? null,
        provider: entry.provider ?? null,
        officialNano: entry.official_nano ?? null,
        balanceAfterNano: entry.balance_after_nano,
        timestamp: entry.ts,
      })),
    };
  }

  async getUsage(userId: string, window: string): Promise<unknown> {
    const usage = await this.withEngineAccount(userId, (accountId) => this.engine.getUsage(accountId, window));
    return {
      window: usage.window,
      sinceTs: usage.since_ts,
      untilTs: usage.until_ts,
      requests: usage.requests,
      totalOfficialNano: usage.total_official_nano,
      totalChargedNano: usage.total_charged_nano,
      buckets: {
        input: { tokens: usage.buckets.input.tokens, officialNano: usage.buckets.input.official_nano },
        output: { tokens: usage.buckets.output.tokens, officialNano: usage.buckets.output.official_nano },
        cacheRead: { tokens: usage.buckets.cache_read.tokens, officialNano: usage.buckets.cache_read.official_nano },
        cacheWrite: { tokens: usage.buckets.cache_write.tokens, officialNano: usage.buckets.cache_write.official_nano },
        webSearch: { requests: usage.buckets.web_search.requests, officialNano: usage.buckets.web_search.official_nano },
        unattributedLegacy: { officialNano: usage.buckets.unattributed_legacy.official_nano },
      },
      models: usage.models.map((model) => ({
        model: model.model,
        provider: model.provider ?? null,
        requests: model.requests,
        inputTokens: model.input_tokens,
        outputTokens: model.output_tokens,
        cacheReadTokens: model.cache_read_tokens,
        cacheWrite5mTokens: model.cache_write_5m_tokens,
        cacheWrite1hTokens: model.cache_write_1h_tokens,
        webSearchRequests: model.web_search_requests,
        officialNano: model.official_nano,
        chargedNano: model.charged_nano,
      })),
      daily: usage.daily.map((day) => ({
        dayTs: day.day_ts,
        requests: day.requests,
        officialNano: day.official_nano,
        chargedNano: day.charged_nano,
      })),
      dailyProviders: usage.daily_providers.map((day) => ({
        dayTs: day.day_ts,
        provider: day.provider,
        requests: day.requests,
        officialNano: day.official_nano,
        chargedNano: day.charged_nano,
      })),
      keys: usage.keys.map((key) => ({
        keyMasked: key.key_masked,
        requests: key.requests,
        officialNano: key.official_nano,
        chargedNano: key.charged_nano,
      })),
    };
  }

  async listApiKeys(userId: string): Promise<unknown> {
    const { accountId, value: engineKeys } = await this.withEngineAccountId(
      userId,
      (id) => this.engine.listKeys(id),
    );
    const keys: unknown[] = [];
    for (const key of engineKeys) {
      const keyMasked = validateMaskedApiKey(key.key_masked);
      const stored = await syncEngineApiKey(this.database, {
        userId,
        engineAccountId: accountId,
        engineKeyId: key.key_id,
        label: key.label,
        keyMasked,
        status: key.status,
      });
      keys.push(apiKeyView(stored, key));
    }
    return { keys };
  }

  async createApiKey(userId: string, input: CreateApiKey): Promise<unknown> {
    // A pending/error mapping may not have an engine account yet; materialize it before issuing.
    await this.ensureEngineAccount(userId);
    const spendLimitNano = input.spendLimitUsd === undefined ? undefined : usdToNano(input.spendLimitUsd);
    const expiresAt = input.expiresAt === undefined ? undefined : new Date(input.expiresAt);
    const { accountId, value: issued } = await this.withEngineAccountId(
      userId,
      async (id) => {
        return this.engine.issueKey(id, {
          ...(input.label !== undefined ? { label: input.label } : {}),
          ...(spendLimitNano !== undefined ? { spendLimitNano } : {}),
          ...(expiresAt !== undefined ? { expiresAt } : {}),
        });
      },
    );
    try {
      } catch (error) {
      try {
        await this.engine.disableKey(issued.key_id);
      } catch {
        throw new EngineAccountUnavailableError("pricing policy check and key compensation both failed", { cause: error });
      }
      throw error;
    }
    const keyMasked = maskApiKey(issued.key);
    let stored: StoredApiKey;
    try {
      stored = await saveIssuedApiKey(this.database, {
        userId,
        engineAccountId: accountId,
        engineKeyId: issued.key_id,
        label: issued.label,
        keyMasked,
      });
    } catch (error) {
      // Do not leave a usable orphan key if the commercial transaction cannot be persisted.
      try {
        await this.engine.disableKey(issued.key_id);
      } catch {
        throw new EngineAccountUnavailableError("API key persistence and compensation both failed", { cause: error });
      }
      throw error;
    }
    return {
      ...apiKeyView(stored, {
        key_id: issued.key_id,
        key_masked: keyMasked,
        label: issued.label,
        status: "active",
        spent_nano: "0",
        spent: "$0.000000000",
        reserved_nano: "0",
        spend_limit_nano: issued.spend_limit_nano,
        expires_ts: issued.expires_ts,
        created_ts: "0",
        last_used_ts: null,
      }),
      key: issued.key,
    };
  }

  async renameApiKey(userId: string, apiKeyId: string, label: string): Promise<unknown> {
    const owned = await findOwnedApiKey(this.database, userId, apiKeyId);
    if (!owned) throw new NotFoundException("API key not found");

    // The engine rename endpoint is global by key_id, so verify the key is still present on the
    // authenticated user's mapped account immediately before invoking it.
    const accountKeys = await this.engine.listKeys(owned.engineAccountId);
    if (!accountKeys.some((key) => key.key_id === owned.engineKeyId)) {
      throw new NotFoundException("API key not found");
    }

    await this.engine.renameKey(owned.engineKeyId, label);

    const updated = (await this.engine.listKeys(owned.engineAccountId))
      .find((key) => key.key_id === owned.engineKeyId);
    if (!updated) {
      throw new EngineClientError("engine omitted the renamed API key", undefined, false);
    }
    const stored = await syncEngineApiKey(this.database, {
      userId,
      engineAccountId: owned.engineAccountId,
      engineKeyId: updated.key_id,
      label: updated.label,
      keyMasked: validateMaskedApiKey(updated.key_masked),
      status: updated.status,
    });
    return apiKeyView(stored, updated);
  }

  async updateApiKeyPolicy(
    userId: string,
    apiKeyId: string,
    input: UpdateApiKeyPolicy,
  ): Promise<unknown> {
    const owned = await findOwnedApiKey(this.database, userId, apiKeyId);
    if (!owned) throw new NotFoundException("API key not found");

    const current = (await this.engine.listKeys(owned.engineAccountId))
      .find((key) => key.key_id === owned.engineKeyId);
    if (!current) throw new NotFoundException("API key not found");

    const spendLimitNano = input.spendLimitUsd === null ? null : usdToNano(input.spendLimitUsd);
    const committedNano = BigInt(current.spent_nano) + BigInt(current.reserved_nano);
    if (spendLimitNano !== null && spendLimitNano < committedNano) {
      throw new ConflictException("spend limit cannot be below billed and reserved usage");
    }
    try {
      await this.engine.replaceKeyPolicy(owned.engineAccountId, owned.engineKeyId, {
        spendLimitNano,
        expiresAt: input.expiresAt === null ? null : new Date(input.expiresAt),
      });
    } catch (error) {
      if (error instanceof EngineClientError && error.status === 404) {
        throw new NotFoundException("API key not found");
      }
      if (error instanceof EngineClientError && error.status === 409) {
        throw new ConflictException("spend limit cannot be below billed and reserved usage");
      }
      if (error instanceof EngineClientError && error.status === 400) {
        throw new BadRequestException("expiration must be in the future");
      }
      throw error;
    }

    const updated = (await this.engine.listKeys(owned.engineAccountId))
      .find((key) => key.key_id === owned.engineKeyId);
    if (!updated) throw new EngineClientError("engine omitted the updated API key", undefined, false);
    const stored = await syncEngineApiKey(this.database, {
      userId,
      engineAccountId: owned.engineAccountId,
      engineKeyId: updated.key_id,
      label: updated.label,
      keyMasked: validateMaskedApiKey(updated.key_masked),
      status: updated.status,
    });
    return apiKeyView(stored, updated);
  }

  async disableApiKey(userId: string, apiKeyId: string): Promise<boolean> {
    const owned = await findOwnedApiKey(this.database, userId, apiKeyId);
    if (!owned) return false;
    await this.engine.disableKey(owned.engineKeyId);
    return markOwnedApiKeyDisabled(this.database, userId, apiKeyId);
  }

  private async withEngineAccount<T>(userId: string, action: (accountId: string) => Promise<T>): Promise<T> {
    return (await this.withEngineAccountId(userId, action)).value;
  }




  private async withEngineAccountId<T>(
    userId: string,
    action: (accountId: string) => Promise<T>,
  ): Promise<{ accountId: string; value: T }> {
    const accountId = await this.ensureEngineAccount(userId);
    try {
      return { accountId, value: await action(accountId) };
    } catch (error) {
      if (!isMissingEngineAccount(error)) throw error;
      await markEngineAccountMissing(this.database, userId, accountId);
      const recoveredId = await this.ensureEngineAccount(userId);
      return { accountId: recoveredId, value: await action(recoveredId) };
    }
  }
}




interface EngineAccountMappingRow {
  engine_account_id: string | null;
  status: "pending" | "active" | "error" | "disabled";
  mult_bp: number;
  email: string;
  customer_type: "b2c" | "b2b";
  bonus_granted: boolean;
  flagged_reason: string | null;
  has_oauth_identity: boolean;
  welcome_bonus_amount_nano: string | null;
}

const rawPoolApiKeyPattern = /^sk-pool-[0-9a-f]{48}$/i;
const maskedPoolApiKeyPattern = /^sk-pool-[0-9a-f]{4}…[0-9a-f]{4}$/i;

function validateMaskedApiKey(value: string): string {
  // AUDIT(C64): defense in depth before the engine value can reach commerce storage or the browser.
  if (rawPoolApiKeyPattern.test(value) || !maskedPoolApiKeyPattern.test(value)) {
    throw new EngineClientError("engine returned an invalid masked API key", undefined, false);
  }
  return value;
}

function apiKeyView(stored: StoredApiKey, engine: EngineApiKey): Record<string, unknown> {
  return {
    id: stored.id,
    label: stored.label,
    keyMasked: stored.keyMasked,
    status: stored.status,
    spentNano: engine.spent_nano,
    spentUsd: engine.spent,
    reservedNano: engine.reserved_nano,
    spendLimitNano: engine.spend_limit_nano,
    expiresAt: secondsToIso(engine.expires_ts),
    lastUsedAt: secondsToIso(engine.last_used_ts),
    createdAt: secondsToIso(engine.created_ts) ?? stored.createdAt.toISOString(),
  };
}

function usdToNano(value: string): bigint {
  const [whole = "0", fraction = ""] = value.split(".");
  return BigInt(whole) * 1_000_000_000n + BigInt(fraction.padEnd(9, "0"));
}

function secondsToIso(value: string | null): string | null {
  if (value === null) return null;
  if (BigInt(value) <= 0n) return null;
  const milliseconds = Number(value) * 1000;
  return Number.isSafeInteger(milliseconds) ? new Date(milliseconds).toISOString() : null;
}

function maskApiKey(key: string): string {
  if (key.length <= 16) return `${key.slice(0, 6)}…`;
  return `${key.slice(0, 12)}…${key.slice(-4)}`;
}

// Total bounded wait is attempts x delay. Kept close to the worker's dispatch tick so the common
// case settles inside one request, without ever turning a dashboard load into a long hang.


export function isRetryableEngineFailure(error: unknown): boolean {
  // A 404 is deliberately NOT here. "No such account" is a permanent answer, and reporting it as
  // "engine is temporarily unavailable" tells the caller to retry something that can never
  // succeed. Provisioning handles its own missing-account window through isMissingEngineAccount,
  // which is the narrow case a 404 legitimately covers.
  return error instanceof EngineAccountUnavailableError ||
    (error instanceof EngineClientError && error.retryable);
}

// One-line diagnostic for the 503 mapping in the account/payments controllers: keeps the
// original engine failure recoverable from logs without exposing the control key or request
// bodies (EngineClientError messages are plain classifications like "engine request timed out").
export function describeEngineFailure(error: unknown): string {
  if (error instanceof EngineAccountUnavailableError) {
    const cause = error.cause instanceof Error
      ? `${error.cause.name}: ${error.cause.message}`
      : String(error.cause ?? "none");
    return `${error.message} (cause: ${cause})`;
  }
  if (error instanceof EngineClientError) {
    return `${error.message} (status: ${error.status ?? "none"}, retryable: ${error.retryable})`;
  }
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

function isMissingEngineAccount(error: unknown): boolean {
  return error instanceof EngineClientError && (
    error.status === 404 || (error.status === 400 && error.message.includes("unknown account"))
  );
}
