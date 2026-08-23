#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd); T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
P=$ROOT/deploy/stage-degradation-policy.json; G=$ROOT/deploy/stage-degrade-gate.py; D=$(sha256sum "$P" | awk '{print $1}')
cat >"$T/green.json" <<EOF
{"timestamp":1000,"samples":100,"errors":0,"latency_p95_ms":200,"metrics":{"stage_safe_sink_up":true,"stage_probe_samples":true,"stage_probe_errors":true,"stage_probe_latency_p95_ms":true},"n_minus_one":"compatible","injected_fault":"none","policy_digest":"$D"}
EOF
python3 "$G" --policy "$P" --evidence "$T/green.json" --expected-digest "$D" --now 1010 | grep -Fq GREEN
for mode in stale missing renamed samples latency errors nminus weakened fault; do
  cp "$T/green.json" "$T/$mode.json"
  case $mode in
    stale) jq '.timestamp=900' "$T/$mode.json" >"$T/x";;
    missing) jq '.metrics.stage_probe_errors=false' "$T/$mode.json" >"$T/x";;
    renamed) jq '.metrics |= del(.stage_probe_errors)+{stage_errors:true}' "$T/$mode.json" >"$T/x";;
    samples) jq '.samples=1' "$T/$mode.json" >"$T/x";;
    latency) jq '.latency_p95_ms=1600' "$T/$mode.json" >"$T/x";;
    errors) jq '.errors=2' "$T/$mode.json" >"$T/x";;
    nminus) jq '.n_minus_one="unknown"' "$T/$mode.json" >"$T/x";;
    fault) jq '.injected_fault="escaped"' "$T/$mode.json" >"$T/x";;
    weakened) cp "$P" "$T/policy.json"; jq '.max_error_rate_bp=9999' "$T/policy.json" >"$T/x"; mv "$T/x" "$T/policy.json"; python3 "$G" --policy "$T/policy.json" --evidence "$T/$mode.json" --expected-digest "$D" --now 1010 >/dev/null 2>&1 && exit 1; continue;;
  esac
  mv "$T/x" "$T/$mode.json"
  python3 "$G" --policy "$P" --evidence "$T/$mode.json" --expected-digest "$D" --now 1010 >/dev/null 2>&1 && { echo "$mode accepted" >&2; exit 1; }
done
printf 'stage-degrade-gate.test: PASS\n'
