import { pathToFileURL } from "node:url";
import { createDatabase } from "./client.js";
import {
  FundingNormalizationJobV2Error,
  getFundingNormalizationStageStatusV2,
  stageFundingNormalizationJobV2,
} from "./funding-normalization-jobs.js";

const planDigestPattern = /^sha256:v2:[0-9a-f]{64}$/;

function usage(): never {
  throw new Error("usage: pricing-stage6-v2-cli <status|stage> <stage5-plan-digest>");
}

export async function runPricingStage6V2Cli(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const [mode, planDigest, extra] = argv;
  if (
    extra !== undefined
    || (mode !== "status" && mode !== "stage")
    || planDigest === undefined
    || !planDigestPattern.test(planDigest)
  ) {
    usage();
  }
  const connectionString = env.DATABASE_URL?.trim();
  if (!connectionString) throw new Error("DATABASE_URL is required");

  const database = createDatabase(connectionString, `pricing-stage6-v2-${mode}`);
  try {
    const stagedJobId = mode === "stage"
      ? await stageFundingNormalizationJobV2(database, { planDigest })
      : undefined;
    const status = await getFundingNormalizationStageStatusV2(database, planDigest);
    process.stdout.write(`${JSON.stringify({
      ...(stagedJobId === undefined ? {} : { staged_job_id: stagedJobId }),
      ...status,
    }, null, 2)}\n`);
  } finally {
    await database.pool.end();
  }
}

function isDirectExecution(moduleUrl: string): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && pathToFileURL(entrypoint).href === moduleUrl;
}

if (isDirectExecution(import.meta.url)) {
  runPricingStage6V2Cli().catch((error: unknown) => {
    const code = error instanceof FundingNormalizationJobV2Error
      ? (error.terminal ? "terminal" : "retryable")
      : "pricing_stage6_v2_failed";
    const name = error instanceof Error ? error.name : "UnknownError";
    process.stderr.write(`Pricing Stage 6 v2 failed (${name}, ${code})\n`);
    process.exitCode = 1;
  });
}
