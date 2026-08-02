import { pathToFileURL } from "node:url";
import { EngineClient } from "@claude-api/engine-client";
import { createDatabase } from "./client.js";
import {
  runStage5MaterializerV2,
  type Stage5MaterializerV2Mode,
} from "./pricing-stage5-materializer-v2-store.js";
import {
  Stage5MaterializerV2Error,
  createStage5OpenKeysInventoryReaderV2,
} from "./pricing-stage5-materializer-v2.js";

function required(env: NodeJS.ProcessEnv, name: string): string {
  const value = env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function usage(): never {
  throw new Error(
    "usage: pricing-stage5-materializer-v2-cli <dry_run|apply> [expected-plan-digest]",
  );
}

export async function runStage5MaterializerV2Cli(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const [rawMode, expectedPlanDigest, extra] = argv;
  if (extra !== undefined || (rawMode !== "dry_run" && rawMode !== "apply")) usage();
  const mode: Stage5MaterializerV2Mode = rawMode;
  if (mode === "dry_run" && expectedPlanDigest !== undefined) usage();
  if (mode === "apply"
      && (expectedPlanDigest === undefined || !/^sha256:v2:[0-9a-f]{64}$/.test(expectedPlanDigest))) {
    usage();
  }

  const database = createDatabase(
    required(env, "DATABASE_URL"),
    `pricing-stage5-v2-${mode}`,
  );
  const controlKey = required(env, "ENGINE_CONTROL_KEY");
  const engine = new EngineClient({
    baseUrl: required(env, "ENGINE_BASE_URL"),
    controlKey,
  });
  const openkeys = createStage5OpenKeysInventoryReaderV2({
    baseUrl: env.OPENKEYS_INTERNAL_BASE_URL?.trim() || "http://127.0.0.1:3410",
    controlKey: env.OPENKEYS_CONTROL_KEY?.trim() || controlKey,
  });
  try {
    const result = await runStage5MaterializerV2(database, engine, openkeys, {
      mode,
      ...(expectedPlanDigest === undefined ? {} : { expectedPlanDigest }),
    });
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
  runStage5MaterializerV2Cli().catch((error: unknown) => {
    const code = error instanceof Stage5MaterializerV2Error
      ? error.code
      : "pricing_stage5_v2_failed";
    const name = error instanceof Error ? error.name : "UnknownError";
    process.stderr.write(`Pricing Stage 5 v2 failed (${name}, ${code})\n`);
    process.exitCode = 1;
  });
}
