#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'stage-observe: root helper required' >&2; exit 1; }
[[ ${SUDO_USER:-} == observe-stage ]] || { echo 'stage-observe: caller rejected' >&2; exit 1; }
raw=${*:-status}
read -r -a words <<<"$raw"
unit_allowed() {
  local unit=$1
  [[ $unit == *.service || $unit == *.slice ]] || unit=$unit.service
  case "$unit" in
    staging.slice|apitoken-staging-foundation-install.service|apitoken-rootless-docker-stage.service|\
    apitoken-staging-image-seed.service|apitoken-postgres-stage.service|apitoken-redis-stage.service|\
    apitoken-stage-source-fetch.service|apitoken-stage-source-fetch.timer|\
    apitoken-stage-watchdog.service|apitoken-stage-watchdog.timer|apitoken-*-stage.service|\
    claude-*-stage.service|claude-*-stage@*.service) printf '%s\n' "$unit" ;;
    *) return 1 ;;
  esac
}
case "${words[0]:-}" in
  help) printf '%s\n' 'stage status|state|ready <port>|logs <stage-unit> [--since <text>]|store-logs <store>|proof <isolation|pressure>' ;;
  state)
    for name in source candidate deployed processed quarantine.dry-run; do
      value=$(cat "/var/lib/apitoken-staging/watchdog/$name.sha" 2>/dev/null || true)
      [[ $value =~ ^[0-9a-f]{40}$ ]] && printf '%s=%s\n' "$name" "$value"
    done
    ;;
  status)
    systemctl is-active staging.slice apitoken-staging-foundation-install.service \
      apitoken-rootless-docker-stage.service apitoken-staging-image-seed.service \
      apitoken-postgres-stage.service apitoken-redis-stage.service \
      apitoken-stage-source-fetch.timer apitoken-stage-watchdog.timer || true
    ip netns list | awk '$1 == "apitoken-stage" { print }'
    ;;
  ready)
    port=${words[1]:-}; [[ $port =~ ^[0-9]+$ && $port -ge 1 && $port -le 65535 ]] || exit 2
    curl -fsS -m 2 -o /dev/null -w "10.254.32.2:$port %{http_code}\n" \
      "http://10.254.32.2:$port/ready" || printf '10.254.32.2:%s 000\n' "$port"
    ;;
  logs)
    unit=$(unit_allowed "${words[1]:-}") || exit 2
    if ((${#words[@]} >= 4)) && [[ ${words[2]} == --since ]]; then
      since=$(IFS=' '; printf '%s' "${words[*]:3}")
      [[ ${#since} -le 64 && $since =~ ^[0-9A-Za-z][0-9A-Za-z\ :.+-]*$ ]] || exit 2
      journalctl --no-pager -n 200 --since "$since" -u "$unit"
    else journalctl --no-pager -n 200 -u "$unit"; fi
    ;;
  store-logs)
    ((${#words[@]} == 2)) || exit 2
    /usr/local/lib/apitoken-watchdog/stage-store-diagnostics.sh "${words[1]}"
    ;;
  proof)
    ((${#words[@]} == 2)) || exit 2
    case "${words[1]}" in
      isolation) /usr/local/lib/apitoken-watchdog/staging-isolation-live.sh ;;
      pressure) /usr/local/lib/apitoken-watchdog/staging-pressure-proof.sh ;;
      *) exit 2 ;;
    esac
    ;;
  *) exit 2 ;;
esac
