# Database migrations

`pnpm --filter @claude-api/db db:migrate` builds the package, opens a dedicated PostgreSQL connection, and holds a session-level advisory lock while Drizzle applies migrations. Concurrent deploys therefore serialize instead of racing. The lock wait defaults to 30 seconds and each SQL statement to 15 minutes; override them with `DB_MIGRATION_LOCK_TIMEOUT_MS` and `DB_MIGRATION_STATEMENT_TIMEOUT_MS`.

## Expand/contract policy

1. **Expand:** ship additive, backward-compatible schema changes first (new nullable columns, tables, indexes, or compatible defaults).
2. **Backfill:** populate and verify existing rows while both old and new application versions remain valid.
3. **Contract later:** only drop old columns/tables or tighten `NOT NULL` in a later release, after the old code is fully removed and no deployed process depends on the old shape.

Roll application code back by deploying the previous release; never down-migrate production schema during rollback.

## Automatic production gate

Do not run production migrations manually during normal delivery. A commit merged to `master` is
tested against disposable PostgreSQL first. Only after the complete TypeScript and Rust suite passes,
the production watchdog:

1. verifies that every already-applied migration still exists byte-for-byte;
2. creates and validates a fresh production PostgreSQL backup;
3. runs the exact tested `packages/db/dist/migrate.js` under the file and PostgreSQL advisory locks;
4. atomically commits the new migration manifest;
5. permits the backend blue-green deployment to start.

Any backup or migration failure quarantines the SHA and blocks application deployment. Production
does not run the package script because it rebuilds. The watchdog consumes the prebuilt immutable
candidate directly. The manual `deploy/deploy.sh --api-only <sha>` path remains a recovery tool and
uses the same locked prebuilt migrator.

For a schema-dependent change, merge a migration-only expand commit first and wait for its
`deploy/migration` and `deploy/watchdog` GitHub statuses to pass. Merge the dependent code only
after production has the expanded schema, and keep the old release compatible throughout. Never
edit, rename, reorder, or delete a committed migration. Destructive contract changes require a
later release after backfill and after all old processes no longer depend on the old shape. See the top-level [`CONTRIBUTING.md`](../../CONTRIBUTING.md) and
[`docs/ops/DEPLOYMENT.md`](../../docs/ops/DEPLOYMENT.md).
