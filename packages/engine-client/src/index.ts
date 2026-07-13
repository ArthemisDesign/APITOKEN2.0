import JSONbigFactory from "json-bigint";
import {
  engineAccountSchema,
  engineApiKeyListSchema,
  engineCreditResultSchema,
  engineLedgerSchema,
  issuedEngineApiKeySchema,
  type CreateEngineAccount,
  type EngineAccount,
  type EngineApiKey,
  type EngineCreditResult,
  type EngineLedgerEntry,
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
      const response = await this.request("/health", { authenticated: false });
      return response.ok;
    } catch {
      return false;
    }
  }

  async createAccount(input: CreateEngineAccount): Promise<{ account: string; multBp: number; handle: string | null }> {
    const body: Record<string, unknown> = {};
    if (input.handle !== undefined) body.handle = input.handle;
    if (input.multBp !== undefined) body.mult_bp = input.multBp;
    const response = await this.request("/admin/account", { method: "POST", body: JSON.stringify(body) });
    const payload = this.parse(response, await response.text()) as Record<string, unknown>;
    if (typeof payload.account !== "string" || typeof payload.mult_bp !== "number") {
      throw new EngineClientError("engine returned an invalid account response", response.status, false);
    }
    return {
      account: payload.account,
      multBp: payload.mult_bp,
      handle: typeof payload.handle === "string" ? payload.handle : null,
    };
  }

  async getAccount(accountId: string): Promise<EngineAccount> {
    const response = await this.request(`/admin/account/${encodeURIComponent(accountId)}`);
    return engineAccountSchema.parse(this.parse(response, await response.text()));
  }

  async creditAccount(accountId: string, amountNano: bigint, reference: string): Promise<EngineCreditResult> {
    if (amountNano <= 0n) throw new RangeError("amountNano must be positive");
    const body = `{"amount_nano":${amountNano.toString()},"ref":${JSON.stringify(reference)}}`;
    const response = await this.request(`/admin/account/${encodeURIComponent(accountId)}/credit`, {
      method: "POST",
      body,
    });
    return engineCreditResultSchema.parse(this.parse(response, await response.text()));
  }

  async listKeys(accountId: string): Promise<EngineApiKey[]> {
    const response = await this.request(`/admin/account/${encodeURIComponent(accountId)}/keys`);
    const result = engineApiKeyListSchema.parse(this.parse(response, await response.text()));
    return result.keys;
  }

  async issueKey(accountId: string, label?: string): Promise<IssuedEngineApiKey> {
    const body: Record<string, unknown> = { account_id: accountId };
    if (label !== undefined) body.label = label;
    const response = await this.request("/admin/key", { method: "POST", body: JSON.stringify(body) });
    return issuedEngineApiKeySchema.parse(this.parse(response, await response.text()));
  }

  async disableKey(keyId: string): Promise<void> {
    const response = await this.request(`/admin/key-id/${encodeURIComponent(keyId)}/status`, {
      method: "POST",
      body: '{"status":"disabled"}',
    });
    const payload = this.parse(response, await response.text()) as Record<string, unknown>;
    if (payload.key_id !== keyId || payload.status !== "disabled" || payload.updated !== 1) {
      throw new EngineClientError("engine returned an invalid key status response", response.status, false);
    }
  }

  async getLedger(accountId: string, limit = 50): Promise<EngineLedgerEntry[]> {
    if (!Number.isInteger(limit) || limit < 1 || limit > 1000) throw new RangeError("limit must be an integer from 1 to 1000");
    const response = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/ledger?limit=${limit}`,
    );
    const result = engineLedgerSchema.parse(this.parse(response, await response.text()));
    return result.entries;
  }

  async getLedgerAfter(accountId: string, afterId: bigint, limit = 1000): Promise<EngineLedgerEntry[]> {
    if (afterId < 0n) throw new RangeError("afterId must not be negative");
    if (!Number.isInteger(limit) || limit < 1 || limit > 1000) throw new RangeError("limit must be an integer from 1 to 1000");
    const response = await this.request(
      `/admin/account/${encodeURIComponent(accountId)}/ledger?after_id=${afterId.toString()}&limit=${limit}`,
    );
    const result = engineLedgerSchema.parse(this.parse(response, await response.text()));
    return result.entries;
  }

  async setAccountMultiplier(accountId: string, multiplierBp: number): Promise<void> {
    if (!Number.isInteger(multiplierBp) || multiplierBp < 0 || multiplierBp > 10_000) {
      throw new RangeError("multiplierBp must be an integer from 0 to 10000");
    }
    const response = await this.request(`/admin/account/${encodeURIComponent(accountId)}/pricing`, {
      method: "POST",
      body: JSON.stringify({ mult_bp: multiplierBp }),
    });
    const payload = this.parse(response, await response.text()) as Record<string, unknown>;
    if (payload.account !== accountId || payload.mult_bp !== multiplierBp || payload.updated !== 1) {
      throw new EngineClientError("engine returned an invalid pricing response", response.status, false);
    }
  }

  private async request(
    path: string,
    options: { method?: string; body?: string; authenticated?: boolean } = {},
  ): Promise<Response> {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
    const headers: Record<string, string> = { accept: "application/json" };
    if (options.body !== undefined) headers["content-type"] = "application/json";
    if (options.authenticated !== false) headers["x-api-key"] = this.controlKey;
    try {
      const request: RequestInit = {
        method: options.method ?? "GET",
        headers,
        signal: controller.signal,
      };
      if (options.body !== undefined) request.body = options.body;
      return await this.fetchImpl(`${this.baseUrl}${path}`, request);
    } catch (error) {
      const message = error instanceof Error && error.name === "AbortError"
        ? "engine request timed out"
        : "engine request failed";
      throw new EngineClientError(message, undefined, true);
    } finally {
      clearTimeout(timeout);
    }
  }

  private parse(response: Response, text: string): unknown {
    let payload: unknown;
    try {
      payload = JSONbig.parse(text);
    } catch {
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
