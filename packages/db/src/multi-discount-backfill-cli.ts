import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import {
  stage5AssignmentMatrixSchema,
  stage5InventorySchema,
  Stage5BackfillError,
  runStage5Backfill,
  type Stage5AssignmentMatrix,
  type Stage5BackfillMode,
  type Stage5Inventory,
} from "./multi-discount-backfill.js";
import { createDatabase } from "./client.js";

function usage(): never {
  throw new Error(
    "usage: multi-discount-backfill-cli <dry_run|safe|approved> <inventory.json> [assignment-matrix.json]",
  );
}

async function readJson(path: string): Promise<unknown> {
  return JSON.parse(await readFile(path, "utf8")) as unknown;
}

export async function runStage5BackfillCli(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const [rawMode, inventoryPath, matrixPath, extra] = argv;
  if (
    extra !== undefined ||
    !inventoryPath ||
    (rawMode !== "dry_run" && rawMode !== "safe" && rawMode !== "approved") ||
    (rawMode === "approved" ? !matrixPath : matrixPath !== undefined)
  ) {
    usage();
  }
  const connectionString = env.DATABASE_URL;
  if (!connectionString) throw new Error("DATABASE_URL is required");

  const mode: Stage5BackfillMode = rawMode;
  const inventory: Stage5Inventory = stage5InventorySchema.parse(await readJson(inventoryPath));
  let assignmentMatrix: Stage5AssignmentMatrix | undefined;
  if (matrixPath) {
    assignmentMatrix = stage5AssignmentMatrixSchema.parse(await readJson(matrixPath));
  }

  const database = createDatabase(connectionString, `stage5-backfill-${mode}`);
  try {
    const result = await runStage5Backfill(database, inventory, {
      mode,
      ...(assignmentMatrix ? { assignment_matrix: assignmentMatrix } : {}),
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
  runStage5BackfillCli().catch((error: unknown) => {
    const code = error instanceof Stage5BackfillError ? error.code : "stage5_backfill_failed";
    const name = error instanceof Error ? error.name : "UnknownError";
    process.stderr.write(`Stage 5 backfill failed (${name}, ${code})\n`);
    process.exitCode = 1;
  });
}
