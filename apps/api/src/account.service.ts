import { Inject, Injectable } from "@nestjs/common";
import {
  completeEngineAccount,
  failEngineAccount,
  findOwnedApiKey,
  getEngineAccountMapping,
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
    const mapping = await getEngineAccountMapping(this.database, userId);
    if (!mapping) throw new EngineAccountUnavailableError("engine account mapping is missing");
    if (mapping.status === "disabled") throw new EngineAccountUnavailableError("engine account is disabled");
    if (mapping.status === "active" && mapping.engineAccountId) return mapping.engineAccountId;

    try {
      const account = await createFundedEngineAccount(this.engine, {
        userId,
        customerType: mapping.customerType,
        handle: `user:${userId}`,
        multBp: mapping.multBp,
      });
      await completeEngineAccount(this.database, userId, account.account);
      return account.account;
    } catch (error) {
      await failEngineAccount(
        this.database,
        userId,
        error instanceof Error ? error.message : "engine account provisioning failed",
      );
      throw new EngineAccountUnavailableError("engine account is temporarily unavailable", { cause: error });
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
      const stored = await syncEngineApiKey(this.database, {
        userId,
        engineAccountId: accountId,
        engineKeyId: key.key_id,
        label: key.label,
        keyMasked: key.key_masked,
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
