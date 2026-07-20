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
    # Каждая строка "user $2y$…" внутри basic_auth — отдельный админ (Q/R/M/…). Переносим ВСЕ,
    # чтобы применение шаблона не выкидывало учётки, добавленные в живой конфиг.
    # Группы учёток различаем по snippet-блоку: (crm_admins) — отдельный набор ТОЛЬКО для CRM.
    if ($0 ~ /^\(crm_admins\)/) grp = "crm"
    else if ($0 ~ /^\(/ || $0 ~ /^[a-z]/) grp = ""
    if ($1 != "header_up" && $2 ~ /^\$2/) {
      if (grp == "crm") crmauth = crmauth $0 ORS
      else auth = auth $0 ORS
    }
    if ($1 == "header_up" && $2 == "x-api-key") control = $0
    if ($1 == "header_up" && $2 == "x-admin-key") commadmin = $0
    if ($1 == "header_up" && $2 == "x-sales-admin-key") salesadmin = $0
    next
  }
  /<BASIC_AUTH_USERS_PLACEHOLDER>/ {
    if (auth == "") exit 41
    printf "%s", auth
    auth_used++
    next
  }
  /<CONTROL_KEY_PLACEHOLDER>/ {
    if (control == "") exit 42
    print control
    control_used++
    next
  }
  /<COMMERCIAL_ADMIN_KEY_PLACEHOLDER>/ {
    if (commadmin == "") exit 44
    print commadmin
    commadmin_used++
    next
  }
  /<CRM_ADMIN_USERS_PLACEHOLDER>/ {
    # Bootstrap: пока в живом конфиге нет блока (crm_admins) с учётками — рендерим запертую
    # заглушку (пароль случайный и никому не известен), чтобы применение шаблона не падало.
    if (crmauth == "") crmauth = "\t\tdisabled $2a$14$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q" ORS
    printf "%s", crmauth
    crmauth_used++
    next
  }
  /<SALES_ADMIN_KEY_PLACEHOLDER>/ {
    if (salesadmin == "") exit 45
    print salesadmin
    salesadmin_used++
    next
  }
  { print }
  END {
    if (auth_used != 1 || control_used != 1 || commadmin_used != 1 || salesadmin_used != 1 || crmauth_used != 1) exit 43
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
