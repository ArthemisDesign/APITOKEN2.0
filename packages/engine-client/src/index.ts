import { Buffer } from "node:buffer";
import JSONbigFactory from "json-bigint";
import {
  engineAccountSchema,
  engineAccountListSchema,
  engineApiKeyListSchema,
  engineCreditResultSchema,
  engineLedgerSchema,
  engineSpendStatsSchema,
  engineUsageSchema,
  issuedEngineApiKeySchema,
  type CreateEngineAccount,
  type EngineAccount,
  type EngineApiKey,
  type EngineCreditResult,
  type EngineLedgerEntry,
  type EngineSpendStats,
  type EngineUsage,
  type IssuedEngineApiKey,
} from "@claude-api/contracts";
import { z } from "zod";

const JSONbig = JSONbigFactory({ storeAsString: true, useNativeBigInt: false });

export class EngineClientError extends Error {
  constructor(
    message: string,
    readonly status: number | undefined,
    readonly retryable: boolean,
  ) {
    super(message);
    this.name = "EngineClientError";
  }
}

export interface EngineClientOptions {
  baseUrl: string;
  controlKey: string;
  timeoutMs?: number;
  fetch?: typeof globalThis.fetch;
}

export interface IssueEngineKeyOptions {
  label?: string;
  spendLimitNano?: bigint;
  expiresAt?: Date;
}

export interface ReplaceEngineKeyPolicyOptions {
  spendLimitNano: bigint | null;
  expiresAt: Date | null;
}

const maxSignedI64 = 9_223_372_036_854_775_807n;
// One extra attempt for idempotent GET reads: engine blue-green slot cutovers fail individual
// requests for a sub-second window, and pausing briefly lets the health-gated origin settle.
const transientGetRetryDelayMs = 300;

function assertMonetaryOperation(amountNano: bigint, reference: string): void {
  if (amountNano <= 0n || amountNano > maxSignedI64) {
    throw new RangeError("amountNano must be a positive signed i64");
  }
  if (!/^[^\s:]+:\S+$/u.test(reference)) {
    throw new RangeError("reference must be provider-qualified as <provider>:<transaction-id>");
  }
}

function waitForRetry(delayMs: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) {
    return Promise.reject(new EngineClientError("engine request aborted", undefined, false));
  }
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      signal?.removeEventListener("abort", abort);
      resolve();
    }, delayMs);
    const abort = () => {
      clearTimeout(timeout);
      reject(new EngineClientError("engine request aborted", undefined, false));
    };
    signal?.addEventListener("abort", abort, { once: true });
  });
}

export class EngineClient {
  private readonly baseUrl: string;
  private readonly controlKey: string;
  private readonly timeoutMs: number;
  private readonly fetchImpl: typeof globalThis.fetch;

  constructor(options: EngineClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/$/, "");
    this.controlKey = options.controlKey;
    this.timeoutMs = options.timeoutMs ?? 10_000;
    this.fetchImpl = options.fetch ?? globalThis.fetch;
  }

  async health(): Promise<boolean> {
    try {
      const { response } = await this.request("/health", { authenticated: false });
      return response.ok;
    } catch {
      return false;
    }
  }

  async readiness(): Promise<boolean> {
    try {
      const { response } = await this.request("/ready", { authenticated: false });
      return response.status === 200;
    } catch {
      return false;
    }
  }

  async createAccount(input: CreateEngineAccount): Promise<{ account: string; multBp: number; handle: string | null }> {
    const body: Record<string, unknown> = {};
    if (input.handle !== undefined) body.handle = input.handle;
    if (input.multBp !== undefined) body.mult_bp = input.multBp;
    const { response, payload } = await this.request("/admin/account", {
      method: "POST",
      body: JSON.stringify(body),
    });
    const result = payload as Record<string, unknown>;
    if (typeof result.account !== "string" || typeof result.mult_bp !== "number") {
      throw new EngineClientError("engine returned an invalid account response", response.status, false);
    }
    return {
      account: result.account,
      multBp: result.mult_bp,
      handle: typeof result.handle === "string" ? result.handle : null,
    };
  }

  async getAccount(accountId: string): Promise<EngineAccount> {
    const { response, payload } = await this.request(`/admin/account/${encodeURIComponent(accountId)}`);
    const account = engineAccountSchema.parse(payload);
    this.assertAccount(account.account, accountId, response);
    return account;
  }

  async getAccounts(accountIds: string[]): Promise<EngineAccount[]> {
    const uniqueIds = [...new Set(accountIds)];
    if (uniqueIds.length === 0) return [];
    if (uniqueIds.length > 500 || uniqueIds.some((id) => !id.startsWith("acct_") || id.length > 200)) {
      throw new RangeError("accountIds must contain 1 to 500 valid engine account IDs");
    }
    const { payload } = await this.request("/admin/accounts/query", {
      method: "POST",
      body: JSON.stringify({ account_ids: uniqueIds }),
    });
    return engineAccountListSchema.parse(payload).accounts;
  }

  async creditAccount(accountId: string, amountNano: bigint, reference: string): Promise<EngineCreditResult> {
    assertMonetaryOperation(amountNano, reference);
    const body = JSON.stringify({ amount_nano: amountNano.toString(), ref: reference });
    const { response, payload } = await this.request(`/admin/account/${encodeURIComponent(accountId)}/credit`, {
      method: "POST",
      body,
    });
    const result = engineCreditResultSchema.parse(payload);
    this.assertAccount(result.account, accountId, response);
    return result;
  }

  /**
   * Списание (отрицательный credit движка): отзыв welcome-бонуса и подобные корректировки.
   * `amountNano` — положительная величина списания; идемпотентно по `ref` (UNIQUE в леджере).
   */
  async debitAccount(accountId: string, amountNano: bigint, reference: string): Promise<EngineCreditResult> {
    assertMonetaryOperation(amountNano, reference);
    const body = JSON.stringify({ amount_nano: (-amountNano).toString(), ref: reference });
    const { response, payload } = await this.request(`/admin/account/${encodeURIComponent(accountId)}/credit`, {
      method: "POST",
      body,
    });
    const result = engineCreditResultSchema.parse(payload);
    this.assertAccount(result.account, accountId, response);
    return result;
  }

  async setAccountStatus(accountId: string, status: "active" | "disabled"): Promise<void> {
    const { response, payload } = await this.request(`/admin/account/${encodeURIComponent(accountId)}/status`, {
      method: "POST",
      body: JSON.stringify({ status }),
    });
    const result = payload as Record<string, unknown>;
    if (result.account !== accountId || result.status !== status || result.updated !== 1) {
      throw new EngineClientError("engine returned an invalid account status response", response.status, false);
    }
  }

  async listKeys(accountId: string): Promise<EngineApiKey[]> {
    const { response, payload } = await this.request(`/admin/account/${encodeURIComponent(accountId)}/keys`);
    const result = engineApiKeyListSchema.parse(payload);
    this.assertAccount(result.account, accountId, response);
    return result.keys;
  }

  async issueKey(accountId: string, options: IssueEngineKeyOptions = {}): Promise<IssuedEngineApiKey> {
    if (options.spendLimitNano !== undefined &&
        (options.spendLimitNano <= 0n || options.spendLimitNano > maxSignedI64)) {
      throw new RangeError("spendLimitNano must be a positive signed 64-bit integer");
    }
    if (options.expiresAt !== undefined &&
        (!Number.isFinite(options.expiresAt.getTime()) ||
         Math.floor(options.expiresAt.getTime() / 1000) <= Math.floor(Date.now() / 1000))) {
      throw new RangeError("expiresAt must be a valid date at least one whole second in the future");
    }
    const body: Record<string, unknown> = { account_id: accountId };
    if (options.label !== undefined) body.label = options.label;
    if (options.spendLimitNano !== undefined) body.spend_limit_nano = options.spendLimitNano.toString();
    if (options.expiresAt !== undefined) body.expires_ts = Math.floor(options.expiresAt.getTime() / 1000);
    const { response, payload } = await this.request("/admin/key", {
      method: "POST",
      body: JSON.stringify(body),
    });
    const key = issuedEngineApiKeySchema.parse(payload);
    this.assertAccount(key.account, accountId, response);
    return key;
  }

  /** Движок принимает active|disabled, поэтому отключение обратимо. */
  async setKeyStatus(keyId: string, status: "active" | "disabled"): Promise<void> {
    const body: Record<string, unknown> = { status };
    const { response, payload } = await this.request(`/admin/key-id/${encodeURIComponent(keyId)}/status`, {
      method: "POST",
      body: JSON.stringify(body),
    });
    const result = payload as Record<string, unknown>;
    if (result.key_id !== keyId || result.status !== status || result.updated !== 1) {
      throw new EngineClientError("engine returned an invalid key status response", response.status, false);
    }
  }

  async disableKey(keyId: string): Promise<void> {
    await this.setKeyStatus(keyId, "disabled");
  }

  async renameKey(keyId: string, label: string): Promise<void> {
    const { response, payload } = await this.request(`/admin/key-id/${encodeURIComponent(keyId)}/label`, {
      method: "POST",
      body: JSON.stringify({ label }),
    });
    const result = payload as Record<string, unknown>;
    if (result.key_id !== keyId || result.updated !== 1) {
      throw new EngineClientError("engine returned an invalid key label response", response.status, false);
    }
  }

  async replaceKeyPolicy(
    accountId: string,
    keyId: string,
    options: ReplaceEngineKeyPolicyOptions,
  ): Promise<void> {
    if (options.spendLimitNano !== null &&
        (options.spendLimitNano <= 0n || options.spendLimitNano > maxSignedI64)) {
      throw new RangeError("spendLimitNano must be null or a positive signed 64-bit integer");
    }
    if (options.expiresAt !== null &&
        (!Number.isFinite(options.expiresAt.getTime()) ||
         Math.floor(options.expiresAt.getTime() / 1000) <= Math.floor(Date.now() / 1000))) {
      throw new RangeError("expiresAt must be null or a valid date at least one whole second in the future");
    }
    const body = {
      spend_limit_nano: options.spendLimitNano?.toString() ?? null,
      expires_ts: options.expiresAt === null ? null : Math.floor(options.expiresAt.getTime() / 1000),
    };
    const { response, payload } = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/key-id/${encodeURIComponent(keyId)}/policy`,
      { method: "POST", body: JSON.stringify(body) },
    );
    const result = payload as Record<string, unknown>;
    if (result.key_id !== keyId || result.updated !== 1) {
      throw new EngineClientError("engine returned an invalid key policy response", response.status, false);
    }
  }

  async getLedger(accountId: string, limit = 50): Promise<EngineLedgerEntry[]> {
    if (!Number.isInteger(limit) || limit < 1 || limit > 1000) throw new RangeError("limit must be an integer from 1 to 1000");
    const { response, payload } = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/ledger?limit=${limit}`,
    );
    const result = engineLedgerSchema.parse(payload);
    this.assertAccount(result.account, accountId, response);
    return result.entries;
  }

  async getUsage(
    accountId: string,
    window = "30d",
    options: { signal?: AbortSignal } = {},
  ): Promise<EngineUsage> {
    if (!/^(all|\d+[dh])$/.test(window)) throw new RangeError("window must be like 30d, 7d, 24h, or all");
    const { response, payload } = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/usage?window=${encodeURIComponent(window)}`,
      options.signal === undefined ? {} : { signal: options.signal },
    );
    const usage = engineUsageSchema.parse(payload);
    this.assertAccount(usage.account, accountId, response);
    return usage;
  }

  /**
   * Операторская сводка расхода всего флота (24ч/7д/30д): провайдеры, top-модели и top-аккаунты.
   * Единственный источник для админской страницы «Расход движка» — включает аккаунты без
   * commerce-юзера (OpenKeys, внутренние), которых нет ни в одной таблице коммерции.
   */
  async getSpendStats(): Promise<EngineSpendStats> {
    const { payload } = await this.request("/spend-stats");
    return engineSpendStatsSchema.parse(payload);
  }

  async getLedgerAfter(accountId: string, afterId: bigint, limit = 1000): Promise<EngineLedgerEntry[]> {    if (afterId < 0n) throw new RangeError("afterId must not be negative");
    if (!Number.isInteger(limit) || limit < 1 || limit > 1000) throw new RangeError("limit must be an integer from 1 to 1000");
    const { response, payload } = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/ledger?after_id=${afterId.toString()}&limit=${limit}`,
    );
    const result = engineLedgerSchema.parse(payload);
    this.assertAccount(result.account, accountId, response);
    return result.entries;
  }

  async acknowledgeLedger(accountId: string, lastId: bigint): Promise<void> {
    if (lastId < 0n) throw new RangeError("lastId must not be negative");
    await this.request(`/admin/account/${encodeURIComponent(accountId)}/ledger/ack`, {
      method: "POST",
      body: JSON.stringify({ last_id: lastId.toString() }),
    });
  }


























  /**
   * Set (`multiplierBp` a number) or clear (`null`) one provider's discount override. Absent
   * override means the account default prices that provider — the engine resolves
   * `override ?? default` on every request, so a write is live immediately.
   */
  async setAccountProviderDiscount(
    accountId: string,
    providerId: string,
    multiplierBp: number | null,
  ): Promise<void> {
    if (multiplierBp !== null
      && (!Number.isInteger(multiplierBp) || multiplierBp < 0 || multiplierBp > 10_000)) {
      throw new RangeError("multiplierBp must be an integer from 0 to 10000, or null");
    }
    const { response, payload } = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/discounts`,
      { method: "POST", body: JSON.stringify({ provider_id: providerId, mult_bp: multiplierBp }) },
    );
    const result = payload as Record<string, unknown>;
    if (result.account !== accountId || result.provider_id !== providerId) {
      throw new EngineClientError("engine returned an invalid discount response", response.status, false);
    }
  }

  /** The account default plus every per-provider override, as the engine currently prices them. */
  async getAccountDiscounts(accountId: string): Promise<{
    multiplierBp: number;
    providers: Record<string, number>;
  }> {
    const { response, payload } = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/discounts`,
      { method: "GET" },
    );
    const result = payload as Record<string, unknown>;
    if (result.account !== accountId || typeof result.mult_bp !== "number") {
      throw new EngineClientError("engine returned an invalid discount response", response.status, false);
    }
    const providers: Record<string, number> = {};
    for (const [providerId, value] of Object.entries((result.providers ?? {}) as Record<string, unknown>)) {
      if (typeof value === "number") providers[providerId] = value;
    }
    return { multiplierBp: result.mult_bp, providers };
  }

  async setAccountMultiplier(accountId: string, multiplierBp: number): Promise<void> {
    if (!Number.isInteger(multiplierBp) || multiplierBp < 0 || multiplierBp > 10_000) {
      throw new RangeError("multiplierBp must be an integer from 0 to 10000");
    }
    const { response, payload } = await this.request(`/admin/account/${encodeURIComponent(accountId)}/pricing`, {
      method: "POST",
      body: JSON.stringify({ mult_bp: multiplierBp }),
    });
    const result = payload as Record<string, unknown>;
    if (result.account !== accountId || result.mult_bp !== multiplierBp || result.updated !== 1) {
      throw new EngineClientError("engine returned an invalid pricing response", response.status, false);
    }
  }

  private async request(
    path: string,
    options: {
      method?: string;
      body?: string;
      authenticated?: boolean;
      acceptedStatuses?: readonly number[];
      signal?: AbortSignal;
    } = {},
  ): Promise<{ response: Response; payload: unknown; rawText: string }> {
    // Retry only idempotent GETs (the default method), exactly once, on failures the client
    // itself classified as retryable (network/timeout, HTTP >= 500, 429). Mutations are never
    // retried here: issueKey, disableKey and pricing writes are not safe to replay blindly.
    const maxAttempts = (options.method ?? "GET") === "GET" ? 2 : 1;
    for (let attempt = 1; ; attempt++) {
      try {
        return await this.attemptRequest(path, options);
      } catch (error) {
        if (options.signal?.aborted
          || attempt >= maxAttempts
          || !(error instanceof EngineClientError && error.retryable)) {
          throw error;
        }
        await waitForRetry(transientGetRetryDelayMs, options.signal);
      }
    }
  }

  private async attemptRequest(
    path: string,
    options: {
      method?: string;
      body?: string;
      authenticated?: boolean;
      acceptedStatuses?: readonly number[];
      signal?: AbortSignal;
    },
  ): Promise<{ response: Response; payload: unknown; rawText: string }> {
    const controller = new AbortController();
    const abortFromCaller = () => controller.abort(options.signal?.reason);
    if (options.signal?.aborted) abortFromCaller();
    else options.signal?.addEventListener("abort", abortFromCaller, { once: true });
    const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
    const headers: Record<string, string> = { accept: "application/json" };
    if (options.body !== undefined) headers["content-type"] = "application/json";
    if (options.authenticated !== false) headers["x-api-key"] = this.controlKey;
    let response: Response | undefined;
    try {
      const request: RequestInit = {
        method: options.method ?? "GET",
        headers,
        signal: controller.signal,
      };
      if (options.body !== undefined) request.body = options.body;
      response = await this.fetchImpl(`${this.baseUrl}${path}`, request);
      const text = await response.text();
      return {
        response,
        payload: this.parse(response, text, options.acceptedStatuses ?? []),
        rawText: text,
      };
    } catch (error) {
      if (error instanceof EngineClientError) throw error;
      const abortedByCaller = options.signal?.aborted === true;
      const timedOut = controller.signal.aborted || (error instanceof Error && error.name === "AbortError");
      const message = abortedByCaller
        ? "engine request aborted"
        : timedOut
          ? "engine request timed out"
          : response === undefined
            ? "engine request failed"
            : "engine response body failed";
      throw new EngineClientError(message, response?.status, !abortedByCaller);
    } finally {
      clearTimeout(timeout);
      options.signal?.removeEventListener("abort", abortFromCaller);
    }
  }

  private assertAccount(actualAccountId: string, expectedAccountId: string, response: Response): void {
    if (actualAccountId !== expectedAccountId) {
      throw new EngineClientError("engine returned a response for a different account", response.status, false);
    }
  }

  private parse(response: Response, text: string, acceptedStatuses: readonly number[]): unknown {
    let payload: unknown;
    try {
      payload = JSONbig.parse(text);
    } catch {
      if (!response.ok) {
        throw new EngineClientError(
          `engine returned HTTP ${response.status} with a non-JSON response`,
          response.status,
          response.status >= 500 || response.status === 429,
        );
      }
      throw new EngineClientError("engine returned invalid JSON", response.status, response.status >= 500);
    }
    if (!response.ok && !acceptedStatuses.includes(response.status)) {
      const error = typeof payload === "object" && payload !== null && "error" in payload
        ? String((payload as { error: unknown }).error)
        : `engine returned HTTP ${response.status}`;
      throw new EngineClientError(error, response.status, response.status >= 500 || response.status === 429);
    }
    return payload;
  }
}
