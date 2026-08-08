import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  applyPricingLedgerPage,
  applyPricingProviderBackfillPage,
  applyPricingTopupBackfillPage,
  advanceAccountStrictChain,
  claimNextPricingControlJob,
  claimNextPricingJob,
  completePricingProviderBackfill,
  completePricingUsageSync,
  confirmPricingControlJob,
  confirmPricingJob,
  getPricingUsageCursor,
  getPricingProviderBackfillCursor,
  getPricingTopupBackfillCursor,
  listPendingStrictChainAccounts,
  listPricingSyncTargets,
  PricingControlNotifyListener,
  recoverStalePricingControlJobs,
  recoverStalePricingJobs,
  releasePricingControlJob,
  retryPricingJob,
  runPricingBackfillSweep,
  type Database,
  type ClaimedPricingControlJob,
  type PricingControlJobDisposition,
  type PricingSyncTarget,
} from "@claude-api/db";
import type {
  PolicyActiveExpectation,
  PricingActiveExpectation,
  PricingMutationAck,
} from "@claude-api/contracts";
import { EngineClient, EngineClientError, type EngineKeyActivationPolicyAck } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

const PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC = 4;
// Догоняющий скан истории пополнений — разовая работа на аккаунт, поэтому лимит страниц за цикл
// держим таким же скромным: цель не «быстро», а «без всплеска нагрузки на движок».
const TOPUP_BACKFILL_MAX_PAGES_PER_SYNC = 4;
// The strict chain advances one durable step per account per pass, so a small bound keeps a
// flush fast; the remainder is picked up by the next pass.
const STRICT_CHAIN_MAX_ACCOUNTS_PER_SWEEP = 25;

@Injectable()
export class PricingWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(PricingWorkerService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });
  private controlNotify: PricingControlNotifyListener | undefined;
  private controlFlushRunning = false;
  private controlFlushQueued = false;

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    const recoveredControl = await recoverStalePricingControlJobs(this.database);
    if (recoveredControl > 0) {
      this.logger.warn(`recovered ${recoveredControl} stale pricing-control jobs`);
    }
    const recovered = await recoverStalePricingJobs(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale pricing jobs`);
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
        // coalescing dispatcher so a tick never doubles a LISTEN/NOTIFY-triggered pass. The
        // strict chain advances on the same fast tick: a newly provisioned account's first key
        // issuance waits on the shadow→strict staging plus delivery, so it is latency-sensitive
        // delivery work, not background recovery.
        this.requestControlFlush();
        await this.flushPricingJobs();
        await this.flushPendingStrictChains();
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

        // getPricingUsageCursor reconciles durable confirmed-credit accrual markers, including
        // refund/dispute reversal, before any engine network I/O. Keep one authority for that
        // mutation: a separate worker-side refund loop can subtract the same marker twice.
        await this.flushPricingBackfill();
        const targets = await listPricingSyncTargets(this.database);
        for (const target of targets) {
          if (this.stopped) break;
          try {
            await this.syncTarget(target);
          } catch (error) {
            this.logger.error(`pricing usage sync failed for ${target.userId}: ${message(error)}`);
          }
        }
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
    if (job.binding.policy_enforcement === "strict") {
      // The strict trigger requires every active key to carry the exact ACK at flip time.
      // Re-stamp before the activation attempt too: a key created between the chain's preflight
      // and this delivery would otherwise reject the flip again and again.
      await this.restampActiveKeysForStrictPolicy(job.spec.account_id, {
        effectivePolicyVersion: job.spec.effective_version,
        policyDigest: job.spec.content_digest,
      });
    }
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
   * The new-account direct strict chain of the release-v2 retirement (docs/commerce/PRICING.md):
   * registration provisioning flags the binding strict_chain_pending, and this flush advances
   * the chain account-locally — shared preflight + durable strict staging once the exact
   * version confirms under shadow, then the engine opt-out marker once the strict delivery
   * confirms. The opt-out disarms the flag, so a replay never duplicates the chain; a failed
   * precondition is recorded on the binding and retried on the next pass instead of producing
   * a partial state, and an account that cannot reach strict/strict/verified is never opted
   * out — it keeps working on its current path.
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
        } else if (result.status === "opted_out") {
          this.logger.log(
            `strict chain completed for ${candidate.userId}: pricing release opt-out marker applied`,
          );
        } else if (result.status === "failed") {
          this.logger.error(`strict chain for ${candidate.userId} cannot advance: ${result.error}`);
        }
      } catch (error) {
        this.logger.error(`strict chain sweep failed for ${candidate.userId}: ${message(error)}`);
      }
    }
  }

  /**
   * The existing-account backfill of the release-v2 retirement (phase 2.2, runbook
   * docs/ops/PRICING_RELEASE_BACKFILL.md): a bounded arm lane on the slow sweep. Each pass
   * takes up to PRICING_BACKFILL_BATCH_SIZE eligible accounts (optionally restricted to the
   * PRICING_BACKFILL_ACCOUNT_ALLOWLIST canary set), materializes the account's policy at the
   * live catalog head, proves release/strict equivalence, and arms the SAME direct strict
   * chain the fast tick already drives — nothing here forks or re-implements the chain.
   * Per-account failures are recorded on the binding (last_error) by the canonical module
   * and logged loudly; one account never blocks the others, and a completed account leaves
   * the candidate set via the durable opt-out audit marker, so the lane is resumable and
   * replay-safe. The per-account completion log line ("strict chain completed …") is
   * emitted by flushPendingStrictChains when the engine opt-out marker lands.
   */
  private async flushPricingBackfill(): Promise<void> {
    if (!this.config.get("PRICING_BACKFILL_ENABLED", { infer: true })) return;
    const batchSize = this.config.get("PRICING_BACKFILL_BATCH_SIZE", { infer: true });
    const allowlist = this.config.get("PRICING_BACKFILL_ACCOUNT_ALLOWLIST", { infer: true })
      .split(",")
      .map((entry) => entry.trim())
      .filter((entry) => entry.length > 0);
    const summary = await runPricingBackfillSweep(this.database, this.engine, {
      limit: batchSize,
      ...(allowlist.length > 0 ? { allowlist } : {}),
    });
    for (const accountId of summary.armed) {
      this.logger.log(`pricing backfill armed the direct strict chain for ${accountId}`);
    }
    for (const failure of summary.failed) {
      this.logger.error(`pricing backfill for ${failure.engineAccountId} cannot advance: ${failure.error}`);
    }
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
