import { drizzle, type NodePgDatabase } from "drizzle-orm/node-postgres";
import { Pool } from "pg";
import * as schema from "./schema.js";

export interface SalesDatabase {
  pool: Pool;
  db: NodePgDatabase<typeof schema>;
}

export function createSalesDatabase(connectionString: string, applicationName = "sales-db"): SalesDatabase {
  const pool = new Pool({ connectionString, max: 10, application_name: applicationName });
  return { pool, db: drizzle(pool, { schema }) };
}
