import { pathToFileURL } from "node:url";
import {
  CatalogGen2Error,
  runCatalogGen2,
  type CatalogGen2Mode,
} from "./multi-discount-catalog-gen2.js";
import { createDatabase } from "./client.js";

function usage(): never {
  throw new Error("usage: multi-discount-catalog-gen2-cli <dry_run|apply>");
}

export async function runCatalogGen2Cli(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const [rawMode, extra] = argv;
  if (extra !== undefined || (rawMode !== "dry_run" && rawMode !== "apply")) {
    usage();
  }
  const connectionString = env.DATABASE_URL;
  if (!connectionString) throw new Error("DATABASE_URL is required");

  const mode: CatalogGen2Mode = rawMode;
  const database = createDatabase(connectionString, `catalog-gen2-${mode}`);
  try {
    const result = await runCatalogGen2(database, { mode });
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } finally {
    await database.pool.end();
  }
}

function isDirectExecution(moduleUrl: string): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && pathToFileURL(entrypoint).href === moduleUrl;
}

if (isDirectExecution(import.meta.url)) {
  runCatalogGen2Cli().catch((error: unknown) => {
    const code = error instanceof CatalogGen2Error ? error.code : "catalog_gen2_failed";
    const name = error instanceof Error ? error.name : "UnknownError";
    process.stderr.write(`catalog generation 2 failed (${name}, ${code})\n`);
    process.exitCode = 1;
  });
}
