import { Controller, Get, Inject } from "@nestjs/common";
import type { HealthStatus } from "@claude-api/contracts";
import type { Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";

@Controller()
export class HealthController {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
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
}
