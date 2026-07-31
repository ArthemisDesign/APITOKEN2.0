import { drizzle, type NodePgDatabase } from "drizzle-orm/node-postgres";
import { Pool } from "pg";
import * as schema from "./schema.js";

export interface OpenkeysDatabase {
  pool: Pool;
  db: NodePgDatabase<typeof schema>;
}

export function createOpenkeysDatabase(connectionString: string, applicationName = "openkeys-db"): OpenkeysDatabase {
  const pool = new Pool({ connectionString, max: 10, application_name: applicationName });
  return { pool, db: drizzle(pool, { schema }) };
}
