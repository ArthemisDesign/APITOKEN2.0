#!/usr/bin/env bash
set -euo pipefail

# Log-only SSH login shell / ForceCommand for the observe account.
# Never exec a user shell. Never call sudo. Never mutate systemd units.

SYSTEMCTL=/usr/bin/systemctl
JOURNALCTL=/usr/bin/journalctl
CURL=/usr/bin/curl

observe_die() {
  printf 'observe: %s\n' "$*" >&2
  exit 2
}

observe_help() {
  printf '%s\n' \
    'observe: log-only host session' \
    'commands:' \
    '  status' \
    '  watchdog' \
    '  logs <unit> [--since <text>]' \
    '  help' \
    'denied: shell, sudo, systemctl start|stop|restart|kill, deploy.sh, watchdog retry|run'
}

observe_unit_allowed() {
  local unit=$1
  [[ $unit == *.service || $unit == *.timer ]] || unit=$unit.service
  case "$unit" in
    apitoken-deploy-watchdog.service|apitoken-deploy-watchdog.timer|\
    apitoken-candidate-validator.service|apitoken-candidate-validator.timer|\
    apitoken-worker.service|apitoken-content-studio.service|\
    apitoken-sales-api.service|apitoken-sales-web.service|\
    apitoken-openkeys.service|apitoken-admin.service|apitoken-devbot.service|\
    caddy.service|claude-authbot.service|\
    claude-api.service|claude-api-openai.service|claude-api-gemini.service|\
    claude-api-kimi.service|claude-router.service|\
    claude-api-backup.service|claude-api-backup.timer)
      printf '%s\n' "$unit"
      return 0
      ;;
    apitoken-api@[0-9]*.service|claude-api@[0-9]*.service|\
    claude-api-anthropic@[0-9]*.service|claude-api-openai@[0-9]*.service|\
    claude-api-gemini@[0-9]*.service|claude-api-kimi@[0-9]*.service|\
    claude-router@[0-9]*.service)
      printf '%s\n' "$unit"
      return 0
      ;;
    *) return 1 ;;
  esac
}

observe_since_allowed() {
  local since=$1
  (( ${#since} >= 1 && ${#since} <= 64 )) || return 1
  [[ $since =~ ^[0-9A-Za-z][0-9A-Za-z\ :.+-]*$ ]]
}

observe_probe() {
  local url=$1 code
  code=$("$CURL" -fsS -m 2 -o /dev/null -w '%{http_code}' -- "$url" 2>/dev/null || printf '000')
  printf '%s %s\n' "$url" "$code"
}

observe_status() {
  local unit
  printf 'observe log-only session\n'
  for unit in \
    apitoken-deploy-watchdog.service \
    claude-api-anthropic@8787.service \
    claude-api-anthropic@8788.service \
    claude-api-openai@8793.service \
    claude-api-openai@8797.service \
    claude-api-gemini@8795.service \
    claude-api-gemini@8799.service \
    claude-api-kimi@8804.service \
    claude-api-kimi@8805.service \
    claude-router@8800.service \
    claude-router@8801.service \
    apitoken-api@3000.service \
    apitoken-api@3001.service \
    apitoken-worker.service \
    caddy.service; do
    printf '%s %s\n' "$unit" "$("$SYSTEMCTL" is-active "$unit" 2>/dev/null || printf 'unknown')"
  done
  observe_probe http://127.0.0.1:8790/ready
  observe_probe http://127.0.0.1:8791/v1/ready
  observe_probe http://127.0.0.1:8792/ready
  observe_probe http://127.0.0.1:8794/ready
  observe_probe http://127.0.0.1:8803/ready
  observe_probe http://127.0.0.1:8802/ready
  if [[ -r /var/lib/apitoken/watchdog/status ]]; then
    printf 'watchdog-status '
    cat /var/lib/apitoken/watchdog/status
    printf '\n'
  fi
}

observe_watchdog() {
  if [[ -r /var/lib/apitoken/watchdog/status ]]; then
    cat /var/lib/apitoken/watchdog/status
    printf '\n'
  else
    printf 'watchdog status file is unreadable\n'
  fi
  "$JOURNALCTL" --no-pager -n 80 -u apitoken-deploy-watchdog.service \
    -u apitoken-candidate-validator.service
}

observe_logs() {
  local unit=$1 since=${2-}
  local resolved
  resolved=$(observe_unit_allowed "$unit") || observe_die "unit is not permitted: $unit"
  if [[ -n $since ]]; then
    observe_since_allowed "$since" || observe_die "invalid --since value"
    "$JOURNALCTL" --no-pager -n 200 --since "$since" -u "$resolved"
  else
    "$JOURNALCTL" --no-pager -n 200 -u "$resolved"
  fi
}

observe_dispatch() {
  local -a words
  local raw=$1 since
  [[ $raw != *$'\n'* && $raw != *$'\r'* ]] || observe_die 'command rejected'
  read -r -a words <<<"$raw"
  (( ${#words[@]} >= 1 )) || observe_die 'empty command'
  case "${words[0]}" in
    status|help|watchdog)
      (( ${#words[@]} == 1 )) || observe_die "unexpected arguments for ${words[0]}"
      ;;
  esac
  case "${words[0]}" in
    status) observe_status ;;
    help|-h|--help) observe_help ;;
    watchdog) observe_watchdog ;;
    logs)
      (( ${#words[@]} >= 2 )) || observe_die 'logs requires a unit'
      if (( ${#words[@]} == 2 )); then
        observe_logs "${words[1]}"
      elif (( ${#words[@]} >= 4 )) && [[ ${words[2]} == --since ]]; then
        since=$(IFS=' '; printf '%s' "${words[*]:3}")
        observe_logs "${words[1]}" "$since"
      else
        observe_die 'usage: logs <unit> [--since <text>]'
      fi
      ;;
    *) observe_die "denied: $raw" ;;
  esac
}

observe_command() {
  local raw
  if [[ -n ${SSH_ORIGINAL_COMMAND:-} ]]; then
    raw=$SSH_ORIGINAL_COMMAND
  elif [[ ${1:-} == -c && -n ${2:-} ]]; then
    raw=$2
  else
    raw=status
  fi
  observe_dispatch "$raw"
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  observe_command "$@"
fi
