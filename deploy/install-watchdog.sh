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
id deploy >/dev/null 2>&1 || { echo 'deploy user is required' >&2; exit 1; }
id apitoken-ci >/dev/null 2>&1 || useradd --system --home-dir /var/lib/apitoken/watchdog/ci-home --create-home --shell /usr/sbin/nologin apitoken-ci
if ! id -Gn apitoken-ci | tr ' ' '\n' | grep -Fxq deploy; then
  usermod -a -G deploy apitoken-ci
fi

install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/controller
install -d -o deploy -g deploy -m 0751 /var/lib/apitoken/watchdog /var/lib/apitoken/watchdog/candidates
install -d -o deploy -g deploy -m 0750 /var/lib/apitoken/watchdog/ci-home
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
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-github.sh" /usr/local/lib/apitoken-watchdog/watchdog-github
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-control.sh" /usr/local/bin/apitoken-watchdog
install -o root -g root -m 0755 "$ROOT/deploy/deploy.sh" /usr/local/lib/apitoken-watchdog/controller/deploy.sh
install -o root -g root -m 0644 "$ROOT/deploy/lib.sh" /usr/local/lib/apitoken-watchdog/controller/lib.sh
install -o root -g root -m 0755 "$ROOT/deploy/api-bluegreen.sh" /usr/local/lib/apitoken-watchdog/controller/api-bluegreen.sh
install -o root -g root -m 0755 "$ROOT/deploy/engine-bluegreen.sh" /usr/local/lib/apitoken-watchdog/controller/engine-bluegreen.sh
install -o root -g root -m 0644 "$ROOT/deploy/commerce-postgres.compose.yaml" \
  /usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml
for unit in \
  apitoken-api@.service apitoken-deploy-watchdog.service apitoken-deploy-watchdog.timer \
  apitoken-worker.service claude-api@.service claude-api-backup.service claude-api-backup.timer \
  claude-api-fingerprint.service claude-api-fingerprint.timer; do
  install -o root -g root -m 0644 "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"
done
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
# Infrastructure delivery is fully automatic; remove markers from the retired approval workflow.
rm -f -- /var/lib/apitoken/watchdog/pending-infrastructure.sha \
  /var/lib/apitoken/watchdog/infrastructure-approved.sha
systemctl daemon-reload
systemctl enable --now apitoken-deploy-watchdog.timer
echo 'watchdog installed and timer enabled; verify with: sudo apitoken-watchdog status'
