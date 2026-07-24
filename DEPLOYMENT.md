# Production deployment runbook

This is the operator runbook for `84.32.48.2`. Controller internals live in
[`deploy/README.md`](deploy/README.md), immutable layout rules in
[`deploy/RELEASES.md`](deploy/RELEASES.md), and the Stage 2 authority design in
[`docs/STAGE2_POSTGRES_AUTHORITY.md`](docs/STAGE2_POSTGRES_AUTHORITY.md).

Pushing or merging to `master` triggers the production-host watchdog. It tests an isolated exact
commit, takes fresh validated database backups, applies commerce migrations, then health-gated
blue-green deploys only the affected engine and/or backend. Engine migrations run transactionally
inside the inactive slot and must pass readiness before admission. It reports every stage on the
GitHub commit without using a paid Actions runner. The manual component controllers below are recovery and
explicit operator tools, not the normal contributor workflow. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Normal automatic delivery

Contributors and AI agents only merge production-ready work to `master`, then watch these GitHub
commit-status contexts:

Schema-dependent delivery uses two merges: an additive migration-only commit must complete first;
only after its migration and overall statuses are green may dependent application code reach
`master`. Contract cleanup is a later release.

| Context | Gate |
|---|---|
| `deploy/tests` | Complete isolated TypeScript/Rust/database/static test suite |
| `deploy/migration` | Validated database backups plus exact tested commerce migrator, or no commerce migration needed |
| `deploy/engine` | Exact-release engine rollout, or no engine change |
| `deploy/backend` | Exact-release API/worker rollout, or no backend change |
| `deploy/watchdog` | End-to-end result |

Affected database, engine, and backend stages also appear as GitHub deployment records in the
`production-database`, `production-engine`, and `production-backend` environments. This is reporting
only: builds and deployments run on the existing production host, so no paid GitHub runner is used.

The watchdog polls `origin/master` about once per minute. A failure quarantines that SHA and stops
the pipeline; neither later migrations nor application cutovers are attempted. Commerce migration
failure always blocks the backend. Engine migration or readiness failure leaves the serving engine
slot untouched. On every idle cycle it also requires exactly one PostgreSQL engine slot to be
active, ready, selected on the recorded release, and enabled. If an out-of-band service command
reactivates the inactive slot, the watchdog reconverges through the same readiness-gated controller;
it never stops the availability anchor before another current slot is verified. Normal releases
require no SSH command.

```bash
# Operator observation only
sudo apitoken-watchdog status
sudo apitoken-watchdog logs
```

Operational-definition changes (`deploy/`, `systemd/`, `compose.yaml`) are also automatic. Only
after the exact immutable candidate passes the complete test gate, a fixed root-owned bridge verifies
its SHA/tree/test marker, installs its watchdog controllers and systemd definitions, and continues
component delivery. A changed Caddy template is rendered with the existing production secrets,
validated, reloaded with automatic rollback, and never copied with repository placeholders. GitHub
workflow changes do not alter the production host and therefore need no host-install stage.

An operator can request an immediate poll or retry a proven transient failure:

```bash
sudo apitoken-watchdog run
sudo apitoken-watchdog retry <full-40-character-sha>
```

Prefer a new corrective commit for code/test/migration failures. A retry is not permission to alter
the immutable candidate or production database by hand.

### One-time GitHub reporting credential

Use a fine-grained personal access token limited to this repository with only **Commit statuses:
read/write** and **Deployments: read/write** (GitHub adds metadata read automatically). No Actions
permission, hosted runner, webhook, or paid GitHub feature is required. Store it only in the
root-owned file consumed by the reporting bridge:

```bash
sudo install -o root -g root -m 0600 /dev/null /etc/apitoken/github-watchdog.env
sudoedit /etc/apitoken/github-watchdog.env
```

```dotenv
GITHUB_REPOSITORY=OWNER/REPOSITORY
GITHUB_TOKEN=github_pat_REDACTED
```

Never put this token in a systemd environment, candidate checkout, application environment, shell
command line, or repository file. The watchdog calls a fixed root-owned bridge through narrowly
scoped sudo; tested candidate code runs as `apitoken-ci` and cannot read the credential. Revoke and
replace it immediately if its file permissions, logs, or host boundary are compromised.

### Least-privilege sudo policy

The `deploy` operator holds only the privileges the pipeline actually uses, defined in
`deploy/sudoers.d/95-apitoken-deploy`. This is what makes the sentence above true: with an
unrestricted `NOPASSWD: ALL` grant, `deploy` can read the GitHub credential and every
`/etc/apitoken/*.env` secret, and can replace the root-owned controllers that are meant to be fixed
trust anchors.

Applying the policy is a deliberate operator action, not an automatic deployment step — a policy
that locks out the pipeline cannot be repaired by the pipeline it governs:

```bash
sudo deploy/install-sudoers.sh --check
sudo deploy/install-sudoers.sh
sudo apitoken-watchdog status
```

The installer validates the candidate with `visudo -c` (treating warnings as fatal, since an unused
alias means an intended privilege is silently not granted), saves timestamped rollback copies under
`/root/sudoers-backups`, removes the legacy unrestricted grant, then verifies every privilege the
pipeline needs and every privilege it must not have. If any check fails it restores the previous
policy automatically and exits non-zero. It also removes `apitoken-ci` from the `deploy` group, so
candidate-derived test code can no longer write group-writable files in the deployment checkout.

The policy deliberately permits `deploy` to re-run the installer at its fixed root-owned path.
Without that, removing the unrestricted grant would be irreversible without console access.

When changing the policy, run `--check` first and keep a second session open until
`sudo apitoken-watchdog status` and a real watchdog cycle have both succeeded.

If you guard the change with a `systemd-run --on-active` timer that restores the old policy, note
that the installed policy deliberately does not permit stopping arbitrary units, so you cannot
cancel that timer once the policy is active. Either let it fire and re-run the installer afterwards
(its own path is permitted), or cancel the timer before applying the policy.

## Non-negotiable rules

- Use a full 40-character Git SHA that passed the integrated suite.
- Deploy engine and commerce API independently. Do not use unqualified `deploy.sh` after the
  PostgreSQL cutover.
- Never restart `apitoken-postgres.service` during an application deploy or rollback.
- Never manually restart API/engine between release selection and blue-green cutover.
- Never edit a finalized directory below `/opt/apitoken/releases` or `/srv/claude-api/releases`.
- Commerce migrations are append-only, expand/contract, and forward-only; the watchdog backs up and
  applies them automatically before application cutover. Binary rollback never reverses them.
- Engine migrations are ordered, transactional, advisory-locked, and forward-only. They run only in
  the inactive candidate after a backup; readiness failure prevents Caddy admission and preserves
  the old slot. Never edit an already-applied engine migration.
- The one-time SQLite-to-PostgreSQL cutover is complete. Do not rerun it for a normal release.

## Local pre-push test gate

```bash
pnpm install --frozen-lockfile
pnpm build
pnpm typecheck
pnpm test
cargo test --workspace
bash -n deploy/*.sh deploy/apitoken-db-dump
git diff --check
```

The Stage 2 host E2E destroys only a uniquely named temporary role/database. Run it only on a host
with the test PostgreSQL container, never against `claude_engine`:

```bash
sudo deploy/test-stage2-e2e.sh /path/to/test/claude-api
```

## Manual recovery: select the production SHA

```bash
ssh deploy@84.32.48.2
cd /opt/apitoken/repo
git fetch origin master
git merge --ff-only origin/master
SHA=$(git rev-parse origin/master)
test ${#SHA} -eq 40
printf '%s\n' "$SHA"
```

The controllers fetch and verify the supplied commit again. API, worker, and Content Studio processes run from
the immutable release selected by `/opt/apitoken/releases/current`; the host checkout is only the
controller source and may retain reviewed host-specific files.

## Manual recovery: deploy the Rust engine

```bash
deploy/deploy.sh --engine-bluegreen --dry-run "$SHA"
deploy/deploy.sh --engine-bluegreen "$SHA"

deploy/engine-bluegreen.sh --dry-run
deploy/engine-bluegreen.sh
```

Phase 1 builds and finalizes `/srv/claude-api/releases/<sha>`, then atomically selects it without
touching the serving process. Phase 2 starts the inactive 8787/8788 slot, proves its exact `MainPID`
runs the selected binary and returns `/ready` 200, lets Caddy admit it, flips the old slot to 503
readiness with `SIGUSR1`, and stops it only after the stable Control API remains healthy.

```bash
curl -fsS http://127.0.0.1:8790/ready
curl -fsS https://api.apitoken.sale/health
systemctl list-units 'claude-api@*.service'
systemctl list-unit-files 'claude-api@*.service'
```

The slot alternates. Consumers must never hard-code 8787 or 8788; commerce always uses
`http://127.0.0.1:8790`.

## Manual recovery: deploy the commerce API

```bash
deploy/deploy.sh --api-only --dry-run "$SHA"
deploy/deploy.sh --api-only "$SHA"

deploy/api-bluegreen.sh --dry-run
deploy/api-bluegreen.sh
```

Phase 1 builds `/opt/apitoken/releases/<sha>`, runs its prebuilt migration under the single migration
lock, and selects it without restarting the old API. Phase 2 alternates 3000/3001, exact-release
gates and admits the target, pre-drains the old process, and moves systemd boot persistence to the
verified target.

```bash
curl -fsS https://backend.apitoken.sale/v1/ready
systemctl list-units 'apitoken-api@*.service'
systemctl list-unit-files 'apitoken-api@*.service'
```

## Worker changes

The worker remains single-instance, but runs from
`/opt/apitoken/releases/current/apps/worker`. Its stop/start creates a short processing gap but no
overlap; durable jobs remain in PostgreSQL.

Build/select the exact immutable SHA and its required workspace packages before restarting it:

```bash
pnpm install --frozen-lockfile
pnpm --filter @claude-api/contracts build
pnpm --filter @claude-api/db build
pnpm --filter @claude-api/engine-client build
pnpm --filter @claude-api/payment-worker build
```

If API and worker changed together, replace the plain API cutover with:

```bash
deploy/api-bluegreen.sh --with-worker --dry-run
deploy/api-bluegreen.sh --with-worker
```

If only the worker changed, first select/build its immutable commerce release, then restart it:

```bash
deploy/deploy.sh --api-only --skip-migrate "$SHA"
sudo systemctl stop apitoken-worker.service
sudo systemctl start apitoken-worker.service
systemctl is-active apitoken-worker.service
```

`--with-worker` restarts the worker and Content Studio, then verifies the studio health endpoint and
both exact working directories. Phase 1 (`deploy.sh --api-only`) must already have built and selected
the release.

## Changes spanning engine and API

Both sides must remain compatible with the prior version during rollout. Prefer additive protocol
and schema changes. Ordinarily deploy and verify the engine first, then deploy the commerce API. If
a change needs the reverse order, the old engine must already understand the new API calls. There is
no atomic cross-component switch.

## Caddy and systemd definition changes

Application deploys do not silently replace host infrastructure definitions. If `deploy/Caddyfile`
changed, use the secret-preserving installer; never copy the placeholder-bearing repository file
directly over production:

```bash
sudo deploy/install-caddy.sh --check
sudo deploy/install-caddy.sh
systemctl is-active caddy
sudo ss -ltnH 'sport = :8790'
```

The installer extracts the existing host-only bcrypt/control-key lines without printing them,
validates the rendered candidate, saves a timestamped rollback copy, and performs a Caddy reload
rather than stop/start. Port 8790 must show `127.0.0.1:8790`, never `*:8790`.

Normal release selection also does not reinstall systemd templates. When a reviewed template itself
changes, verify and install it before the matching blue-green cycle; `daemon-reload` does not replace
the already-running process:

```bash
sudo systemd-analyze verify systemd/claude-api@.service systemd/apitoken-api@.service
sudo install -o root -g root -m 0644 systemd/claude-api@.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/apitoken-api@.service /etc/systemd/system/
sudo systemctl daemon-reload
```

Install only definitions actually changed. The subsequent component controller starts the inactive
slot under the new definition and preserves the old running slot as the rollback anchor.

## Rollback

Engine rollback to `previous`:

```bash
deploy/rollback.sh --engine-bluegreen --dry-run
deploy/rollback.sh --engine-bluegreen
deploy/engine-bluegreen.sh --dry-run
deploy/engine-bluegreen.sh
```

API rollback:

```bash
deploy/rollback.sh --api-only --dry-run
deploy/rollback.sh --api-only
deploy/api-bluegreen.sh --dry-run
deploy/api-bluegreen.sh
```

An explicit existing SHA may follow the selector. Rollback changes links and slots but never reverses
a database migration. A release whose migration breaks the prior binary is not rollout-safe.

### Automatic post-admission rollback

Before traffic is admitted, the blue-green controllers already fail closed: the old slot keeps
serving and nothing is rolled back. Once the new slot has been admitted and the old one drained,
however, a failed final verification would otherwise leave an unverified release serving with no
automatic way back. In that window only, the watchdog re-selects `previous` and re-runs the same
health-gated controller.

This never masks the failure: the candidate is still quarantined and `deploy/watchdog` still goes
red. A successful rollback only changes what is serving while you investigate. If the rollback
itself fails, the warning says so explicitly — inspect the slots before any further mutation.

## Retention

Every delivery creates an immutable release per affected component and a validated pre-deployment
dump per database. Nothing else removes them, so the watchdog prunes both at the start of each
cycle, while it holds the exclusive lock and no deploy, rollback, or migration is in flight.

- Build candidates: removed after 24 hours (measured from test completion when a marker exists).
- Immutable releases: the newest ten per component root are kept.
- Pre-deployment dumps: the newest ten per database are kept.

`current`, `previous`, the recorded component SHAs, and any release backing a live process are
always retained regardless of those counts. Live releases are resolved from each unit's `MainPID`
through `/proc/<pid>/exe` and `/proc/<pid>/cwd`, exactly like the readiness gates, rather than
trusting the symlinks alone. The hourly `<database>.dump` rotation artifacts are never pruned — they
remain the authoritative recovery objects.

## Failure behavior

- A phase-1 failure restores original `current`/`previous` links.
- If phase 1 succeeded but phase 2 has not run, the old in-memory process keeps serving. Run the
  matching blue-green controller; do not restart old through the moved symlink.
- Before admission, a phase-2 failure stops only the failed target and preserves the old slot.
- After admission/pre-drain, recovery keeps the verified new slot rather than reviving a drained one.
- If automatic recovery reports it is incomplete, inspect that warning and the exact unit journal
  before making another mutation.

```bash
sudo journalctl -u 'claude-api@*' -u 'apitoken-api@*' -u apitoken-worker --since today
sudo caddy validate --config /etc/caddy/Caddyfile
systemctl is-active caddy apitoken-worker claude-api-backup.timer
```

## Backups

The hourly timer writes custom-format dumps of `commerce` and `claude_engine`; daily Borgmatic then
copies `/var/lib/apitoken/backups` off-host.

```bash
sudo install -o root -g root -m 0644 systemd/claude-api-backup.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-backup.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now claude-api-backup.timer
sudo systemctl start claude-api-backup.service
systemctl show claude-api-backup.service -p Result
```

`commerce.dump` and `claude_engine.dump` must be non-empty, mode 0600, and readable by the matching
PostgreSQL `pg_restore --list`. Replication is not a backup; future HA still needs independent PITR.

## Final post-deploy gate

```bash
sudo deploy/configure-engine-control-url.sh --check
curl -fsS http://127.0.0.1:8790/ready
curl -fsS https://api.apitoken.sale/health
curl -fsS https://backend.apitoken.sale/v1/ready
systemctl is-active caddy apitoken-worker claude-api-backup.timer
git status --short
git rev-parse HEAD
```

At idle, expect one live engine owner, no pending settlement work, no leaked active capacity leases
or inflight count, zero reserved money, and no duplicate charge request IDs. See the Stage 2 document
for data-level verification and the caveat that nonzero counts can be legitimate during traffic.
