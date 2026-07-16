#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }

TEMPLATE=${CADDY_TEMPLATE:-/opt/apitoken/repo/deploy/Caddyfile}
LIVE=${CADDY_CONFIG:-/etc/caddy/Caddyfile}
CHECK_ONLY=0
[[ ${1:-} != --check ]] || CHECK_ONLY=1
[[ $# -le 1 ]] || { echo "usage: $0 [--check]" >&2; exit 2; }
[[ -f "$TEMPLATE" && -f "$LIVE" ]]

tmp=$(mktemp /etc/caddy/.Caddyfile.stage2.XXXXXX)
backup=
cleanup() { rm -f "$tmp"; }
trap cleanup EXIT

# Preserve the two production-only secret-bearing lines without placing either secret in argv,
# stdout, the repository, or a world-readable temporary file.
chmod 0600 "$tmp"
awk '
  NR == FNR {
    if ($1 == "admin" && $2 ~ /^\$2/) auth = $0
    if ($1 == "header_up" && $2 == "x-api-key") control = $0
    next
  }
  /<BCRYPT_HASH_PLACEHOLDER>/ {
    if (auth == "") exit 41
    print auth
    auth_used++
    next
  }
  /<CONTROL_KEY_PLACEHOLDER>/ {
    if (control == "") exit 42
    print control
    control_used++
    next
  }
  { print }
  END {
    if (auth_used != 1 || control_used != 1) exit 43
  }
' "$LIVE" "$TEMPLATE" >"$tmp"

! grep -q '<[A-Z_]*PLACEHOLDER>' "$tmp"
caddy validate --adapter caddyfile --config "$tmp" >/dev/null
if [[ $CHECK_ONLY -eq 1 ]]; then
  echo "rendered Stage-2 Caddy configuration is valid"
  exit 0
fi

backup="$LIVE.pre-stage2.$(date -u +%Y%m%dT%H%M%SZ)"
cp -a "$LIVE" "$backup"
cp "$tmp" "$LIVE"
chown --reference="$backup" "$LIVE"
chmod --reference="$backup" "$LIVE"
if ! caddy reload --adapter caddyfile --config "$LIVE"; then
  cp -a "$backup" "$LIVE"
  caddy reload --adapter caddyfile --config "$LIVE" || true
  echo "Caddy reload failed; restored $backup" >&2
  exit 1
fi
echo "Caddy health-gated engine slots installed; rollback copy: $backup"
