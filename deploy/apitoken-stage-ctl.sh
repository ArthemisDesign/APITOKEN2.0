#!/usr/bin/env bash
set -euo pipefail
HELPER=/usr/local/lib/apitoken-watchdog/stage-ctl-helper.sh
PROMOTION=/usr/local/lib/apitoken-watchdog/stage-promotion-helper.sh
LIVE=/usr/local/lib/apitoken-watchdog/stage-live-control.sh
raw=${SSH_ORIGINAL_COMMAND:-${*:-}}
[[ -n $raw && $raw != *$'\n'* && $raw != *$'\r'* ]] || exit 2
read -r -a words <<<"$raw"
case "${words[0]}:${#words[@]}" in
  emergency-stop:1|reseed:1) exec sudo -n "$HELPER" "${words[0]}" ;;
  attest:4) exec sudo -n "$PROMOTION" attest "${words[1]}" "${words[2]}" "${words[3]}" ;;
  sync:2) exec sudo -n "$PROMOTION" sync "${words[1]}" ;;
  live-enable:4) exec sudo -n "$LIVE" enable "${words[1]}" "${words[2]}" "${words[3]}" ;;
  live-probe:3) exec sudo -n "$LIVE" probe "${words[1]}" "${words[2]}" ;;
  live-disable:2) exec sudo -n "$LIVE" disable "${words[1]}" ;;
  *) exit 2 ;;
esac
