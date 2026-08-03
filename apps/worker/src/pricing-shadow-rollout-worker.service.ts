import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { PolicyActiveExpectation, PricingMutationAck } from "@claude-api/contracts";
import {
  claimPricingShadowPolicyJobsV2,
  completePricingShadowPolicyJobV2,
  failPricingShadowPolicyJobV2,
  PRICING_SHADOW_ROLLOUT_BINDING_V2,
  PricingShadowRolloutV2Error,
  recoverStalePricingShadowPolicyJobsV2,
  type ClaimedPricingShadowPolicyJobV2,
  type Database,
  type PricingShadowPolicyJobDispositionV2,
} from "@claude-api/db";
import { EngineClient, EngineClientError } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

export type ShadowRolloutEngine = Pick<
  EngineClient,
  | "getAccountPricingState"
  | "getActiveAccountPolicy"
  | "getAccountPolicyVersion"
  | "prepareAccountPolicy"
  | "activateAccountPolicy"
  | "lockedOpenkeysPolicyTransition"
>;

export class PricingShadowRolloutDeliveryError extends Error {
  constructor(
    message: string,
    readonly disposition: Extract<PricingShadowPolicyJobDispositionV2, "retry" | "blocked">,
  ) {
    super(message);
    this.name = "PricingShadowRolloutDeliveryError";
  }
}

function blocked(message: string): PricingShadowRolloutDeliveryError {
  return new PricingShadowRolloutDeliveryError(message, "blocked");
}

function sameBinding(
  left: { policy_enforcement: string; funding_enforcement: string; reconciliation_state: string },
  right: { policy_enforcement: string; funding_enforcement: string; reconciliation_state: string },
): boolean {
  return left.policy_enforcement === right.policy_enforcement
    && left.funding_enforcement === right.funding_enforcement
    && left.reconciliation_state === right.reconciliation_state;
}

function requireMutation(
  ack: PricingMutationAck,
  accepted: readonly string[],
  phase: string,
): void {
  if (ack.result !== "rejected") {
    if (accepted.includes(ack.result)) return;
    throw blocked(`${phase} returned unexpected result ${ack.result}`);
  }
  throw blocked(`${phase} rejected with ${ack.code}`);
}

async function deliverGenericShadowPolicy(
  engine: ShadowRolloutEngine,
  job: ClaimedPricingShadowPolicyJobV2,
  payload: Extract<ClaimedPricingShadowPolicyJobV2["payload"], { kind: "policy_shadow" }>,
): Promise<Record<string, unknown>> {
  const policy = payload.policy;
  const binding = payload.binding;
  const state = await engine.getAccountPricingState(policy.account_id);
  let expectation: PolicyActiveExpectation;
  if (state === "unbound") {
    expectation = "unbound";
  } else if ("inactive" in state) {
    expectation = { inactive: state.inactive.binding };
  } else {
    const active = state.active;
    if (active.policy.effective_version === policy.effective_version) {
      if (active.policy.content_digest !== policy.content_digest) {
        throw blocked("engine already holds a different policy under the rollout target version");
      }
      if (sameBinding(active.binding, binding)
          && JSON.stringify(active.policy) === JSON.stringify(policy)) {
        return {
          result: "unchanged",
          source: "engine_readback",
          request_digest: job.requestDigest,
          active,
        };
      }
    }
    if (active.policy.effective_version > policy.effective_version) {
      throw blocked("engine policy is newer than the rollout target");
    }
    expectation = {
      exact: {
        target: {
          version: active.policy.effective_version,
          content_digest: active.policy.content_digest,
        },
        binding: active.binding,
      },
    };
  }
  requireMutation(await engine.prepareAccountPolicy(policy), ["stored", "unchanged"], "shadow policy prepare");
  const readback = await engine.getAccountPolicyVersion(policy.account_id, policy.effective_version);
  if (!readback || JSON.stringify(readback) !== JSON.stringify(policy)) {
    throw blocked("engine shadow policy readback differs from the durable request");
  }
  const ack = await engine.activateAccountPolicy(policy, binding, expectation);
  requireMutation(ack, ["applied", "unchanged"], "shadow policy activation");
  return {
    result: ack.result,
    source: "engine_ack",
    request_digest: job.requestDigest,
    ack,
  };
}

async function deliverLockedOpenkeysTransition(
  engine: ShadowRolloutEngine,
  job: ClaimedPricingShadowPolicyJobV2,
  payload: Extract<ClaimedPricingShadowPolicyJobV2["payload"], { kind: "locked_openkeys_transition" }>,
): Promise<Record<string, unknown>> {
  const policy = payload.policy;
  const expected = payload.expected_active;
  const active = await engine.getActiveAccountPolicy(policy.account_id);
  if (active
      && active.policy.effective_version === policy.effective_version
      && active.policy.content_digest === policy.content_digest
      && sameBinding(active.binding, PRICING_SHADOW_ROLLOUT_BINDING_V2)) {
    return {
      result: "unchanged",
      source: "engine_readback",
      request_digest: job.requestDigest,
      active,
    };
  }
  if (!active) throw blocked("locked OpenKeys account has no active policy");
  if (active.policy.effective_version !== expected.target.version
      || active.policy.content_digest !== expected.target.content_digest
      || !sameBinding(active.binding, expected.binding)) {
    throw blocked("locked OpenKeys active policy drifted from the durable expectation");
  }
  if (!active.policy.replacement_locked) {
    throw blocked("active OpenKeys policy is not replacement-locked");
  }
  const ack = await engine.lockedOpenkeysPolicyTransition(policy.account_id, {
    policy,
    expected_active: expected,
  });
  requireMutation(ack, ["applied", "unchanged"], "locked OpenKeys transition");
  return {
    result: ack.result,
    source: "engine_ack",
    request_digest: job.requestDigest,
    ack,
  };
}

/** Delivers one claimed durable shadow policy job and returns its exact ACK evidence payload. */
export async function deliverPricingShadowPolicyJobV2(
  engine: ShadowRolloutEngine,
  job: ClaimedPricingShadowPolicyJobV2,
): Promise<Record<string, unknown>> {
  if (job.payload.kind === "locked_openkeys_transition") {
    return deliverLockedOpenkeysTransition(engine, job, job.payload);
  }
  return deliverGenericShadowPolicy(engine, job, job.payload);
}

export function pricingShadowRolloutDisposition(
  error: unknown,
): Extract<PricingShadowPolicyJobDispositionV2, "retry" | "blocked"> {
  if (error instanceof PricingShadowRolloutDeliveryError) return error.disposition;
  if (error instanceof PricingShadowRolloutV2Error) return error.permanent ? "blocked" : "retry";
  if (error instanceof EngineClientError) return error.retryable ? "retry" : "blocked";
  return "retry";
}

@Injectable()
export class PricingShadowRolloutWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(PricingShadowRolloutWorkerService.name);
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
    const recovered = await recoverStalePricingShadowPolicyJobsV2(
      this.database,
      this.config.get("PRICING_SHADOW_ROLLOUT_LEASE_MS", { infer: true }),
      this.config.get("PRICING_SHADOW_ROLLOUT_MAX_ATTEMPTS", { infer: true }),
    );
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale shadow rollout leases`);
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("PRICING_SHADOW_ROLLOUT_POLL_MS", { infer: true });
    const leaseMs = this.config.get("PRICING_SHADOW_ROLLOUT_LEASE_MS", { infer: true });
    const maxAttempts = this.config.get("PRICING_SHADOW_ROLLOUT_MAX_ATTEMPTS", { infer: true });
    const batchSize = this.config.get("PRICING_SHADOW_ROLLOUT_BATCH_SIZE", { infer: true });
    this.logger.log(`pricing shadow rollout worker ${this.workerId} started`);
    while (!this.stopped) {
      try {
        const jobs = await claimPricingShadowPolicyJobsV2(this.database, this.workerId, {
          batchSize,
          leaseMs,
          maxAttempts,
        });
        for (const job of jobs) {
          if (this.stopped) break;
          await this.processJob(job);
        }
      } catch (error) {
        this.logger.error(`shadow rollout claim failed: ${message(error)}`);
      }
      await this.sleep(pollMs);
    }
  }

  private async processJob(job: ClaimedPricingShadowPolicyJobV2): Promise<void> {
    try {
      const ack = await deliverPricingShadowPolicyJobV2(this.engine, job);
      await completePricingShadowPolicyJobV2(this.database, job, this.workerId, ack);
      this.logger.log(`shadow policy job ${job.id} confirmed as ${String(ack.result)}`);
    } catch (error) {
      const disposition = pricingShadowRolloutDisposition(error);
      try {
        const status = await failPricingShadowPolicyJobV2(
          this.database,
          job,
          this.workerId,
          disposition,
          message(error),
          {
            retryMs: this.config.get("PRICING_SHADOW_ROLLOUT_RETRY_MS", { infer: true }),
            maxAttempts: this.config.get("PRICING_SHADOW_ROLLOUT_MAX_ATTEMPTS", { infer: true }),
          },
        );
        if (status === "retry") {
          this.logger.warn(`shadow policy job ${job.id} will retry: ${message(error)}`);
        } else {
          this.logger.error(`shadow policy job ${job.id} is ${status}: ${message(error)}`);
        }
      } catch (releaseError) {
        this.logger.error(`failed to release shadow policy job ${job.id}: ${message(releaseError)}`);
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
  return error instanceof Error ? error.message : "shadow rollout delivery failed";
}
