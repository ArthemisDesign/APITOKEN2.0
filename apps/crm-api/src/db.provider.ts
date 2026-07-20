import type { OnApplicationShutdown, Provider } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { createCrmDatabase, type CrmDatabase } from "@claude-api/crm-db";
import type { Environment } from "./config.js";

export const CRM_DB = Symbol("CRM_DB");

class CrmDatabaseHolder implements OnApplicationShutdown {
  constructor(readonly database: CrmDatabase) {}
  async onApplicationShutdown(): Promise<void> {
    await this.database.pool.end();
  }
}

export const crmDatabaseProvider: Provider = {
  provide: CRM_DB,
  inject: [ConfigService],
  useFactory: (config: ConfigService<Environment, true>) =>
    new CrmDatabaseHolder(createCrmDatabase(config.get("CRM_DATABASE_URL", { infer: true }))),
};

export function dbOf(holder: unknown): CrmDatabase {
  return (holder as CrmDatabaseHolder).database;
}
