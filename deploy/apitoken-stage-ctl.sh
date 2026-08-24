#!/usr/bin/env bash
set -euo pipefail
HELPER=/usr/local/lib/apitoken-watchdog/stage-ctl-helper.sh
PROMOTION=/usr/local/lib/apitoken-watchdog/stage-promotion-helper.sh
LIVE=/usr/local/lib/apitoken-watchdog/stage-live-control.sh
raw=${SSH_ORIGINAL_COMMAND:-${*:-}}
[[ -n $raw && $raw != *$'\n'* && $raw != *$'\r'* ]] || exit 2
read -r -a words <<<"$raw"
case "${words[0]}" in
  emergency-stop|reseed)
    [[ ${#words[@]} -eq 1 ]] || exit 2
    exec sudo -n "$HELPER" "${words[0]}"
    ;;
  attest)
    # SSH_ORIGINAL_COMMAND is a single string. A reason with spaces is more than one word.
    # Keep sha and actor as tokens 2 and 3; join the rest as the reason.
    [[ ${#words[@]} -ge 4 ]] || exit 2
    sha=${words[1]}
    actor=${words[2]}
    reason=${words[*]:3}
    [[ ${#reason} -le 512 ]] || exit 2
    exec sudo -n "$PROMOTION" attest "$sha" "$actor" "$reason"
    ;;
  sync)
    [[ ${#words[@]} -eq 2 ]] || exit 2
    exec sudo -n "$PROMOTION" sync "${words[1]}"
    ;;
  live-enable)
    [[ ${#words[@]} -eq 4 ]] || exit 2
    exec sudo -n "$LIVE" enable "${words[1]}" "${words[2]}" "${words[3]}"
    ;;
  live-probe)
    [[ ${#words[@]} -eq 3 ]] || exit 2
    exec sudo -n "$LIVE" probe "${words[1]}" "${words[2]}"
    ;;
  live-disable)
    [[ ${#words[@]} -eq 2 ]] || exit 2
    exec sudo -n "$LIVE" disable "${words[1]}"
    ;;
  *) exit 2 ;;
esac
