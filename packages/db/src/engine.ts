import { randomUUID } from "node:crypto";
import type { Database } from "./client.js";

export interface EngineAccountMapping {
  engineAccountId: string | null;
  status: "pending" | "active" | "error" | "disabled";
  multBp: number;
  customerType: "b2c" | "b2b";
  lastError: string | null;
}

export interface StoredApiKey {
  id: string;
  engineKeyId: string;
  engineAccountId: string;
  label: string | null;
  keyMasked: string;
  status: "active" | "disabled";
  createdAt: Date;
}

export async function getEngineAccountMapping(database: Database, userId: string): Promise<EngineAccountMapping | null> {
  const result = await database.pool.query<EngineAccountRow>(`
    SELECT ea.engine_account_id, ea.status, ea.mult_bp, ea.last_error,
           COALESCE(cp.customer_type, 'b2c') AS customer_type
    FROM engine_accounts ea
    LEFT JOIN customer_profiles cp ON cp.user_id = ea.user_id
    WHERE ea.user_id = $1
  `, [userId]);
  const row = result.rows[0];
  return row ? {
    engineAccountId: row.engine_account_id,
    status: row.status,
    multBp: row.mult_bp,
    customerType: row.customer_type,
    lastError: row.last_error,
  } : null;
}

export async function markEngineAccountMissing(
  database: Database,
  userId: string,
  engineAccountId: string,
): Promise<void> {
  await database.pool.query(`
    UPDATE engine_accounts
    SET engine_account_id = NULL, status = 'error', last_error = 'engine account not found', updated_at = now()
    WHERE user_id = $1 AND engine_account_id = $2
  `, [userId, engineAccountId]);
}

export async function saveIssuedApiKey(database: Database, input: {
  userId: string;
  engineAccountId: string;
  engineKeyId: string;
  label: string | null;
  keyMasked: string;
}): Promise<StoredApiKey> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<ApiKeyRow>(`
      INSERT INTO api_keys (
        id, user_id, engine_account_id, engine_key_id, label, key_masked, status
      ) VALUES ($1, $2, $3, $4, $5, $6, 'active')
      ON CONFLICT (engine_key_id) WHERE engine_key_id IS NOT NULL DO UPDATE
      SET label = EXCLUDED.label, key_masked = EXCLUDED.key_masked, status = 'active', updated_at = now()
      WHERE api_keys.user_id = EXCLUDED.user_id
        AND api_keys.engine_account_id = EXCLUDED.engine_account_id
      RETURNING id, engine_key_id, engine_account_id, label, key_masked, status, created_at
    `, [randomUUID(), input.userId, input.engineAccountId, input.engineKeyId, input.label, input.keyMasked]);
    const row = result.rows[0];
    if (!row) throw new Error("engine key identifier belongs to a different user or account");
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('user', $1, 'api_key.created', 'api_key', $2, $3::jsonb)
    `, [input.userId, row.id, JSON.stringify({ label: input.label })]);
    await client.query("COMMIT");
    return mapApiKey(row);
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** Import or refresh a key already present in the engine without ever receiving its raw secret. */
export async function syncEngineApiKey(database: Database, input: {
  userId: string;
  engineAccountId: string;
  engineKeyId: string;
  label: string | null;
  keyMasked: string;
  status: "active" | "disabled";
}): Promise<StoredApiKey> {
  const result = await database.pool.query<ApiKeyRow>(`
    INSERT INTO api_keys (
      id, user_id, engine_account_id, engine_key_id, label, key_masked, status
    ) VALUES ($1, $2, $3, $4, $5, $6, $7)
    ON CONFLICT (engine_key_id) WHERE engine_key_id IS NOT NULL DO UPDATE
    SET label = EXCLUDED.label, key_masked = EXCLUDED.key_masked,
        status = EXCLUDED.status, updated_at = now()
    WHERE api_keys.user_id = EXCLUDED.user_id
      AND api_keys.engine_account_id = EXCLUDED.engine_account_id
    RETURNING id, engine_key_id, engine_account_id, label, key_masked, status, created_at
  `, [
    randomUUID(), input.userId, input.engineAccountId, input.engineKeyId,
    input.label, input.keyMasked, input.status,
  ]);
  const row = result.rows[0];
  if (!row) throw new Error("engine key identifier belongs to a different user or account");
  return mapApiKey(row);
}

export async function findOwnedApiKey(database: Database, userId: string, apiKeyId: string): Promise<StoredApiKey | null> {
  const result = await database.pool.query<ApiKeyRow>(`
    SELECT ak.id, ak.engine_key_id, ak.engine_account_id, ak.label, ak.key_masked, ak.status, ak.created_at
    FROM api_keys ak
    JOIN engine_accounts ea
      ON ea.user_id = ak.user_id AND ea.engine_account_id = ak.engine_account_id
    WHERE ak.id = $1 AND ak.user_id = $2 AND ak.engine_key_id IS NOT NULL
  `, [apiKeyId, userId]);
  return result.rows[0] ? mapApiKey(result.rows[0]) : null;
}

export async function markOwnedApiKeyDisabled(database: Database, userId: string, apiKeyId: string): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ id: string }>(`
      UPDATE api_keys SET status = 'disabled', updated_at = now()
      WHERE id = $1 AND user_id = $2
      RETURNING id
    `, [apiKeyId, userId]);
    if (!result.rows[0]) {
      await client.query("ROLLBACK");
      return false;
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id)
      VALUES ('user', $1, 'api_key.disabled', 'api_key', $2)
    `, [userId, apiKeyId]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

interface EngineAccountRow {
  engine_account_id: string | null;
  status: "pending" | "active" | "error" | "disabled";
  mult_bp: number;
  customer_type: "b2c" | "b2b";
  last_error: string | null;
}

interface ApiKeyRow {
  id: string;
  engine_key_id: string;
  engine_account_id: string;
  label: string | null;
  key_masked: string;
  status: "active" | "disabled";
  created_at: Date;
}

function mapApiKey(row: ApiKeyRow): StoredApiKey {
  return {
    id: row.id,
    engineKeyId: row.engine_key_id,
    engineAccountId: row.engine_account_id,
    label: row.label,
    keyMasked: row.key_masked,
    status: row.status,
    createdAt: row.created_at,
  };
}
