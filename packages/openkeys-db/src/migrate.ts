import { fileURLToPath, pathToFileURL } from "node:url";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";

export const MIGRATION_DATABASE_URL_ENV = "OPENKEYS_DATABASE_URL";
export const MIGRATION_LOCK_TIMEOUT_ENV = "OPENKEYS_DB_MIGRATION_LOCK_TIMEOUT_MS";
export const MIGRATION_STATEMENT_TIMEOUT_ENV = "OPENKEYS_DB_MIGRATION_STATEMENT_TIMEOUT_MS";
// Отдельный от commerce/sales advisory-lock: контексты мигрируют независимо.
export const MIGRATION_LOCK_KEY = "5417338209911240771";
export const DEFAULT_MIGRATION_LOCK_TIMEOUT_MS = 30_000;
export const DEFAULT_MIGRATION_STATEMENT_TIMEOUT_MS = 15 * 60_000;
export const MIGRATION_CONNECTION_TIMEOUT_MS = 10_000;
export const MIGRATION_CLEANUP_TIMEOUT_MS = 10_000;
export const MIGRATIONS_FOLDER = fileURLToPath(new URL("../migrations", import.meta.url));

export interface MigrationConfig {
  connectionString: string;
  lockTimeoutMs: number;
  statementTimeoutMs: number;
}

function readTimeoutMs(env: NodeJS.ProcessEnv, name: string, fallback: number): number {
  const raw = env[name];
  if (raw === undefined) return fallback;
  if (!/^\d+$/.test(raw)) throw new Error(`${name} must be a positive integer`);

  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0 || value > 2_147_483_647) {
    throw new Error(`${name} must be between 1 and 2147483647 milliseconds`);
  }
  return value;
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), timeoutMs);
        timer.unref();
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

export function resolveMigrationConfig(env: NodeJS.ProcessEnv = process.env): MigrationConfig {
  const connectionString = env[MIGRATION_DATABASE_URL_ENV];
  if (!connectionString) throw new Error(`${MIGRATION_DATABASE_URL_ENV} is required`);

  return {
    connectionString,
    lockTimeoutMs: readTimeoutMs(env, MIGRATION_LOCK_TIMEOUT_ENV, DEFAULT_MIGRATION_LOCK_TIMEOUT_MS),
    statementTimeoutMs: readTimeoutMs(
      env,
      MIGRATION_STATEMENT_TIMEOUT_ENV,
      DEFAULT_MIGRATION_STATEMENT_TIMEOUT_MS,
    ),
  };
}

export async function runMigrations(env: NodeJS.ProcessEnv = process.env): Promise<void> {
  const config = resolveMigrationConfig(env);
  const client = new Client({
    connectionString: config.connectionString,
    connectionTimeoutMillis: MIGRATION_CONNECTION_TIMEOUT_MS,
    query_timeout: config.statementTimeoutMs,
  });
  let lockAcquired = false;
  let runError: unknown;

  try {
    await client.connect();
    await client.query(`SET lock_timeout = '${config.lockTimeoutMs}ms'`);
    await client.query(`SET statement_timeout = '${config.statementTimeoutMs}ms'`);

    console.info("acquiring openkeys migration lock");
    await client.query("SELECT pg_advisory_lock($1::bigint)", [MIGRATION_LOCK_KEY]);
    lockAcquired = true;
    console.info("acquired openkeys migration lock");

    await migrate(drizzle(client), { migrationsFolder: MIGRATIONS_FOLDER });
  } catch (error) {
    runError = error;
    throw error;
  } finally {
    let cleanupError: unknown;

    if (lockAcquired) {
      try {
        await withTimeout(
          client.query("SELECT pg_advisory_unlock($1::bigint)", [MIGRATION_LOCK_KEY]),
          MIGRATION_CLEANUP_TIMEOUT_MS,
          "migration lock cleanup timed out",
        );
      } catch (error) {
        cleanupError = error;
        console.error("failed to release migration lock explicitly; closing the client will release it");
      }
    }

    try {
      await client.end();
    } catch (error) {
      cleanupError ??= error;
      console.error("failed to close migration database client");
    }

    if (runError === undefined && cleanupError !== undefined) throw cleanupError;
  }
}

function isDirectExecution(moduleUrl: string): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && pathToFileURL(entrypoint).href === moduleUrl;
}

if (isDirectExecution(import.meta.url)) {
  runMigrations().catch((error: unknown) => {
    const errorName = error instanceof Error ? error.name : "UnknownError";
    console.error(`openkeys database migration failed (${errorName})`);
    process.exitCode = 1;
  });
}
