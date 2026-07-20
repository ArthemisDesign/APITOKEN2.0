import { Controller, Get, Inject, ServiceUnavailableException } from "@nestjs/common";
import { CRM_DB, dbOf } from "./db.provider.js";
import { ReadinessService } from "./readiness.service.js";

const READINESS_CHECK_TIMEOUT_MS = 2_000;

type DependencyStatus = "up" | "down";

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
    @Inject(CRM_DB) private readonly holder: unknown,
    private readonly readiness: ReadinessService,
  ) {}

  @Get("health")
  async health(): Promise<{ ok: boolean; service: string; database: DependencyStatus }> {
    const database = await dbOf(this.holder).pool.query("SELECT 1")
      .then(() => "up" as const)
      .catch(() => "down" as const);
    return { ok: database === "up", service: "crm-api", database };
  }

  @Get("live")
  live(): { status: "ok" } {
    return { status: "ok" };
  }

  @Get("ready")
  async ready(): Promise<{ status: "ok"; database: DependencyStatus }> {
    if (!this.readiness.isAccepting()) {
      throw new ServiceUnavailableException({ status: "unavailable" });
    }
    const database = await withTimeout(
      dbOf(this.holder).pool.query("SELECT 1").then(() => "up" as const).catch(() => "down" as const),
      READINESS_CHECK_TIMEOUT_MS,
    ).catch(() => "down" as const);
    if (database !== "up" || !this.readiness.isAccepting()) {
      throw new ServiceUnavailableException({ status: "unavailable", database });
    }
    return { status: "ok", database };
  }
}
