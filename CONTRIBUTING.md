# Contributing and production delivery

This repository uses trunk delivery with a production-host watchdog. Feature branches and pull
requests are safe: only a commit that reaches `master` is considered for production. Nobody should
SSH to the host to deploy ordinary engine/backend changes or to run their migration manually.

The customer frontend remains an independent Vercel deployment. This workflow covers the Rust
engine, commerce API/worker, Content Studio, Sales, OpenKeys, and their PostgreSQL migrations.

## Contributor and AI-agent workflow

1. Fetch the current remote state and work on the appropriate `comp/*` or short feature branch.
   Several agents and contributors share this repository, and a branch is not an isolation
   boundary: one working tree has one checked-out branch, so switching branches in a shared
   directory rewrites a co-resident worker's files and carries their uncommitted changes onto your
   branch. Take the branch in a dedicated worktree instead:

   ```bash
   git worktree add ~/wt/<task> -b <type>/<task> origin/master
   cd ~/wt/<task>
   ```

   AI agents must work this way, must not switch, stash, reset, clean, merge or rebase any branch,
   and must stage explicit paths rather than `git add -A`. An agent reports its work as
   `git diff --stat origin/master...HEAD`, never as working-tree status: anything else in the tree
   may belong to another agent and must not be reverted, repaired, or explained.
2. If the change needs a schema update, deliver it in two production commits. First merge the
   additive/expand migration without code that depends on it, then wait for `deploy/migration` and
   `deploy/watchdog` to turn green. Only then merge the dependent application change. Never edit,
   rename, or delete an existing migration. See [`packages/db/MIGRATIONS.md`](packages/db/MIGRATIONS.md).
3. Keep old application code compatible with the expanded schema. Destructive contract cleanup is
   a later migration after every deployed version has stopped using the old shape.
4. Run the relevant local tests. Before merging a cross-component change, run the complete gate:

   ```bash
   pnpm install --frozen-lockfile
   pnpm build
   pnpm typecheck
   pnpm test
   bash deploy/sccache-cargo.sh cargo test --locked --workspace
   bash -n deploy/*.sh deploy/apitoken-db-dump
   git diff --check
   ```

   The merge script selects TypeScript, Rust, and deployment lanes from the exact committed diff and
   runs the selected independent lanes concurrently. Shell syntax and exact-range whitespace checks
   always run. Documentation-only changes can therefore stay cheap; an unknown path, deletion or
   rename is handled conservatively, and a change to the merge selector, shared classifier, merge
   regression suite, or Rust cache wrapper forces every lane. For an ordinary workspace edit, the
   TypeScript lane builds, typechecks, and tests the changed package, every workspace consumer, and
   their internal prerequisites. Shared pnpm/TypeScript inputs, workspace deletions, and selector
   changes fall back to the complete workspace. Successful Next.js builds atomically refresh
   validated `.next/cache` archives under the clone's git common directory, so fresh worktrees can
   reuse compiler state; a missing, corrupt, or unsafe archive is simply a cache miss. Rust
   compilation goes through a
   checksum-pinned `sccache`; its binary, 10 GiB object cache, and Cargo 1.91+ intermediate build
   directory live in the clone's git common directory and are reused by all linked worktrees. Final
   Cargo targets remain local to each worktree. The wrapper falls back to uncached Cargo if its
   one-time bootstrap is unavailable; set `SCCACHE_DISABLE=1` for an explicit uncached run.

5. Push the branch and land it with `./deploy/agent-merge.sh` (add `--allow-primary-tree` when you
   work in a plain clone rather than a worktree). That script is the only supported way to reach
   `master`. Before the expensive gate it rejects a red target, rebases onto the latest committed
   target SHA, and reads that SHA's `deploy/watchdog` context through the GitHub API. A pending
   target may overlap its rollout with speculative gates, but the script never pushes until the
   locked target check is green. It automatically reuses the credential already configured for
   `git push` (`git credential`, macOS Keychain), so neither `gh` nor a separately exported
   `GITHUB_TOKEN` is required. Temporarily unavailable status is polled by the script itself every
   five seconds. Before queueing, it requests trusted production-host validation for the exact
   rebased and pushed feature SHA, then overlaps that path-aware host gate with the fail-closed
   path-aware local gate. It takes the merge lock only after both pass. A later target move or push
   race that changes the SHA republishes the feature branch and repeats both gates for that new SHA;
   an unchanged SHA reuses both results. Once the same SHA reaches `master`, the host consumes its
   root-owned frozen candidate instead of testing it again. The script holds the lock until
   `deploy/watchdog` reports on the pushed SHA. Merging or pushing to `master` by hand races the
   production deployment of whoever merged immediately before it.
6. AI agents never ask a person to provide a GitHub token or prove that a deployment is green. If
   no reusable credential exists, repair the local Git credential helper and rerun; never merge
   blind. Work is complete only when the script reports the exact pushed SHA's `deploy/watchdog`
   context green, and the agent includes that verdict in its final report.

Do not trigger a second deployment to repair a red one. Fix the failure on a new branch and merge a
new commit. An operator may retry the same SHA only when the failure was proven transient and the
tested tree has not changed.

## What happens after `master` changes

The production host polls the read-only Git remote every five seconds and isolates the exact
40-character SHA. Validation is path-aware: TypeScript changes select the pnpm/database lane, engine
changes select the Rust lane, deployment changes select the operational regression suites, and any
unknown path fails safe into every lane. Selected language and operational lanes run concurrently.
The host then:

1. installs and builds every deployable TypeScript artifact, then typechecks/tests the
   dependency-aware changed-package closure against disposable PostgreSQL (or the full workspace
   for shared/deleted inputs), and/or runs the selected locked Rust workspace lane against its
   separate database using one shared Cargo target;
2. builds production engine binaries in that same tested Rust lane when an engine rollout is needed,
   records runtime-artifact digests, and freezes the candidate;
3. runs shell/whitespace checks and the deployment suites selected for operational changes;
4. makes fresh validated backups of both production databases and applies any new, append-only
   commerce migrations from that exact tested candidate under the migration lock;
5. starts an affected engine candidate only in the inactive slot; its ordered engine migrations run
   transactionally under the engine advisory lock, and failed migration/readiness can never admit it;
6. promotes the already-tested engine and commerce artifacts—without recompiling them—then deploys
   affected bounded contexts concurrently where their release roots, databases, and units are
   independent; engine and commerce remain ordered behind their shared deployment lock;
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
| `deploy/tests` | Selected isolated TypeScript/database, Rust, and operational validation lanes |
| `deploy/migration` | Production backup and automatic migration gate, or no migration required |
| `deploy/engine` | Engine blue-green deployment and final exact-release verification, or no engine change |
| `deploy/backend` | Commerce API/worker deployment and final exact-release verification, or no backend change |
| `deploy/sales` | Sales portal deployment and health verification, or no sales change |
| `deploy/openkeys` | OpenKeys deployment and health verification, or no OpenKeys change |
| `deploy/watchdog` | Overall delivery result; this is the status to use as the final merge/deploy signal |

`pending` means the host is testing, installing, or deploying the candidate. `failure`/`error`
means the commit did not complete production delivery. A green component status never overrides a
red overall status.

The same free GitHub API integration creates deployment records for the affected
`production-database`, `production-engine`, `production-backend`, `production-sales`, and
`production-openkeys` environments. Those records make the production history and final environment
URL visible next to Vercel's deployments; they do not run code on GitHub infrastructure.

The merge client sends exact-SHA requests through the transient `candidate-validation` environment.
Two low-priority host workers may validate distinct SHAs while the strictly serialized production
watchdog deploys their committed parent. They fetch only SHAs reachable from already-pushed
branches, require the committed `master` parent and every production baseline to be ancestors, run
the path-aware gate only for the feature delta, and freeze each result in the root-owned candidate
cache. PostgreSQL, Cargo, status, and candidate paths are isolated per worker; same-SHA work is
locked and reused by production. The host refetches `master` before a green verdict, so a stale
branch is rebased and revalidated. A red feature candidate updates only its own validation
deployment and `deploy/tests`; it cannot quarantine or change the healthy production verdict.

Changes to `deploy/`, `systemd/`, `observability/`, or `compose.yaml` are delivered automatically,
like application code, but through a stricter path. Only after the exact immutable candidate passes
its selected gate does a fixed root-owned bridge re-verify its SHA, tree, and clean worktree against
the test marker, then install that candidate's own controllers and systemd definitions. The running
old controller leaves the overall status pending and exits; the next five-second poll resumes from
the same frozen candidate under the newly installed controller. A changed Caddy template is rendered against the live host
secrets, validated, and reloaded with an automatic rollback copy; the repository file with its
placeholders is never copied over production. Changes under `.github/` do not touch the production
host and therefore need no host-install stage.

The operator and recovery commands are in [`DEPLOYMENT.md`](DEPLOYMENT.md); deployment controller
internals are in [`deploy/README.md`](deploy/README.md).
