#!/usr/bin/env bash
set -euo pipefail
HELPER=/usr/local/lib/apitoken-watchdog/stage-observe-helper.sh
raw=${SSH_ORIGINAL_COMMAND:-${*:-status}}
[[ $raw != *$'\n'* && $raw != *$'\r'* ]] || { echo 'observe-stage: command rejected' >&2; exit 2; }
read -r -a words <<<"$raw"
case "${words[0]:-}" in
  status|help) ((${#words[@]} == 1)) || exit 2 ;;
  ready) ((${#words[@]} == 2)) || exit 2 ;;
  logs)
    if ((${#words[@]} == 2)); then
      :
    elif ((${#words[@]} >= 4)) && [[ ${words[2]} == --since ]]; then
      :
    else
      exit 2
    fi
    ;;
  *) echo 'observe-stage: denied' >&2; exit 2 ;;
esac
exec sudo -n "$HELPER" "$raw"
