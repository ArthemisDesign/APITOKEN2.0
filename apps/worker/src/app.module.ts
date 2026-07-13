import { hostname } from "node:os";
import { Inject, Injectable, Module, OnApplicationShutdown } from "@nestjs/common";
import { ConfigModule, ConfigService } from "@nestjs/config";
import { createDatabase } from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import { validateEnvironment, type Environment } from "./config.js";
import { CreditWorkerService } from "./credit-worker.service.js";
import { PricingWorkerService } from "./pricing-worker.service.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

@Injectable()
class DatabaseShutdown implements OnApplicationShutdown {
  constructor(@Inject(DATABASE) private readonly database: ReturnType<typeof createDatabase>) {}
  async onApplicationShutdown(): Promise<void> {
    await this.database.pool.end();
  }
}

@Module({
  imports: [ConfigModule.forRoot({ isGlobal: true, validate: validateEnvironment })],
  providers: [
    {
      provide: DATABASE,
      inject: [ConfigService],
      useFactory: (config: ConfigService<Environment, true>) =>
        createDatabase(config.get("DATABASE_URL", { infer: true })),
    },
    {
      provide: ENGINE_CLIENT,
      inject: [ConfigService],
      useFactory: (config: ConfigService<Environment, true>) => new EngineClient({
        baseUrl: config.get("ENGINE_BASE_URL", { infer: true }),
        controlKey: config.get("ENGINE_CONTROL_KEY", { infer: true }),
        timeoutMs: config.get("ENGINE_TIMEOUT_MS", { infer: true }),
      }),
    },
    { provide: WORKER_ID, useFactory: () => `${hostname()}:${process.pid}` },
    CreditWorkerService,
    PricingWorkerService,
    DatabaseShutdown,
  ],
})
export class AppModule {}
