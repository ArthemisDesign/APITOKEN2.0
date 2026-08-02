import { Buffer } from "node:buffer";
import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type {
  FundingNormalizationApplyResultV2,
  FundingNormalizationPlanV2,
  PricingReleaseInventoryAccountV2,
  PricingReleaseInventoryPageV2,
} from "@claude-api/contracts";
import {
  buildFundingNormalizationCoverageV2,
  claimNextFundingNormalizationAccountV2,
  claimNextFundingNormalizationJobV2,
  confirmFundingNormalizationJobV2,
  failFundingNormalizationJobV2,
  FundingNormalizationJobV2Error,
  getFundingNormalizationStateV2,
  recoverStaleFundingNormalizationJobsV2,
  renewFundingNormalizationJobLeaseV2,
  retryFundingNormalizationAccountV2,
  retryFundingNormalizationJobV2,
  sameFundingNormalizationInventoryIdentityV2,
  storeFundingNormalizationPlanV2,
  type Database,
  type FundingNormalizationJobV2,
} from "@claude-api/db";
import { EngineClient, EngineClientError } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

interface FundingNormalizationInventoryClientV2 {
  getPricingReleaseInventoryV2(options: {
    afterAccountId?: string;
    limit: number;
  }): Promise<PricingReleaseInventoryPageV2>;
}

interface FundingNormalizationApplyClientV2 {
  getFundingNormalizationPlanV2(accountId: string): Promise<FundingNormalizationPlanV2 | null>;
  applyFundingNormalizationV2(
    accountId: string,
    input: {
      expected_source_state_digest: string;
      expected_normalization_digest: string;
    },
  ): Promise<FundingNormalizationApplyResultV2 | null>;
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

/** Exhausts the producer cursor while rejecting duplicate, regressing, or malformed continuation state. */
export async function collectFundingNormalizationInventoryV2(
  engine: FundingNormalizationInventoryClientV2,
  pageSize: number,
  heartbeat: () => Promise<void> = async () => {},
): Promise<PricingReleaseInventoryAccountV2[]> {
  if (!Number.isSafeInteger(pageSize) || pageSize < 1 || pageSize > 500) {
    throw new RangeError("funding normalization inventory page size must be within 1..=500");
  }
  const accounts: PricingReleaseInventoryAccountV2[] = [];
  const seen = new Set<string>();
  let afterAccountId: string | undefined;
  let previousAccountId: string | undefined;
  for (;;) {
    const options = afterAccountId === undefined
      ? { limit: pageSize }
      : { afterAccountId, limit: pageSize };
    const page = await engine.getPricingReleaseInventoryV2(options);
    for (const account of page.accounts) {
      if (
        (afterAccountId !== undefined && compareUtf8(account.account_id, afterAccountId) <= 0)
        || (previousAccountId !== undefined && compareUtf8(account.account_id, previousAccountId) <= 0)
        || seen.has(account.account_id)
      ) {
        throw new FundingNormalizationJobV2Error(
          `engine funding inventory cursor regressed or duplicated ${account.account_id}`,
          false,
        );
      }
      seen.add(account.account_id);
      accounts.push(account);
      previousAccountId = account.account_id;
    }
    if (page.next_after_account_id === null) return accounts;
    const lastAccountId = page.accounts.at(-1)?.account_id;
    if (
      lastAccountId === undefined
      || page.next_after_account_id !== lastAccountId
      || (afterAccountId !== undefined && compareUtf8(page.next_after_account_id, afterAccountId) <= 0)
    ) {
      throw new FundingNormalizationJobV2Error(
        "engine funding inventory returned a non-monotonic continuation cursor",
        false,
      );
    }
    afterAccountId = page.next_after_account_id;
    await heartbeat();
  }
}

export async function applyFreshFundingNormalizationPlanV2(
  engine: FundingNormalizationApplyClientV2,
  accountId: string,
): Promise<
  | { kind: "missing" }
  | { kind: "blocked"; plan: FundingNormalizationPlanV2 }
  | { kind: "applied"; result: FundingNormalizationApplyResultV2 | null }
> {
  const plan = await engine.getFundingNormalizationPlanV2(accountId);
  if (plan === null) return { kind: "missing" };
  if (plan.status === "blocked") return { kind: "blocked", plan };
  if (plan.normalization_digest === null) {
    throw new FundingNormalizationJobV2Error(
      `engine account ${accountId} has no normalization target`,
      true,
    );
  }
  const result = await engine.applyFundingNormalizationV2(accountId, {
    expected_source_state_digest: plan.source_state_digest,
    expected_normalization_digest: plan.normalization_digest,
  });
  return { kind: "applied", result };
}

@Injectable()
export class FundingNormalizationWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(FundingNormalizationWorkerService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    const recovered = await recoverStaleFundingNormalizationJobsV2(
      this.database,
      this.config.get("FUNDING_NORMALIZATION_LEASE_MS", { infer: true }),
    );
    if (recovered.parents > 0 || recovered.accounts > 0) {
      this.logger.warn(
        `recovered ${recovered.parents} parent and ${recovered.accounts} account funding-normalization leases`,
      );
    }
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("FUNDING_NORMALIZATION_POLL_MS", { infer: true });
    const leaseMs = this.config.get("FUNDING_NORMALIZATION_LEASE_MS", { infer: true });
    this.logger.log(`funding normalization worker ${this.workerId} started`);
    while (!this.stopped) {
      try {
        const job = await claimNextFundingNormalizationJobV2(this.database, this.workerId, leaseMs);
        if (job) await this.processJob(job);
      } catch (error) {
        this.logger.error(`funding normalization claim failed: ${message(error)}`);
      }
      await this.sleep(pollMs);
    }
  }

  private async processJob(job: FundingNormalizationJobV2): Promise<void> {
    const retryMs = this.config.get("FUNDING_NORMALIZATION_RETRY_MS", { infer: true });
    try {
      await renewFundingNormalizationJobLeaseV2(this.database, job, this.workerId);
      const firstInventory = await this.collectInventory(job);
      const stableInventory = await this.collectInventory(job);
      if (!sameFundingNormalizationInventoryIdentityV2(firstInventory, stableInventory)) {
        throw new FundingNormalizationJobV2Error(
          "engine identity inventory changed between consecutive full scans",
          false,
        );
      }

      await this.processBoundedSlice(job, stableInventory);

      const state = await getFundingNormalizationStateV2(this.database, job);
      const coverage = buildFundingNormalizationCoverageV2(stableInventory, state);
      if (coverage.extraAccountIds.length > 0) {
        throw new FundingNormalizationJobV2Error(
          `funding queue contains ${coverage.extraAccountIds.length} accounts outside current balance inventory`,
          true,
        );
      }
      const incomplete = coverage.missingAccountIds.length
        + coverage.pendingCount
        + coverage.processingCount
        + coverage.retryCount
        + coverage.blockerCount;
      if (incomplete > 0) {
        throw new FundingNormalizationJobV2Error(
          `funding normalization remains incomplete for ${incomplete} account states`,
          false,
        );
      }

      // Final repeat scan closes provisioning drift as far as this consumer can observe. Stage 9
      // repeats coverage while holding the engine's global release lock before its single CAS.
      const finalInventory = await this.collectInventory(job);
      if (!sameFundingNormalizationInventoryIdentityV2(stableInventory, finalInventory)) {
        throw new FundingNormalizationJobV2Error(
          "engine identity inventory changed before funding normalization confirmation",
          false,
        );
      }
      const finalState = await getFundingNormalizationStateV2(this.database, job);
      const finalCoverage = buildFundingNormalizationCoverageV2(finalInventory, finalState);
      if (
        finalCoverage.missingAccountIds.length > 0
        || finalCoverage.extraAccountIds.length > 0
        || finalCoverage.pendingCount > 0
        || finalCoverage.processingCount > 0
        || finalCoverage.retryCount > 0
        || finalCoverage.blockerCount > 0
        || finalCoverage.readyCount !== finalCoverage.balanceAccountIds.length
      ) {
        throw new FundingNormalizationJobV2Error(
          "final funding normalization coverage is not exact",
          false,
        );
      }
      const resultDigest = await confirmFundingNormalizationJobV2(this.database, this.engine, job, this.workerId, {
        engineInventory: finalInventory,
      });
      this.logger.log(
        `funding normalization job ${job.id} confirmed ${finalCoverage.readyCount} balance accounts as ${resultDigest}`,
      );
    } catch (error) {
      await this.releaseParent(job, error, retryMs);
    }
  }

  private async collectInventory(
    job: FundingNormalizationJobV2,
  ): Promise<PricingReleaseInventoryAccountV2[]> {
    const pageSize = this.config.get("FUNDING_NORMALIZATION_INVENTORY_PAGE_SIZE", { infer: true });
    return collectFundingNormalizationInventoryV2(this.engine, pageSize, async () => {
      await renewFundingNormalizationJobLeaseV2(this.database, job, this.workerId);
    });
  }

  private async processBoundedSlice(
    job: FundingNormalizationJobV2,
    inventory: readonly PricingReleaseInventoryAccountV2[],
  ): Promise<void> {
    const batchSize = this.config.get("FUNDING_NORMALIZATION_BATCH_SIZE", { infer: true });
    const retryMs = this.config.get("FUNDING_NORMALIZATION_RETRY_MS", { infer: true });
    const state = await getFundingNormalizationStateV2(this.database, job);
    const coverage = buildFundingNormalizationCoverageV2(inventory, state);
    if (coverage.extraAccountIds.length > 0) {
      throw new FundingNormalizationJobV2Error(
        `funding queue contains account outside target inventory: ${coverage.extraAccountIds[0]}`,
        true,
      );
    }

    const refreshIds = [...new Set([
      ...coverage.missingAccountIds,
      ...coverage.dueBlockerAccountIds,
    ])].slice(0, batchSize);
    let accountOperations = 0;
    for (const accountId of refreshIds) {
      if (this.stopped) break;
      const plan = await this.engine.getFundingNormalizationPlanV2(accountId);
      if (plan === null) {
        throw new FundingNormalizationJobV2Error(
          `engine account ${accountId} disappeared while staging normalization`,
          false,
        );
      }
      await storeFundingNormalizationPlanV2(
        this.database,
        job,
        this.workerId,
        plan,
        "observed",
        retryMs,
      );
      accountOperations += 1;
      await renewFundingNormalizationJobLeaseV2(this.database, job, this.workerId);
    }

    while (!this.stopped && accountOperations < batchSize) {
      const account = await claimNextFundingNormalizationAccountV2(this.database, job, this.workerId);
      if (!account) return;
      accountOperations += 1;
      try {
        // Never POST a digest read from the durable queue: this helper's immediately preceding
        // GET is the only source for both expected identities.
        const attempt = await applyFreshFundingNormalizationPlanV2(
          this.engine,
          account.engineAccountId,
        );
        if (attempt.kind === "missing") {
          await retryFundingNormalizationAccountV2(
            this.database,
            job,
            this.workerId,
            account.engineAccountId,
            "engine account disappeared after inventory scan",
            retryMs,
          );
          continue;
        }
        if (attempt.kind === "blocked") {
          await storeFundingNormalizationPlanV2(
            this.database,
            job,
            this.workerId,
            attempt.plan,
            "blocked",
            retryMs,
          );
          continue;
        }
        const result = attempt.result;
        if (result === null) {
          await retryFundingNormalizationAccountV2(
            this.database,
            job,
            this.workerId,
            account.engineAccountId,
            "engine account disappeared during normalization apply",
            retryMs,
          );
          continue;
        }
        await storeFundingNormalizationPlanV2(
          this.database,
          job,
          this.workerId,
          result.normalization,
          result.status,
          retryMs,
        );
      } catch (error) {
        await retryFundingNormalizationAccountV2(
          this.database,
          job,
          this.workerId,
          account.engineAccountId,
          message(error),
          retryMs,
        );
        if (isTerminal(error)) throw error;
      } finally {
        await renewFundingNormalizationJobLeaseV2(this.database, job, this.workerId);
      }
    }
  }

  private async releaseParent(
    job: FundingNormalizationJobV2,
    error: unknown,
    retryMs: number,
  ): Promise<void> {
    try {
      if (isTerminal(error)) {
        await failFundingNormalizationJobV2(this.database, job, this.workerId, message(error));
        this.logger.error(`funding normalization job ${job.id} failed closed: ${message(error)}`);
      } else {
        await retryFundingNormalizationJobV2(this.database, job, this.workerId, message(error), retryMs);
        this.logger.warn(`funding normalization job ${job.id} will retry: ${message(error)}`);
      }
    } catch (releaseError) {
      this.logger.error(
        `failed to release funding normalization job ${job.id}: ${message(releaseError)}`,
      );
    }
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([
      new Promise((resolve) => setTimeout(resolve, milliseconds)),
      this.stopSignal,
    ]);
  }
}

function isTerminal(error: unknown): boolean {
  if (error instanceof FundingNormalizationJobV2Error) return error.terminal;
  return error instanceof EngineClientError && !error.retryable;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "funding normalization failed";
}
