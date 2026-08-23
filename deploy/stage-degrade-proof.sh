#!/usr/bin/env bash
set -euo pipefail
POLICY=/usr/local/lib/apitoken-watchdog/stage-degradation-policy.json
GATE=/usr/local/lib/apitoken-watchdog/stage-degrade-gate.py
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
digest=$(sha256sum "$POLICY" | awk '{print $1}')
metrics=$(ip netns exec apitoken-stage runuser -u deploy-stage -- \
  /usr/local/lib/apitoken-watchdog/stage-load-generator.py)
now=$(date +%s)
jq -cn --argjson m "$metrics" --arg d "$digest" --argjson now "$now" \
  '{timestamp:$now,samples:$m.samples,errors:$m.errors,latency_p95_ms:$m.latency_p95_ms,metrics:{stage_safe_sink_up:true,stage_probe_samples:true,stage_probe_errors:true,stage_probe_latency_p95_ms:true},n_minus_one:"compatible",injected_fault:"none",policy_digest:$d}' >"$T/green.json"
python3 "$GATE" --policy "$POLICY" --evidence "$T/green.json" --expected-digest "$digest" --now "$now"
jq '.latency_p95_ms=99999,.injected_fault="caught"' "$T/green.json" >"$T/red.json"
if python3 "$GATE" --policy "$POLICY" --evidence "$T/red.json" --expected-digest "$digest" --now "$now" >/dev/null 2>&1; then
  echo 'stage-degrade-proof: injected regression escaped' >&2; exit 1
fi
printf 'stage-degrade-proof: injected regression caught\n'
