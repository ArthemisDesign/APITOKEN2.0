# Contributing and production delivery

This repository uses trunk delivery with a production-host watchdog. Feature branches and pull
requests are safe: only a commit that reaches `master` is considered for production. Nobody should
SSH to the host to deploy ordinary engine/backend changes or to run their migration manually.

The customer frontend remains an independent Vercel deployment. This workflow covers the Rust
engine, commerce API, commerce worker, and commerce PostgreSQL migrations.

## Contributor and AI-agent workflow

1. Fetch the current remote state and work on the appropriate `comp/*` or short feature branch.
2. If the change needs a schema update, add the new expand migration **before** code which depends
   on it. Never edit, rename, or delete an existing migration. See
   [`packages/db/MIGRATIONS.md`](packages/db/MIGRATIONS.md).
3. Keep application changes compatible with both the old and expanded schema during rollout.
4. Run the relevant local tests. Before merging a cross-component change, run the complete gate:

   ```bash
   pnpm install --frozen-lockfile
   pnpm build
   pnpm typecheck
   pnpm test
   cargo test --locked --workspace
   bash -n deploy/*.sh deploy/apitoken-db-dump
   git diff --check
   ```

5. Push the branch and merge it into `master` only when it is ready for production. A direct push
   to `master` has the same production meaning and should be exceptional.
6. Watch the commit statuses and production deployments in GitHub. Work is complete only when
   `deploy/watchdog` is green.

Do not trigger a second deployment to repair a red one. Fix the failure on a new branch and merge a
new commit. An operator may retry the same SHA only when the failure was proven transient and the
tested tree has not changed.

## What happens after `master` changes

The production host polls the read-only Git remote approximately once per minute and isolates the
exact 40-character SHA. It then:

1. installs dependencies from the lockfile and builds all TypeScript packages;
2. starts disposable PostgreSQL, runs the candidate migrations there, and runs all TypeScript tests;
3. runs the complete locked Rust workspace tests plus shell/whitespace checks;
4. makes fresh validated backups of both production databases and applies any new, append-only
   commerce migrations from that exact tested candidate under the migration lock;
5. starts an affected engine candidate only in the inactive slot; its ordered engine migrations run
   transactionally under the engine advisory lock, and failed migration/readiness can never admit it;
6. deploys only the affected engine and/or backend components through their health-gated
   blue-green controllers, restarts the single worker with the backend release, and verifies the
   exact production SHA;
7. records final status on the GitHub commit.

If any test, backup, migration, deployment, readiness check, or final verification fails, the SHA
is quarantined and later stages do not run. In particular, a failed commerce migration blocks the
backend start, while a failed inactive-slot engine migration blocks traffic admission. Migrations
are forward-only, so every migration must remain compatible with the currently serving release.

Commerce and engine schema changes have different executors but the same migration-first rule. A
commerce migration in `packages/db/migrations/` completes before backend cutover. An engine
migration in `crates/registry/migrations_pg/` is applied by the inactive engine before its `/ready`
can pass and before Caddy receives traffic. Add a new ordered/versioned engine migration and register
it in the engine migrator; never rewrite an already-applied engine SQL file.

## GitHub deployment statuses

No paid GitHub Actions runner is used. The production host reports commit statuses through a
root-owned, least-privilege GitHub credential; candidate code cannot read that credential. Open the
commit in GitHub to see:

| Context | Meaning |
|---|---|
| `deploy/tests` | Isolated build, disposable-database migration test, TypeScript tests, Rust tests, and static checks |
| `deploy/migration` | Production backup and automatic migration gate, or no migration required |
| `deploy/engine` | Engine blue-green deployment and final exact-release verification, or no engine change |
| `deploy/backend` | Commerce API/worker deployment and final exact-release verification, or no backend change |
| `deploy/watchdog` | Overall delivery result; this is the status to use as the final merge/deploy signal |

`pending` means the host is working or waiting for an infrastructure review. `failure`/`error`
means the commit did not complete production delivery. A green component status never overrides a
red overall status.

The same free GitHub API integration creates deployment records for the affected
`production-database`, `production-engine`, and `production-backend` environments. Those records
make the production history and final environment URL visible next to Vercel's deployments; they do
not run code on GitHub infrastructure.

Changes to `deploy/`, `systemd/`, `compose.yaml`, or `.github/` are deliberately not installed as
ordinary application code. The tests still run, but `deploy/watchdog` remains pending until an
operator reviews and applies the exact candidate definitions and approves that SHA. This protects
Caddy, systemd, and the watchdog itself from self-modifying deployments.

The operator and recovery commands are in [`DEPLOYMENT.md`](DEPLOYMENT.md); deployment controller
internals are in [`deploy/README.md`](deploy/README.md).
