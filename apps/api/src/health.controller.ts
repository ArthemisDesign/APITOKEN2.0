import { Controller, Get, Inject, ServiceUnavailableException } from "@nestjs/common";
import type { HealthStatus } from "@claude-api/contracts";
import type { Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";
import { ReadinessService } from "./readiness.service.js";

const READINESS_CHECK_TIMEOUT_MS = 2_000;

type DependencyStatus = "up" | "down";
type ReadinessStatus = {
  status: "ok" | "unavailable";
  database?: DependencyStatus;
  engine?: DependencyStatus;
};

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error("readiness check timed out")), timeoutMs);
        timer.unref();
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

@Controller()
export class HealthController {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    private readonly readiness: ReadinessService,
  ) {}

  @Get("health")
  async health(): Promise<HealthStatus> {
    const [database, engine] = await Promise.all([
      this.database.pool.query("SELECT 1").then(() => "up" as const).catch(() => "down" as const),
      this.engine.health().then((ok) => ok ? "up" as const : "down" as const),
    ]);
    return {
      ok: database === "up" && engine === "up",
      service: "commercial-api",
      database,
      engine,
    };
  }

  @Get("live")
  live(): { status: "ok" } {
    return { status: "ok" };
  }

  @Get("ready")
  async ready(): Promise<ReadinessStatus> {
    if (!this.readiness.isAccepting()) {
      throw new ServiceUnavailableException({ status: "unavailable" });
    }

    const [database, engine] = await Promise.all([
      withTimeout(
        this.database.pool.query("SELECT 1").then(() => "up" as const).catch(() => "down" as const),
        READINESS_CHECK_TIMEOUT_MS,
      ).catch(() => "down" as const),
      withTimeout(
        this.engine.readiness().then((ok) => ok ? "up" as const : "down" as const),
        READINESS_CHECK_TIMEOUT_MS,
      ).catch(() => "down" as const),
    ]);
    // Slot health is this Nest process: drain and local commerce Postgres.
    // Engine Control API reachability stays in the JSON as telemetry so Caddy
    // `:8791` does not depool admin auth, the sales feed, or other non-engine
    // commerce routes when Anthropic `:8790` is down.
    const response: ReadinessStatus = {
      status: database === "up" && this.readiness.isAccepting()
        ? "ok"
        : "unavailable",
      database,
      engine,
    };
    if (response.status === "unavailable") {
      throw new ServiceUnavailableException(response);
    }
    return response;
  }
}
