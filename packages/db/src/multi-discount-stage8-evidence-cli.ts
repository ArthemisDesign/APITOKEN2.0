import { pathToFileURL } from "node:url";
import { createDatabase } from "./client.js";
import { collectStage8CommerceEvidence } from "./multi-discount-stage8-evidence.js";

export async function runStage8CommerceEvidenceCli(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  if (argv.length !== 0) throw new Error("usage: multi-discount-stage8-evidence-cli");
  const connectionString = env.DATABASE_URL;
  if (!connectionString) throw new Error("DATABASE_URL is required");

  const database = createDatabase(connectionString, "stage8-commerce-evidence");
  try {
    const report = await collectStage8CommerceEvidence(database);
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    if (!report.passed) process.exitCode = 2;
  } finally {
    await database.pool.end();
  }
}

function isDirectExecution(moduleUrl: string): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && pathToFileURL(entrypoint).href === moduleUrl;
}

if (isDirectExecution(import.meta.url)) {
  runStage8CommerceEvidenceCli().catch((error: unknown) => {
    const name = error instanceof Error ? error.name : "UnknownError";
    process.stderr.write(`Stage 8 commerce evidence failed (${name})\n`);
    process.exitCode = 1;
  });
}
