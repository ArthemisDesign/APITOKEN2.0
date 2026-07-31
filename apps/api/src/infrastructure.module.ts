import { Global, Inject, Injectable, Module, OnApplicationShutdown } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { createDatabase, type Database } from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";

export const DATABASE = Symbol("DATABASE");
export const ENGINE_CLIENT = Symbol("ENGINE_CLIENT");

@Injectable()
class DatabaseShutdown implements OnApplicationShutdown {
  constructor(@Inject(DATABASE) private readonly database: Database) {}
  async onApplicationShutdown(): Promise<void> {
    await this.database.pool.end();
  }
}

@Global()
@Module({
  providers: [
    {
      provide: DATABASE,
      inject: [ConfigService],
      useFactory: (config: ConfigService<Environment, true>) =>
        createDatabase(config.get("DATABASE_URL", { infer: true }), "commerce-api"),
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
    DatabaseShutdown,
  ],
  exports: [DATABASE, ENGINE_CLIENT],
})
export class InfrastructureModule {}
