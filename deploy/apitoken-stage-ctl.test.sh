#!/usr/bin/env bash
# Forced-command parsing for stage-ctl. A reason with spaces must remain one argument.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT
fail() { printf 'apitoken-stage-ctl.test: %s\n' "$*" >&2; exit 1; }

bash -n "$ROOT/deploy/apitoken-stage-ctl.sh" || fail 'deploy/apitoken-stage-ctl.sh does not parse'
grep -Fq 'reason=${words[*]:3}' "$ROOT/deploy/apitoken-stage-ctl.sh" \
  || fail 'stage-ctl must join remaining words as the attest reason'
! grep -Fq 'attest:4)' "$ROOT/deploy/apitoken-stage-ctl.sh" \
  || fail 'stage-ctl must not require exactly four attest words'

mkdir -p "$TEMP/bin"
cat >"$TEMP/bin/sudo" <<'EOF'
#!/usr/bin/env bash
[[ ${1-} == -n ]] && shift
printf '%s\n' "$*"
EOF
chmod +x "$TEMP/bin/sudo"

SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
run() {
  PATH="$TEMP/bin:$PATH" SSH_ORIGINAL_COMMAND=$1 bash "$ROOT/deploy/apitoken-stage-ctl.sh"
}

out=$(run "attest $SHA actor one-token") \
  || fail "a one-token reason was refused: ${out-}"
[[ $out == */stage-promotion-helper.sh\ attest\ "$SHA"\ actor\ one-token ]] \
  || fail "one-token reason was not forwarded: $out"

out=$(run "attest $SHA actor operator authorized alignment of agent helpers") \
  || fail "a spaced reason was refused: ${out-}"
[[ $out == */stage-promotion-helper.sh\ attest\ "$SHA"\ actor\ operator\ authorized\ alignment\ of\ agent\ helpers ]] \
  || fail "spaced reason was not joined: $out"

if run "attest $SHA actor" >/dev/null 2>&1; then
  fail 'attest without a reason was accepted'
fi
if run "attest $SHA" >/dev/null 2>&1; then
  fail 'attest without actor and reason was accepted'
fi
if run "sync $SHA extra" >/dev/null 2>&1; then
  fail 'sync with a trailing token was accepted'
fi

printf 'apitoken-stage-ctl.test: ok\n'
