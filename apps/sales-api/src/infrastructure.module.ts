import { randomUUID } from "node:crypto";
import { hostname } from "node:os";
import { Global, Inject, Injectable, Module, OnApplicationShutdown } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { createSalesDatabase, type SalesDatabase } from "@claude-api/sales-db";
import type { Environment } from "./config.js";

export const SALES_DATABASE = Symbol("SALES_DATABASE");
export const WORKER_ID = Symbol("WORKER_ID");

@Injectable()
class DatabaseShutdown implements OnApplicationShutdown {
  constructor(@Inject(SALES_DATABASE) private readonly database: SalesDatabase) {}
  async onApplicationShutdown(): Promise<void> {
    await this.database.pool.end();
  }
}

@Global()
@Module({
  providers: [
    {
      provide: SALES_DATABASE,
      inject: [ConfigService],
      useFactory: (config: ConfigService<Environment, true>) =>
        createSalesDatabase(config.get("SALES_DATABASE_URL", { infer: true })),
    },
    {
      provide: WORKER_ID,
      useFactory: () => `${hostname()}:${process.pid}:${randomUUID().slice(0, 8)}`,
    },
    DatabaseShutdown,
  ],
  exports: [SALES_DATABASE, WORKER_ID],
})
export class InfrastructureModule {}
