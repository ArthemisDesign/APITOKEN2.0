import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { EngineClient } from "@claude-api/engine-client";
import { createDatabase } from "./client.js";
import {
  Stage8EvidenceV2Error,
  collectStage8CombinedEvidenceV2,
  parseStage8EngineEvidenceV2,
} from "./multi-discount-stage8-evidence.js";
import { createStage5OpenKeysInventoryReaderV2 } from "./pricing-stage5-materializer-v2.js";

function required(env: NodeJS.ProcessEnv, name: string): string {
  const value = env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

export async function runStage8CommerceEvidenceCli(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const [engineEvidencePath, extra] = argv;
  if (!engineEvidencePath || extra !== undefined) {
    throw new Error("usage: multi-discount-stage8-evidence-cli <engine-evidence-json-path>");
  }
  const connectionString = required(env, "DATABASE_URL");
  const engineControlKey = required(env, "ENGINE_CONTROL_KEY");
  const openkeysControlKey = env.OPENKEYS_CONTROL_KEY?.trim()
    || engineControlKey;
  const engineEvidence = parseStage8EngineEvidenceV2(await readFile(engineEvidencePath, "utf8"));
  const openkeys = createStage5OpenKeysInventoryReaderV2({
    baseUrl: env.OPENKEYS_INTERNAL_BASE_URL?.trim() || "http://127.0.0.1:3410",
    controlKey: openkeysControlKey,
  });

  const database = createDatabase(connectionString, "stage8-combined-evidence-v2");
  try {
    const engine = new EngineClient({
      baseUrl: env.ENGINE_BASE_URL?.trim() || "http://127.0.0.1:8790",
      controlKey: engineControlKey,
    });
    const report = await collectStage8CombinedEvidenceV2(
      database,
      { engine, openkeys },
      engineEvidence,
    );
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
    const code = error instanceof Stage8EvidenceV2Error ? error.code : "stage8_combined_failed";
    const name = error instanceof Error ? error.name : "UnknownError";
    process.stderr.write(`Stage 8 combined evidence failed (${name}, ${code})\n`);
    process.exitCode = 1;
  });
}
