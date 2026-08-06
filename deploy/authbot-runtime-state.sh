#!/usr/bin/env bash
set -euo pipefail

UNIT=claude-authbot.service
RELEASE_ROOT=/srv/claude-api/releases

[[ ${EUID:-$(id -u)} -eq 0 ]] || exit 1
[[ $# -eq 1 ]] || exit 2
case "$1" in
  release-sha) mode=release-sha ;;
  *)
    [[ $1 =~ ^[0-9a-f]{64}$ ]] || exit 2
    mode=digest
    expected=$1
    ;;
esac

load_state=''
if ! load_state=$(/usr/bin/systemctl show --property=LoadState --value "$UNIT" 2>/dev/null); then
  [[ $load_state == not-found ]] || exit 1
fi
if [[ $load_state == not-found ]]; then
  [[ $mode == release-sha ]] || printf '%s\n' inactive
  exit 0
fi
[[ $load_state == loaded ]] || exit 1
state=$(/usr/bin/systemctl show --property=ActiveState --value "$UNIT" 2>/dev/null) || exit 1
case "$state" in
  active) ;;
  inactive|failed)
    [[ $mode == release-sha ]] || printf '%s\n' inactive
    exit 0
    ;;
  *) exit 1 ;;
esac
pid=$(/usr/bin/systemctl show --property=MainPID --value "$UNIT" 2>/dev/null) || exit 1
[[ $pid =~ ^[1-9][0-9]*$ ]] || exit 1

if [[ $mode == release-sha ]]; then
  resolved=$(/usr/bin/readlink -f -- "/proc/$pid/exe" 2>/dev/null) || exit 1
  [[ $resolved =~ ^$RELEASE_ROOT/([0-9a-f]{40})/authbot$ ]] || exit 1
  release_sha=${BASH_REMATCH[1]}
else
  hash_line=$(/usr/bin/sha256sum -- "/proc/$pid/exe" 2>/dev/null) || exit 1
  digest=${hash_line%%[[:space:]]*}
  [[ $digest =~ ^[0-9a-f]{64}$ ]] || exit 1
fi

final_load_state=$(/usr/bin/systemctl show --property=LoadState --value "$UNIT" 2>/dev/null) || exit 1
final_state=$(/usr/bin/systemctl show --property=ActiveState --value "$UNIT" 2>/dev/null) || exit 1
final_pid=$(/usr/bin/systemctl show --property=MainPID --value "$UNIT" 2>/dev/null) || exit 1
[[ $final_load_state == loaded && $final_state == active && $final_pid == "$pid" ]] || exit 1

if [[ $mode == release-sha ]]; then
  printf '%s\n' "$release_sha"
elif [[ $digest == "$expected" ]]; then
  printf '%s\n' exact
else
  printf '%s\n' different
fi
