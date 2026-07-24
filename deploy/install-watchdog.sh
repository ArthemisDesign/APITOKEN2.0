#!/usr/bin/env bash
set -euo pipefail

# One-time root installer for the host-local, free GitHub polling watchdog.
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'run as root' >&2; exit 1; }
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"
command -v systemctl >/dev/null || { echo 'systemd is required' >&2; exit 1; }
command -v curl >/dev/null || { echo 'curl is required' >&2; exit 1; }
command -v jq >/dev/null || { echo 'jq is required' >&2; exit 1; }
command -v openssl >/dev/null || { echo 'openssl is required' >&2; exit 1; }
id deploy >/dev/null 2>&1 || { echo 'deploy user is required' >&2; exit 1; }
id apitoken-ci >/dev/null 2>&1 || useradd --system --home-dir /var/lib/apitoken/watchdog/ci-home --create-home --shell /usr/sbin/nologin apitoken-ci
if ! id -Gn apitoken-ci | tr ' ' '\n' | grep -Fxq deploy; then
  usermod -a -G deploy apitoken-ci
fi

install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/controller
install -d -o deploy -g deploy -m 0751 /var/lib/apitoken/watchdog /var/lib/apitoken/watchdog/candidates
install -d -o deploy -g deploy -m 0750 /var/lib/apitoken/watchdog/ci-home
install -d -o deploy -g deploy -m 0750 \
  /var/lib/apitoken/watchdog/deploy-build-cache \
  /var/lib/apitoken/watchdog/deploy-build-cache/cargo \
  /var/lib/apitoken/watchdog/deploy-build-cache/xdg-cache \
  /var/lib/apitoken/watchdog/deploy-build-cache/xdg-config \
  /var/lib/apitoken/watchdog/deploy-build-cache/xdg-data
# Candidate tests need traverse-only access through these parents. State contents remain unlistable.
chmod o+x /var/lib/apitoken /var/lib/apitoken/watchdog /var/lib/apitoken/watchdog/candidates
chown apitoken-ci:apitoken-ci /var/lib/apitoken/watchdog/ci-home
install -d -o root -g root -m 0755 /opt/apitoken-watchdog
install -o root -g root -m 0755 "$ROOT/deploy/watchdog.sh" /usr/local/lib/apitoken-watchdog/watchdog.sh
install -o root -g root -m 0644 "$ROOT/deploy/watchdog-lib.sh" /usr/local/lib/apitoken-watchdog/watchdog-lib.sh
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-test-db.sh" /usr/local/lib/apitoken-watchdog/watchdog-test-db
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-backup.sh" /usr/local/lib/apitoken-watchdog/watchdog-backup.sh
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-migrate.sh" /usr/local/lib/apitoken-watchdog/watchdog-migrate.sh
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-infrastructure.sh" /usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-retention.sh" /usr/local/lib/apitoken-watchdog/watchdog-retention.sh
# The sudo policy and its validating installer are delivered like any other operational definition,
# but applying them is a deliberate operator action (deploy/install-sudoers.sh) rather than an
# automatic step: a bad policy cannot be repaired by the pipeline that the policy governs.
install -o root -g root -m 0755 "$ROOT/deploy/install-sudoers.sh" /usr/local/lib/apitoken-watchdog/install-sudoers.sh
install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/sudoers.d
install -o root -g root -m 0644 "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  /usr/local/lib/apitoken-watchdog/sudoers.d/95-apitoken-deploy
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-github.sh" /usr/local/lib/apitoken-watchdog/watchdog-github
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-control.sh" /usr/local/bin/apitoken-watchdog
install -o root -g root -m 0755 "$ROOT/deploy/collect-monitoring-metrics.sh" /usr/local/lib/apitoken-watchdog/collect-monitoring-metrics.sh
install -o root -g root -m 0755 "$ROOT/deploy/apitoken-db-dump" /usr/local/lib/apitoken-watchdog/apitoken-db-dump
install -o root -g root -m 0755 "$ROOT/deploy/deploy.sh" /usr/local/lib/apitoken-watchdog/controller/deploy.sh
install -o root -g root -m 0644 "$ROOT/deploy/lib.sh" /usr/local/lib/apitoken-watchdog/controller/lib.sh
install -o root -g root -m 0755 "$ROOT/deploy/api-bluegreen.sh" /usr/local/lib/apitoken-watchdog/controller/api-bluegreen.sh
install -o root -g root -m 0755 "$ROOT/deploy/engine-bluegreen.sh" /usr/local/lib/apitoken-watchdog/controller/engine-bluegreen.sh
# Required by the watchdog's post-admission recovery path.
install -o root -g root -m 0755 "$ROOT/deploy/rollback.sh" /usr/local/lib/apitoken-watchdog/controller/rollback.sh
install -o root -g root -m 0755 "$ROOT/deploy/sales-deploy.sh" /usr/local/lib/apitoken-watchdog/controller/sales-deploy.sh
install -o root -g root -m 0644 "$ROOT/deploy/commerce-postgres.compose.yaml" \
  /usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml
install -o root -g root -m 0644 "$ROOT/deploy/affinity-redis.compose.yaml" \
  /usr/local/lib/apitoken-watchdog/controller/affinity-redis.compose.yaml
for unit in \
  apitoken-api@.service apitoken-deploy-watchdog.service apitoken-deploy-watchdog.timer \
  apitoken-postgres.service apitoken-affinity-redis.service apitoken-worker.service apitoken-content-studio.service claude-api@.service claude-api-backup.service claude-api-backup.timer \
  claude-api-fingerprint.service claude-api-fingerprint.timer \
  apitoken-sales-api.service apitoken-sales-web.service \
  apitoken-monitoring-collector.service apitoken-monitoring-collector.timer; do
  install -o root -g root -m 0644 "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"
done

# Shared affinity is deliberately ephemeral, but its keyed identifiers and Redis password must be
# stable across engine restarts. Provision them once without printing secret values. The engine
# keeps working from local memory if this service is unavailable.
server_env=/srv/claude-api/data/server.env
install -d -o deploy -g deploy -m 0750 /srv/claude-api/data
[[ ! -L $server_env ]] || { echo "$server_env must not be a symlink" >&2; exit 1; }
if [[ ! -e $server_env ]]; then
  install -o deploy -g deploy -m 0600 /dev/null "$server_env"
fi
chown deploy:deploy "$server_env"
chmod 0600 "$server_env"
if ! grep -Eq '^CLAUDE_API_REDIS_PASSWORD=.+$' "$server_env"; then
  printf 'CLAUDE_API_REDIS_PASSWORD=%s\n' "$(openssl rand -hex 32)" >>"$server_env"
fi
if ! grep -Eq '^CLAUDE_API_AFFINITY_SECRET=.+$' "$server_env"; then
  printf 'CLAUDE_API_AFFINITY_SECRET=%s\n' "$(openssl rand -hex 32)" >>"$server_env"
fi
if ! grep -Eq '^CLAUDE_API_REDIS_URL=.+$' "$server_env"; then
  redis_password=$(sed -n 's/^CLAUDE_API_REDIS_PASSWORD=//p' "$server_env" | tail -n 1)
  [[ $redis_password =~ ^[0-9a-fA-F]{64}$ ]] \
    || { echo 'managed Redis password must be 64 hex characters' >&2; exit 1; }
  printf 'CLAUDE_API_REDIS_URL=redis://default:%s@127.0.0.1:6379/0\n' "$redis_password" >>"$server_env"
fi
install -d -o root -g root -m 0700 /var/lib/apitoken/affinity-redis
install -d -o root -g deploy -m 0775 /run/lock
for lock in apitoken-watchdog apitoken-deploy apitoken-db-migrate; do
  touch "/run/lock/$lock.lock"; chown root:deploy "/run/lock/$lock.lock"; chmod 0664 "/run/lock/$lock.lock"
done
[[ -d /opt/apitoken/repo/.git ]] || { echo 'missing /opt/apitoken/repo checkout' >&2; exit 1; }
[[ -d /opt/apitoken-watchdog/rust-toolchain/bin ]] || { echo 'install Rust in /opt/apitoken-watchdog/rust-toolchain first' >&2; exit 1; }
[[ -f /etc/apitoken/github-watchdog.env && ! -L /etc/apitoken/github-watchdog.env ]] \
  || { echo 'missing root-only /etc/apitoken/github-watchdog.env' >&2; exit 1; }
[[ $(stat -c '%u:%a' /etc/apitoken/github-watchdog.env) == 0:600 ]] \
  || { echo '/etc/apitoken/github-watchdog.env must be root-owned mode 0600' >&2; exit 1; }
[[ -d /opt/apitoken/releases/current/packages/db/migrations ]] || { echo 'current immutable commerce migration directory is missing' >&2; exit 1; }
manifest=/var/lib/apitoken/watchdog/database-migrations.manifest
if [[ ! -e $manifest ]] || [[ $(head -n 1 -- "$manifest") != 'format=apitoken-drizzle-manifest-v2' ]]; then
  # Upgrade the original whole-file manifest. The currently selected immutable commerce release
  # has already been migrated, so it is the authoritative baseline for the semantic v2 format.
  wd_migration_manifest /opt/apitoken/releases/current >"$manifest.tmp.$$"
  chown root:deploy "$manifest.tmp.$$"
  chmod 0640 "$manifest.tmp.$$"
  mv -f -- "$manifest.tmp.$$" "$manifest"
fi

if [[ ! -e /var/lib/apitoken/watchdog/processed.sha ]]; then
  processed=$(git -C /opt/apitoken/repo rev-parse HEAD)
  engine=$(basename -- "$(readlink -f /srv/claude-api/releases/current)")
  backend=$(basename -- "$(readlink -f /opt/apitoken/releases/current)")
  for sha in "$processed" "$engine" "$backend"; do
    [[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo "invalid release baseline: $sha" >&2; exit 1; }
  done
  printf '%s\n' "$processed" >/var/lib/apitoken/watchdog/processed.sha
  printf '%s\n' "$engine" >/var/lib/apitoken/watchdog/engine.sha
  printf '%s\n' "$backend" >/var/lib/apitoken/watchdog/backend.sha
  chown root:deploy /var/lib/apitoken/watchdog/{processed,engine,backend}.sha
  chmod 0640 /var/lib/apitoken/watchdog/{processed,engine,backend}.sha
fi

# Sales bounded context has its own release lifecycle; bootstrap its baseline from the live
# sales release if present, else from processed HEAD (so the next sales change triggers a deploy).
install -d -o deploy -g deploy -m 0755 /opt/apitoken/sales-releases
if [[ ! -e /var/lib/apitoken/watchdog/sales.sha ]]; then
  sales_baseline=""
  if [[ -L /opt/apitoken/sales-releases/current ]]; then
    sales_baseline=$(basename -- "$(readlink -f /opt/apitoken/sales-releases/current)")
  fi
  [[ $sales_baseline =~ ^[0-9a-f]{40}$ ]] || sales_baseline=$(git -C /opt/apitoken/repo rev-parse HEAD)
  printf '%s\n' "$sales_baseline" >/var/lib/apitoken/watchdog/sales.sha
  chown root:deploy /var/lib/apitoken/watchdog/sales.sha
  chmod 0640 /var/lib/apitoken/watchdog/sales.sha
fi
# Infrastructure delivery is fully automatic; remove markers from the retired approval workflow.
rm -f -- /var/lib/apitoken/watchdog/pending-infrastructure.sha \
  /var/lib/apitoken/watchdog/infrastructure-approved.sha
systemctl daemon-reload
"$ROOT/deploy/install-monitoring.sh"
systemctl enable apitoken-affinity-redis.service
systemctl restart apitoken-affinity-redis.service
systemctl enable --now apitoken-deploy-watchdog.timer
echo 'watchdog installed and timer enabled; verify with: sudo apitoken-watchdog status'
