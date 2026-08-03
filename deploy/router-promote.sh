#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { printf 'router promotion must run as root\n' >&2; exit 1; }
[[ $# -eq 1 ]] || { printf 'usage: router-promote.sh 8798|8800|8801\n' >&2; exit 2; }

TARGET_PORT=$1
case "$TARGET_PORT" in
  8798|8800|8801) ;;
  *) printf 'invalid router backend port: %s\n' "$TARGET_PORT" >&2; exit 2 ;;
esac

LIVE=${CADDY_CONFIG:-/etc/caddy/Caddyfile}
SNIPPET=${ROUTER_ACTIVE_SNIPPET:-/etc/caddy/router-active.caddy}
STABLE_READY_URL=${ROUTER_STABLE_READY_URL:-http://127.0.0.1:8802/ready}
TARGET_READY_URL="http://127.0.0.1:$TARGET_PORT/ready"
[[ $LIVE == /etc/caddy/Caddyfile ]] || { printf 'Caddy config path is fixed\n' >&2; exit 2; }
[[ $SNIPPET == /etc/caddy/router-active.caddy ]] || { printf 'router state path is fixed\n' >&2; exit 2; }
[[ -f $LIVE && ! -L $LIVE ]] || { printf 'live Caddy config is missing or unsafe\n' >&2; exit 1; }
[[ -f $SNIPPET && ! -L $SNIPPET ]] || { printf 'router backend state is missing or unsafe\n' >&2; exit 1; }
[[ $(stat -c '%u' -- "$SNIPPET") == 0 ]] || { printf 'router backend state is not root-owned\n' >&2; exit 1; }
caddy validate --adapter caddyfile --config "$LIVE" >/dev/null \
  || { printf 'current Caddy configuration is invalid; refusing router mutation\n' >&2; exit 1; }
curl --noproxy '*' --fail --silent --show-error --max-time 3 "$TARGET_READY_URL" >/dev/null \
  || { printf 'candidate router is not ready at %s\n' "$TARGET_READY_URL" >&2; exit 1; }

directory=${SNIPPET%/*}
candidate=$(mktemp "$directory/.router-active.candidate.XXXXXX")
backup=$(mktemp "$directory/.router-active.rollback.XXXXXX")
cleanup() { rm -f -- "$candidate" "$backup"; }
trap cleanup EXIT
chmod 0644 "$candidate" "$backup"
cp -a -- "$SNIPPET" "$backup"
printf '(router_backend) {\n\treverse_proxy 127.0.0.1:%s\n}\n' "$TARGET_PORT" >"$candidate"
chown root:root "$candidate"

restore() {
  local rollback_tmp
  rollback_tmp=$(mktemp "$directory/.router-active.restore.XXXXXX") || return 1
  if ! cp -a -- "$backup" "$rollback_tmp" || ! mv -f -- "$rollback_tmp" "$SNIPPET"; then
    rm -f -- "$rollback_tmp"
    printf 'CRITICAL: could not restore router backend state\n' >&2
    return 1
  fi
  if caddy validate --adapter caddyfile --config "$LIVE" >/dev/null \
      && caddy reload --adapter caddyfile --config "$LIVE"; then
    return 0
  fi
  printf 'CRITICAL: restored router backend state but Caddy rejected rollback reload\n' >&2
  return 1
}

# Same-directory rename is the state commit. Caddy keeps the old config and its established
# streams until the validated reload succeeds; a rejected candidate is restored and reloaded.
mv -f -- "$candidate" "$SNIPPET"
if ! caddy validate --adapter caddyfile --config "$LIVE" >/dev/null \
    || ! caddy reload --adapter caddyfile --config "$LIVE"; then
  restore || true
  printf 'router backend promotion was rejected; previous backend restored\n' >&2
  exit 1
fi

for _ in $(seq 1 15); do
  if curl --noproxy '*' --fail --silent --show-error --max-time 2 "$STABLE_READY_URL" >/dev/null; then
    printf 'router backend promoted to 127.0.0.1:%s\n' "$TARGET_PORT"
    exit 0
  fi
  sleep 1
done

restore || true
printf 'stable router origin did not converge after promotion; previous backend restored\n' >&2
exit 1
