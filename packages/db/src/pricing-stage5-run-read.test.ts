import type { PoolClient } from "pg";
import { describe, expect, it, vi } from "vitest";
import type { Database } from "./client.js";
import { readPricingStage5RunV2 } from "./pricing-stage5-materializer-v2-store.js";

const digest = (seed: string): string => `sha256:v2:${seed.repeat(64)}`;

function fixture(row: Record<string, unknown> | undefined) {
  const query = vi.fn(async (statement: string, values?: unknown[]) => {
    if (statement.includes("FROM pricing_stage5_runs_v2")) {
      expect(statement).toContain("LEFT JOIN pricing_release_plans_v2 target");
      expect(statement).toContain("LEFT JOIN pricing_release_plans_v2 recovery");
      expect(values).toEqual([digest("a")]);
      return { rows: row === undefined ? [] : [row] };
    }
    return { rows: [] };
  });
  const release = vi.fn();
  const client = { query, release } as unknown as PoolClient;
  const connect = vi.fn().mockResolvedValue(client);
  const database = { pool: { connect } } as unknown as Database;
  return { database, query, release, connect };
}

function storedRun() {
  return {
    run_id: "2d20f96d-0f2b-4cff-9fa0-7c4b7fe1a6c5",
    plan_digest: digest("a"),
    status: "prepared",
    target_generation: "21",
    target_plan_digest: digest("b"),
    target_release_digest: digest("c"),
    recovery_generation: "22",
    recovery_plan_digest: digest("d"),
    recovery_release_digest: digest("e"),
    blocker_count: "0",
  };
}

describe("readPricingStage5RunV2", () => {
  it("reads the exact run and release lineage in a read-only repeatable snapshot", async () => {
    const state = fixture(storedRun());

    await expect(readPricingStage5RunV2(state.database, digest("a"))).resolves.toEqual(storedRun());
    expect(state.query.mock.calls[0]?.[0]).toBe("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    expect(state.query.mock.calls.at(-1)?.[0]).toBe("COMMIT");
    expect(state.release).toHaveBeenCalledOnce();
  });

  it("returns null only when the exact run is absent", async () => {
    const state = fixture(undefined);

    await expect(readPricingStage5RunV2(state.database, digest("a"))).resolves.toBeNull();
    expect(state.query.mock.calls.at(-1)?.[0]).toBe("COMMIT");
    expect(state.release).toHaveBeenCalledOnce();
  });

  it("rolls back when an existing run has incomplete release lineage", async () => {
    const state = fixture({ ...storedRun(), target_plan_digest: null });

    await expect(readPricingStage5RunV2(state.database, digest("a"))).rejects.toThrow();
    expect(state.query.mock.calls.at(-1)?.[0]).toBe("ROLLBACK");
    expect(state.release).toHaveBeenCalledOnce();
  });

  it("rejects an invalid digest before acquiring a database connection", async () => {
    const state = fixture(undefined);

    await expect(readPricingStage5RunV2(state.database, "bad-digest")).rejects.toThrow();
    expect(state.connect).not.toHaveBeenCalled();
  });
});
