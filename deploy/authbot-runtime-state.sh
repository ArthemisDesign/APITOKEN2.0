#!/usr/bin/env bash
set -euo pipefail

UNIT=claude-authbot.service

[[ ${EUID:-$(id -u)} -eq 0 ]] || exit 1
[[ $# -eq 1 && $1 =~ ^[0-9a-f]{64}$ ]] || exit 2
expected=$1

state=$(/usr/bin/systemctl show --property=ActiveState --value "$UNIT" 2>/dev/null) || exit 1
case "$state" in
  active) ;;
  inactive|failed)
    printf '%s\n' inactive
    exit 0
    ;;
  *) exit 1 ;;
esac
pid=$(/usr/bin/systemctl show --property=MainPID --value "$UNIT" 2>/dev/null) || exit 1
[[ $pid =~ ^[1-9][0-9]*$ ]] || exit 1

hash_line=$(/usr/bin/sha256sum -- "/proc/$pid/exe" 2>/dev/null) || exit 1
digest=${hash_line%%[[:space:]]*}
[[ $digest =~ ^[0-9a-f]{64}$ ]] || exit 1

final_state=$(/usr/bin/systemctl show --property=ActiveState --value "$UNIT" 2>/dev/null) || exit 1
final_pid=$(/usr/bin/systemctl show --property=MainPID --value "$UNIT" 2>/dev/null) || exit 1
[[ $final_state == active && $final_pid == "$pid" ]] || exit 1

if [[ $digest == "$expected" ]]; then
  printf '%s\n' exact
else
  printf '%s\n' different
fi
