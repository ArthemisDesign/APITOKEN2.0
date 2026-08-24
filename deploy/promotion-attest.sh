#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 3 ]] || { echo 'usage: promotion-attest.sh <sha> <actor> <reason>' >&2; exit 2; }
sha=$1 actor=$2 reason=$3
[[ $sha =~ ^[0-9a-f]{40}$ ]] || exit 2
[[ $actor =~ ^[A-Za-z0-9_.-]{1,128}$ ]] || exit 2
[[ -n $reason && ${#reason} -le 512 && $reason != *$'\n'* && $reason != *$'\r'* ]] || exit 2
# The host forced command joins remaining words as the reason, so ordinary operator prose is valid.
exec ssh -o BatchMode=yes stage-ctl@84.32.48.2 -- attest "$sha" "$actor" "$reason"
