#!/usr/bin/env bash
# Run the phase-6.4c canary on the production host with the exact deployed router binary.
# The live API key is streamed over SSH stdin and then over curl config stdin. It is never placed in
# argv, a temporary file, a child-process environment, or output.
set -euo pipefail

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
TARGET=${APITOKEN_CANARY_SSH_TARGET:-deploy@84.32.48.2}
: "${APITOKEN_API_KEY:?APITOKEN_API_KEY must already be set in the caller environment}"
: "${APITOKEN_CANARY_EXPECTED_SHA:?set APITOKEN_CANARY_EXPECTED_SHA to the landed GREEN SHA}"
[[ $APITOKEN_CANARY_EXPECTED_SHA =~ ^[0-9a-f]{40}$ ]] || {
  printf 'APITOKEN_CANARY_EXPECTED_SHA must be a full lowercase commit SHA\n' >&2
  exit 2
}
case $APITOKEN_API_KEY in
  *$'\n'*|*$'\r'*|*'"'*|*'\'*)
    printf 'APITOKEN_API_KEY contains a character unsafe for curl config stdin\n' >&2
    exit 2
    ;;
esac
CANARY_KEY=$APITOKEN_API_KEY
EXPECTED_SHA=$APITOKEN_CANARY_EXPECTED_SHA
export -n CANARY_KEY EXPECTED_SHA 2>/dev/null || true
unset APITOKEN_API_KEY APITOKEN_CANARY_EXPECTED_SHA

{
  printf '%s\n%s\n' "$CANARY_KEY" "$EXPECTED_SHA"
  while IFS= read -r line || [[ -n $line ]]; do
    printf '%s\n' "$line"
  done <"$HERE/router_fallback_live_canary_remote.sh"
} | env -u APITOKEN_API_KEY -u APITOKEN_CANARY_EXPECTED_SHA \
  ssh -T -o BatchMode=yes -o LogLevel=ERROR "$TARGET" \
  "exec bash -c 'IFS= read -r APITOKEN_API_KEY || exit 64; IFS= read -r APITOKEN_CANARY_EXPECTED_SHA || exit 64; export -n APITOKEN_API_KEY APITOKEN_CANARY_EXPECTED_SHA 2>/dev/null || true; source /dev/stdin'"
CANARY_KEY=
unset CANARY_KEY
