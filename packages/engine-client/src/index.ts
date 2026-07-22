import JSONbigFactory from "json-bigint";
import {
  engineAccountSchema,
  engineApiKeyListSchema,
  engineCreditResultSchema,
  engineLedgerSchema,
  engineUsageSchema,
  issuedEngineApiKeySchema,
  type CreateEngineAccount,
  type EngineAccount,
  type EngineApiKey,
  type EngineCreditResult,
  type EngineLedgerEntry,
  type EngineUsage,
  type IssuedEngineApiKey,
} from "@claude-api/contracts";

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

  async creditAccount(accountId: string, amountNano: bigint, reference: string): Promise<EngineCreditResult> {
    if (amountNano <= 0n) throw new RangeError("amountNano must be positive");
    const body = JSON.stringify({ amount_nano: amountNano.toString(), ref: reference });
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

  async disableKey(keyId: string): Promise<void> {
    const { response, payload } = await this.request(`/admin/key-id/${encodeURIComponent(keyId)}/status`, {
      method: "POST",
      body: '{"status":"disabled"}',
    });
    const result = payload as Record<string, unknown>;
    if (result.key_id !== keyId || result.status !== "disabled" || result.updated !== 1) {
      throw new EngineClientError("engine returned an invalid key status response", response.status, false);
    }
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

  async getUsage(accountId: string, window = "30d"): Promise<EngineUsage> {
    if (!/^(all|\d+[dh])$/.test(window)) throw new RangeError("window must be like 30d, 7d, 24h, or all");
    const { response, payload } = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/usage?window=${encodeURIComponent(window)}`,
    );
    const usage = engineUsageSchema.parse(payload);
    this.assertAccount(usage.account, accountId, response);
    return usage;
  }

  async getLedgerAfter(accountId: string, afterId: bigint, limit = 1000): Promise<EngineLedgerEntry[]> {
    if (afterId < 0n) throw new RangeError("afterId must not be negative");
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
    options: { method?: string; body?: string; authenticated?: boolean } = {},
  ): Promise<{ response: Response; payload: unknown }> {
    const controller = new AbortController();
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
      return { response, payload: this.parse(response, text) };
    } catch (error) {
      if (error instanceof EngineClientError) throw error;
      const timedOut = controller.signal.aborted || (error instanceof Error && error.name === "AbortError");
      const message = timedOut
        ? "engine request timed out"
        : response === undefined
          ? "engine request failed"
          : "engine response body failed";
      throw new EngineClientError(message, response?.status, true);
    } finally {
      clearTimeout(timeout);
    }
  }

  private assertAccount(actualAccountId: string, expectedAccountId: string, response: Response): void {
    if (actualAccountId !== expectedAccountId) {
      throw new EngineClientError("engine returned a response for a different account", response.status, false);
    }
  }

  private parse(response: Response, text: string): unknown {
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
    if (!response.ok) {
      const error = typeof payload === "object" && payload !== null && "error" in payload
        ? String((payload as { error: unknown }).error)
        : `engine returned HTTP ${response.status}`;
      throw new EngineClientError(error, response.status, response.status >= 500 || response.status === 429);
    }
    return payload;
  }
}
