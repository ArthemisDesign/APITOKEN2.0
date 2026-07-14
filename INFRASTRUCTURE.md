# Production infrastructure

This document records the non-secret production topology for `apitoken.sale`. Credentials, OAuth
secrets, payment-provider keys, subscription tokens and database passwords must never be added to
the repository. Production secrets live in root-readable files below `/etc/apitoken` on the host.

## Current topology

```text
api.apitoken.sale --------------------------> commercial host 84.32.48.2 (Chicago)
                                                | reverse proxy
                                                `-> Rust core on 127.0.0.1:8787 after migration

future browser at apitoken.sale
        |
        `-> backend.apitoken.sale ----------> commercial host 84.32.48.2
                                                |-> NestJS API on 127.0.0.1:3000
                                                |-> payment/email/pricing worker
                                                `-> PostgreSQL 18 on 127.0.0.1:5433

commercial host -- encrypted Borg/SSH --> 84.32.109.82:2223/backup/.repo
```

`api.apitoken.sale` is the public Anthropic-compatible core endpoint. Its public proxy exposes only
`/v1/*` and `/health`, not the engine Control API. `backend.apitoken.sale` is the browser-facing
commercial API. The Rust engine remains authoritative for API keys, balances, reservations and
usage. The commercial PostgreSQL database owns users, authentication, payment state, B2C/B2B
pricing state and durable jobs. The commercial services access the engine only through its Control
API at `http://127.0.0.1:8787`. The legacy core host is not an upstream or fallback in this topology.

## Commercial host

| Item | Value |
|---|---|
| Public IPv4 | `84.32.48.2` |
| Location | Chicago, United States |
| CPU | AMD Ryzen 7 7700X, 8 cores / 16 threads |
| RAM | 96 GB DDR5 |
| Storage | 2 x 1 TB NVMe, Linux software RAID 1, about 894 GB usable |
| Network | 10 Gbit/s port, 100 TB egress allowance |
| OS | Ubuntu 24.04 LTS |
| Hostname | `apitokensale` |
| Deployment user | `deploy` (SSH public key only) |

The initial RAID synchronization must finish before planned power interruption. Check it with
`cat /proc/mdstat`.

Installed host tooling:

- Node.js 24 and pnpm 9.7.0;
- current stable Rust toolchain for the `deploy` user;
- Docker Engine with the Compose and Buildx plugins;
- PostgreSQL 16 client, build-essential, Clang, CMake, OpenSSL and SQLite development tools;
- UFW, Fail2ban and unattended security upgrades;
- BorgBackup and Borgmatic;
- Caddy 2 from its official stable package repository.

UFW denies inbound traffic by default and permits only SSH, HTTP and HTTPS. SSH password
authentication is disabled. Root can authenticate by key for recovery, while routine deployment
uses `deploy`. The application API and PostgreSQL bind to loopback. Caddy owns public ports 80/443,
redirects HTTP to HTTPS and terminates TLS for the two configured hostnames.

The active DNS record is `A *.apitoken.sale -> 84.32.48.2`. The apex remains independent for the
future frontend. Exact DNS records override the wildcard if they are added later.

## Host paths

```text
/opt/apitoken/repo             checked-out application repository
/var/lib/apitoken/postgres     PostgreSQL container data
/var/lib/apitoken/backups      application-consistent database export staging
/var/log/apitoken              application-owned logs when file output is needed
/etc/apitoken/postgres.env     root-readable PostgreSQL Compose environment
/etc/apitoken/api.env          root-readable API environment
/etc/apitoken/worker.env       root-readable worker environment
/etc/apitoken-backup           Borg identity, passphrase and exported recovery key
/etc/borgmatic/config.yaml     Borgmatic sources and retention policy
```

The repository itself is not a secret store. Environment files are provisioned directly on the
server with mode `0600`.

## Service management

Repository-managed deployment units:

```text
systemd/apitoken-postgres.service
systemd/apitoken-api.service
systemd/apitoken-worker.service
deploy/commerce-postgres.compose.yaml
```

Normal operations:

```bash
sudo systemctl status apitoken-postgres apitoken-api apitoken-worker
sudo systemctl restart apitoken-api apitoken-worker
sudo journalctl -u apitoken-api -u apitoken-worker --since today
sudo systemctl status caddy
sudo caddy validate --config /etc/caddy/Caddyfile
```

The PostgreSQL container publishes only to `127.0.0.1:5433`. API and worker use the same database
and authentication-token encryption key. Both use the server-side engine Control key, which must
never be returned to clients or placed in frontend configuration.

### Deployment state recorded on 2026-07-14

- The deployed Git revision was built and tested on the commercial host before service startup.
- PostgreSQL, API and worker services are enabled and running.
- `GET http://127.0.0.1:3000/v1/health` reports PostgreSQL as healthy. Its engine component remains
  down until the Rust core is migrated to this host; the backend has no legacy-core fallback.
- Seven commerce migrations are applied.
- The worker's credit and pricing processors are active. Until SMTP is connected, its environment
  deliberately uses `NODE_ENV=development` with `EMAIL_DELIVERY_MODE=disabled`; verification and
  reset messages remain durably queued. Change both settings when production SMTP is ready.
- Wildcard DNS resolves through public resolvers. Caddy serves valid public certificates for
  `api.apitoken.sale` and `backend.apitoken.sale`.
- `https://backend.apitoken.sale/v1/health` reaches the commercial API. It reports engine `down`
  until the local core migration is completed.
- The core proxy exposes only `/v1/*` and `/health`; `/admin/*`, `/pool` and unspecified paths return
  `404`. `/health` currently returns `502` because nothing is listening on local port `8787` yet.
- A dedicated read-only GitHub deploy key exists at
  `/home/deploy/.ssh/github_deploy_ed25519` and is registered for direct `git fetch` and `git pull`.

## Backups

| Item | Value |
|---|---|
| Cherry Backup Storage ID | `931342` |
| Public endpoint | `84.32.109.82:2223` |
| Borg repository | `ssh://backup-user@84.32.109.82:2223/backup/.repo` |
| Encryption | Borg `repokey-blake2` |
| Schedule | Daily, systemd `borgmatic.timer` |
| Retention | 7 daily, 4 weekly, 6 monthly |
| Checks | Repository weekly, recent archives monthly |

Borg backs up `/etc`, `/opt/apitoken`, `/var/lib/apitoken`, `/home/deploy` and `/root`, excluding
replaceable caches, toolchains and the live `/var/lib/apitoken/postgres` data directory. Docker
image/cache data under `/var/lib/docker` is intentionally not included. Once PostgreSQL is deployed,
backups must include application-consistent logical database dumps staged below
`/var/lib/apitoken/backups`; raw live database files are not a restore strategy.

`deploy/apitoken-db-dump` creates the PostgreSQL custom-format dump. It is installed as
`/usr/local/sbin/apitoken-db-dump` and configured as Borgmatic's `before_backup` hook. Validate a
dump with `pg_restore --list /var/lib/apitoken/backups/commerce.dump`.

The Borg private identity, repository key and passphrase are required for disaster recovery. An
independent copy exists on the operator workstation and must also be kept in an encrypted password
manager or other off-host location. A real restore of `/etc/hostname` was performed after repository
initialization.

The Cherry backup volume is tied to the Cherry server lifecycle. Do not terminate the server before
copying required archives to independent storage.

## Deployment procedure

```bash
ssh deploy@84.32.48.2
cd /opt/apitoken/repo
git fetch origin
git pull --ff-only origin master
pnpm install --frozen-lockfile
pnpm build
pnpm typecheck
pnpm test
cargo test --workspace
sudo systemctl restart apitoken-postgres apitoken-api apitoken-worker
```

Direct GitHub commands use the host's registered read-only deploy key. Production releases must
still be verified against the intended pushed commit hash before services restart.

Run PostgreSQL migrations after the database is healthy and before restarting a new API revision:

```bash
set -a
. /etc/apitoken/api.env
set +a
pnpm db:migrate
```

Production releases should deploy a specific tested commit. Never use `git reset --hard` on the
server as an update mechanism.

## Vercel frontend

The customer frontend lives in `apps/web` and deploys independently to Vercel. Configure the Vercel
project with `apps/web` as its Root Directory, use the Next.js framework preset, set
`NEXT_PUBLIC_BACKEND_URL=https://backend.apitoken.sale/v1`, and attach the production apex domain
`apitoken.sale`. Vercel must install from the repository-level pnpm workspace and lockfile.

Keep `apitoken.sale` as the canonical browser origin. The commercial API deliberately allows that
one exact origin for credentialed CORS and state-changing requests. Redirect alternate frontend
hosts such as `www.apitoken.sale` to the apex instead of serving the application from multiple
origins. The Vercel frontend contains no Control API or payment-provider secrets.

## Work still requiring external configuration

- Migrate and start the Rust core on `127.0.0.1:8787`; no legacy-core fallback is configured.
- Configure SMTP on a separate mail host.
- Add Google and GitHub OAuth application credentials.
- Add Cryptomus credentials and test its deployed webhook.
- Import `apps/web` into Vercel, attach `apitoken.sale`, and update the apex DNS using the exact
  records Vercel provides for the project.
- Add external monitoring/alert delivery and an independent second backup location.
- Before multiple Rust engine nodes share subscriptions, implement centralized durable subscription
  ownership/leases; never share the engine SQLite database over NFS.
