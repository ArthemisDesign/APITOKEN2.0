import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { EngineClient } from "@claude-api/engine-client";
import {
  runStage7OpenKeysBackfill,
  Stage7OpenKeysBackfillError,
  type Stage7OpenKeysBackfillMode,
} from "./openkeys-stage7-backfill";

function usage(): never {
  throw new Error(
    "usage: openkeys-stage7-backfill-cli <dry_run|apply> <stage5-dry-run.json> <assignment-matrix.json>",
  );
}

async function readJson(path: string): Promise<unknown> {
  return JSON.parse(await readFile(path, "utf8")) as unknown;
}

export async function runStage7OpenKeysBackfillCli(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const [rawMode, dryRunPath, matrixPath, extra] = argv;
  if (
    extra !== undefined ||
    !dryRunPath ||
    !matrixPath ||
    (rawMode !== "dry_run" && rawMode !== "apply")
  ) {
    usage();
  }
  const baseUrl = env.ENGINE_BASE_URL;
  const controlKey = env.ENGINE_CONTROL_KEY;
  if (!baseUrl) throw new Error("ENGINE_BASE_URL is required");
  if (!controlKey) throw new Error("ENGINE_CONTROL_KEY is required");

  const engine = new EngineClient({ baseUrl, controlKey });
  const result = await runStage7OpenKeysBackfill(
    engine,
    await readJson(dryRunPath),
    await readJson(matrixPath),
    rawMode as Stage7OpenKeysBackfillMode,
  );
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  if (result.result === "blocked") {
    throw new Stage7OpenKeysBackfillError(
      "stage7_preflight_blocked",
      `Stage 7 preflight found ${result.counts.conflict} conflicting OpenKeys account(s)`,
    );
  }
}

function isDirectExecution(moduleUrl: string): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && pathToFileURL(entrypoint).href === moduleUrl;
}

if (isDirectExecution(import.meta.url)) {
  runStage7OpenKeysBackfillCli().catch((error: unknown) => {
    const code = error instanceof Stage7OpenKeysBackfillError
      ? error.code
      : "stage7_openkeys_backfill_failed";
    const name = error instanceof Error ? error.name : "UnknownError";
    process.stderr.write(`Stage 7 OpenKeys backfill failed (${name}, ${code})\n`);
    process.exitCode = 1;
  });
}
