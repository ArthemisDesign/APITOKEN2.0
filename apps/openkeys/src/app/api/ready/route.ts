import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { getDatabase } from "@/lib/db";
import { getEngineClient } from "@/lib/engine";
import {
  assertOpenKeysDatabaseContract,
  OPENKEYS_DATABASE_CONTRACT_QUERY,
  type OpenKeysDatabaseContractRow,
} from "@/lib/openkeys-pricing";
import { assertSecretBoxReady } from "@/lib/secret-box";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/** Readiness intentionally exposes no dependency details to the public internet. */
export async function GET(): Promise<NextResponse> {
  try {
    loadConfig();
    assertSecretBoxReady();
    const { pool } = getDatabase();
    const [database, schema, databaseContract] = await Promise.all([
      pool.query("SELECT 1"),
      pool.query(`
        SELECT j.id, j.batch_id, j.item_index, j.status, j.updated_at,
               k.removed_by, k.removal_reason, k.secret_version, k.secret_key_id
        FROM openkeys_issuance_jobs j
        LEFT JOIN openkeys_keys k ON false
        LIMIT 0
      `),
      pool.query<OpenKeysDatabaseContractRow>(OPENKEYS_DATABASE_CONTRACT_QUERY),
      // Authenticated and read-only: this proves ENGINE_CONTROL_KEY as well as engine reachability.
      getEngineClient().getSpendStats(),
    ]);
    assertOpenKeysDatabaseContract(databaseContract.rows);
    if (database.rowCount !== 1 || schema.rowCount !== 0) {
      throw new Error("dependency unavailable");
    }
    return NextResponse.json({ status: "ready" }, {
      headers: { "cache-control": "no-store" },
    });
  } catch {
    return NextResponse.json({ status: "unavailable" }, {
      status: 503,
      headers: { "cache-control": "no-store" },
    });
  }
}
