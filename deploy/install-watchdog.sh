#!/usr/bin/env bash
set -euo pipefail

# One-time root installer for the host-local, free GitHub polling watchdog.
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'run as root' >&2; exit 1; }
INSTALL_MODE=full
REDIS_RESTART_REQUIRED=0
AUTHBOT_PROXY_ADMIN_KEY_CREATED=0
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

proxy_admin_key_file_is_valid() {
  local key_file=$1 size
  [[ -f $key_file && ! -L $key_file ]] || return 1
  size=$(wc -c <"$key_file") || return 1
  size=${size//[[:space:]]/}
  [[ $size == 64 || $size == 65 ]] || return 1
  LC_ALL=C awk '
    NR != 1 || length($0) != 64 || $0 !~ /^[0-9a-f]+$/ { exit 1 }
    END { if (NR != 1) exit 1 }
  ' "$key_file"
}

proxy_admin_key_files_equal() {
  LC_ALL=C awk '
    NR == FNR { expected = $0; next }
    $0 != expected { different = 1 }
    END { if (different || NR != 2) exit 1 }
  ' "$1" "$2"
}

provision_authbot_proxy_admin_key() {
  local key_file=${PROXY_ADMIN_KEY_FILE:-/etc/apitoken/proxy-admin.key}
  local authbot_env=${AUTHBOT_ENV:-/srv/claude-api/data/authbot.env}
  local server_env=${SERVER_ENV:-/srv/claude-api/data/server.env}
  local key_dir=${key_file%/*} authbot_dir=${authbot_env%/*}
  local key_candidate= env_candidate= legacy_candidate= legacy_rows
  command -v openssl >/dev/null || { echo 'openssl is required' >&2; return 1; }
  install -d -o root -g root -m 0755 "$key_dir"
  [[ $authbot_dir == "$key_dir" ]] || install -d -o deploy -g deploy -m 0750 "$authbot_dir"

  if [[ -e $key_file || -L $key_file ]]; then
    proxy_admin_key_file_is_valid "$key_file" \
      || { echo "$key_file must be a regular file containing one 64-lowercase-hex key" >&2; return 1; }
  fi
  if [[ -e $authbot_env || -L $authbot_env ]]; then
    [[ -f $authbot_env && ! -L $authbot_env ]] \
      || { echo "$authbot_env must be a regular file" >&2; return 1; }
    legacy_rows=$(LC_ALL=C awk '
      /^[[:space:]]*AUTH_BOT_PROXY_ADMIN_KEY[[:space:]]*=/ {
        count++
        if ($0 !~ /^AUTH_BOT_PROXY_ADMIN_KEY=[0-9a-f]+$/ ||
            length($0) != length("AUTH_BOT_PROXY_ADMIN_KEY=") + 64) bad = 1
      }
      END { if (bad || count > 1) exit 1; print count + 0 }
    ' "$authbot_env") \
      || { echo 'legacy proxy-admin key state is malformed' >&2; return 1; }
  else
    legacy_rows=0
  fi
  if [[ -e $server_env || -L $server_env ]]; then
    [[ -f $server_env && ! -L $server_env ]] \
      || { echo "$server_env must be a regular file" >&2; return 1; }
    if LC_ALL=C awk '
        /^[[:space:]]*AUTH_BOT_PROXY_ADMIN_KEY(_FILE)?[[:space:]]*=/ { found=1 }
        END { exit found ? 0 : 1 }
      ' "$server_env"; then
      echo 'server.env must not contain proxy-admin credential settings' >&2
      return 1
    fi
  fi

  if [[ $legacy_rows == 1 ]]; then
    legacy_candidate=$(mktemp "$key_dir/.proxy-admin.key.legacy.XXXXXX")
    chmod 0600 "$legacy_candidate"
    if ! LC_ALL=C awk '
        /^AUTH_BOT_PROXY_ADMIN_KEY=[0-9a-f]+$/ {
          print substr($0, length("AUTH_BOT_PROXY_ADMIN_KEY=") + 1)
        }
      ' "$authbot_env" >"$legacy_candidate" \
        || ! proxy_admin_key_file_is_valid "$legacy_candidate"; then
      rm -f -- "$legacy_candidate"
      return 1
    fi
    if [[ -e $key_file ]]; then
      if ! proxy_admin_key_files_equal "$legacy_candidate" "$key_file"; then
        rm -f -- "$legacy_candidate"
        echo 'legacy and canonical proxy-admin keys differ' >&2
        return 1
      fi
      rm -f -- "$legacy_candidate"
      legacy_candidate=
    else
      key_candidate=$legacy_candidate
      legacy_candidate=
    fi
  elif [[ ! -e $key_file ]]; then
    key_candidate=$(mktemp "$key_dir/.proxy-admin.key.XXXXXX")
    chmod 0600 "$key_candidate"
    if ! openssl rand -hex 32 >"$key_candidate" \
        || ! proxy_admin_key_file_is_valid "$key_candidate"; then
      rm -f -- "$key_candidate"
      return 1
    fi
  fi

  if [[ -n $key_candidate ]]; then
    if ! chown root:root "$key_candidate" || ! chmod 0600 "$key_candidate"; then
      rm -f -- "$key_candidate"
      return 1
    fi
    if ! ln -- "$key_candidate" "$key_file"; then
      rm -f -- "$key_candidate"
      echo 'canonical proxy-admin key appeared during provisioning' >&2
      return 1
    fi
    rm -f -- "$key_candidate"
    AUTHBOT_PROXY_ADMIN_KEY_CREATED=1
  fi
  chown root:root "$key_file"
  chmod 0600 "$key_file"

  if [[ $legacy_rows == 1 ]]; then
    env_candidate=$(mktemp "$authbot_dir/.authbot.env.XXXXXX")
    chmod 0600 "$env_candidate"
    if ! LC_ALL=C awk '
        !/^[[:space:]]*AUTH_BOT_PROXY_ADMIN_KEY[[:space:]]*=/ { print }
      ' "$authbot_env" >"$env_candidate" \
        || ! chown root:root "$env_candidate" || ! mv -- "$env_candidate" "$authbot_env"; then
      rm -f -- "$env_candidate"
      return 1
    fi
    chmod 0600 "$authbot_env"
  fi
}

# Every transaction that can start the Redis containers must provision their data directories
# first. Docker creates a missing bind-mount target as root, and redis:7.4-alpine runs as the
# image's fixed redis uid/gid (999:1000); the container would then lose write access to its own
# /data and enter MISCONF after the next persistence cycle while PING still answered. Kept out of
# activate_redis_definition because watchdog-lib.test.sh evaluates that function directly with
# stubbed systemctl/docker to prove the activation fence.
provision_redis_data_dirs() {
  # Re-applying root ownership while the container remains up makes the live process lose write
  # access to its bind-mounted /data; Redis then enters MISCONF after the next persistence cycle
  # even though PING remains healthy.
  [[ ! -L /var/lib/apitoken/affinity-redis ]] \
    || { echo '/var/lib/apitoken/affinity-redis must not be a symlink' >&2; exit 1; }
  install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis
  # Second instance for cache affinity. The historical directory name above now belongs to response
  # history, because renaming it would mean abandoning the conversations it already holds.
  [[ ! -L /var/lib/apitoken/affinity-redis-l2 ]] \
    || { echo '/var/lib/apitoken/affinity-redis-l2 must not be a symlink' >&2; exit 1; }
  install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis-l2
}

activate_redis_definition() {
  systemctl enable apitoken-affinity-redis.service
  if (( REDIS_RESTART_REQUIRED )); then
    # `systemctl restart` executes ExecStop=`docker compose down` and creates a customer-visible
    # response-history outage. Compose can reconcile the additive definition in place: the 6379
    # service identity/config remains unchanged, so only the new 6380 service is started.
    local redis_env=/srv/claude-api/data/server.env
    local redis_compose=/usr/local/lib/apitoken-watchdog/controller/affinity-redis.compose.yaml
    [[ -f $redis_env && ! -L $redis_env ]] \
      || { echo "$redis_env must be a regular file" >&2; return 1; }
    [[ -f $redis_compose && ! -L $redis_compose ]] \
      || { echo "$redis_compose must be a regular file" >&2; return 1; }
    docker compose --env-file "$redis_env" -f "$redis_compose" \
      up -d --wait --remove-orphans
  else
    echo 'Redis definitions unchanged; preserving the running affinity cache'
  fi
}

publish_fixed_helper() {
  local source=$1 target=$2 staged
  staged=${target}.tmp.$$
  install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/controller
  install -o root -g root -m 0755 "$source" "$staged"
  mv -f -- "$staged" "$target"
}

publish_authbot_runtime_helper() {
  publish_fixed_helper "$ROOT/deploy/authbot-runtime-state.sh" \
    /usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh
}

publish_pricing_retirement_postdrop_helper() {
  publish_fixed_helper "$ROOT/deploy/pricing-retirement-postdrop.sh" \
    /usr/local/lib/apitoken-watchdog/pricing-retirement-postdrop.sh
}

install_and_verify_sudo_policy() {
  local authbot_helper=/usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh
  local authbot_backup=${authbot_helper}.rollback.$$
  local postdrop_helper=/usr/local/lib/apitoken-watchdog/pricing-retirement-postdrop.sh
  local postdrop_backup=${postdrop_helper}.rollback.$$
  local had_authbot=0
  local had_postdrop=0
  if [[ -e $authbot_helper || -L $authbot_helper ]]; then
    [[ -f $authbot_helper && ! -L $authbot_helper ]] \
      || { echo "$authbot_helper must be a regular file" >&2; return 1; }
    cp -p -- "$authbot_helper" "$authbot_backup"
    had_authbot=1
  fi
  if [[ -e $postdrop_helper || -L $postdrop_helper ]]; then
    [[ -f $postdrop_helper && ! -L $postdrop_helper ]] \
      || { echo "$postdrop_helper must be a regular file" >&2; return 1; }
    cp -p -- "$postdrop_helper" "$postdrop_backup"
    had_postdrop=1
  fi
  publish_authbot_runtime_helper
  publish_pricing_retirement_postdrop_helper
  install -o root -g root -m 0755 "$ROOT/deploy/install-sudoers.sh" \
    /usr/local/lib/apitoken-watchdog/install-sudoers.sh
  install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/sudoers.d
  install -o root -g root -m 0644 "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
    /usr/local/lib/apitoken-watchdog/sudoers.d/95-apitoken-deploy
  install -o root -g root -m 0644 "$ROOT/systemd/apitoken-sudoers-install.service" \
    /etc/systemd/system/apitoken-sudoers-install.service
  systemctl daemon-reload
  if ! systemctl start apitoken-sudoers-install.service; then
    if (( had_authbot == 1 )); then mv -f -- "$authbot_backup" "$authbot_helper"; else rm -f -- "$authbot_helper"; fi
    if (( had_postdrop == 1 )); then mv -f -- "$postdrop_backup" "$postdrop_helper"; else rm -f -- "$postdrop_helper"; fi
    return 1
  fi
  rm -f -- "$authbot_backup" "$postdrop_backup"
}

install_controller_definitions() {
  local watchdog_target=/usr/local/lib/apitoken-watchdog/watchdog.sh
  local watchdog_staged=${watchdog_target}.tmp.$$
  install -d -o root -g root -m 0755 \
    /usr/local/lib/apitoken-watchdog/controller /opt/apitoken-watchdog
  install -d -o root -g root -m 0700 /var/lib/apitoken/watchdog/large-payload
  install -d -o deploy -g deploy -m 0750 /var/lib/apitoken/spool
  # The payload candidate gate executes this harness from its fixed installed root; publish it in
  # the same controller transaction so a marker-bearing release can actually run its evidence gate.
  install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/tests
  install -o root -g root -m 0644 "$ROOT/tests/large_payload_mock_load.py" \
    /usr/local/lib/apitoken-watchdog/tests/large_payload_mock_load.py
  install -o root -g root -m 0644 "$ROOT/tests/large_payload_candidate_gate.py" \
    /usr/local/lib/apitoken-watchdog/tests/large_payload_candidate_gate.py
  [[ ! -L /var/lib/apitoken/watchdog/router-proof ]] \
    || { echo 'router proof path must not be a symlink' >&2; return 1; }
  mkdir -m 0700 /var/lib/apitoken/watchdog/router-proof 2>/dev/null || true
  [[ -d /var/lib/apitoken/watchdog/router-proof && ! -L /var/lib/apitoken/watchdog/router-proof ]] \
    || { echo 'router proof path must be a real directory' >&2; return 1; }
  # The parent state root is deploy-owned, so pathname-based root chown could be redirected by a
  # rename/symlink swap between check and mutation. Resolve the directory once with cd, prove the
  # physical path, then operate only on the opened handle.
  if ! (cd /var/lib/apitoken/watchdog/router-proof \
      && [[ $(pwd -P) == /var/lib/apitoken/watchdog/router-proof ]] \
      && chown deploy:deploy . \
      && chmod 0700 . \
      && rm -f -- ./success); then
    echo 'router proof directory could not be securely provisioned' >&2
    return 1
  fi
  publish_authbot_runtime_helper
  # Install the immutable production contour and its closed validator before the entrypoint. The
  # watchdog cannot start with a partially published or schema-invalid inventory.
  install -o root -g root -m 0644 "$ROOT/deploy/contour-production.json" \
    /usr/local/lib/apitoken-watchdog/contour-production.json
  install -o root -g root -m 0644 "$ROOT/deploy/contour-stage.json" \
    /usr/local/lib/apitoken-watchdog/contour-stage.json
  install -d -o root -g deploy-stage -m 0750 /usr/local/lib/apitoken-watchdog/stage
  chown root:deploy-stage /usr/local/lib/apitoken-watchdog/stage
  chmod 0750 /usr/local/lib/apitoken-watchdog/stage
  install -o root -g deploy-stage -m 0640 "$ROOT/deploy/contour-stage.json" \
    /usr/local/lib/apitoken-watchdog/stage/contour-stage.json
  install -o root -g deploy-stage -m 0640 "$ROOT/deploy/contour-config.schema.json" \
    /usr/local/lib/apitoken-watchdog/stage/contour-config.schema.json
  install -o root -g deploy-stage -m 0750 "$ROOT/deploy/contour-config.py" \
    /usr/local/lib/apitoken-watchdog/stage/contour-config.py
  install -o root -g deploy-stage -m 0640 "$ROOT/deploy/contour-config.sh" \
    /usr/local/lib/apitoken-watchdog/stage/contour-config.sh
  for stage_runtime in stage-watchdog.sh stage-watchdog-validate.sh; do
    install -o root -g deploy-stage -m 0750 "$ROOT/deploy/$stage_runtime" \
      "/usr/local/lib/apitoken-watchdog/stage/$stage_runtime"
  done
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-github-stage.sh" \
    /usr/local/lib/apitoken-watchdog/stage/watchdog-github-stage
  install -o root -g root -m 0755 "$ROOT/deploy/stage-source-fetch.sh" \
    /usr/local/lib/apitoken-watchdog/stage-source-fetch
  install -o root -g root -m 0755 "$ROOT/deploy/stage-report-publish.sh" \
    /usr/local/lib/apitoken-watchdog/stage-report-publish
  install -o root -g root -m 0755 "$ROOT/deploy/stage-unit-renderer.py" \
    /usr/local/lib/apitoken-watchdog/stage-unit-renderer.py
  install -o root -g root -m 0755 "$ROOT/deploy/stage-stub-server.py" \
    /usr/local/lib/apitoken-watchdog/stage-stub-server.py
  install -o root -g root -m 0644 "$ROOT/deploy/staging-twin-inventory.json" \
    /usr/local/lib/apitoken-watchdog/staging-twin-inventory.json
  install -o root -g deploy-stage -m 0640 "$ROOT/deploy/staging-Caddyfile" \
    /usr/local/lib/apitoken-watchdog/staging-Caddyfile
  for stage_helper in install-staging-foundation.sh apitoken-observe-stage.sh \
    stage-observe-helper.sh apitoken-stage-ctl.sh stage-ctl-helper.sh \
    staging-isolation-live.sh staging-pressure-proof.sh staging-image-seed.sh \
    stage-store-diagnostics.sh stage-sync.sh promotion-attest.sh stage-seed.sh stage-gc.sh; do
    install -o root -g root -m 0755 "$ROOT/deploy/$stage_helper" \
      "/usr/local/lib/apitoken-watchdog/$stage_helper"
  done
  # The forced SSH shells execute /usr/local/bin, not the controller source path. Refresh wrappers
  # in every controller transaction so parser/security fixes do not depend on full reprovisioning.
  if id observe-stage >/dev/null 2>&1; then
    install -o root -g root -m 0755 "$ROOT/deploy/apitoken-observe-stage.sh" \
      /usr/local/bin/apitoken-observe-stage
  fi
  if id stage-ctl >/dev/null 2>&1; then
    install -o root -g root -m 0755 "$ROOT/deploy/apitoken-stage-ctl.sh" \
      /usr/local/bin/apitoken-stage-ctl
  fi
  # Stage manager units execute from /etc/systemd/system. Refresh their trusted definitions in
  # every controller transaction so unit-only fixes do not require a full installer replay.
  if [[ -d /etc/systemd/system ]]; then
    for stage_unit in staging.slice apitoken-rootless-docker-stage.service \
      apitoken-staging-image-seed.service apitoken-postgres-stage.service apitoken-redis-stage.service \
      apitoken-stage-watchdog.service apitoken-stage-watchdog.timer \
      apitoken-stage-safe-sinks.service apitoken-stage-caddy.service; do
      install -o root -g root -m 0644 "$ROOT/systemd/$stage_unit" "/etc/systemd/system/$stage_unit"
    done
    systemctl daemon-reload
  fi
  install -o root -g root -m 0644 "$ROOT/systemd/staging.slice" \
    /usr/local/lib/apitoken-watchdog/staging.slice
  install -o root -g root -m 0644 "$ROOT/systemd/apitoken-rootless-docker-stage.service" \
    /usr/local/lib/apitoken-watchdog/apitoken-rootless-docker-stage.service
  install -o root -g root -m 0644 "$ROOT/deploy/staging-postgres.compose.yaml" \
    /usr/local/lib/apitoken-watchdog/staging-postgres.compose.yaml
  install -o root -g root -m 0644 "$ROOT/deploy/staging-redis.compose.yaml" \
    /usr/local/lib/apitoken-watchdog/staging-redis.compose.yaml
  install -o root -g root -m 0440 "$ROOT/deploy/sudoers.d/96-apitoken-stage" \
    /usr/local/lib/apitoken-watchdog/96-apitoken-stage
  install -o root -g root -m 0644 "$ROOT/deploy/49-apitoken-stage-cgroup.rules" \
    /usr/local/lib/apitoken-watchdog/49-apitoken-stage-cgroup.rules
  install -o root -g root -m 0644 "$ROOT/deploy/stage-unit-whitelist.json" \
    /usr/local/lib/apitoken-watchdog/stage-unit-whitelist.json
  install -o root -g root -m 0644 "$ROOT/deploy/contour-config.schema.json" \
    /usr/local/lib/apitoken-watchdog/contour-config.schema.json
  install -o root -g root -m 0755 "$ROOT/deploy/contour-config.py" \
    /usr/local/lib/apitoken-watchdog/contour-config.py
  install -o root -g root -m 0644 "$ROOT/deploy/contour-config.sh" \
    /usr/local/lib/apitoken-watchdog/contour-config.sh
  python3 /usr/local/lib/apitoken-watchdog/contour-config.py \
    --schema /usr/local/lib/apitoken-watchdog/contour-config.schema.json \
    --config /usr/local/lib/apitoken-watchdog/contour-production.json --emit json >/dev/null
  python3 /usr/local/lib/apitoken-watchdog/contour-config.py \
    --schema /usr/local/lib/apitoken-watchdog/contour-config.schema.json \
    --config /usr/local/lib/apitoken-watchdog/contour-stage.json \
    --against /usr/local/lib/apitoken-watchdog/contour-production.json --emit json >/dev/null
  python3 /usr/local/lib/apitoken-watchdog/stage-unit-renderer.py \
    --contour /usr/local/lib/apitoken-watchdog/contour-stage.json \
    --whitelist /usr/local/lib/apitoken-watchdog/stage-unit-whitelist.json \
    --unit api >/dev/null
  install -o root -g root -m 0644 "$ROOT/deploy/watchdog-lib.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-lib.sh
  install -o root -g root -m 0755 "$ROOT/deploy/validation-plan.sh" \
    /usr/local/lib/apitoken-watchdog/controller/validation-plan.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-live-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-live-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-smoke-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-preflight-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-preflight-v2-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-preflight-v3-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-paid-smoke-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-paid-smoke-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-paid-smoke-v2-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-paid-smoke-v2-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-paid-smoke-v3-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-paid-smoke-v3-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-surface-probe-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-paid-inspect-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-test-db.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-test-db
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-backup.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-backup.sh
  install -o root -g root -m 0755 "$ROOT/deploy/pricing-retirement-admission.sh" \
    /usr/local/lib/apitoken-watchdog/pricing-retirement-admission.sh
  install -o root -g root -m 0644 "$ROOT/deploy/pricing-retired-schema-manifest.sh" \
    /usr/local/lib/apitoken-watchdog/pricing-retired-schema-manifest.sh
  install -o root -g root -m 0755 "$ROOT/deploy/pricing-retirement-postdrop.sh" \
    /usr/local/lib/apitoken-watchdog/pricing-retirement-postdrop.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-migrate.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-migrate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-infrastructure.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog-retention.sh" \
    /usr/local/lib/apitoken-watchdog/watchdog-retention.sh
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
  install -o root -g root -m 0755 "$ROOT/deploy/router-bluegreen.sh" \
    /usr/local/lib/apitoken-watchdog/controller/router-bluegreen.sh
  install -o root -g root -m 0755 "$ROOT/deploy/router-promote.sh" \
    /usr/local/lib/apitoken-watchdog/controller/router-promote.sh
  install -o root -g root -m 0755 "$ROOT/deploy/large-payload-headroom.sh" \
    /usr/local/lib/apitoken-watchdog/controller/large-payload-headroom.sh
  install -o root -g root -m 0755 "$ROOT/deploy/large-payload-evidence.sh" \
    /usr/local/lib/apitoken-watchdog/controller/large-payload-evidence.sh
  install -o root -g root -m 0755 "$ROOT/deploy/large-payload-candidate-gate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/large-payload-candidate-gate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/engine-migrate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh
  install -o root -g root -m 0755 "$ROOT/deploy/codex-homes-migrate.sh" \
    /usr/local/lib/apitoken-watchdog/controller/codex-homes-migrate.sh
  # Required by the watchdog's post-admission recovery path.
  install -o root -g root -m 0755 "$ROOT/deploy/rollback.sh" \
    /usr/local/lib/apitoken-watchdog/controller/rollback.sh
  install -o root -g root -m 0755 "$ROOT/deploy/sales-deploy.sh" \
    /usr/local/lib/apitoken-watchdog/controller/sales-deploy.sh
  install -o root -g root -m 0755 "$ROOT/deploy/openkeys-deploy.sh" \
    /usr/local/lib/apitoken-watchdog/controller/openkeys-deploy.sh
  install -o root -g root -m 0755 "$ROOT/deploy/admin-deploy.sh" \
    /usr/local/lib/apitoken-watchdog/controller/admin-deploy.sh
  install -o root -g root -m 0755 "$ROOT/deploy/devbot-deploy.sh" \
    /usr/local/lib/apitoken-watchdog/controller/devbot-deploy.sh
  # Both one-shot settlement diagnostics reached terminal GREEN evidence in 2026-08 and are
  # intentionally retired before their historical pricing snapshot tables leave retention.
  rm -f -- \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-settlement-diagnostic-gate.sh \
    /usr/local/lib/apitoken-watchdog/controller/gpt-image-2-settlement-v2-diagnostic-gate.sh
  # The entrypoint is the controller transaction's commit point: every dependency is present first.
  install -o root -g root -m 0755 "$ROOT/deploy/watchdog.sh" "$watchdog_staged"
  mv -f -- "$watchdog_staged" "$watchdog_target"
}

install_systemd_definitions() {
  command -v systemctl >/dev/null || { echo 'systemd is required' >&2; return 1; }
  command -v systemd-tmpfiles >/dev/null || { echo 'systemd-tmpfiles is required' >&2; return 1; }
  local restart_authbot=0 engine_current
  # Provision before installing/restarting the unit and before a later Caddy render in the same
  # infrastructure transaction. This is idempotent and never rotates an existing valid key.
  provision_authbot_proxy_admin_key
  (( AUTHBOT_PROXY_ADMIN_KEY_CREATED == 0 )) || restart_authbot=1
  # The watchdog's `ProtectSystem=full` namespace keeps /etc/tmpfiles.d read-only even after sudo.
  # Stage the exact candidate input under the fixed root-owned controller path; a manager-spawned
  # root oneshot below publishes it from a fresh namespace, just like the sudoers installer.
  install -o root -g root -m 0755 "$ROOT/deploy/install-tmpfiles.sh" \
    /usr/local/lib/apitoken-watchdog/install-tmpfiles.sh
  install -o root -g root -m 0644 "$ROOT/systemd/apitoken-tmpfiles.conf" \
    /usr/local/lib/apitoken-watchdog/apitoken-tmpfiles.conf
  # Observe useradd writes passwd, shells, and /home/observe. Those paths stay read-only
  # in this ProtectSystem=full / ProtectHome=read-only namespace even after sudo (2026-08-22
  # host journal: observe installer aborted with Read-only file system). Stage the scripts here;
  # a manager-spawned oneshot publishes the account, same as tmpfiles and sudoers.
  install -o root -g root -m 0755 "$ROOT/deploy/install-observe.sh" \
    /usr/local/lib/apitoken-watchdog/install-observe.sh
  install -o root -g root -m 0755 "$ROOT/deploy/apitoken-observe.sh" \
    /usr/local/lib/apitoken-watchdog/apitoken-observe.sh
  install -o root -g root -m 0755 "$ROOT/deploy/install-sysctl.sh" \
    /usr/local/lib/apitoken-watchdog/install-sysctl.sh
  install -o root -g root -m 0644 "$ROOT/systemd/sysctl-apitoken-redis.conf" \
    /usr/local/lib/apitoken-watchdog/sysctl-apitoken-redis.conf
  for unit in \
    apitoken-api@.service \
    apitoken-deploy-watchdog.service apitoken-deploy-watchdog.timer \
    apitoken-candidate-validator.service apitoken-candidate-validator.timer \
    apitoken-sudoers-install.service apitoken-tmpfiles-install.service \
    apitoken-sysctl-install.service apitoken-observe-install.service \
    apitoken-staging-foundation-install.service apitoken-rootless-docker-stage.service \
    apitoken-staging-image-seed.service apitoken-postgres-stage.service \
    apitoken-redis-stage.service apitoken-stage-source-fetch.service apitoken-stage-source-fetch.timer \
    apitoken-stage-report.service apitoken-stage-report.path \
    apitoken-stage-watchdog.service apitoken-stage-watchdog.timer \
    apitoken-stage-safe-sinks.service apitoken-stage-caddy.service staging.slice \
    apitoken-postgres.service apitoken-affinity-redis.service apitoken-worker.service apitoken-content-studio.service claude-api.service claude-api@.service claude-api-anthropic@.service claude-api-openai.service claude-api-openai@.service claude-api-gemini.service claude-api-gemini@.service claude-api-kimi.service claude-api-kimi@.service claude-api-backup.service claude-api-backup.timer \
    claude-api-fingerprint.service claude-api-fingerprint.timer \
    apitoken-sales-api.service apitoken-sales-web.service claude-authbot.service \
    claude-router.service claude-router@.service \
    claude-router.slice claude-api-anthropic.slice claude-api-openai.slice claude-api-gemini.slice \
    apitoken-openkeys.service \
    apitoken-admin.service \
    apitoken-devbot.service \
    apitoken-crm-api.service apitoken-crm-web.service \
    apitoken-monitoring-collector.service apitoken-monitoring-collector.timer; do
    if [[ $unit == apitoken-affinity-redis.service ]]; then
      [[ ! -L "/etc/systemd/system/$unit" ]] \
        || { echo "/etc/systemd/system/$unit must not be a symlink" >&2; return 1; }
      if ! cmp -s "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"; then
        REDIS_RESTART_REQUIRED=1
      fi
    fi
    if [[ "$unit" == claude-authbot.service \
      && -f "/etc/systemd/system/$unit" \
      && ! -L "/etc/systemd/system/$unit" ]] \
      && ! cmp -s "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"; then
      restart_authbot=1
    fi
    install -o root -g root -m 0644 "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"
  done
  systemctl daemon-reload
  if (( restart_authbot )); then
    # Unit contract changes must take effect; ordinary binary-only engine deploys still preserve
    # in-flight device authorization in deploy.sh.
    systemctl try-restart claude-authbot.service
  fi
  systemctl start apitoken-tmpfiles-install.service
  systemctl start apitoken-sysctl-install.service
  systemctl start apitoken-observe-install.service
  # A manager-spawned root oneshot applies the trusted master-sourced staging foundation outside
  # this watchdog's read-only passwd/home namespace. It installs no stage application process.
  systemctl restart apitoken-staging-foundation-install.service

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
  install -o root -g root -m 0644 "$ROOT/deploy/monitoring-authority-drift.awk" \
    /usr/local/lib/apitoken-watchdog/monitoring-authority-drift.awk
  "$ROOT/deploy/install-monitoring.sh"
}

# Narrow transactions install only the exact concern selected by the fixed root bridge. They are
# deliberately fenced before bootstrap provisioning and unrelated service restarts.
case "$INSTALL_MODE" in
  controller)
    install_and_verify_sudo_policy
    install_controller_definitions
    echo 'production watchdog controller definitions installed'
    exit 0
    ;;
  systemd)
    install_systemd_definitions
    provision_redis_data_dirs
    activate_redis_definition
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
# Publish the backward-compatible authbot helper and verify its required sudo rule before the new
# watchdog entrypoint can become visible. Policy failure restores both the old policy (inside the
# sudoers installer) and the prior helper, leaving the old watchdog compatible.
install_and_verify_sudo_policy
install_controller_definitions
install -o root -g root -m 0755 "$ROOT/deploy/apitoken-db-dump" /usr/local/lib/apitoken-watchdog/apitoken-db-dump
install -o root -g root -m 0644 "$ROOT/deploy/commerce-postgres.compose.yaml" \
  /usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml
redis_compose_target=/usr/local/lib/apitoken-watchdog/controller/affinity-redis.compose.yaml
[[ ! -L $redis_compose_target ]] \
  || { echo "$redis_compose_target must not be a symlink" >&2; exit 1; }
if ! cmp -s "$ROOT/deploy/affinity-redis.compose.yaml" "$redis_compose_target"; then
  REDIS_RESTART_REQUIRED=1
fi
install -o root -g root -m 0644 "$ROOT/deploy/affinity-redis.compose.yaml" \
  "$redis_compose_target"
install_systemd_definitions

# Shared affinity is deliberately ephemeral, but its keyed identifiers and Redis password must be
# stable across engine restarts. Provision them once without printing secret values. The engine
# keeps working from local memory if this service is unavailable.
server_env=/srv/claude-api/data/server.env
payload_canary_authorization=/srv/claude-api/data/large-payload-canary.authorization
install -d -o deploy -g deploy -m 0750 /srv/claude-api/data
install -d -o deploy -g deploy -m 0700 \
  /srv/claude-api/data/gemini /srv/claude-api/data/gemini/credentials \
  /srv/claude-api/data/kimi /srv/claude-api/data/kimi/credentials
[[ ! -L $server_env ]] || { echo "$server_env must not be a symlink" >&2; exit 1; }
if [[ ! -e $server_env ]]; then
  install -o deploy -g deploy -m 0600 /dev/null "$server_env"
fi
chown deploy:deploy "$server_env"
chmod 0600 "$server_env"
[[ ! -L $payload_canary_authorization ]] \
  || { echo "$payload_canary_authorization must not be a symlink" >&2; exit 1; }
if [[ ! -e $payload_canary_authorization ]]; then
  canary_keys=$(sed -n 's/^CLAUDE_API_KEYS=//p' "$server_env" | tail -n 1)
  canary_key=${canary_keys%%,*}
  [[ -n $canary_key && ${#canary_key} -le 480 && $canary_key != *[$'\r\n']* ]] \
    || { echo 'a bounded CLAUDE_API_KEYS entry is required for payload canary auth' >&2; exit 1; }
  umask 077
  printf 'Bearer %s\n' "$canary_key" \
    | install -o root -g root -m 0600 /dev/stdin "$payload_canary_authorization"
fi
[[ -f $payload_canary_authorization && $(stat -c '%a' -- "$payload_canary_authorization") == 600 ]] \
  || { echo "$payload_canary_authorization must be a mode-0600 regular file" >&2; exit 1; }
unset canary_keys canary_key
if ! grep -Eq '^CLAUDE_API_REDIS_PASSWORD=.+$' "$server_env"; then
  printf 'CLAUDE_API_REDIS_PASSWORD=%s\n' "$(openssl rand -hex 32)" >>"$server_env"
  REDIS_RESTART_REQUIRED=1
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
# Cache affinity has its own instance so it cannot evict Codex response history, and history cannot
# evict it. 6379 deliberately stays with history: it already holds stored conversations, and moving
# that side would strand them at cutover. Affinity is lossy by design, so it is the safe side to
# move — the cost is one TTL of reduced cross-slot prompt-cache hits.
if ! grep -Eq '^CLAUDE_API_AFFINITY_REDIS_URL=.+$' "$server_env"; then
  redis_password=$(sed -n 's/^CLAUDE_API_REDIS_PASSWORD=//p' "$server_env" | tail -n 1)
  [[ $redis_password =~ ^[0-9a-fA-F]{64}$ ]] \
    || { echo 'managed Redis password must be 64 hex characters' >&2; exit 1; }
  printf 'CLAUDE_API_AFFINITY_REDIS_URL=redis://default:%s@127.0.0.1:6380/0\n' "$redis_password" \
    >>"$server_env"
fi
# The data directories live in provision_redis_data_dirs so every transaction that can start the
# containers provisions them first — including the narrow --systemd-only path.
provision_redis_data_dirs
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
# Monitoring requires both Redis exporters to report `redis_up=1`. Reconcile the additive 6380
# service first; doing this after monitoring makes a two-instance candidate fail deterministically.
activate_redis_definition
install_monitoring_definitions
systemctl enable --now apitoken-candidate-validator.timer
systemctl enable --now apitoken-deploy-watchdog.timer
systemctl enable apitoken-stage-source-fetch.timer apitoken-stage-report.path apitoken-stage-watchdog.timer
systemctl start apitoken-stage-source-fetch.timer apitoken-stage-report.path apitoken-stage-watchdog.timer
echo 'production watchdog and parallel candidate validator installed; verify with: sudo apitoken-watchdog status'
