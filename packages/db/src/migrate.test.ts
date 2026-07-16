import { readFileSync } from "node:fs";
import { basename, dirname, isAbsolute, join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  DEFAULT_MIGRATION_LOCK_TIMEOUT_MS,
  DEFAULT_MIGRATION_STATEMENT_TIMEOUT_MS,
  MIGRATION_DATABASE_URL_ENV,
  MIGRATION_LOCK_KEY,
  MIGRATIONS_FOLDER,
  resolveMigrationConfig,
} from "./migrate.js";

describe("migration configuration", () => {
  it("keeps the advisory lock key stable", () => {
    expect(MIGRATION_LOCK_KEY).toBe("719471115124720130");
  });

  it("resolves the package migrations folder", () => {
    expect(isAbsolute(MIGRATIONS_FOLDER)).toBe(true);
    expect(basename(MIGRATIONS_FOLDER)).toBe("migrations");
    expect(basename(dirname(MIGRATIONS_FOLDER))).toBe("db");
  });

  it("reads DATABASE_URL with bounded timeout defaults", () => {
    expect(resolveMigrationConfig({ [MIGRATION_DATABASE_URL_ENV]: "postgresql://db.example/app" })).toEqual({
      connectionString: "postgresql://db.example/app",
      lockTimeoutMs: DEFAULT_MIGRATION_LOCK_TIMEOUT_MS,
      statementTimeoutMs: DEFAULT_MIGRATION_STATEMENT_TIMEOUT_MS,
    });
  });

  it("accepts timeout overrides", () => {
    expect(
      resolveMigrationConfig({
        DATABASE_URL: "postgresql://db.example/app",
        DB_MIGRATION_LOCK_TIMEOUT_MS: "45000",
        DB_MIGRATION_STATEMENT_TIMEOUT_MS: "1200000",
      }),
    ).toEqual({
      connectionString: "postgresql://db.example/app",
      lockTimeoutMs: 45_000,
      statementTimeoutMs: 1_200_000,
    });
  });

  it("rejects missing connection configuration and unbounded timeouts", () => {
    expect(() => resolveMigrationConfig({})).toThrow("DATABASE_URL is required");
    expect(() =>
      resolveMigrationConfig({
        DATABASE_URL: "postgresql://db.example/app",
        DB_MIGRATION_LOCK_TIMEOUT_MS: "0",
      }),
    ).toThrow("DB_MIGRATION_LOCK_TIMEOUT_MS must be between 1 and 2147483647 milliseconds");
  });

  it("keeps the tier-5 expansion compatible across adjacent rollout migrations", () => {
    for (const migration of [
      "0008_prepay_tier_columns.sql",
      "0009_round_joshua_kane.sql",
      "0010_optimal_komodo.sql",
    ]) {
      const sql = readFileSync(join(MIGRATIONS_FOLDER, migration), "utf8");
      expect(sql).toContain('"current_tier" BETWEEN 0 AND 5');
      expect(sql).not.toContain('"current_tier" BETWEEN 0 AND 4');
    }
  });
});
