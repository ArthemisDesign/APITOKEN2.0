import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  claimNextPricingStage8CaptureJobV2,
  collectStage8CombinedEvidenceV2,
  completePricingStage8CaptureJobV2,
  createStage5OpenKeysInventoryReaderV2,
  persistPricingStage8EngineArtifactV2,
  PricingStage8CaptureJobV2Error,
  recoverStalePricingStage8CaptureJobsV2,
  releasePricingStage8CaptureJobV2,
  Stage8EvidenceV2Error,
  type ClaimedPricingStage8CaptureJobV2,
  type Database,
  type PricingStage8CaptureArtifactV2,
  type PricingStage8CaptureJobDispositionV2,
  type Stage8CombinedEvidenceV2,
  type Stage8EngineEvidenceV2,
  type Stage5V2OpenKeysReader,
} from "@claude-api/db";
import { EngineClient, EngineClientError } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

interface PricingStage8CaptureAttemptV2 {
  capture(): Promise<{ raw: string }>;
  persist(raw: string): Promise<PricingStage8CaptureArtifactV2>;
  collect(evidence: Stage8EngineEvidenceV2): Promise<Stage8CombinedEvidenceV2>;
  complete(
    artifactId: string,
    combined: Stage8CombinedEvidenceV2,
    rawCombined: string,
  ): Promise<void>;
}

/** Executes one exact source→durable-source→combined→durable-completion choreography. */
export async function executePricingStage8CaptureAttemptV2(
  attempt: PricingStage8CaptureAttemptV2,
): Promise<Stage8CombinedEvidenceV2> {
  const captured = await attempt.capture();
  const artifact = await attempt.persist(captured.raw);
  const combined = await attempt.collect(artifact.evidence);
  const rawCombined = `${JSON.stringify(combined, null, 2)}\n`;
  await attempt.complete(artifact.artifactId, combined, rawCombined);
  return combined;
}

@Injectable()
export class Stage8CaptureWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(Stage8CaptureWorkerService.name);
  private readonly openkeys: Stage5V2OpenKeysReader;
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });

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
    const recovered = await recoverStalePricingStage8CaptureJobsV2(
      this.database,
      this.config.get("STAGE8_CAPTURE_LEASE_MS", { infer: true }),
      this.config.get("STAGE8_CAPTURE_MAX_ATTEMPTS", { infer: true }),
    );
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale Stage 8 capture leases`);
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("STAGE8_CAPTURE_POLL_MS", { infer: true });
    const leaseMs = this.config.get("STAGE8_CAPTURE_LEASE_MS", { infer: true });
    const maxAttempts = this.config.get("STAGE8_CAPTURE_MAX_ATTEMPTS", { infer: true });
    this.logger.log(`Stage 8 capture worker ${this.workerId} started`);
    while (!this.stopped) {
      try {
        const job = await claimNextPricingStage8CaptureJobV2(
          this.database,
          this.workerId,
          leaseMs,
          maxAttempts,
        );
        if (job) await this.processJob(job);
      } catch (error) {
        this.logger.error(`Stage 8 capture claim failed: ${message(error)}`);
      }
      await this.sleep(pollMs);
    }
  }

  private async processJob(job: ClaimedPricingStage8CaptureJobV2): Promise<void> {
    try {
      const combined = await executePricingStage8CaptureAttemptV2({
        capture: async () => this.engine.capturePricingStage8EvidenceV2(job.request),
        persist: async (raw) => persistPricingStage8EngineArtifactV2(
          this.database,
          job,
          this.workerId,
          raw,
        ),
        collect: async (evidence) => collectStage8CombinedEvidenceV2(
          this.database,
          { engine: this.engine, openkeys: this.openkeys },
          evidence,
        ),
        complete: async (artifactId, combinedEvidence, rawCombined) =>
          completePricingStage8CaptureJobV2(
            this.database,
            job,
            this.workerId,
            artifactId,
            combinedEvidence,
            rawCombined,
          ),
      });
      this.logger.log(
        `Stage 8 capture job ${job.id} ${combined.passed ? "passed" : "blocked"} as ${combined.evidence_digest}`,
      );
    } catch (error) {
      await this.releaseJob(job, error);
    }
  }

  private async releaseJob(
    job: ClaimedPricingStage8CaptureJobV2,
    error: unknown,
  ): Promise<void> {
    const disposition = stage8CaptureDisposition(error);
    try {
      const status = await releasePricingStage8CaptureJobV2(
        this.database,
        job,
        this.workerId,
        disposition,
        message(error),
        this.config.get("STAGE8_CAPTURE_RETRY_MS", { infer: true }),
        this.config.get("STAGE8_CAPTURE_MAX_ATTEMPTS", { infer: true }),
      );
      if (status === "dead") {
        this.logger.error(`Stage 8 capture job ${job.id} failed closed: ${message(error)}`);
      } else {
        this.logger.warn(`Stage 8 capture job ${job.id} will retry: ${message(error)}`);
      }
    } catch (releaseError) {
      this.logger.error(`failed to release Stage 8 capture job ${job.id}: ${message(releaseError)}`);
    }
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([
      new Promise((resolve) => setTimeout(resolve, milliseconds)),
      this.stopSignal,
    ]);
  }
}

export function stage8CaptureDisposition(error: unknown): PricingStage8CaptureJobDispositionV2 {
  if (error instanceof PricingStage8CaptureJobV2Error) return error.permanent ? "dead" : "retry";
  if (error instanceof EngineClientError) return error.retryable ? "retry" : "dead";
  if (error instanceof Stage8EvidenceV2Error) return "dead";
  return "retry";
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Stage 8 capture failed";
}
