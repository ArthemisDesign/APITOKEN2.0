#!/usr/bin/env bash
# The engine's own shutdown budget must always fit inside systemd's stop timeout.
#
# When it did not, systemd killed the process in the middle of its correct shutdown: the legacy
# OpenAI unit carried the 90-second default while the engine was draining for up to 620 seconds, so
# `State 'stop-sigterm' timed out. Killing.` landed mid-drain on 2026-07-30 and 15 reservations were
# abandoned in `delivering`, then charged the full preflight hold — $51.25 in one morning.
#
# The ladder is currently correct, but nothing kept it that way: it is three numbers in two
# unrelated files, and the failure is silent and expensive. This gate pins the ordering.
set -eEuo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
CONFIG="$ROOT/crates/server/src/config.rs"

fail() {
  printf 'shutdown-ladder: %s\n' "$*" >&2
  exit 1
}

# The engine clamps both budgets, so the worst case an operator can configure is the upper bound of
# the clamp — not the default. A gate that trusted the defaults would pass while an env override
# quietly pushed the process past systemd's patience.
bounded_max() {
  local name=$1 line
  line=$(grep -F "bounded_u64(\"$name\"" "$CONFIG" | head -n1) \
    || fail "no bounded_u64 clamp found for $name"
  printf '%s\n' "$line" | sed -n 's/.*bounded_u64([^,]*,[^,]*,[^,]*,[[:space:]]*\([0-9]\{1,\}\).*/\1/p'
}

READINESS_MAX=$(bounded_max CLAUDE_API_READINESS_DELAY_SECS)
DRAIN_MAX=$(bounded_max CLAUDE_API_DRAIN_DEADLINE_SECS)
OPENAI_FLOOR=$(sed -n 's/.*OPENAI_SHARED_DRAIN_DEADLINE_SECS:[[:space:]]*u64[[:space:]]*=[[:space:]]*\([0-9]\{1,\}\).*/\1/p' "$CONFIG" | head -n1)

[[ -n $READINESS_MAX && -n $DRAIN_MAX && -n $OPENAI_FLOOR ]] \
  || fail "could not read the shutdown budget from crates/server/src/config.rs"

# The OpenAI plane raises the drain deadline to its own floor, so the worst drain in the fleet is
# whichever of the two is larger.
WORST_DRAIN=$DRAIN_MAX
((OPENAI_FLOOR > WORST_DRAIN)) && WORST_DRAIN=$OPENAI_FLOOR
WORST_INTERNAL=$((READINESS_MAX + WORST_DRAIN))

# Settlement barriers and the mandatory billing flush run after the drain and are deliberately not
# bounded by it — losing a settlement to a local timeout would be worse than a slow stop. They need
# headroom above the drain, and this is the floor for it.
REQUIRED_MARGIN=${SHUTDOWN_LADDER_MARGIN_SECS:-30}
REQUIRED=$((WORST_INTERNAL + REQUIRED_MARGIN))

checked=0
for unit in "$ROOT"/systemd/claude-api*.service; do
  [[ -e $unit ]] || continue
  # Only the units that actually serve traffic run the drain; timers and one-shots do not.
  grep -Fq ' serve' "$unit" || continue
  name=${unit##*/}
  timeout=$(sed -n 's/^TimeoutStopSec=\([0-9]\{1,\}\).*/\1/p' "$unit" | head -n1)
  [[ -n $timeout ]] \
    || fail "$name serves traffic but sets no TimeoutStopSec; systemd would kill it at the 90s default, mid-drain"
  ((timeout >= REQUIRED)) \
    || fail "$name has TimeoutStopSec=$timeout, below the ${REQUIRED}s the engine can spend shutting down (${READINESS_MAX}s readiness + ${WORST_DRAIN}s drain + ${REQUIRED_MARGIN}s for settlement barriers and the billing flush)"
  checked=$((checked + 1))
done

((checked > 0)) || fail "no serving engine units were checked; the glob or the unit layout changed"

printf 'deploy/shutdown-ladder.test.sh: %d serving units clear the %ds shutdown budget\n' \
  "$checked" "$REQUIRED"
