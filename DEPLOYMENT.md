# Production deployment runbook

This is the operator runbook for `84.32.48.2`. Controller internals live in
[`deploy/README.md`](deploy/README.md), immutable layout rules in
[`deploy/RELEASES.md`](deploy/RELEASES.md), and the Stage 2 authority design in
[`docs/STAGE2_POSTGRES_AUTHORITY.md`](docs/STAGE2_POSTGRES_AUTHORITY.md).

Pushing `master` does **not** deploy production. An operator selects an exact tested commit and runs
the component-specific two-phase controller as `deploy`; the scripts use `sudo` only for bounded
service and unit operations.

## Non-negotiable rules

- Use a full 40-character Git SHA that passed the integrated suite.
- Deploy engine and commerce API independently. Do not use unqualified `deploy.sh` after the
  PostgreSQL cutover.
- Never restart `apitoken-postgres.service` during an application deploy or rollback.
- Never manually restart API/engine between release selection and blue-green cutover.
- Never edit a finalized directory below `/opt/apitoken/releases` or `/srv/claude-api/releases`.
- Commerce migrations are expand/contract and forward-only; binary rollback never reverses them.
- The one-time SQLite-to-PostgreSQL cutover is complete. Do not rerun it for a normal release.

## Test the intended commit

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

## Select the production SHA

```bash
ssh deploy@84.32.48.2
cd /opt/apitoken/repo
git fetch origin master
git merge --ff-only origin/master
SHA=$(git rev-parse origin/master)
test ${#SHA} -eq 40
printf '%s\n' "$SHA"
```

The controllers fetch and verify the supplied commit again. The host checkout must also remain at
that SHA because the current worker still runs from the mutable checkout.

## Deploy the Rust engine

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

## Deploy the commerce API

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

The worker is the remaining Stage 1 exception: it is single-instance and runs from
`/opt/apitoken/repo/apps/worker`, not an immutable release. Its stop/start creates a short processing
gap but no overlap; durable jobs remain in PostgreSQL.

Build the exact checked-out SHA and its required workspace packages before restarting it:

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

If only the worker changed:

```bash
sudo systemctl stop apitoken-worker.service
sudo systemctl start apitoken-worker.service
systemctl is-active apitoken-worker.service
```

`--with-worker` only stops and starts the unit; it does not install dependencies, build TypeScript,
or copy artifacts.

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
