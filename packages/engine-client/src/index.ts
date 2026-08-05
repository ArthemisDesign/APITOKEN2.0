import { Buffer } from "node:buffer";
import JSONbigFactory from "json-bigint";
import {
  accountPolicyBindingSchema,
  accountPolicySpecSchema,
  engineAccountSchema,
  engineAccountListSchema,
  engineApiKeyListSchema,
  engineCreditResultSchema,
  engineLedgerSchema,
  engineSpendStatsSchema,
  engineUsageSchema,
  fundingNormalizationApplyRequestV2Schema,
  fundingNormalizationApplyResultV2Schema,
  fundingNormalizationPlanV2Schema,
  issuedEngineApiKeySchema,
  lockedOpenkeysPolicyTransitionIdentitySchema,
  lockedOpenkeysPolicyTransitionRequestSchema,
  policyActiveExpectationSchema,
  pricingActiveExpectationSchema,
  pricingCatalogSpecSchema,
  pricingMutationAckSchema,
  pricingPolicySnapshotSchema,
  pricingReleaseAssignmentExtensionIdentityV2Schema,
  pricingReleaseAssignmentExtensionV2Schema,
  pricingReleaseActivationAckV2Schema,
  pricingReleaseActivationRequestV2Schema,
  pricingReleaseHeadV2Schema,
  pricingReleaseInventoryPageV2Schema,
  pricingReleasePolicyV2Schema,
  pricingReleaseProvisioningContextEnvelopeV2Schema,
  pricingReleaseRecoveryLinkV2Schema,
  pricingReleaseV2Schema,
  pricingStage8CaptureRequestV2Schema,
  pricingStage8EngineEvidenceV2Schema,
  providerSwitchSpecSchema,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type CreateEngineAccount,
  type EngineAccount,
  type EngineApiKey,
  type EngineCreditResult,
  type EngineLedgerEntry,
  type EngineSpendStats,
  type EngineUsage,
  type FundingNormalizationApplyRequestV2,
  type FundingNormalizationApplyResultV2,
  type FundingNormalizationPlanV2,
  type IssuedEngineApiKey,
  type LockedOpenkeysPolicyTransitionIdentity,
  type LockedOpenkeysPolicyTransitionRequest,
  type PolicyActiveExpectation,
  type PricingActiveExpectation,
  type PricingCatalogSpec,
  type PricingMutationAck,
  type PricingPolicySnapshot,
  type PricingReleaseAssignmentExtensionIdentityV2,
  type PricingReleaseAssignmentExtensionV2,
  type PricingReleaseActivationAckV2,
  type PricingReleaseActivationRequestV2,
  type PricingReleaseHeadV2,
  type PricingReleaseInventoryPageV2,
  type PricingReleasePolicyV2,
  type PricingReleaseProvisioningContextV2,
  type PricingReleaseRecoveryLinkV2,
  type PricingReleaseV2,
  type PricingStage8CaptureRequestV2,
  type PricingStage8EngineEvidenceV2,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import { z } from "zod";

export {
  assertOpenKeysCatalog,
  assertOpenKeysSwitches,
  buildOfficialOpenKeysPolicy,
  canonicalPricingJson,
  OFFICIAL_ONE_TO_ONE_CONTRACT,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  officialOpenKeysBinding,
  OPENKEYS_POLICY_PROVIDERS,
  OpenKeysPolicyError,
  stage7OpenKeysDigest,
  type OpenKeysPricingAuthority,
} from "./openkeys-policy.js";

export {
  buildOpenKeysPricingReleasePolicyV2,
  buildPricingReleaseAssignmentExtensionV2,
  buildServicePricingReleasePolicyV2,
  canonicalPricingReleaseV2Json,
  ensureOpenKeysPricingReleaseProvisioningV2,
  ensureServicePricingReleaseProvisioningV2,
  PricingReleaseAccountProvisioningV2Error,
  pricingReleaseV2Digest,
  type PricingReleaseAccountProvisioningResultV2,
  type PricingReleaseProvisioningTransportV2,
} from "./release-provisioning.js";

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

export interface PricingReleaseInventoryV2Options {
  afterAccountId?: string;
  limit?: number;
}

export type TypedPricingMutationAck<Identity> =
  | { result: "stored" | "applied" | "unchanged"; identity: Identity }
  | (Extract<PricingMutationAck, { result: "rejected" }> & { identity: Identity });

const maxSignedI64 = 9_223_372_036_854_775_807n;
const catalogPrepareIdentitySchema = z.object({ catalog: pricingCatalogSpecSchema }).strict();
const catalogActivationIdentitySchema = z.object({
  catalog: pricingCatalogSpecSchema,
  expectation: pricingActiveExpectationSchema,
}).strict();
const switchPrepareIdentitySchema = z.object({ switches: providerSwitchSpecSchema }).strict();
const switchActivationIdentitySchema = z.object({
  switches: providerSwitchSpecSchema,
  expectation: pricingActiveExpectationSchema,
}).strict();
const policyPrepareIdentitySchema = z.object({ policy: accountPolicySpecSchema }).strict();
const policyActivationIdentitySchema = z.object({
  policy: accountPolicySpecSchema,
  activation: z.object({
    account_id: z.string(),
    effective_version: z.number().int().safe().positive(),
    content_digest: z.string(),
    binding: accountPolicyBindingSchema,
  }).strict(),
  expectation: policyActiveExpectationSchema,
}).strict();
const releasePolicyV2PrepareIdentitySchema = pricingReleasePolicyV2Schema.pick({
  policy_id: true,
  policy_version: true,
  content_digest: true,
});
const releaseV2PrepareIdentitySchema = pricingReleaseV2Schema.pick({
  generation: true,
  content_digest: true,
  release_kind: true,
});
const releaseRecoveryLinkV2PrepareIdentitySchema = pricingReleaseRecoveryLinkV2Schema.pick({
  target_generation: true,
  recovery_generation: true,
  link_digest: true,
});
const releaseInventoryLimitV2Schema = z.number().int().min(1).max(500);
const fundingNormalizationAccountIdV2Schema = z.string().startsWith("acct_").max(200);
const stage8EvidenceMaxResponseBytes = 16 * 1024 * 1024;
// One extra attempt for idempotent GET reads: engine blue-green slot cutovers fail individual
// requests for a sub-second window, and pausing briefly lets the health-gated origin settle.
const transientGetRetryDelayMs = 300;

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

  /**
   * Списание (отрицательный credit движка): отзыв welcome-бонуса и подобные корректировки.
   * `amountNano` — положительная величина списания; идемпотентно по `ref` (UNIQUE в леджере).
   */
  async debitAccount(accountId: string, amountNano: bigint, reference: string): Promise<EngineCreditResult> {
    if (amountNano <= 0n) throw new RangeError("amountNano must be positive");
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
    const { response, payload } = await this.request(`/admin/key-id/${encodeURIComponent(keyId)}/status`, {
      method: "POST",
      body: JSON.stringify({ status }),
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

  async preparePricingCatalog(
    input: PricingCatalogSpec,
  ): Promise<TypedPricingMutationAck<{ catalog: PricingCatalogSpec }>> {
    const catalog = pricingCatalogSpecSchema.parse(input);
    return this.pricingMutation(
      "/admin/pricing/catalog/prepare",
      catalog,
      catalogPrepareIdentitySchema,
      { catalog },
    );
  }

  async getPricingCatalogVersion(
    productId: string,
    generation: number,
  ): Promise<PricingCatalogSpec | null> {
    const target = pricingCatalogSpecSchema.pick({ product_id: true, generation: true }).parse({
      product_id: productId,
      generation,
    });
    const { response, payload } = await this.request(
      `/admin/pricing/catalog/${encodeURIComponent(target.product_id)}/version/${target.generation}`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    return this.parsePricingResponse(
      z.object({ catalog: pricingCatalogSpecSchema }).strict(),
      payload,
      response,
    ).catalog;
  }

  async getActivePricingCatalog(productId: string): Promise<PricingCatalogSpec | null> {
    const target = pricingCatalogSpecSchema.pick({ product_id: true }).parse({ product_id: productId });
    const { response, payload } = await this.request(
      `/admin/pricing/catalog/${encodeURIComponent(target.product_id)}/active`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    return this.parsePricingResponse(
      z.object({ catalog: pricingCatalogSpecSchema }).strict(),
      payload,
      response,
    ).catalog;
  }

  async activatePricingCatalog(
    input: PricingCatalogSpec,
    expectation: PricingActiveExpectation,
  ): Promise<TypedPricingMutationAck<z.infer<typeof catalogActivationIdentitySchema>>> {
    const catalog = pricingCatalogSpecSchema.parse(input);
    const expected = pricingActiveExpectationSchema.parse(expectation);
    return this.pricingMutation(
      `/admin/pricing/catalog/${encodeURIComponent(catalog.product_id)}/activate`,
      { catalog, expectation: expected },
      catalogActivationIdentitySchema,
      { catalog, expectation: expected },
    );
  }

  async prepareProviderSwitches(
    input: ProviderSwitchSpec,
  ): Promise<TypedPricingMutationAck<{ switches: ProviderSwitchSpec }>> {
    const switches = providerSwitchSpecSchema.parse(input);
    return this.pricingMutation(
      "/admin/pricing/switches/prepare",
      switches,
      switchPrepareIdentitySchema,
      { switches },
    );
  }

  async getProviderSwitchVersion(generation: number): Promise<ProviderSwitchSpec | null> {
    if (!Number.isSafeInteger(generation) || generation <= 0) {
      throw new RangeError("generation must be a positive safe integer");
    }
    const { response, payload } = await this.request(
      `/admin/pricing/switches/version/${generation}`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    return this.parsePricingResponse(
      z.object({ switches: providerSwitchSpecSchema }).strict(),
      payload,
      response,
    ).switches;
  }

  async getActiveProviderSwitches(): Promise<ProviderSwitchSpec | null> {
    const { response, payload } = await this.request(
      "/admin/pricing/switches/active",
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    return this.parsePricingResponse(
      z.object({ switches: providerSwitchSpecSchema }).strict(),
      payload,
      response,
    ).switches;
  }

  async activateProviderSwitches(
    input: ProviderSwitchSpec,
    expectation: PricingActiveExpectation,
  ): Promise<TypedPricingMutationAck<z.infer<typeof switchActivationIdentitySchema>>> {
    const switches = providerSwitchSpecSchema.parse(input);
    const expected = pricingActiveExpectationSchema.parse(expectation);
    return this.pricingMutation(
      "/admin/pricing/switches/activate",
      { switches, expectation: expected },
      switchActivationIdentitySchema,
      { switches, expectation: expected },
    );
  }

  async prepareAccountPolicy(
    input: AccountPolicySpec,
  ): Promise<TypedPricingMutationAck<{ policy: AccountPolicySpec }>> {
    const policy = accountPolicySpecSchema.parse(input);
    return this.pricingMutation(
      "/admin/pricing/policy/prepare",
      policy,
      policyPrepareIdentitySchema,
      { policy },
    );
  }

  async getAccountPolicyVersion(
    accountId: string,
    effectiveVersion: number,
  ): Promise<AccountPolicySpec | null> {
    if (!Number.isSafeInteger(effectiveVersion) || effectiveVersion <= 0) {
      throw new RangeError("effectiveVersion must be a positive safe integer");
    }
    const { response, payload } = await this.request(
      `/admin/pricing/policy/${encodeURIComponent(accountId)}/version/${effectiveVersion}`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    return this.parsePricingResponse(
      z.object({ policy: accountPolicySpecSchema }).strict(),
      payload,
      response,
    ).policy;
  }

  async getActiveAccountPolicy(accountId: string): Promise<{
    policy: AccountPolicySpec;
    binding: AccountPolicyBinding;
  } | null> {
    const { response, payload } = await this.request(
      `/admin/pricing/policy/${encodeURIComponent(accountId)}/active`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    return this.parsePricingResponse(z.object({
      active: z.object({
        policy: accountPolicySpecSchema,
        binding: accountPolicyBindingSchema,
      }).strict(),
    }).strict(), payload, response).active;
  }

  async getAccountPricingState(accountId: string): Promise<PricingPolicySnapshot> {
    const { response, payload } = await this.request(
      `/admin/pricing/policy/${encodeURIComponent(accountId)}/state`,
    );
    const state = this.parsePricingResponse(z.object({
      state: z.object({
        account_id: z.string(),
        policy: pricingPolicySnapshotSchema,
      }).passthrough(),
    }).strict(), payload, response).state;
    if (state.account_id !== accountId) {
      throw new EngineClientError("engine returned pricing state for a different account", response.status, false);
    }
    return state.policy;
  }

  async activateAccountPolicy(
    input: AccountPolicySpec,
    binding: AccountPolicyBinding,
    expectation: PolicyActiveExpectation,
  ): Promise<TypedPricingMutationAck<z.infer<typeof policyActivationIdentitySchema>>> {
    const policy = accountPolicySpecSchema.parse(input);
    const targetBinding = accountPolicyBindingSchema.parse(binding);
    const expected = policyActiveExpectationSchema.parse(expectation);
    const ack = await this.pricingMutation(
      `/admin/pricing/policy/${encodeURIComponent(policy.account_id)}/activate`,
      { policy, binding: targetBinding, expectation: expected },
      policyActivationIdentitySchema,
      undefined,
    );
    const expectedIdentity = {
      policy,
      activation: {
        account_id: policy.account_id,
        effective_version: policy.effective_version,
        content_digest: policy.content_digest,
        binding: targetBinding,
      },
      expectation: expected,
    };
    if (JSON.stringify(ack.identity) !== JSON.stringify(expectedIdentity)) {
      throw new EngineClientError("engine pricing ACK identity does not match the request", undefined, false);
    }
    return ack;
  }

  async lockedOpenkeysPolicyTransition(
    accountId: string,
    input: LockedOpenkeysPolicyTransitionRequest,
  ): Promise<TypedPricingMutationAck<LockedOpenkeysPolicyTransitionIdentity>> {
    const targetAccountId = fundingNormalizationAccountIdV2Schema.parse(accountId);
    const request = lockedOpenkeysPolicyTransitionRequestSchema.parse(input);
    if (request.policy.account_id !== targetAccountId) {
      throw new EngineClientError(
        "locked OpenKeys transition policy does not match the target account",
        undefined,
        false,
      );
    }
    const expectedIdentity: LockedOpenkeysPolicyTransitionIdentity = {
      policy: request.policy,
      active: {
        target: {
          version: request.policy.effective_version,
          content_digest: request.policy.content_digest,
        },
        binding: {
          policy_enforcement: "shadow",
          funding_enforcement: "legacy_single",
          reconciliation_state: "verified",
        },
      },
      expected_active: request.expected_active,
    };
    return this.pricingMutation(
      `/admin/pricing/policy/${encodeURIComponent(targetAccountId)}/locked-openkeys-transition`,
      request,
      lockedOpenkeysPolicyTransitionIdentitySchema,
      expectedIdentity,
    );
  }

  async preparePricingReleasePolicyV2(
    input: PricingReleasePolicyV2,
  ): Promise<TypedPricingMutationAck<z.infer<typeof releasePolicyV2PrepareIdentitySchema>>> {
    const policy = pricingReleasePolicyV2Schema.parse(input);
    const identity = releasePolicyV2PrepareIdentitySchema.parse({
      policy_id: policy.policy_id,
      policy_version: policy.policy_version,
      content_digest: policy.content_digest,
    });
    return this.pricingMutation(
      "/admin/pricing/v2/policy/prepare",
      policy,
      releasePolicyV2PrepareIdentitySchema,
      identity,
    );
  }

  async getPricingReleasePolicyV2(
    policyId: string,
    policyVersion: number,
  ): Promise<PricingReleasePolicyV2 | null> {
    const target = releasePolicyV2PrepareIdentitySchema
      .omit({ content_digest: true })
      .parse({ policy_id: policyId, policy_version: policyVersion });
    const { response, payload } = await this.request(
      `/admin/pricing/v2/policy/${encodeURIComponent(target.policy_id)}/version/${target.policy_version}`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    const policy = this.parsePricingResponse(
      z.object({ policy: pricingReleasePolicyV2Schema }).strict(),
      payload,
      response,
    ).policy;
    if (policy.policy_id !== target.policy_id || policy.policy_version !== target.policy_version) {
      throw new EngineClientError("engine returned a different pricing release policy", response.status, false);
    }
    return policy;
  }

  async preparePricingReleaseV2(
    input: PricingReleaseV2,
  ): Promise<TypedPricingMutationAck<z.infer<typeof releaseV2PrepareIdentitySchema>>> {
    const release = pricingReleaseV2Schema.parse(input);
    const identity = releaseV2PrepareIdentitySchema.parse({
      generation: release.generation,
      content_digest: release.content_digest,
      release_kind: release.release_kind,
    });
    return this.pricingMutation(
      "/admin/pricing/v2/release/prepare",
      release,
      releaseV2PrepareIdentitySchema,
      identity,
    );
  }

  async getPricingReleaseV2(generation: number): Promise<PricingReleaseV2 | null> {
    const targetGeneration = pricingReleaseV2Schema.shape.generation.parse(generation);
    const { response, payload } = await this.request(
      `/admin/pricing/v2/release/${targetGeneration}`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    const release = this.parsePricingResponse(
      z.object({ release: pricingReleaseV2Schema }).strict(),
      payload,
      response,
    ).release;
    if (release.generation !== targetGeneration) {
      throw new EngineClientError("engine returned a different pricing release", response.status, false);
    }
    return release;
  }

  async preparePricingReleaseRecoveryLinkV2(
    input: PricingReleaseRecoveryLinkV2,
  ): Promise<TypedPricingMutationAck<z.infer<typeof releaseRecoveryLinkV2PrepareIdentitySchema>>> {
    const recoveryLink = pricingReleaseRecoveryLinkV2Schema.parse(input);
    if (recoveryLink.recovery_generation <= recoveryLink.target_generation) {
      throw new RangeError("recovery_generation must be newer than target_generation");
    }
    const identity = releaseRecoveryLinkV2PrepareIdentitySchema.parse({
      target_generation: recoveryLink.target_generation,
      recovery_generation: recoveryLink.recovery_generation,
      link_digest: recoveryLink.link_digest,
    });
    return this.pricingMutation(
      "/admin/pricing/v2/recovery-link/prepare",
      recoveryLink,
      releaseRecoveryLinkV2PrepareIdentitySchema,
      identity,
    );
  }

  async getPricingReleaseRecoveryLinkV2(
    targetGeneration: number,
    recoveryGeneration: number,
  ): Promise<PricingReleaseRecoveryLinkV2 | null> {
    const generations = releaseRecoveryLinkV2PrepareIdentitySchema
      .omit({ link_digest: true })
      .parse({
        target_generation: targetGeneration,
        recovery_generation: recoveryGeneration,
      });
    if (generations.recovery_generation <= generations.target_generation) {
      throw new RangeError("recoveryGeneration must be newer than targetGeneration");
    }
    const { response, payload } = await this.request(
      `/admin/pricing/v2/recovery-link/${generations.target_generation}/${generations.recovery_generation}`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    const recoveryLink = this.parsePricingResponse(
      z.object({ recovery_link: pricingReleaseRecoveryLinkV2Schema }).strict(),
      payload,
      response,
    ).recovery_link;
    if (recoveryLink.target_generation !== generations.target_generation ||
        recoveryLink.recovery_generation !== generations.recovery_generation) {
      throw new EngineClientError("engine returned a different pricing recovery link", response.status, false);
    }
    return recoveryLink;
  }

  async preparePricingReleaseAssignmentExtensionV2(
    input: PricingReleaseAssignmentExtensionV2,
  ): Promise<TypedPricingMutationAck<PricingReleaseAssignmentExtensionIdentityV2>> {
    const extension = pricingReleaseAssignmentExtensionV2Schema.parse(input);
    const identity = pricingReleaseAssignmentExtensionIdentityV2Schema.parse({
      provisioning_head_generation: extension.provisioning_head_generation,
      provisioning_head_version: extension.provisioning_head_version,
      account_id: extension.members[0]!.assignment.account_id,
      extension_group_digest: extension.extension_group_digest,
    });
    return this.pricingMutation(
      "/admin/pricing/v2/assignment-extension/prepare",
      extension,
      pricingReleaseAssignmentExtensionIdentityV2Schema,
      identity,
    );
  }

  async getPricingReleaseAssignmentExtensionV2(
    provisioningHeadVersion: number,
    accountId: string,
  ): Promise<PricingReleaseAssignmentExtensionV2 | null> {
    const target = pricingReleaseAssignmentExtensionIdentityV2Schema
      .pick({ provisioning_head_version: true, account_id: true })
      .parse({ provisioning_head_version: provisioningHeadVersion, account_id: accountId });
    const { response, payload } = await this.request(
      `/admin/pricing/v2/assignment-extension/${target.provisioning_head_version}/${encodeURIComponent(target.account_id)}`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    const extension = this.parsePricingResponse(
      z.object({ extension: pricingReleaseAssignmentExtensionV2Schema }).strict(),
      payload,
      response,
    ).extension;
    if (extension.provisioning_head_version !== target.provisioning_head_version
        || extension.members.some((member) => member.assignment.account_id !== target.account_id)) {
      throw new EngineClientError("engine returned a different pricing assignment extension", response.status, false);
    }
    return extension;
  }

  async getPricingReleaseHeadV2(): Promise<PricingReleaseHeadV2 | null> {
    const { response, payload } = await this.request("/admin/pricing/v2/head");
    return this.parsePricingResponse(
      z.object({ head: pricingReleaseHeadV2Schema.nullable() }).strict(),
      payload,
      response,
    ).head;
  }

  async getPricingReleaseProvisioningContextV2(): Promise<PricingReleaseProvisioningContextV2 | null> {
    const { response, payload } = await this.request("/admin/pricing/v2/provisioning-context");
    return this.parsePricingResponse(
      pricingReleaseProvisioningContextEnvelopeV2Schema,
      payload,
      response,
    ).context;
  }

  /**
   * Captures one read-only engine Stage 8 artifact. The exact raw response is returned alongside
   * the strict normalized view so the commerce worker can durably preserve and independently
   * verify the canonical integer-preserving evidence digest.
   */
  async capturePricingStage8EvidenceV2(
    input: PricingStage8CaptureRequestV2,
  ): Promise<{ evidence: PricingStage8EngineEvidenceV2; raw: string }> {
    const request = pricingStage8CaptureRequestV2Schema.parse(input);
    const { response, payload, rawText } = await this.request(
      "/admin/pricing/v2/stage8-evidence/capture",
      {
        method: "POST",
        body: JSON.stringify(request),
        maxResponseBytes: stage8EvidenceMaxResponseBytes,
      },
    );
    const evidence = this.parsePricingResponse(pricingStage8EngineEvidenceV2Schema, payload, response);
    if (
      evidence.release.target_generation !== String(request.target_generation)
      || evidence.release.recovery_generation !== String(request.recovery_generation)
      || evidence.window_start_ts !== String(request.window_start_ts)
      || evidence.window_end_ts !== String(request.window_end_ts)
      || evidence.min_samples_per_provider !== String(request.min_samples_per_provider)
      || evidence.gemini_client_admissions !== String(request.gemini_client_admissions)
      || evidence.financial_samples.length > request.financial_sample_size
    ) {
      throw new EngineClientError(
        "engine Stage 8 evidence does not match the explicit capture request",
        response.status,
        false,
      );
    }
    return { evidence, raw: rawText };
  }

  async activatePricingReleaseV2(
    input: PricingReleaseActivationRequestV2,
  ): Promise<PricingReleaseActivationAckV2> {
    const request = pricingReleaseActivationRequestV2Schema.parse(input);
    const { response, payload } = await this.request("/admin/pricing/v2/activate", {
      method: "POST",
      body: JSON.stringify(request),
      acceptedStatuses: [400, 409],
    });
    const ack = this.parsePricingResponse(pricingReleaseActivationAckV2Schema, payload, response);
    if (ack.result === "rejected") return ack;
    assertPricingReleaseActivationReceipt(request, ack);
    return ack;
  }

  /** Returns one bounded page. Callers building release evidence must exhaust the cursor. */
  async getPricingReleaseInventoryV2(
    options: PricingReleaseInventoryV2Options = {},
  ): Promise<PricingReleaseInventoryPageV2> {
    const query: string[] = [];
    if (options.afterAccountId !== undefined) {
      const cursor = pricingReleaseInventoryPageV2Schema.shape.next_after_account_id
        .unwrap()
        .parse(options.afterAccountId);
      query.push(`after_account_id=${encodeURIComponent(cursor)}`);
    }
    if (options.limit !== undefined) {
      const limit = releaseInventoryLimitV2Schema.parse(options.limit);
      query.push(`limit=${limit}`);
    }
    const suffix = query.length === 0 ? "" : `?${query.join("&")}`;
    const { response, payload } = await this.request(`/admin/pricing/v2/inventory${suffix}`);
    return this.parsePricingResponse(
      z.object({ inventory: pricingReleaseInventoryPageV2Schema }).strict(),
      payload,
      response,
    ).inventory;
  }

  async getFundingNormalizationPlanV2(
    accountId: string,
  ): Promise<FundingNormalizationPlanV2 | null> {
    const targetAccountId = fundingNormalizationAccountIdV2Schema.parse(accountId);
    const { response, payload } = await this.request(
      `/admin/pricing/v2/funding/${encodeURIComponent(targetAccountId)}/normalization`,
      { acceptedStatuses: [404] },
    );
    if (response.status === 404) return null;
    const normalization = this.parsePricingResponse(
      z.object({ normalization: fundingNormalizationPlanV2Schema }).strict(),
      payload,
      response,
    ).normalization;
    this.assertAccount(normalization.account_id, targetAccountId, response);
    return normalization;
  }

  async applyFundingNormalizationV2(
    accountId: string,
    input: FundingNormalizationApplyRequestV2,
  ): Promise<FundingNormalizationApplyResultV2 | null> {
    const targetAccountId = fundingNormalizationAccountIdV2Schema.parse(accountId);
    const request = fundingNormalizationApplyRequestV2Schema.parse(input);
    const { response, payload } = await this.request(
      `/admin/pricing/v2/funding/${encodeURIComponent(targetAccountId)}/normalization`,
      {
        method: "POST",
        body: JSON.stringify(request),
        acceptedStatuses: [404, 409],
      },
    );
    if (response.status === 404) return null;
    const result = this.parsePricingResponse(
      z.object({ result: fundingNormalizationApplyResultV2Schema }).strict(),
      payload,
      response,
    ).result;
    this.assertAccount(result.normalization.account_id, targetAccountId, response);
    const success = result.status === "stored" || result.status === "unchanged";
    if (success !== response.ok) {
      throw new EngineClientError("engine returned an inconsistent funding normalization status", response.status, false);
    }
    return result;
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
      maxResponseBytes?: number;
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
      maxResponseBytes?: number;
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
      const text = await this.readResponseText(response, options.maxResponseBytes);
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

  private async readResponseText(response: Response, maxBytes?: number): Promise<string> {
    if (maxBytes === undefined) return response.text();
    if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
      throw new RangeError("maxResponseBytes must be a positive safe integer");
    }
    if (response.body === null) return "";
    const reader = response.body.getReader();
    const chunks: Buffer[] = [];
    let bytes = 0;
    try {
      for (;;) {
        const chunk = await reader.read();
        if (chunk.done) break;
        bytes += chunk.value.byteLength;
        if (bytes > maxBytes) {
          await reader.cancel("bounded engine response exceeded");
          throw new EngineClientError(
            "engine Stage 8 evidence exceeds the bounded response size",
            response.status,
            false,
          );
        }
        chunks.push(Buffer.from(chunk.value));
      }
    } finally {
      reader.releaseLock();
    }
    return Buffer.concat(chunks, bytes).toString("utf8");
  }

  private async pricingMutation<Identity>(
    path: string,
    body: unknown,
    identitySchema: z.ZodType<Identity>,
    expectedIdentity: Identity | undefined,
  ): Promise<TypedPricingMutationAck<Identity>> {
    const { response, payload } = await this.request(path, {
      method: "POST",
      body: JSON.stringify(body),
      acceptedStatuses: [400, 409, 423],
    });
    const ack = this.parsePricingResponse(pricingMutationAckSchema, payload, response);
    const identity = this.parsePricingResponse(identitySchema, ack.identity, response);
    if (expectedIdentity !== undefined && JSON.stringify(identity) !== JSON.stringify(expectedIdentity)) {
      throw new EngineClientError("engine pricing ACK identity does not match the request", undefined, false);
    }
    return {
      ...ack,
      identity,
    } as TypedPricingMutationAck<Identity>;
  }

  private parsePricingResponse<Schema extends z.ZodTypeAny>(
    schema: Schema,
    payload: unknown,
    response: Response,
  ): z.output<Schema> {
    const result = schema.safeParse(payload);
    if (!result.success) {
      throw new EngineClientError("engine returned a malformed pricing response", response.status, false);
    }
    return result.data as z.output<Schema>;
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

function assertPricingReleaseActivationReceipt(
  request: PricingReleaseActivationRequestV2,
  ack: Extract<PricingReleaseActivationAckV2, { result: "applied" | "unchanged" }>,
): void {
  const receipt = ack.activation;
  const expectation = request.expectation;
  const expectedFrom = expectation === "absent"
    ? { generation: null, digest: null, headVersion: 0 }
    : {
        generation: expectation.exact.active_generation,
        digest: expectation.exact.active_digest,
        headVersion: expectation.exact.head_version,
      };
  const expectedHead = request.activation_kind === "cutover"
    ? {
        generation: request.evidence.target_generation,
        digest: request.evidence.target_digest,
      }
    : {
        generation: request.evidence.recovery_generation,
        digest: request.evidence.recovery_digest,
      };
  if (
    receipt.activation_kind !== request.activation_kind
    || receipt.from_generation !== expectedFrom.generation
    || receipt.from_digest !== expectedFrom.digest
    || receipt.expected_head_version !== expectedFrom.headVersion
    || receipt.head.active_generation !== expectedHead.generation
    || receipt.head.active_digest !== expectedHead.digest
    || receipt.head.head_version !== expectedFrom.headVersion + 1
    || receipt.head.updated_ts !== receipt.activated_ts
    || receipt.evidence_digest !== request.evidence.evidence_digest
    || receipt.operator_id !== request.operator_id
    || receipt.reason !== request.reason
  ) {
    throw new EngineClientError(
      "engine pricing release activation receipt does not match the immutable request",
      undefined,
      false,
    );
  }
}
