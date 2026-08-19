#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }

TEMPLATE=${CADDY_TEMPLATE:-/opt/apitoken/repo/deploy/Caddyfile}
LIVE=${CADDY_CONFIG:-/etc/caddy/Caddyfile}
LIVE_DIR=${LIVE%/*}
PROXY_ADMIN_KEY_FILE=${PROXY_ADMIN_KEY_FILE:-/etc/apitoken/proxy-admin.key}
ROUTER_ACTIVE_SNIPPET=${ROUTER_ACTIVE_SNIPPET:-/etc/caddy/router-active.caddy}
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CHECK_ONLY=0
[[ ${1:-} != --check ]] || CHECK_ONLY=1
[[ $# -le 1 ]] || { echo "usage: $0 [--check]" >&2; exit 2; }
[[ -f "$TEMPLATE" && -f "$LIVE" ]]
[[ -f "$PROXY_ADMIN_KEY_FILE" && ! -L "$PROXY_ADMIN_KEY_FILE" ]] \
  || { echo "$PROXY_ADMIN_KEY_FILE must be a regular file" >&2; exit 1; }
[[ $(stat -c '%u:%g:%a' -- "$PROXY_ADMIN_KEY_FILE") == 0:0:600 ]] \
  || { echo "$PROXY_ADMIN_KEY_FILE must be root-owned mode 0600" >&2; exit 1; }
[[ "$ROUTER_ACTIVE_SNIPPET" == /etc/caddy/router-active.caddy ]]

router_backend_port() {
  local ports
  ports=$(sed -n 's/^[[:space:]]*reverse_proxy 127\.0\.0\.1:\([0-9][0-9]*\)[[:space:]]*$/\1/p' \
    "$ROUTER_ACTIVE_SNIPPET") || return 1
  [[ $ports != *$'\n'* ]] || return 1
  case "$ports" in 8798|8800|8801) printf '%s\n' "$ports" ;; *) return 1 ;; esac
}

ensure_router_active_snippet() {
  local port= candidate bootstrap_tmp
  if [[ -e $ROUTER_ACTIVE_SNIPPET || -L $ROUTER_ACTIVE_SNIPPET ]]; then
    [[ -f $ROUTER_ACTIVE_SNIPPET && ! -L $ROUTER_ACTIVE_SNIPPET ]] \
      || { echo 'router active-backend state is unsafe' >&2; return 1; }
    [[ $(stat -c '%u' -- "$ROUTER_ACTIVE_SNIPPET") == 0 ]] \
      || { echo 'router active-backend state must be root-owned' >&2; return 1; }
    router_backend_port >/dev/null \
      || { echo 'router active-backend state is malformed' >&2; return 1; }
    return 0
  fi

  # Bootstrap only: preserve the currently serving backend while the Caddy template learns the
  # runtime import. This file does not affect the running Caddy config until the validated reload.
  for candidate in 8798 8800 8801; do
    if curl --noproxy '*' --fail --silent --show-error --max-time 2 \
        "http://127.0.0.1:$candidate/ready" >/dev/null 2>&1; then
      port=$candidate
      break
    fi
  done
  [[ -n $port ]] || { echo 'no ready router backend exists for Caddy bootstrap' >&2; return 1; }
  bootstrap_tmp=$(mktemp /etc/caddy/.router-active.bootstrap.XXXXXX)
  if ! { printf '(router_backend) {\n\treverse_proxy 127.0.0.1:%s\n}\n' "$port" >"$bootstrap_tmp" \
      && chown root:root "$bootstrap_tmp" \
      && chmod 0644 "$bootstrap_tmp" \
      && mv -f -- "$bootstrap_tmp" "$ROUTER_ACTIVE_SNIPPET"; }; then
    rm -f -- "$bootstrap_tmp"
    return 1
  fi
  echo "initialized router active-backend state on 127.0.0.1:$port"
}

ensure_router_active_snippet

tmp=$(mktemp "$LIVE_DIR/.Caddyfile.stage2.XXXXXX")
legacy_payload=$(mktemp /etc/caddy/.admin-legacy.XXXXXX)
legacy_response=$(mktemp /etc/caddy/.admin-import-response.XXXXXX)
curl_config=$(mktemp /etc/caddy/.admin-import-curl.XXXXXX)
openai_response=$(mktemp /etc/caddy/.openai-surface.XXXXXX)
admin_session_headers=$(mktemp /etc/caddy/.admin-session-headers.XXXXXX)
admin_login_response=$(mktemp /etc/caddy/.admin-login-response.XXXXXX)
backup=
rollback_tmp=
cleanup() {
  rm -f -- "$tmp" "$legacy_payload" "$legacy_response" "$curl_config" "$openai_response" \
    "$admin_session_headers" "$admin_login_response"
  [[ -z $rollback_tmp ]] || rm -f -- "$rollback_tmp"
}
trap cleanup EXIT

# Preserve shared production-only service keys without placing any secret in argv, stdout, the
# repository, or a world-readable temporary file. AWK receives only the private raw-key path,
# renders its canonical value, and rejects a divergent value already present in live Caddy.
chmod 0600 "$tmp" "$legacy_payload" "$legacy_response" "$curl_config" "$openai_response" \
  "$admin_session_headers" "$admin_login_response"
awk -v proxy_admin_key_file="$PROXY_ADMIN_KEY_FILE" -v render_output="$tmp" \
  -f "$SCRIPT_DIR/render-caddy.awk" "$LIVE" "$TEMPLATE"

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
# Prove the staged managed-admin consumer contract through the public TLS boundary before the
# infrastructure SHA can be accepted. This is credential-free: a document must redirect without a
# Basic challenge, while the same-origin login projection must render the form directly.
if grep -q '^crm\.apitoken\.sale {' "$LIVE"; then
  admin_session_ready=0
  admin_document_status=not-run
  admin_document_location=absent
  admin_document_www=absent
  admin_login_status=not-run
  admin_login_form=absent
  # Evaluate both public surfaces on every attempt so a failure reports the exact safe predicates
  # without leaking headers, cookies or form contents.
  for _ in 1 2 3 4 5 6 7 8; do
    : >"$admin_session_headers"
    if ! admin_document_status=$(curl --noproxy '*' --silent --show-error --max-time 8 \
      --resolve crm.apitoken.sale:443:127.0.0.1 \
      -H 'accept: text/html' -D "$admin_session_headers" -o /dev/null -w '%{http_code}' \
      https://crm.apitoken.sale/); then
      admin_document_status=curl-error
    fi
    if grep -Eiq '^location: /__admin-auth/login\?return_to=%2F[[:space:]]*$' \
      "$admin_session_headers"; then
      admin_document_location=expected
    else
      admin_document_location=absent-or-unexpected
    fi
    if grep -Eiq '^www-authenticate:' "$admin_session_headers"; then
      admin_document_www=present
    else
      admin_document_www=absent
    fi

    : >"$admin_session_headers"
    : >"$admin_login_response"
    if ! admin_login_status=$(curl --noproxy '*' --silent --show-error --max-time 8 \
      --resolve crm.apitoken.sale:443:127.0.0.1 \
      -H 'accept: text/html' -D "$admin_session_headers" -o "$admin_login_response" \
      -w '%{http_code}' 'https://crm.apitoken.sale/__admin-auth/login?return_to=%2F'); then
      admin_login_status=curl-error
    fi
    if grep -Fq 'action="/__admin-auth/login"' "$admin_login_response"; then
      admin_login_form=present
    else
      admin_login_form=absent
    fi

    if [[ $admin_document_status == 303 \
        && $admin_document_location == expected \
        && $admin_document_www == absent \
        && $admin_login_status == 200 \
        && $admin_login_form == present ]]; then
      admin_session_ready=1
      break
    fi
    sleep 1
  done
  if [[ $admin_session_ready != 1 ]]; then
    if ! restore_live_config; then
      echo "managed admin session smoke failed and Caddy rollback could not be activated" >&2
      exit 1
    fi
    echo "managed admin session smoke failed: document_status=$admin_document_status;" \
      "document_location=$admin_document_location; document_www=$admin_document_www;" \
      "login_status=$admin_login_status; login_form=$admin_login_form;" \
      "restored and activated $backup" >&2
    exit 1
  fi
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
