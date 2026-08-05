import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  applyPricingLedgerPage,
  applyPricingProviderBackfillPage,
  applyPricingTopupBackfillPage,
  advanceAccountStrictChain,
  claimNextPricingControlJob,
  claimNextPricingJob,
  claimNextPricingReleaseActivationJobV2,
  closeElapsedTierWindows,
  completePricingProviderBackfill,
  completePricingUsageSync,
  confirmPricingControlJob,
  confirmPricingJob,
  confirmPricingReleaseActivationJobV2,
  getPricingUsageCursor,
  getPricingProviderBackfillCursor,
  getPricingTopupBackfillCursor,
  listPendingStrictChainAccounts,
  listPricingSyncTargets,
  PricingControlNotifyListener,
  reconcileTierLadderMultipliers,
  recoverStalePricingControlJobs,
  recoverStalePricingJobs,
  recoverStalePricingReleaseActivationJobsV2,
  refreshTierWindowUsage,
  releasePricingControlJob,
  releasePricingReleaseActivationJobV2,
  retryPricingJob,
  utcMonthStart,
  createStage5OpenKeysInventoryReaderV2,
  type Database,
  type ClaimedPricingControlJob,
  type ClaimedPricingReleaseActivationJobV2,
  type PricingControlJobDisposition,
  PricingReleaseActivationJobV2Error,
  type PricingReleaseActivationJobDispositionV2,
  type PricingSyncTarget,
  type Stage5V2OpenKeysReader,
} from "@claude-api/db";
import type {
  PolicyActiveExpectation,
  PricingActiveExpectation,
  PricingMutationAck,
  PricingReleaseActivationAckV2,
} from "@claude-api/contracts";
import { EngineClient, EngineClientError, type EngineKeyActivationPolicyAck } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

const PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC = 4;
// Догоняющий скан истории пополнений — разовая работа на аккаунт, поэтому лимит страниц за цикл
// держим таким же скромным: цель не «быстро», а «без всплеска нагрузки на движок».
const TOPUP_BACKFILL_MAX_PAGES_PER_SYNC = 4;
// The strict chain fires once per converted/edited B2B account, so a small bound keeps a sweep
// cycle fast; the remainder is picked up by the next pass.
const STRICT_CHAIN_MAX_ACCOUNTS_PER_SWEEP = 25;

@Injectable()
export class PricingWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(PricingWorkerService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });
  private readonly openkeys: Stage5V2OpenKeysReader;
  private controlNotify: PricingControlNotifyListener | undefined;
  private controlFlushRunning = false;
  private controlFlushQueued = false;

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {
    this.openkeys = createStage5OpenKeysInventoryReaderV2({
      baseUrl: this.config.get("OPENKEYS_INTERNAL_BASE_URL", { infer: true }),
      controlKey: this.config.get("OPENKEYS_CONTROL_KEY", { infer: true })
        ?? this.config.get("ENGINE_CONTROL_KEY", { infer: true }),
    });
  }

  async onModuleInit(): Promise<void> {
    const recoveredControl = await recoverStalePricingControlJobs(this.database);
    if (recoveredControl > 0) {
      this.logger.warn(`recovered ${recoveredControl} stale pricing-control jobs`);
    }
    const recoveredActivation = await recoverStalePricingReleaseActivationJobsV2(this.database);
    if (recoveredActivation > 0) {
      this.logger.warn(`recovered ${recoveredActivation} stale pricing-release activation jobs`);
    }
    const recovered = await recoverStalePricingJobs(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale pricing jobs`);
    // После изменения констант лестницы существующие профили сходятся к ней на первом старте;
    // engine получает новые множители через обычные durable pricing jobs в flushPricingJobs.
    const reconciled = await reconcileTierLadderMultipliers(this.database);
    if (reconciled > 0) this.logger.warn(`reconciled ${reconciled} b2c profiles to current tier ladder`);
    // LISTEN/NOTIFY (migration 0041) wakes the control-job flush on the committing
    // transaction instead of waiting out the sweep. The sweep in run() stays as the
    // recovery path for notifications missed while the listener was reconnecting.
    this.controlNotify = new PricingControlNotifyListener(
      this.config.get("DATABASE_URL", { infer: true }),
      {
        onWake: () => this.requestControlFlush(),
        onError: (error) => this.logger.warn(`pricing-control notify listener reconnecting: ${error.message}`),
      },
    );
    this.controlNotify.start();
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.controlNotify?.stop();
    await this.loop;
  }

  /**
   * Event-driven dispatch of pricing-control jobs. Bursts coalesce: a wake during an
   * active flush schedules exactly one follow-up pass, and concurrent passes are safe
   * anyway because claiming uses FOR UPDATE SKIP LOCKED.
   */
  private requestControlFlush(): void {
    if (this.stopped) return;
    if (this.controlFlushRunning) {
      this.controlFlushQueued = true;
      return;
    }
    this.controlFlushRunning = true;
    void (async () => {
      try {
        do {
          this.controlFlushQueued = false;
          await this.flushPricingControlJobs();
        } while (this.controlFlushQueued && !this.stopped);
      } catch (error) {
        this.logger.error(message(error));
      } finally {
        this.controlFlushRunning = false;
      }
    })();
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("PRICING_POLL_MS", { infer: true });
    const dispatchMs = this.config.get("PRICING_DISPATCH_MS", { infer: true });
    // Job delivery and the fleet sweep are different workloads on the same loop. Delivery is a
    // cheap indexed claim and is what a newly provisioned account's first dashboard load blocks
    // on; the sweep walks every pricing target doing per-user engine I/O. Running delivery only
    // once per sweep made a signup wait for the whole sweep plus the poll interval, so the wait
    // grew with the size of the fleet. Deliver on a short tick, sweep on the slow one. The short
    // tick is also the bounded recovery for a missed LISTEN/NOTIFY wake: notifications are
    // fire-and-forget, so without it a dropped event would hide until the next sweep.
    let nextSweepAt = 0;
    this.logger.log(
      `pricing worker ${this.workerId} started (dispatch ${dispatchMs}ms, sweep ${pollMs}ms)`,
    );
    while (!this.stopped) {
      try {
        // Delivery first and every tick: never behind the sweep. Control jobs go through the
        // coalescing dispatcher so a tick never doubles a LISTEN/NOTIFY-triggered pass.
        this.requestControlFlush();
        await this.flushPricingJobs();
        await this.flushPricingReleaseActivationJobs();
      } catch (error) {
        this.logger.error(message(error));
      }
      if (Date.now() < nextSweepAt) {
        await this.sleep(dispatchMs);
        continue;
      }
      nextSweepAt = Date.now() + pollMs;
      try {
        // C68: recovery is part of normal polling, so a failed retry-state write cannot strand a
        // processing lease until process restart.
        const recovered = await recoverStalePricingJobs(this.database);
        if (recovered > 0) this.logger.warn(`recovered ${recovered} stale pricing jobs`);
        const recoveredControl = await recoverStalePricingControlJobs(this.database);
        if (recoveredControl > 0) {
          this.logger.warn(`recovered ${recoveredControl} stale pricing-control jobs`);
        }
        const recoveredActivation = await recoverStalePricingReleaseActivationJobsV2(this.database);
        if (recoveredActivation > 0) {
          this.logger.warn(`recovered ${recoveredActivation} stale pricing-release activation jobs`);
        }

        const now = new Date();
        // getPricingUsageCursor reconciles durable confirmed-credit accrual markers, including
        // refund/dispute reversal, before any engine network I/O. Keep one authority for that
        // mutation: a separate worker-side refund loop can subtract the same marker twice.
        const targets = await listPricingSyncTargets(this.database);
        const syncedUserIds: string[] = [];
        for (const target of targets) {
          if (this.stopped) break;
          try {
            await this.syncTarget(target);
            syncedUserIds.push(target.userId);
          } catch (error) {
            this.logger.error(`pricing usage sync failed for ${target.userId}: ${message(error)}`);
          }
        }

        // C20: the denormalized counter is rebuilt from immutable, deduplicated events using each
        // event's own timestamp instead of trusting the page in which the event happened to arrive.
        await refreshTierWindowUsage(this.database, syncedUserIds, now);

        if (
          syncedUserIds.length > 0 &&
          afterMonthCloseGrace(now, this.config.get("PRICING_CLOSE_GRACE_MS", { infer: true }))
        ) {
          // C19: only users whose ledger sync completed in this cycle are eligible for closure.
          // AUDIT-TODO(C19): run pnpm db:generate + migrate after adding a durable per-account
          // engine cutoff watermark; require it to cover the exact window end before closure.
          const closed = await closeElapsedTierWindows(this.database, now, syncedUserIds);
          if (closed > 0) {
            await refreshTierWindowUsage(this.database, syncedUserIds, now);
            this.logger.log(`closed ${closed} elapsed pricing windows`);
          }
        }
        // Strict chains stay inside the sweep: that flush is part of the recovery workload,
        // not latency-sensitive delivery.
        await this.flushPendingStrictChains();
      } catch (error) {
        this.logger.error(message(error));
      }
      await this.sleep(dispatchMs);
    }
  }

  private async syncTarget(target: PricingSyncTarget): Promise<void> {
    let cursor = await getPricingUsageCursor(this.database, target);
    // If the previous acknowledgement failed after the commerce transaction committed, replay the
    // durable cursor before fetching. Otherwise an idle ledger would never give retention another
    // opportunity to observe that already-applied page.
    await this.engine.acknowledgeLedger(target.engineAccountId, cursor);
    for (;;) {
      const entries = await this.engine.getLedgerAfter(target.engineAccountId, cursor, 1000);
      if (entries.length === 0) {
        // An empty page is also a completed ledger scan. getPricingUsageCursor invalidates the
        // previous completion marker before network I/O, so restore it only after this response.
        await completePricingUsageSync(this.database, target);
        break;
      }
      await applyPricingLedgerPage(this.database, target, entries);
      cursor = BigInt(entries.at(-1)!.id);
      await this.engine.acknowledgeLedger(target.engineAccountId, cursor);
      if (entries.length < 1000) break;
    }
    const providerRows = await this.backfillTargetProviders(target, cursor);
    if (providerRows > 0) {
      this.logger.log(`completed provider recovery for ${providerRows} usage rows of ${target.userId}`);
    }
    const topupRows = await this.backfillTargetTopups(target, cursor);
    if (topupRows > 0) {
      this.logger.log(`recorded ${topupRows} historical engine top-ups of ${target.userId}`);
    }
  }

  /**
   * История пополнений: обычный курсор расхода уже стоит выше старых топапов, поэтому отчётная
   * таблица заполняется отдельным маркером с начала леджера — один раз на аккаунт, ограниченным
   * числом страниц за цикл.
   */
  private async backfillTargetTopups(
    target: PricingSyncTarget,
    throughLedgerId: bigint,
  ): Promise<number> {
    const start = await getPricingTopupBackfillCursor(this.database, target, throughLedgerId);
    if (start === null) return 0;
    let cursor: bigint = start;
    let recorded = 0;
    for (let page = 0; page < TOPUP_BACKFILL_MAX_PAGES_PER_SYNC; page += 1) {
      const entries = await this.engine.getLedgerAfter(target.engineAccountId, cursor, 1000);
      if (entries.length === 0) {
        recorded += await applyPricingTopupBackfillPage(this.database, target, [], throughLedgerId);
        return recorded;
      }
      const nextCursor = entries.reduce<bigint>((highest, entry) => {
        const ledgerId = BigInt(entry.id);
        return ledgerId > highest ? ledgerId : highest;
      }, cursor);
      if (nextCursor <= cursor) throw new Error("engine top-up backfill ledger page did not advance");
      const terminal = nextCursor >= throughLedgerId || entries.length < 1000;
      recorded += await applyPricingTopupBackfillPage(
        this.database,
        target,
        entries,
        terminal ? throughLedgerId : nextCursor,
      );
      cursor = nextCursor;
      if (terminal) return recorded;
    }
    return recorded;
  }

  private async backfillTargetProviders(
    target: PricingSyncTarget,
    throughLedgerId: bigint,
  ): Promise<number> {
    const backfillCursor = await getPricingProviderBackfillCursor(
      this.database,
      target,
      throughLedgerId,
    );
    if (backfillCursor === null) return 0;
    let cursor: bigint = backfillCursor;

    let resolved = 0;
    for (let page = 0; page < PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC; page += 1) {
      const entries = await this.engine.getLedgerAfter(target.engineAccountId, cursor, 1000);
      if (entries.length === 0) {
        return resolved + await completePricingProviderBackfill(
          this.database,
          target,
          throughLedgerId,
        );
      }
      resolved += await applyPricingProviderBackfillPage(this.database, target, entries);
      const nextCursor = entries.reduce<bigint>((highest, entry) => {
        const ledgerId = BigInt(entry.id);
        return ledgerId > highest ? ledgerId : highest;
      }, cursor);
      if (nextCursor <= cursor) {
        throw new Error("engine provider backfill ledger page did not advance");
      }
      cursor = nextCursor;
      const terminal = cursor >= throughLedgerId || entries.length < 1000;
      resolved += await completePricingProviderBackfill(
        this.database,
        target,
        terminal ? throughLedgerId : cursor,
      );
      if (terminal) return resolved;
    }
    return resolved;
  }

  private async flushPricingControlJobs(): Promise<void> {
    for (;;) {
      const job = await claimNextPricingControlJob(this.database, this.workerId);
      if (!job) return;
      try {
        const ack = await this.deliverPricingControlJob(job);
        await confirmPricingControlJob(this.database, job, ack);
      } catch (error) {
        const disposition = pricingControlDisposition(error);
        try {
          await releasePricingControlJob(this.database, job, disposition, message(error));
        } catch (releaseError) {
          this.logger.error(`failed to release pricing-control job ${job.id}: ${message(releaseError)}`);
          throw releaseError;
        }
        if (disposition === "dead") {
          this.logger.error(`pricing-control job ${job.id} failed permanently: ${message(error)}`);
        }
      }
    }
  }

  private async deliverPricingControlJob(job: ClaimedPricingControlJob): Promise<PricingMutationAck> {
    if (job.kind === "catalog") {
      requirePricingMutation(
        await this.engine.preparePricingCatalog(job.spec),
        ["stored", "unchanged"],
        "catalog prepare",
      );
      const active = await this.engine.getActivePricingCatalog(job.spec.product_id);
      const expectation: PricingActiveExpectation = active === null
        ? "absent"
        : { exact: { version: active.generation, content_digest: active.content_digest } };
      fenceVersion(
        active?.generation,
        active?.content_digest,
        job.spec.generation,
        job.spec.content_digest,
        "catalog",
      );
      const ack = await this.engine.activatePricingCatalog(job.spec, expectation);
      requirePricingMutation(ack, ["applied", "unchanged"], "catalog activation");
      return ack;
    }

    if (job.kind === "switches") {
      requirePricingMutation(
        await this.engine.prepareProviderSwitches(job.spec),
        ["stored", "unchanged"],
        "provider-switch prepare",
      );
      const active = await this.engine.getActiveProviderSwitches();
      const expectation: PricingActiveExpectation = active === null
        ? "absent"
        : { exact: { version: active.generation, content_digest: active.content_digest } };
      fenceVersion(
        active?.generation,
        active?.content_digest,
        job.spec.generation,
        job.spec.content_digest,
        "provider switches",
      );
      const ack = await this.engine.activateProviderSwitches(job.spec, expectation);
      requirePricingMutation(ack, ["applied", "unchanged"], "provider-switch activation");
      return ack;
    }

    requirePricingMutation(
      await this.engine.prepareAccountPolicy(job.spec),
      ["stored", "unchanged"],
      "account-policy prepare",
    );
    const state = await this.engine.getAccountPricingState(job.spec.account_id);
    let expectation: PolicyActiveExpectation;
    if (state === "unbound") {
      expectation = "unbound";
    } else if ("inactive" in state) {
      expectation = { inactive: state.inactive.binding };
    } else {
      fenceVersion(
        state.active.policy.effective_version,
        state.active.policy.content_digest,
        job.spec.effective_version,
        job.spec.content_digest,
        "account policy",
      );
      expectation = {
        exact: {
          target: {
            version: state.active.policy.effective_version,
            content_digest: state.active.policy.content_digest,
          },
          binding: state.active.binding,
        },
      };
    }
    const ack = await this.engine.activateAccountPolicy(job.spec, job.binding, expectation);
    requirePricingMutation(ack, ["applied", "unchanged"], "account-policy activation");
    if (job.binding.policy_enforcement === "strict") {
      await this.restampActiveKeysForStrictPolicy(job.spec.account_id, {
        effectivePolicyVersion: job.spec.effective_version,
        policyDigest: job.spec.content_digest,
      });
    }
    return ack;
  }

  /**
   * Request auth on a strict account admits only keys stamped with the exact active policy
   * head, so every strict activation — the shadow→strict cutover and each later strict→strict
   * policy advance — must re-stamp the account's active keys with the new ACK before the job
   * confirms; a failure retries the job (the engine replays the activation as `unchanged`)
   * until the stamps converge. Keys the customer disabled are left untouched. A key disabled
   * in the race between the list and the write would be re-enabled once; the customer can
   * disable it again immediately (disabling needs no ACK), and the next delivery re-stamps.
   */
  private async restampActiveKeysForStrictPolicy(
    accountId: string,
    ack: EngineKeyActivationPolicyAck,
  ): Promise<void> {
    const keys = await this.engine.listKeys(accountId);
    for (const key of keys) {
      if (key.status !== "active") continue;
      await this.engine.setKeyStatus(key.key_id, "active", ack);
    }
  }

  /**
   * Automatic half of the per-account strict lane (docs/commerce/PRICING.md): a B2C→B2B
   * conversion and every b2b_client policy save flag the binding strict_chain_pending, and
   * this sweep runs the shared preflight + durable strict staging as soon as the shadow
   * delivery of that exact version confirms. The staging transaction disarms the flag, so a
   * replay never duplicates the cutover; a failed precondition is recorded on the binding and
   * retried on the next pass instead of producing a partial state.
   */
  private async flushPendingStrictChains(): Promise<void> {
    const candidates = await listPendingStrictChainAccounts(
      this.database,
      STRICT_CHAIN_MAX_ACCOUNTS_PER_SWEEP,
    );
    for (const candidate of candidates) {
      if (this.stopped) return;
      try {
        const result = await advanceAccountStrictChain(this.database, this.engine, candidate);
        if (result.status === "staged") {
          this.logger.log(
            `strict chain staged for ${candidate.userId}: job ${result.jobId} ` +
            `(funding ${result.funding}, ${result.keysStamped} active keys stamped)`,
          );
        } else if (result.status === "failed") {
          this.logger.error(`strict chain for ${candidate.userId} cannot advance: ${result.error}`);
        }
      } catch (error) {
        this.logger.error(`strict chain sweep failed for ${candidate.userId}: ${message(error)}`);
      }
    }
  }

  private async flushPricingReleaseActivationJobs(): Promise<void> {
    for (;;) {
      const job = await claimNextPricingReleaseActivationJobV2(
        this.database,
        this.workerId,
        { engine: this.engine, openkeys: this.openkeys },
      );
      if (!job) return;
      try {
        const ack = await this.deliverPricingReleaseActivationJob(job);
        await confirmPricingReleaseActivationJobV2(
          this.database,
          job,
          this.workerId,
          ack,
        );
      } catch (error) {
        const disposition = pricingReleaseActivationDisposition(error);
        try {
          await releasePricingReleaseActivationJobV2(
            this.database,
            job,
            this.workerId,
            disposition,
            message(error),
          );
        } catch (releaseError) {
          this.logger.error(
            `failed to release pricing-release activation job ${job.id}: ${message(releaseError)}`,
          );
          throw releaseError;
        }
        if (disposition === "dead") {
          this.logger.error(
            `pricing-release activation job ${job.id} failed permanently: ${message(error)}`,
          );
        }
      }
    }
  }

  private async deliverPricingReleaseActivationJob(
    job: ClaimedPricingReleaseActivationJobV2,
  ): Promise<PricingReleaseActivationAckV2> {
    const ack = await this.engine.activatePricingReleaseV2(job.request);
    if (ack.result === "rejected") {
      throw new PricingReleaseActivationDeliveryError(
        `pricing release activation rejected with ${ack.code}`,
        "dead",
      );
    }
    return ack;
  }

  private async flushPricingJobs(): Promise<void> {
    for (;;) {
      const job = await claimNextPricingJob(this.database, this.workerId);
      if (!job) return;
      try {
        await this.engine.setAccountMultiplier(job.engineAccountId, job.multiplierBp);
        await confirmPricingJob(this.database, job);
      } catch (error) {
        try {
          await retryPricingJob(this.database, job, message(error));
        } catch (retryError) {
          // C68: periodic stale-lease recovery above guarantees this processing job becomes
          // claimable again even if persisting its retry state failed during a database outage.
          // AUDIT-TODO(C68): move lease-expiry reclamation into claimNextPricingJob itself.
          this.logger.error(`failed to release pricing job ${job.id}: ${message(retryError)}`);
          throw retryError;
        }
      }
    }
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([
      new Promise((resolve) => setTimeout(resolve, milliseconds)),
      this.stopSignal,
    ]);
  }
}

function afterMonthCloseGrace(now: Date, graceMs: number): boolean {
  return now.getTime() >= utcMonthStart(now).getTime() + graceMs;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "pricing worker failed";
}

class PricingControlDeliveryError extends Error {
  constructor(
    message: string,
    readonly disposition: PricingControlJobDisposition,
  ) {
    super(message);
    this.name = "PricingControlDeliveryError";
  }
}

function fenceVersion(
  activeVersion: number | undefined,
  activeDigest: string | undefined,
  targetVersion: number,
  targetDigest: string,
  targetName: string,
): void {
  if (activeVersion === undefined) return;
  if (activeVersion > targetVersion) {
    throw new PricingControlDeliveryError(
      `${targetName} target version ${targetVersion} is older than engine version ${activeVersion}`,
      "superseded",
    );
  }
  if (activeVersion === targetVersion && activeDigest !== targetDigest) {
    throw new PricingControlDeliveryError(
      `${targetName} version ${targetVersion} has a different engine digest`,
      "dead",
    );
  }
}

export function requirePricingMutation(
  ack: PricingMutationAck,
  accepted: ReadonlyArray<PricingMutationAck["result"]>,
  phase: string,
): void {
  if (ack.result !== "rejected") {
    if (accepted.includes(ack.result)) return;
    throw new PricingControlDeliveryError(`${phase} returned unexpected result ${ack.result}`, "dead");
  }
  const disposition: PricingControlJobDisposition =
    ack.code === "stale" ? "superseded"
      : ack.code === "invalid" || ack.code === "version_conflict" || ack.code === "locked"
        ? "dead"
        : "retry";
  throw new PricingControlDeliveryError(`${phase} rejected with ${ack.code}`, disposition);
}

export function pricingControlDisposition(error: unknown): PricingControlJobDisposition {
  if (error instanceof PricingControlDeliveryError) return error.disposition;
  if (error instanceof EngineClientError) return error.retryable ? "retry" : "dead";
  return "retry";
}

class PricingReleaseActivationDeliveryError extends Error {
  constructor(
    message: string,
    readonly disposition: PricingReleaseActivationJobDispositionV2,
  ) {
    super(message);
    this.name = "PricingReleaseActivationDeliveryError";
  }
}

export function pricingReleaseActivationDisposition(
  error: unknown,
): PricingReleaseActivationJobDispositionV2 {
  if (error instanceof PricingReleaseActivationDeliveryError) return error.disposition;
  if (error instanceof PricingReleaseActivationJobV2Error) {
    return error.permanent ? "dead" : "retry";
  }
  if (error instanceof EngineClientError) return error.retryable ? "retry" : "dead";
  return "retry";
}
