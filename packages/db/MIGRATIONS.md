# Database migrations

`pnpm --filter @claude-api/db db:migrate` builds the package, opens a dedicated PostgreSQL connection, and holds a session-level advisory lock while Drizzle applies migrations. Concurrent deploys therefore serialize instead of racing. The lock wait defaults to 30 seconds and each SQL statement to 15 minutes; override them with `DB_MIGRATION_LOCK_TIMEOUT_MS` and `DB_MIGRATION_STATEMENT_TIMEOUT_MS`.

## Expand/contract policy

1. **Expand:** ship additive, backward-compatible schema changes first (new nullable columns, tables, indexes, or compatible defaults).
2. **Backfill:** populate and verify existing rows while both old and new application versions remain valid.
3. **Contract later:** only drop old columns/tables or tighten `NOT NULL` in a later release, after the old code is fully removed and no deployed process depends on the old shape.

Roll application code back by deploying the previous release; never down-migrate production schema during rollback.

Production does not run the package script from an immutable release because that script rebuilds.
`deploy/deploy.sh --api-only <sha>` executes the already-built
`/opt/apitoken/releases/<sha>/packages/db/dist/migrate.js` under the same advisory/file lock before
moving `releases/current`. See the top-level `DEPLOYMENT.md` for the two-phase API rollout.
