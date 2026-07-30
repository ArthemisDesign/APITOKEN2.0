#!/usr/bin/env bash
set -euo pipefail

# One-time root installer for the host-local, free GitHub polling watchdog.
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'run as root' >&2; exit 1; }
INSTALL_MODE=full
case "${1:-}" in
  '') ;;
  --controller-only) INSTALL_MODE=controller ;;
  --systemd-only) INSTALL_MODE=systemd ;;
  --monitoring-only) INSTALL_MODE=monitoring ;;
  *) echo "usage: $0 [--controller-only|--systemd-only|--monitoring-only]" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] \
  || { echo "usage: $0 [--controller-only|--systemd-only|--monitoring-only]" >&2; exit 2; }
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

install_controller_definitions() {
  install -d -o root -g root -m 0755 \
    /usr/local/lib/apitoken-watchdog/controller /opt/apitoken-watchdog
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog.sh
  install -o root -g root -m 0644 "$ROOT/deploy/watchdog-lib.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-lib.sh
  install -o root -g root -m 0755 "$ROOT/deploy/validation-plan.sh" \
    /usr/local/lib/apitoken-watchdog/controller/validation-plan.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-test-db.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-test-db
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-backup.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-backup.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-migrate.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-migrate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-infrastructure.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-retention.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-retention.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-codex-promote.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-codex-promote.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-github.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-github
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-control.sh" \
    /usr/local/bin/apitoken-watchdog
  install -o root -g root -m 0755 "$ROOT/deploy/deploy.sh" \
    /usr/local/lib/apitoken-watchdog/controller/deploy.sh
  install -o root -g root -m 0644 "$ROOT/deploy/lib.sh" \
    /usr/local/lib/apitoken-watchdog/controller/lib.sh
  install -o root -g root -m 0755 "$ROOT/deploy/commerce-release-bundle.sh" \
    /usr/local/lib/apitoken-watchdog/controller/commerce-release-bundle.sh
  install -o root -g root -m 0644 "$ROOT/deploy/release-tree-digest.mjs" \
    /usr/local/lib/apitoken-watchdog/controller/release-tree-digest.mjs
  install -o root -g root -m 0755 "$ROOT/deploy/content-studio-start.sh" \
    /usr/local/lib/apitoken-watchdog/controller/content-studio-start.sh
  install -o root -g root -m 0755 "$ROOT/deploy/api-bluegreen.sh" \
    /usr/local/lib/apitoken-watchdog/controller/api-bluegreen.sh
  install -o root -g root -m 0755 "$ROOT/deploy/engine-bluegreen.sh" \
    /usr/local/lib/apitoken-watchdog/controller/engine-bluegreen.sh
  install -o root -g root -m 0755 "$ROOT/deploy/engine-migrate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/codex-homes-migrate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/codex-homes-migrate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/codex-app-servers.sh" \
    /usr/local/lib/apitoken-watchdog/controller/codex-app-servers.sh
  # Required by the watchdog's post-admission recovery path.
  install -o root -g root -m 0755 "$ROOT/deploy/rollback.sh" \
    /usr/local/lib/apitoken-watchdog/controller/rollback.sh
  install -o root -g root -m 0755 "$ROOT/deploy/sales-deploy.sh" \
    /usr/local/lib/apitoken-watchdog/controller/sales-deploy.sh
  install -o root -g root -m 0755 "$ROOT/deploy/openkeys-deploy.sh" \
    /usr/local/lib/apitoken-watchdog/controller/openkeys-deploy.sh
}

install_systemd_definitions() {
  command -v systemctl >/dev/null || { echo 'systemd is required' >&2; return 1; }
  command -v systemd-tmpfiles >/dev/null || { echo 'systemd-tmpfiles is required' >&2; return 1; }
  local restart_authbot=0 engine_current
  # The watchdog's `ProtectSystem=full` namespace keeps /etc/tmpfiles.d read-only even after sudo.
  # Stage the exact candidate input under the fixed root-owned controller path; a manager-spawned
  # root oneshot below publishes it from a fresh namespace, just like the sudoers installer.
  install -o root -g root -m 0755 "$ROOT/deploy/install-tmpfiles.sh" \
    /usr/local/lib/apitoken-watchdog/install-tmpfiles.sh
  install -o root -g root -m 0644 "$ROOT/systemd/apitoken-tmpfiles.conf" \
    /usr/local/lib/apitoken-watchdog/apitoken-tmpfiles.conf
  for unit in \
    apitoken-api@.service \
    apitoken-deploy-watchdog.service apitoken-deploy-watchdog.timer \
    apitoken-candidate-validator.service apitoken-candidate-validator.timer \
    apitoken-sudoers-install.service apitoken-tmpfiles-install.service \
    apitoken-postgres.service apitoken-affinity-redis.service apitoken-worker.service apitoken-content-studio.service claude-api.service claude-api@.service claude-api-anthropic@.service claude-api-openai.service claude-api-openai@.service claude-api-codex-app-server@.service claude-api-codex-app-servers.service claude-api-codex-app-servers-ready.target claude-api-codex-app-servers.timer claude-api-gemini.service claude-api-backup.service claude-api-backup.timer \
    claude-api-fingerprint.service claude-api-fingerprint.timer \
    apitoken-sales-api.service apitoken-sales-web.service claude-authbot.service \
    apitoken-openkeys.service \
    apitoken-monitoring-collector.service apitoken-monitoring-collector.timer; do
    if [[ "$unit" == claude-authbot.service \
      && -f "/etc/systemd/system/$unit" \
      && ! -L "/etc/systemd/system/$unit" ]] \
      && ! cmp -s "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"; then
      restart_authbot=1
    fi
    install -o root -g root -m 0644 "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"
  done
  systemctl daemon-reload
  engine_current=$(readlink -f -- /srv/claude-api/releases/current 2>/dev/null || true)
  if [[ -n $engine_current \
      && -f "$engine_current/.openai-bluegreen-v1" \
      && ! -L "$engine_current/.openai-bluegreen-v1" \
      && $(<"$engine_current/.openai-bluegreen-v1") == openai-bluegreen-v1 ]]; then
    systemctl enable --now claude-api-codex-app-servers.timer
  else
    # A legacy binary ignores shared-daemon transport. Keep the timer disarmed across reboot so it
    # cannot race the singleton and create two owners of the same auth.json.
    systemctl disable --now claude-api-codex-app-servers.timer
  fi
  if (( restart_authbot )); then
    # Unit contract changes must take effect; ordinary binary-only engine deploys still preserve
    # in-flight device authorization in deploy.sh.
    systemctl try-restart claude-authbot.service
  fi
  systemctl start apitoken-tmpfiles-install.service

  # Journald storage must be an explicit decision rather than a side effect of boot ordering. Under
  # the default `Storage=auto` journald picks volatile-vs-persistent once at start by testing
  # whether /var/log/journal exists.
  local journald_dropin=/etc/systemd/journald.conf.d/10-apitoken.conf
  if install -d -o root -g root -m 0755 /etc/systemd/journald.conf.d 2>/dev/null; then
    install -d -o root -g systemd-journal -m 2755 /var/log/journal
    if ! cmp -s "$ROOT/systemd/journald-apitoken.conf" "$journald_dropin"; then
      install -o root -g root -m 0644 "$ROOT/systemd/journald-apitoken.conf" "$journald_dropin"
      systemctl restart systemd-journald
      journalctl --flush
    fi
  else
    echo 'journald drop-in skipped: /etc/systemd is read-only in this namespace;' \
      'the next infrastructure deployment will apply it' >&2
  fi
  systemctl daemon-reload
}

install_monitoring_definitions() {
  install -o root -g root -m 0755 "$ROOT/deploy/collect-monitoring-metrics.sh" \
    /usr/local/lib/apitoken-watchdog/collect-monitoring-metrics.sh
  "$ROOT/deploy/install-monitoring.sh"
}

# Narrow transactions install only the exact concern selected by the fixed root bridge. They are
# deliberately fenced before bootstrap provisioning and unrelated service restarts.
case "$INSTALL_MODE" in
  controller)
    install_controller_definitions
    echo 'production watchdog controller definitions installed'
    exit 0
    ;;
  systemd)
    install_systemd_definitions
    echo 'production systemd definitions installed'
    exit 0
    ;;
  monitoring)
    install_monitoring_definitions
    echo 'production monitoring definitions installed'
    exit 0
    ;;
esac

command -v systemctl >/dev/null || { echo 'systemd is required' >&2; exit 1; }
command -v curl >/dev/null || { echo 'curl is required' >&2; exit 1; }
command -v jq >/dev/null || { echo 'jq is required' >&2; exit 1; }
command -v openssl >/dev/null || { echo 'openssl is required' >&2; exit 1; }
id deploy >/dev/null 2>&1 || { echo 'deploy user is required' >&2; exit 1; }
id apitoken-ci >/dev/null 2>&1 || useradd --system --home-dir /var/lib/apitoken/watchdog/ci-home --create-home --shell /usr/sbin/nologin apitoken-ci
# apitoken-ci must NOT be in the deploy group. That membership let candidate-derived test code write
# deploy-group-writable tracked files in the deployment checkout, undermining the isolation the test
# gate depends on. install-sudoers.sh removes it; re-adding it here would silently undo that on the
# next infrastructure install. The CI account needs only its own home plus traverse access to the
# candidate root, both granted directly below.
if id -Gn apitoken-ci | tr ' ' '\n' | grep -Fxq deploy; then
  gpasswd -d apitoken-ci deploy >/dev/null \
    || echo 'warning: could not remove apitoken-ci from the deploy group' >&2
fi

install -d -o deploy -g deploy -m 0751 /var/lib/apitoken/watchdog /var/lib/apitoken/watchdog/candidates
install -d -o deploy -g deploy -m 0750 /var/lib/apitoken/watchdog/ci-home
install -d -o apitoken-ci -g apitoken-ci -m 0750 \
  /var/lib/apitoken/watchdog/ci-home/cargo-target \
  /var/lib/apitoken/watchdog/ci-home/cargo-target-shadow-1 \
  /var/lib/apitoken/watchdog/ci-home/cargo-target-shadow-2 \
  /var/lib/apitoken/watchdog/ci-home/next-cache
install -d -o deploy -g deploy -m 0750 \
  /var/lib/apitoken/watchdog/deploy-build-cache \
  /var/lib/apitoken/watchdog/deploy-build-cache/cargo \
  /var/lib/apitoken/watchdog/deploy-build-cache/xdg-cache \
  /var/lib/apitoken/watchdog/deploy-build-cache/xdg-config \
  /var/lib/apitoken/watchdog/deploy-build-cache/xdg-data
# Candidate tests need traverse-only access through these parents. State contents remain unlistable.
chmod o+x /var/lib/apitoken /var/lib/apitoken/watchdog /var/lib/apitoken/watchdog/candidates
chown apitoken-ci:apitoken-ci /var/lib/apitoken/watchdog/ci-home
install_controller_definitions
# The sudo policy and its validating installer are delivered together. They are applied below by a
# dedicated root oneshot after daemon-reload: this installer inherits the watchdog's read-only
# /root and /etc mount namespace even though its effective user is root, while a manager-spawned
# unit gets its own namespace and can keep rollback copies before replacing /etc/sudoers.d.
install -o root -g root -m 0755 "$ROOT/deploy/install-sudoers.sh" /usr/local/lib/apitoken-watchdog/install-sudoers.sh
install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/sudoers.d
install -o root -g root -m 0644 "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  /usr/local/lib/apitoken-watchdog/sudoers.d/95-apitoken-deploy
install -o root -g root -m 0755 "$ROOT/deploy/apitoken-db-dump" /usr/local/lib/apitoken-watchdog/apitoken-db-dump
install -o root -g root -m 0644 "$ROOT/deploy/commerce-postgres.compose.yaml" \
  /usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml
install -o root -g root -m 0644 "$ROOT/deploy/affinity-redis.compose.yaml" \
  /usr/local/lib/apitoken-watchdog/controller/affinity-redis.compose.yaml
install_systemd_definitions

# Shared affinity is deliberately ephemeral, but its keyed identifiers and Redis password must be
# stable across engine restarts. Provision them once without printing secret values. The engine
# keeps working from local memory if this service is unavailable.
server_env=/srv/claude-api/data/server.env
install -d -o deploy -g deploy -m 0750 /srv/claude-api/data
install -d -o deploy -g deploy -m 0700 \
  /srv/claude-api/data/gemini /srv/claude-api/data/gemini/credentials
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
for lock in apitoken-watchdog apitoken-candidate-validator apitoken-source-fetch \
  apitoken-deploy apitoken-db-migrate; do
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
# Deployment observability files must be readable by the monitoring collector, which runs with an
# empty CapabilityBoundingSet and therefore has no CAP_DAC_OVERRIDE to bypass a 0640 mode. They hold
# only a phase, public commit SHAs, a fixed detail string, and timestamps — no secret.
for observable in status candidate-validation-1.status candidate-validation-2.status \
  rejected.sha pending-migration.sha; do
  if [[ -f /var/lib/apitoken/watchdog/$observable ]]; then
    chmod 0644 "/var/lib/apitoken/watchdog/$observable"
  fi
done

systemctl daemon-reload
# This unit validates the candidate, saves a rollback copy, replaces the policy, verifies every
# required and forbidden privilege as `deploy`, and restores the old policy on any failure.
systemctl start apitoken-sudoers-install.service
install_monitoring_definitions
systemctl enable apitoken-affinity-redis.service
systemctl restart apitoken-affinity-redis.service
systemctl enable --now apitoken-candidate-validator.timer
systemctl enable --now apitoken-deploy-watchdog.timer
echo 'production watchdog and parallel candidate validator installed; verify with: sudo apitoken-watchdog status'
