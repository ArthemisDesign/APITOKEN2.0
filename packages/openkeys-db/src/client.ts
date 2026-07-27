import { drizzle, type NodePgDatabase } from "drizzle-orm/node-postgres";
import { Pool } from "pg";
import * as schema from "./schema.js";

export interface OpenkeysDatabase {
  pool: Pool;
  db: NodePgDatabase<typeof schema>;
}

export function createOpenkeysDatabase(connectionString: string): OpenkeysDatabase {
  const pool = new Pool({ connectionString, max: 10 });
  return { pool, db: drizzle(pool, { schema }) };
}
