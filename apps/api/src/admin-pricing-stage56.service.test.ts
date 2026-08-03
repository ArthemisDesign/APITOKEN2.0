import type { ConfigService } from "@nestjs/config";
import {
  getFundingNormalizationStageStatusV2,
  runStage5MaterializerV2,
  stageFundingNormalizationJobV2,
  type Database,
} from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AdminService } from "./admin.service.js";
import type { Environment } from "./config.js";

vi.mock("@claude-api/db", async (importOriginal) => {
  const original = await importOriginal<typeof import("@claude-api/db")>();
  return {
    ...original,
    getFundingNormalizationStageStatusV2: vi.fn(),
    runStage5MaterializerV2: vi.fn(),
    stageFundingNormalizationJobV2: vi.fn(),
  };
});

const mockedRunStage5 = vi.mocked(runStage5MaterializerV2);
const mockedGetStage6 = vi.mocked(getFundingNormalizationStageStatusV2);
const mockedStage6 = vi.mocked(stageFundingNormalizationJobV2);

const digest = (seed: string): string => `sha256:v2:${seed.repeat(64)}`;

function service(): { service: AdminService; database: Database; engine: EngineClient } {
  const database = {} as Database;
  const engine = {} as EngineClient;
  const values = {
    OPENKEYS_INTERNAL_BASE_URL: "http://127.0.0.1:3410",
    OPENKEYS_CONTROL_KEY: undefined,
    ENGINE_CONTROL_KEY: "e".repeat(32),
  };
  const config = {
    get: vi.fn((name: keyof typeof values) => values[name]),
  } as unknown as ConfigService<Environment, true>;
  return { service: new AdminService(database, engine, config), database, engine };
}

function stage5Result(mode: "dry_run" | "apply") {
  return {
    mode,
    status: mode === "dry_run" ? "dry_run" as const : "materializing" as const,
    run_id: mode === "dry_run" ? null : "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    writes_committed: mode === "apply",
    engine_prepared: mode === "apply",
    plan: {
      plan_digest: digest("a"),
      commerce_inventory_digest: digest("b"),
      engine_scan_first_digest: digest("c"),
      engine_scan_second_digest: digest("c"),
      openkeys_scan_first_digest: digest("d"),
      openkeys_scan_second_digest: digest("d"),
      service_inventory_digest: digest("e"),
      funding_plan_digest: digest("f"),
      target_generation: 41,
      recovery_generation: 42,
      target: { content_digest: digest("1") },
      recovery: { content_digest: digest("2") },
      blockers: [],
    },
  };
}

function stage6Status(jobId: string | null = null) {
  return {
    stage5_plan_digest: digest("a"),
    stage5_status: "materializing" as const,
    target_generation: "41",
    target_plan_digest: digest("1"),
    target_release_digest: null,
    target_status: "materializing" as const,
    recovery_generation: "42",
    recovery_plan_digest: digest("2"),
    recovery_release_digest: null,
    recovery_status: "materializing" as const,
    job_id: jobId,
    job_status: jobId === null ? null : "pending" as const,
    job_attempts: jobId === null ? null : 0,
    job_last_error: null,
    job_result_digest: null,
    pending_accounts: 0,
    processing_accounts: 0,
    retry_accounts: 0,
    ready_accounts: 0,
    blocker_accounts: 0,
    target_funding_manifest_digest: null,
    recovery_funding_manifest_digest: null,
  };
}

describe("managed pricing Stage 5/6 service", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("uses the protected engine/OpenKeys authorities and returns a bounded strict Stage 5 summary", async () => {
    mockedRunStage5.mockResolvedValue(stage5Result("dry_run") as never);
    const fixture = service();

    await expect(fixture.service.dryRunPricingStage5V2()).resolves.toMatchObject({
      mode: "dry_run",
      plan_digest: digest("a"),
      target_generation: 41,
      recovery_generation: 42,
      blocker_count: 0,
      blockers: [],
    });
    expect(mockedRunStage5).toHaveBeenCalledWith(
      fixture.database,
      fixture.engine,
      expect.objectContaining({ getPage: expect.any(Function) }),
      { mode: "dry_run" },
    );
  });

  it("binds Stage 5 materialization and Stage 6 staging to the exact digest and operator audit", async () => {
    const fixture = service();
    mockedRunStage5.mockResolvedValue(stage5Result("apply") as never);
    await fixture.service.materializePricingStage5V2({
      plan_digest: digest("a"),
      reason: "materialize the reviewed inventory",
    }, "operator@example.test");
    expect(mockedRunStage5).toHaveBeenCalledWith(
      fixture.database,
      fixture.engine,
      expect.objectContaining({ getPage: expect.any(Function) }),
      {
        mode: "apply",
        expectedPlanDigest: digest("a"),
        audit: {
          actorId: "operator@example.test",
          reason: "materialize the reviewed inventory",
        },
      },
    );

    const jobId = "4f53639f-ced1-472f-998e-50e426bd5734";
    mockedStage6.mockResolvedValue(jobId);
    mockedGetStage6.mockResolvedValue(stage6Status(jobId));
    await expect(fixture.service.stagePricingStage6V2({
      plan_digest: digest("a"),
      reason: "normalize every balance account online",
    }, "operator@example.test")).resolves.toMatchObject({
      staged_job_id: jobId,
      job_id: jobId,
      job_status: "pending",
    });
    expect(mockedStage6).toHaveBeenCalledWith(fixture.database, {
      planDigest: digest("a"),
      audit: {
        actorId: "operator@example.test",
        reason: "normalize every balance account online",
      },
    });
    expect(mockedGetStage6).toHaveBeenCalledWith(fixture.database, digest("a"));
  });
});
