#!/usr/bin/env bash
set -euo pipefail
HELPER=/usr/local/lib/apitoken-watchdog/stage-ctl-helper.sh
raw=${SSH_ORIGINAL_COMMAND:-${*:-}}
[[ -n $raw && $raw != *$'\n'* && $raw != *$'\r'* ]] || exit 2
read -r -a words <<<"$raw"
((${#words[@]} == 1)) || exit 2
case "${words[0]}" in attest|sync|emergency-stop|reseed) ;; *) exit 2 ;; esac
exec sudo -n "$HELPER" "${words[0]}"
