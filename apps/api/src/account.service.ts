import { Inject, Injectable, NotFoundException } from "@nestjs/common";
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
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";
import { createFundedEngineAccount } from "./engine-provisioning.js";

export class EngineAccountUnavailableError extends Error {}

@Injectable()
export class AccountService {
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
        SELECT ea.engine_account_id, ea.status, ea.mult_bp,
               COALESCE(cp.customer_type, 'b2c') AS customer_type,
               EXISTS (
                 SELECT 1 FROM auth_identities ai
                 WHERE ai.user_id = ea.user_id AND ai.provider IN ('google', 'github')
               ) AS welcome_bonus_eligible
        FROM engine_accounts ea
        LEFT JOIN customer_profiles cp ON cp.user_id = ea.user_id
        WHERE ea.user_id = $1
        FOR UPDATE OF ea
      `, [userId]);
      const row = mappingResult.rows[0];
      const mapping = row ? {
        engineAccountId: row.engine_account_id,
        status: row.status,
        multBp: row.mult_bp,
        customerType: row.customer_type,
        welcomeBonusEligible: row.welcome_bonus_eligible,
      } : null;
      if (!mapping) throw new EngineAccountUnavailableError("engine account mapping is missing");
      if (mapping.status === "disabled") throw new EngineAccountUnavailableError("engine account is disabled");
      if (mapping.status === "active" && mapping.engineAccountId) {
        await client.query("COMMIT");
        transactionOpen = false;
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
          welcomeBonusEligible: mapping.welcomeBonusEligible,
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
        SET engine_account_id = $3, status = 'active', last_error = NULL, updated_at = now()
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
        balanceAfterNano: entry.balance_after_nano,
        timestamp: entry.ts,
      })),
    };
  }

  async getUsage(userId: string, window: string): Promise<unknown> {
    const usage = await this.withEngineAccount(userId, (accountId) => this.engine.getUsage(accountId, window));
    return {
      window: usage.window,
      requests: usage.requests,
      totalOfficialNano: usage.total_official_nano,
      totalChargedNano: usage.total_charged_nano,
      buckets: {
        input: { tokens: usage.buckets.input.tokens, officialNano: usage.buckets.input.official_nano },
        output: { tokens: usage.buckets.output.tokens, officialNano: usage.buckets.output.official_nano },
        cacheRead: { tokens: usage.buckets.cache_read.tokens, officialNano: usage.buckets.cache_read.official_nano },
        cacheWrite: { tokens: usage.buckets.cache_write.tokens, officialNano: usage.buckets.cache_write.official_nano },
        webSearch: { requests: usage.buckets.web_search.requests, officialNano: usage.buckets.web_search.official_nano },
      },
      models: usage.models.map((model) => ({
        model: model.model,
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
      keys.push(apiKeyView(stored, key.spent_nano, key.spent));
    }
    return { keys };
  }

  async createApiKey(userId: string, label?: string): Promise<unknown> {
    const { accountId, value: issued } = await this.withEngineAccountId(
      userId,
      (id) => this.engine.issueKey(id, label),
    );
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
    return { ...apiKeyView(stored, "0", "$0.000000000"), key: issued.key };
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
    return apiKeyView(stored, updated.spent_nano, updated.spent);
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
  customer_type: "b2c" | "b2b";
  welcome_bonus_eligible: boolean;
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

function apiKeyView(stored: StoredApiKey, spentNano: string, spentUsd: string): Record<string, unknown> {
  return {
    id: stored.id,
    label: stored.label,
    keyMasked: stored.keyMasked,
    status: stored.status,
    spentNano,
    spentUsd,
    createdAt: stored.createdAt.toISOString(),
  };
}

function maskApiKey(key: string): string {
  if (key.length <= 16) return `${key.slice(0, 6)}…`;
  return `${key.slice(0, 12)}…${key.slice(-4)}`;
}

export function isRetryableEngineFailure(error: unknown): boolean {
  return error instanceof EngineAccountUnavailableError ||
    (error instanceof EngineClientError && (error.retryable || error.status === 404));
}

function isMissingEngineAccount(error: unknown): boolean {
  return error instanceof EngineClientError && (
    error.status === 404 || (error.status === 400 && error.message.includes("unknown account"))
  );
}
