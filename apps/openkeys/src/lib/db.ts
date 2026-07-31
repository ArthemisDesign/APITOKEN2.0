import "server-only";
import { createOpenkeysDatabase, type OpenkeysDatabase } from "@claude-api/openkeys-db";
import { loadConfig } from "./config";

let cached: OpenkeysDatabase | undefined;

export function getDatabase(): OpenkeysDatabase {
  cached ??= createOpenkeysDatabase(loadConfig().databaseUrl, "openkeys-web");
  return cached;
}
