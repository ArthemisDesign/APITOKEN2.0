#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }

TEMPLATE=${CADDY_TEMPLATE:-/opt/apitoken/repo/deploy/Caddyfile}
LIVE=${CADDY_CONFIG:-/etc/caddy/Caddyfile}
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CHECK_ONLY=0
[[ ${1:-} != --check ]] || CHECK_ONLY=1
[[ $# -le 1 ]] || { echo "usage: $0 [--check]" >&2; exit 2; }
[[ -f "$TEMPLATE" && -f "$LIVE" ]]

tmp=$(mktemp /etc/caddy/.Caddyfile.stage2.XXXXXX)
backup=
cleanup() { rm -f "$tmp"; }
trap cleanup EXIT

# Preserve the production-only secret classes without placing any secret in argv, stdout, the
# repository, or a world-readable temporary file. The unified admin surfaces reuse the existing
# panel_admins snippet while CRM keeps its separate credential group.
chmod 0600 "$tmp"
awk -f "$SCRIPT_DIR/render-caddy.awk" "$LIVE" "$TEMPLATE" >"$tmp"

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
