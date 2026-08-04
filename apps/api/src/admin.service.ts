import { Buffer } from "node:buffer";
import { createHash, randomBytes } from "node:crypto";
import { Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  multiplierForDiscount,
  pricingCatalogJobStageRequestV2Schema,
  pricingControlJobStageResponseV2Schema,
  pricingPolicyDeliveryRepairResponseV2Schema,
  pricingReleaseActivationStageResponseV2Schema,
  pricingStage5ControlResultV2Schema,
  pricingStage6StatusV2Schema,
  pricingStage8CaptureControlV2Schema,
  pricingStage8CaptureStageResponseV2Schema,
  pricingShadowRolloutControlV2Schema,
  pricingShadowRolloutStageResponseV2Schema,
  type PricingCatalogJobStageRequestV2,
  type PricingControlJobStageResponseV2,
  type PricingSwitchJobStageRequestV2,
  type PricingReleaseActivationStageRequestV2,
  type PricingPolicyDeliveryRepairRequestV2,
  type PricingPolicyDeliveryRepairResponseV2,
  type PricingStage5ControlResultV2,
  type PricingStage5MaterializeRequestV2,
  type PricingStage6StageRequestV2,
  type PricingStage6StatusV2,
  type PricingStage8CaptureStageRequestV2,
  type PricingShadowRolloutStageRequestV2,
  type PricingReleaseInventoryAccountV2,
  type PricingPolicyEditorRule,
  type ProviderSwitchEditorMutation,
  type ServiceAccountInventoryMutationV2,
} from "@claude-api/contracts";
import {
  BusinessCustomerNotFoundError,
  BusinessInvitationConflictError,
  BusinessInvitationNotFoundError,
  createStage5OpenKeysInventoryReaderV2,
  createBusinessInvite,
  decodeAuthEncryptionKey,
  decryptAuthToken,
  encryptAuthToken,
  evaluateRefundEligibility,
  getManagedPricingPolicy,
  getManagedPricingCatalog,
  getFundingNormalizationStageStatusV2,
  listManagedServicePricingPolicies,
  engineAccountIdentityInventoryDigestV2,
  getBusinessInviteToken,
  getEngineAccountMapping,
  listAdminUserOverview,
  recordAdminCredit,
  readServiceAccountInventoryV2,
  readPricingReleaseActivationControlV2,
  readPricingShadowRolloutControlV2,
  readPricingStage8CaptureControlV2,
  repairDeadPreCutoverPolicyDelivery,
  PricingPolicyDeliveryRepairError,
  runStage5MaterializerV2,
  syncPricingReleasePolicyOverrideV2,
  PricingReleaseProvisioningV2Error,
  revokeBusinessInvite,
  rotateBusinessInvite,
  setBusinessPricing,
  stagePricingReleaseActivationJobV2,
  stageFundingNormalizationJobV2,
  stagePricingShadowRolloutV2,
  stagePricingStage8CaptureJobV2,
  stageStoredPricingCatalogControlJob,
  stageStoredProviderSwitchControlJob,
  updateManagedPricingPolicy,
  updateManagedProviderSwitches,
  upsertServiceAccountInventoryV2,
  type AdminUserOverviewRow,
  type AdminUserOverviewQuery,
  type Database,
  type PricingReleaseActivationControlV2,
  type PricingShadowRolloutControlV2,
  type PricingStage8CaptureControlV2,
  type Stage5MaterializerV2Result,
} from "@claude-api/db";
import {
  EngineClient,
  ensureServicePricingReleaseProvisioningV2,
  PricingReleaseAccountProvisioningV2Error,
} from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";

export class AdminServiceAccountInventoryError extends Error {
  constructor(
    public readonly code:
      | "engine_account_missing"
      | "engine_inventory_unstable"
      | "account_owned_by_openkeys"
      | "pricing_release_not_ready",
    message: string,
  ) {
    super(message);
    this.name = "AdminServiceAccountInventoryError";
  }
}

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
    discountPercent?: number;
    policy?: { rules: readonly PricingPolicyEditorRule[] };
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
      multiplierBp: input.discountPercent === undefined ? 10_000 : multiplierForDiscount(input.discountPercent),
      expiresAt,
      idempotencyKey: input.idempotencyKey,
      actorId: input.actorId,
      reason: input.reason,
      ...(input.policy === undefined ? {} : { policyRules: input.policy.rules }),
    });
    const storedToken = invite.idempotentReplay
      ? decryptAuthToken(invite.encryptedToken, key)
      : token;
    const policy = input.policy === undefined ? null : await getManagedPricingPolicy(this.database, {
      ownerType: "b2b_invitation",
      ownerId: invite.id,
    });
    return {
      id: invite.id,
      email: invite.email,
      discountPercent: input.policy === undefined ? 100 - invite.multiplierBp / 100 : null,
      policy,
      expiresAt: invite.expiresAt.toISOString(),
      inviteUrl: this.inviteUrl(storedToken),
      deliveryStatus: invite.deliveryStatus,
      idempotentReplay: invite.idempotentReplay,
    };
  }

  async getManagedPricingPolicy(
    ownerType: "global_b2c" | "b2b_client" | "b2b_invitation" | "service",
    ownerId: string,
    productId?: string,
  ): Promise<unknown> {
    const policy = await getManagedPricingPolicy(this.database, {
      ownerType,
      ownerId,
      ...(productId ? { productId } : {}),
    });
    if (!policy) throw new BusinessCustomerNotFoundError("managed pricing policy not found");
    return policy;
  }

  async getManagedPricingCatalog(productId?: string): Promise<unknown> {
    return getManagedPricingCatalog(this.database, productId);
  }

  async listManagedServicePricingPolicies(): Promise<unknown> {
    return { policies: await listManagedServicePricingPolicies(this.database) };
  }

  async getServiceAccountInventoryV2(): Promise<unknown> {
    return readServiceAccountInventoryV2(this.database);
  }

  async dryRunPricingStage5V2(): Promise<PricingStage5ControlResultV2> {
    const result = await runStage5MaterializerV2(
      this.database,
      this.engine,
      this.stage5OpenKeysInventoryReaderV2(),
      { mode: "dry_run" },
    );
    return serializePricingStage5ControlResultV2(result);
  }

  async materializePricingStage5V2(
    input: PricingStage5MaterializeRequestV2,
    actorId: string,
  ): Promise<PricingStage5ControlResultV2> {
    const result = await runStage5MaterializerV2(
      this.database,
      this.engine,
      this.stage5OpenKeysInventoryReaderV2(),
      {
        mode: "apply",
        expectedPlanDigest: input.plan_digest,
        audit: { actorId, reason: input.reason },
      },
    );
    return serializePricingStage5ControlResultV2(result);
  }

  async getPricingStage6V2(planDigest: string): Promise<PricingStage6StatusV2> {
    return pricingStage6StatusV2Schema.parse(
      await getFundingNormalizationStageStatusV2(this.database, planDigest),
    );
  }

  async stagePricingStage6V2(
    input: PricingStage6StageRequestV2,
    actorId: string,
  ): Promise<PricingStage6StatusV2> {
    const stagedJobId = await stageFundingNormalizationJobV2(this.database, {
      planDigest: input.plan_digest,
      audit: { actorId, reason: input.reason },
    });
    const status = await getFundingNormalizationStageStatusV2(this.database, input.plan_digest);
    return pricingStage6StatusV2Schema.parse({ staged_job_id: stagedJobId, ...status });
  }

  async stagePricingCatalogJobV2(
    input: PricingCatalogJobStageRequestV2,
    actorId: string,
  ): Promise<PricingControlJobStageResponseV2> {
    const jobId = await stageStoredPricingCatalogControlJob(
      this.database,
      input.product_id,
      input.generation,
      { actorId, reason: input.reason },
    );
    return pricingControlJobStageResponseV2Schema.parse({ status: "staged", job_id: jobId });
  }

  async stagePricingSwitchJobV2(
    input: PricingSwitchJobStageRequestV2,
    actorId: string,
  ): Promise<PricingControlJobStageResponseV2> {
    const jobId = await stageStoredProviderSwitchControlJob(
      this.database,
      input.generation,
      { actorId, reason: input.reason },
    );
    return pricingControlJobStageResponseV2Schema.parse({ status: "staged", job_id: jobId });
  }

  async repairPricingPolicyDeliveryV2(
    input: PricingPolicyDeliveryRepairRequestV2,
    actorId: string,
  ): Promise<PricingPolicyDeliveryRepairResponseV2> {
    const provisioningContext = await this.engine.getPricingReleaseProvisioningContextV2();
    if (provisioningContext !== null) {
      throw new PricingPolicyDeliveryRepairError(
        "repair_not_eligible",
        "legacy policy delivery repair is unavailable after the global pricing release cutover",
      );
    }
    return pricingPolicyDeliveryRepairResponseV2Schema.parse(
      await repairDeadPreCutoverPolicyDelivery(this.database, {
        jobId: input.job_id,
        expectedEffectiveVersion: input.expected_effective_version,
        expectedContentDigest: input.expected_content_digest,
        actorId,
        reason: input.reason,
      }),
    );
  }

  async getPricingReleaseActivationControlV2(): Promise<Record<string, unknown>> {
    const control = await readPricingReleaseActivationControlV2(this.database);
    let engineHead: Awaited<ReturnType<EngineClient["getPricingReleaseHeadV2"]>> = null;
    let engineAvailable = false;
    try {
      engineHead = await this.engine.getPricingReleaseHeadV2();
      engineAvailable = true;
    } catch {
      // Local durable evidence remains observable, but the UI must fail closed before staging.
    }
    const engineObservedAt = new Date();
    return serializePricingReleaseActivationControlV2(
      control,
      engineObservedAt,
      engineAvailable,
      engineHead,
    );
  }

  async stagePricingReleaseActivationV2(
    input: PricingReleaseActivationStageRequestV2,
    actorId: string,
  ): Promise<Record<string, unknown>> {
    const jobId = await stagePricingReleaseActivationJobV2(this.database, {
      activationKind: input.activation_kind,
      evidenceDigest: input.evidence_digest,
      operatorId: actorId,
      reason: input.reason,
    });
    return pricingReleaseActivationStageResponseV2Schema.parse({
      job_id: jobId,
      activation_kind: input.activation_kind,
      evidence_digest: input.evidence_digest,
      status: "accepted",
    });
  }

  async getPricingStage8CaptureControlV2(): Promise<Record<string, unknown>> {
    return serializePricingStage8CaptureControlV2(
      await readPricingStage8CaptureControlV2(this.database),
    );
  }

  async stagePricingStage8CaptureV2(
    input: PricingStage8CaptureStageRequestV2,
    actorId: string,
  ): Promise<Record<string, unknown>> {
    const staged = await stagePricingStage8CaptureJobV2(this.database, {
      idempotencyKey: input.idempotency_key,
      request: {
        target_generation: input.target_generation,
        recovery_generation: input.recovery_generation,
        window_start_ts: input.window_start_ts,
        window_end_ts: input.window_end_ts,
        min_samples_per_provider: input.min_samples_per_provider,
        financial_sample_size: input.financial_sample_size,
        gemini_client_admissions: input.gemini_client_admissions,
      },
      operatorId: actorId,
      reason: input.reason,
    });
    return pricingStage8CaptureStageResponseV2Schema.parse({
      job_id: staged.jobId,
      request_digest: staged.requestDigest,
      status: "accepted",
    });
  }

  async getPricingShadowRolloutControlV2(): Promise<Record<string, unknown>> {
    return serializePricingShadowRolloutControlV2(
      await readPricingShadowRolloutControlV2(this.database),
    );
  }

  async stagePricingShadowRolloutV2(
    input: PricingShadowRolloutStageRequestV2,
    actorId: string,
  ): Promise<Record<string, unknown>> {
    const staged = await stagePricingShadowRolloutV2(this.database, this.engine, {
      idempotencyKey: input.idempotency_key,
      stage5RunId: input.stage5_run_id,
      actorId,
      reason: input.reason,
    });
    return pricingShadowRolloutStageResponseV2Schema.parse({
      rollout_id: staged.rolloutId,
      rollout_digest: staged.rolloutDigest,
      job_count: staged.jobCount,
      idempotent_replay: staged.idempotentReplay,
      status: "accepted",
    });
  }

  async upsertServiceAccountInventoryV2(
    serviceId: string,
    input: ServiceAccountInventoryMutationV2,
    actorId: string,
  ): Promise<unknown> {
    const engineInventory = await this.stableEngineAccountIdentityInventoryV2();
    const engineAccount = engineInventory.accounts.find(
      (account) => account.account_id === input.engine_account_id,
    );
    if (!engineAccount) {
      throw new AdminServiceAccountInventoryError(
        "engine_account_missing",
        `engine account ${input.engine_account_id} does not exist`,
      );
    }

    const accountDetails = await this.engine.getAccounts([input.engine_account_id]);
    const details = accountDetails.find((account) => account.account === input.engine_account_id);
    if (!details) {
      throw new AdminServiceAccountInventoryError(
        "engine_account_missing",
        `engine account ${input.engine_account_id} disappeared during validation`,
      );
    }
    if (details.status !== engineAccount.status) {
      throw new AdminServiceAccountInventoryError(
        "engine_inventory_unstable",
        `engine account ${input.engine_account_id} status changed during validation`,
      );
    }
    if (/^openkeys-/i.test(details.handle ?? "")) {
      throw new AdminServiceAccountInventoryError(
        "account_owned_by_openkeys",
        `engine account ${input.engine_account_id} belongs to OpenKeys`,
      );
    }

    const currentInventory = await readServiceAccountInventoryV2(this.database);
    const current = currentInventory.accounts.find((account) => account.service_id === serviceId);
    const exactReplay = current !== undefined
      && current.engine_account_id === input.engine_account_id
      && current.purpose === input.purpose
      && current.responsible === input.responsible
      && current.status === engineAccount.status;
    if (!exactReplay) {
      try {
        await ensureServicePricingReleaseProvisioningV2(this.engine, {
          accountId: input.engine_account_id,
          serviceId,
          purpose: input.purpose,
          responsible: input.responsible,
        });
      } catch (error) {
        if (error instanceof PricingReleaseAccountProvisioningV2Error) {
          throw new AdminServiceAccountInventoryError(
            "pricing_release_not_ready",
            "service account pricing release provisioning is not durably ready",
          );
        }
        throw error;
      }
    }

    return upsertServiceAccountInventoryV2(this.database, {
      serviceId,
      expectedSourceVersion: input.expected_source_version,
      expectedContentDigest: input.expected_content_digest,
      engineAccountId: input.engine_account_id,
      purpose: input.purpose,
      responsible: input.responsible,
      status: engineAccount.status,
      engineInventoryDigest: engineInventory.digest,
      actorId,
      reason: input.reason,
    });
  }

  async updateManagedProviderSwitches(
    input: ProviderSwitchEditorMutation,
    actorId: string,
  ): Promise<unknown> {
    return updateManagedProviderSwitches(this.database, { ...input, actorId });
  }

  private async stableEngineAccountIdentityInventoryV2(): Promise<{
    accounts: PricingReleaseInventoryAccountV2[];
    digest: string;
  }> {
    const scan = async (): Promise<{
      accounts: PricingReleaseInventoryAccountV2[];
      digest: string;
    }> => {
      const accounts: PricingReleaseInventoryAccountV2[] = [];
      const seen = new Set<string>();
      let afterAccountId: string | undefined;
      let previousAccountId: string | null = null;
      for (;;) {
        const page = await this.engine.getPricingReleaseInventoryV2({
          ...(afterAccountId === undefined ? {} : { afterAccountId }),
          limit: 500,
        });
        for (const account of page.accounts) {
          if (
            seen.has(account.account_id)
            || (previousAccountId !== null
              && Buffer.compare(Buffer.from(account.account_id, "utf8"), Buffer.from(previousAccountId, "utf8")) <= 0)
          ) {
            throw new AdminServiceAccountInventoryError(
              "engine_inventory_unstable",
              "engine pricing inventory returned a duplicate or regressing account cursor",
            );
          }
          seen.add(account.account_id);
          previousAccountId = account.account_id;
          accounts.push(account);
        }
        if (page.next_after_account_id === null) break;
        if (
          page.accounts.length === 0
          || page.next_after_account_id === afterAccountId
          || page.next_after_account_id !== page.accounts.at(-1)?.account_id
        ) {
          throw new AdminServiceAccountInventoryError(
            "engine_inventory_unstable",
            "engine pricing inventory returned an invalid continuation cursor",
          );
        }
        afterAccountId = page.next_after_account_id;
      }
      return { accounts, digest: engineAccountIdentityInventoryDigestV2(accounts) };
    };

    const first = await scan();
    const second = await scan();
    if (first.digest !== second.digest) {
      throw new AdminServiceAccountInventoryError(
        "engine_inventory_unstable",
        "engine account identity inventory changed during service-account validation",
      );
    }
    return second;
  }

  private stage5OpenKeysInventoryReaderV2() {
    return createStage5OpenKeysInventoryReaderV2({
      baseUrl: this.config.get("OPENKEYS_INTERNAL_BASE_URL", { infer: true }),
      controlKey: this.config.get("OPENKEYS_CONTROL_KEY", { infer: true })
        ?? this.config.get("ENGINE_CONTROL_KEY", { infer: true }),
    });
  }

  async updateManagedPricingPolicy(
    ownerType: "global_b2c" | "b2b_client" | "b2b_invitation" | "service",
    ownerId: string,
    input: { expectedVersion: number; rules: readonly PricingPolicyEditorRule[]; reason: string },
    actorId: string,
    productId?: string,
  ): Promise<unknown> {
    return updateManagedPricingPolicy(this.database, {
      ownerType,
      ownerId,
      ...(productId ? { productId } : {}),
      expectedVersion: input.expectedVersion,
      rules: input.rules,
      actorId,
      reason: input.reason,
    });
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
    input: { discountPercent?: number; policy?: { expectedVersion: number; rules: readonly PricingPolicyEditorRule[] } },
    actorId: string,
    reason: string,
  ): Promise<Record<string, unknown>> {
    if (input.policy) {
      const policy = await updateManagedPricingPolicy(this.database, {
        ownerType: "b2b_client",
        ownerId: userId,
        expectedVersion: input.policy.expectedVersion,
        rules: input.policy.rules,
        actorId,
        reason,
      });
      const releaseV2 = await this.syncReleasePolicyOverride(userId);
      return {
        userId,
        policy,
        syncStatus: policy.targets.every((target) => target.syncState === "confirmed") ? "confirmed" : "pending",
        releaseV2,
      };
    }
    if (input.discountPercent === undefined) throw new Error("business pricing mutation is empty");
    const result = await setBusinessPricing(this.database, {
      userId,
      multiplierBp: multiplierForDiscount(input.discountPercent),
      actorId,
      reason,
    });
    return { userId, discountPercent: input.discountPercent, syncStatus: "pending", pricingJobId: result.jobId };
  }

  private async syncReleasePolicyOverride(userId: string): Promise<Record<string, unknown>> {
    const mapping = await this.database.pool.query<{ engine_account_id: string | null }>(
      `SELECT engine_account_id FROM engine_accounts WHERE user_id = $1`,
      [userId],
    );
    const engineAccountId = mapping.rows[0]?.engine_account_id;
    if (!engineAccountId) return { status: "no_engine_account" };
    try {
      const result = await syncPricingReleasePolicyOverrideV2(this.database, this.engine, {
        userId,
        engineAccountId,
      });
      return result;
    } catch (error) {
      if (error instanceof PricingReleaseProvisioningV2Error) {
        return { status: "error", code: error.code, message: error.message };
      }
      throw error;
    }
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

function serializePricingReleaseActivationControlV2(
  control: PricingReleaseActivationControlV2,
  engineObservedAt: Date,
  engineAvailable: boolean,
  engineHead: Awaited<ReturnType<EngineClient["getPricingReleaseHeadV2"]>>,
): Record<string, unknown> {
  return {
    database_observed_at: control.databaseObservedAt.toISOString(),
    unresolved_pricing_jobs: control.unresolvedPricingJobs,
    engine: {
      observed_at: engineObservedAt.toISOString(),
      available: engineAvailable,
      head: engineHead,
    },
    releases: control.releases.map((release) => ({
      generation: release.generation,
      release_kind: release.releaseKind,
      status: release.status,
      content_digest: release.contentDigest,
      engine_release_digest: release.engineReleaseDigest,
      commerce_inventory_digest: release.commerceInventoryDigest,
      engine_inventory_digest: release.engineInventoryDigest,
      openkeys_inventory_digest: release.openkeysInventoryDigest,
      service_inventory_digest: release.serviceInventoryDigest,
      created_at: release.createdAt.toISOString(),
      updated_at: release.updatedAt.toISOString(),
    })),
    evidence: control.evidence.map((evidence) => ({
      evidence_digest: evidence.evidenceDigest,
      engine_evidence_digest: evidence.engineEvidenceDigest,
      engine_captured_at: evidence.engineCapturedAt?.toISOString() ?? null,
      target_generation: evidence.targetGeneration,
      target_digest: evidence.targetDigest,
      recovery_generation: evidence.recoveryGeneration,
      recovery_digest: evidence.recoveryDigest,
      service_inventory_digest: evidence.serviceInventoryDigest,
      legacy_inflight_count: evidence.legacyInflightCount,
      blocker_count: evidence.blockerCount,
      passed: evidence.passed,
      observed_at: evidence.observedAt.toISOString(),
      valid_until: evidence.validUntil.toISOString(),
      target_status: evidence.targetStatus,
      recovery_status: evidence.recoveryStatus,
      target_engine_digest: evidence.targetEngineDigest,
      recovery_engine_digest: evidence.recoveryEngineDigest,
      fresh: evidence.fresh,
      source_complete: evidence.sourceComplete,
      local_blockers: evidence.localBlockers,
    })),
    jobs: control.jobs.map((job) => ({
      id: job.id,
      activation_kind: job.activationKind,
      release_generation: job.releaseGeneration,
      release_digest: job.releaseDigest,
      evidence_digest: job.evidenceDigest,
      status: job.status,
      attempts: job.attempts,
      operator_id: job.operatorId,
      reason: job.reason,
      last_error: job.lastError,
      result_digest: job.resultDigest,
      confirmed_at: job.confirmedAt?.toISOString() ?? null,
      created_at: job.createdAt.toISOString(),
      updated_at: job.updatedAt.toISOString(),
    })),
    receipts: control.receipts.map((receipt) => ({
      activation_id: receipt.activationId,
      activation_kind: receipt.activationKind,
      release_generation: receipt.releaseGeneration,
      release_digest: receipt.releaseDigest,
      evidence_digest: receipt.evidenceDigest,
      head_version: receipt.headVersion,
      receipt_digest: receipt.receiptDigest,
      activated_at: receipt.activatedAt.toISOString(),
      created_at: receipt.createdAt.toISOString(),
    })),
  };
}

function serializePricingShadowRolloutControlV2(
  control: PricingShadowRolloutControlV2,
): Record<string, unknown> {
  return pricingShadowRolloutControlV2Schema.parse({
    database_observed_at: control.databaseObservedAt.toISOString(),
    counts_by_status: control.countsByStatus,
    rollouts: control.rollouts.map((rollout) => ({
      id: rollout.id,
      idempotency_key: rollout.idempotencyKey,
      stage5_run_id: rollout.stage5RunId,
      rollout_digest: rollout.rolloutDigest,
      target_generation: rollout.targetGeneration,
      target_digest: rollout.targetDigest,
      recovery_generation: rollout.recoveryGeneration,
      recovery_digest: rollout.recoveryDigest,
      catalog_generation: rollout.catalogGeneration,
      main_catalog_digest: rollout.mainCatalogDigest,
      openkeys_catalog_digest: rollout.openkeysCatalogDigest,
      switch_generation: rollout.switchGeneration,
      switch_digest: rollout.switchDigest,
      engine_inventory_digest: rollout.engineInventoryDigest,
      assignment_manifest_digest: rollout.assignmentManifestDigest,
      policy_manifest_digest: rollout.policyManifestDigest,
      assignment_count: rollout.assignmentCount,
      job_count: rollout.jobCount,
      job_counts_by_status: rollout.jobCountsByStatus,
      actor_id: rollout.actorId,
      reason: rollout.reason,
      status: rollout.status,
      last_error: rollout.lastError,
      completed_at: rollout.completedAt?.toISOString() ?? null,
      created_at: rollout.createdAt.toISOString(),
      updated_at: rollout.updatedAt.toISOString(),
    })),
    jobs: control.jobs.map((job) => ({
      id: job.id,
      rollout_id: job.rolloutId,
      subject_digest: job.subjectDigest,
      account_status: job.accountStatus,
      account_class: job.accountClass,
      owner_context: job.ownerContext,
      release_policy_digest: job.releasePolicyDigest,
      content_digest: job.contentDigest,
      expected_active_digest: job.expectedActiveDigest,
      request_digest: job.requestDigest,
      status: job.status,
      attempts: job.attempts,
      last_error: job.lastError,
      ack_digest: job.ackDigest,
      confirmed_at: job.confirmedAt?.toISOString() ?? null,
      completed_at: job.completedAt?.toISOString() ?? null,
      created_at: job.createdAt.toISOString(),
      updated_at: job.updatedAt.toISOString(),
    })),
  });
}

function serializePricingStage8CaptureControlV2(
  control: PricingStage8CaptureControlV2,
): Record<string, unknown> {
  return pricingStage8CaptureControlV2Schema.parse({
    database_observed_at: control.databaseObservedAt.toISOString(),
    counts_by_status: control.countsByStatus,
    jobs: control.jobs.map((job) => ({
      id: job.id,
      idempotency_key: job.idempotencyKey,
      request_digest: job.requestDigest,
      target_generation: job.targetGeneration,
      recovery_generation: job.recoveryGeneration,
      window_start_at: job.windowStartAt.toISOString(),
      window_end_at: job.windowEndAt.toISOString(),
      min_samples_per_provider: job.minSamplesPerProvider,
      financial_sample_size: job.financialSampleSize,
      gemini_client_admissions: job.geminiClientAdmissions,
      operator_id: job.operatorId,
      reason: job.reason,
      status: job.status,
      attempts: job.attempts,
      next_attempt_at: job.nextAttemptAt.toISOString(),
      locked_at: job.lockedAt?.toISOString() ?? null,
      locked_by: job.lockedBy,
      last_error: job.lastError,
      result_engine_evidence_digest: job.resultEngineEvidenceDigest,
      result_combined_evidence_digest: job.resultCombinedEvidenceDigest,
      result_passed: job.resultPassed,
      completed_at: job.completedAt?.toISOString() ?? null,
      created_at: job.createdAt.toISOString(),
      updated_at: job.updatedAt.toISOString(),
    })),
    artifacts: control.artifacts.map((artifact) => ({
      id: artifact.id,
      job_id: artifact.jobId,
      attempt: artifact.attempt,
      engine_evidence_digest: artifact.engineEvidenceDigest,
      engine_captured_at: artifact.engineCapturedAt.toISOString(),
      combined_evidence_digest: artifact.combinedEvidenceDigest,
      combined_passed: artifact.combinedPassed,
      combined_write_result: artifact.combinedWriteResult,
      combined_observed_at: artifact.combinedObservedAt?.toISOString() ?? null,
      combined_valid_until: artifact.combinedValidUntil?.toISOString() ?? null,
      combined_blocker_count: artifact.combinedBlockerCount,
      combined_blockers: artifact.combinedBlockers,
      combined_blockers_truncated: artifact.combinedBlockersTruncated,
      completed_at: artifact.completedAt?.toISOString() ?? null,
      created_at: artifact.createdAt.toISOString(),
    })),
  });
}

function serializePricingStage5ControlResultV2(
  result: Stage5MaterializerV2Result,
): PricingStage5ControlResultV2 {
  return pricingStage5ControlResultV2Schema.parse({
    mode: result.mode,
    status: result.status,
    plan_digest: result.plan.plan_digest,
    run_id: result.run_id,
    writes_committed: result.writes_committed,
    engine_prepared: result.engine_prepared,
    commerce_inventory_digest: result.plan.commerce_inventory_digest,
    engine_scan_first_digest: result.plan.engine_scan_first_digest,
    engine_scan_second_digest: result.plan.engine_scan_second_digest,
    openkeys_scan_first_digest: result.plan.openkeys_scan_first_digest,
    openkeys_scan_second_digest: result.plan.openkeys_scan_second_digest,
    service_inventory_digest: result.plan.service_inventory_digest,
    funding_plan_digest: result.plan.funding_plan_digest,
    target_generation: result.plan.target_generation,
    target_plan_digest: result.plan.target.content_digest,
    recovery_generation: result.plan.recovery_generation,
    recovery_plan_digest: result.plan.recovery.content_digest,
    blocker_count: result.plan.blockers.length,
    blockers: result.plan.blockers,
  });
}

/** nano-USD (строка целого) → десятичная USD-строка с 4 знаками; без float. */
function nanoToUsd(nano: string): string {
  const negative = nano.startsWith("-");
  const digits = (negative ? nano.slice(1) : nano).padStart(10, "0");
  const whole = digits.slice(0, -9);
  const frac = digits.slice(-9, -5); // 4 знака после точки достаточно для панели
  return `${negative ? "-" : ""}${whole}.${frac}`;
}
