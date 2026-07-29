#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }

TEMPLATE=${CADDY_TEMPLATE:-/opt/apitoken/repo/deploy/Caddyfile}
LIVE=${CADDY_CONFIG:-/etc/caddy/Caddyfile}
LIVE_DIR=${LIVE%/*}
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CHECK_ONLY=0
[[ ${1:-} != --check ]] || CHECK_ONLY=1
[[ $# -le 1 ]] || { echo "usage: $0 [--check]" >&2; exit 2; }
[[ -f "$TEMPLATE" && -f "$LIVE" ]]

tmp=$(mktemp "$LIVE_DIR/.Caddyfile.stage2.XXXXXX")
legacy_payload=$(mktemp /etc/caddy/.admin-legacy.XXXXXX)
legacy_response=$(mktemp /etc/caddy/.admin-import-response.XXXXXX)
curl_config=$(mktemp /etc/caddy/.admin-import-curl.XXXXXX)
openai_response=$(mktemp /etc/caddy/.openai-surface.XXXXXX)
backup=
rollback_tmp=
cleanup() {
  rm -f -- "$tmp" "$legacy_payload" "$legacy_response" "$curl_config" "$openai_response"
  [[ -z $rollback_tmp ]] || rm -f -- "$rollback_tmp"
}
trap cleanup EXIT

# Preserve production-only service keys without placing any secret in argv, stdout, the repository,
# or a world-readable temporary file. Human admin credentials live in PostgreSQL after cutover.
chmod 0600 "$tmp" "$legacy_payload" "$legacy_response" "$curl_config" "$openai_response"
awk -f "$SCRIPT_DIR/render-caddy.awk" "$LIVE" "$TEMPLATE" >"$tmp"

! grep -q '<[A-Z_]*PLACEHOLDER>' "$tmp"
caddy validate --adapter caddyfile --config "$tmp" >/dev/null
if [[ $CHECK_ONLY -eq 1 ]]; then
  echo "rendered Caddy configuration is valid"
  exit 0
fi

# On the one-time cutover, import the live panel and CRM bcrypt rows before removing basic_auth.
# Any malformed/missing group or API failure aborts while the old Caddy configuration is untouched.
if grep -Eq '^\((panel_admins|crm_admins)\)' "$LIVE"; then
  awk -f "$SCRIPT_DIR/export-legacy-admins.awk" "$LIVE" >"$legacy_payload"
  admin_key=$(awk '
    $1 == "header_up" && $2 == "x-admin-key" {
      value = $3
      sub(/^"/, "", value)
      sub(/"$/, "", value)
      if (found && value != key) exit 42
      key = value
      found = 1
    }
    END { if (!found || length(key) < 32) exit 43; print key }
  ' "$LIVE")
  {
    printf 'url = "http://127.0.0.1:8791/v1/internal/admin-auth/legacy-import"\n'
    printf 'request = "POST"\n'
    printf 'header = "content-type: application/json"\n'
    printf 'header = "x-admin-key: %s"\n' "$admin_key"
    printf 'data = "@%s"\n' "$legacy_payload"
    printf 'output = "%s"\n' "$legacy_response"
    printf 'fail-with-body\n'
    printf 'silent\nshow-error\nmax-time = 15\n'
  } >"$curl_config"
  curl --config "$curl_config"
  node -e '
    const fs = require("node:fs");
    const value = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    if (!Number.isInteger(value.main_admin_accounts) || value.main_admin_accounts < 1 ||
        !Number.isInteger(value.crm_accounts) || value.crm_accounts < 1) process.exit(1);
  ' "$legacy_response"
  unset admin_key
  echo "legacy panel and CRM administrators imported into the managed account store"
fi

backup="$LIVE.pre-stage2.$(date -u +%Y%m%dT%H%M%SZ)"
cp -a "$LIVE" "$backup"
chown --reference="$backup" "$tmp"
chmod --reference="$backup" "$tmp"
mv -f -- "$tmp" "$LIVE"

restore_live_config() {
  rollback_tmp=$(mktemp "$LIVE_DIR/.Caddyfile.rollback.XXXXXX") || return 1
  if ! cp -a -- "$backup" "$rollback_tmp"; then
    echo "Caddy rollback copy failed; live candidate remains active" >&2
    return 1
  fi
  if ! mv -f -- "$rollback_tmp" "$LIVE"; then
    echo "Caddy rollback publication failed; live candidate remains active" >&2
    return 1
  fi
  rollback_tmp=
  if caddy reload --adapter caddyfile --config "$LIVE"; then
    return 0
  fi
  echo "Caddy rollback reload failed; restarting from the restored configuration" >&2
  if systemctl restart caddy; then
    return 0
  fi
  echo "CRITICAL: Caddy rejected both rollback reload and restart" >&2
  return 1
}

if ! caddy reload --adapter caddyfile --config "$LIVE"; then
  if ! restore_live_config; then
    echo "Caddy candidate reload failed and rollback could not be activated" >&2
    exit 1
  fi
  echo "Caddy reload failed; restored and activated $backup" >&2
  exit 1
fi
# Syntax-valid routing can still point the public OpenAI hostname at the wrong provider. Exercise a
# quota-free unauthenticated request through loopback TLS before committing the infrastructure SHA.
if grep -q '^openai\.api\.apitoken\.sale {' "$LIVE"; then
  openai_ready=0
  for _ in 1 2 3 4 5 6 7 8; do
    if openai_status=$(curl --noproxy '*' --silent --show-error --max-time 8 \
        --resolve openai.api.apitoken.sale:443:127.0.0.1 \
        -H 'content-type: application/json' \
        -d '{}' \
        -o "$openai_response" -w '%{http_code}' https://openai.api.apitoken.sale/v1/responses) \
        && node -e '
          const value = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8"));
          const error = value && value.error;
          const status = Number(process.argv[2]);
          if (!error || error.type !== "invalid_request_error" || !(
            (status === 400 && error.param === "model") ||
            (status === 401 && error.code === "invalid_api_key") ||
            (status === 404 && error.code === "model_not_found")
          )) process.exit(1);
        ' "$openai_response" "$openai_status"; then
      openai_ready=1
      break
    fi
    sleep 1
  done
  if [[ $openai_ready != 1 ]]; then
    if ! restore_live_config; then
      echo "OpenAI hostname smoke failed and Caddy rollback could not be activated" >&2
      exit 1
    fi
    echo "OpenAI hostname smoke failed; restored and activated $backup" >&2
    exit 1
  fi
fi
echo "Caddy configuration installed; rollback copy: $backup"
