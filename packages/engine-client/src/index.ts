import JSONbigFactory from "json-bigint";
import {
  engineAccountSchema,
  engineCreditResultSchema,
  type CreateEngineAccount,
  type EngineAccount,
  type EngineCreditResult,
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
