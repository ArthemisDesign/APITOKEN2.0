#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
bash -n "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- '--sizes-mib "8,32,64,128,256" --concurrency 4' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- 'large_payload_candidate_gate.py' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- 'verdict.tmp' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- 'verdict_rc' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- '$sha.reason' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- 'payload-canary: load-driver missing' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq 'payload_canary_reason' "$ROOT/deploy/router-bluegreen.sh"
grep -Fq 'payload-canary: failed exact-SHA candidate evidence' "$ROOT/deploy/router-bluegreen.sh"
! grep -Fq 'anthropic/test' "$ROOT/tests/large_payload_mock_load.py" \
  || { echo 'payload canary still forwards a namespaced model' >&2; exit 1; }
grep -Fq '{"messages":[{"role":"user","content":"' "$ROOT/tests/large_payload_mock_load.py" \
  || { echo 'payload canary is not router-local missing-model JSON' >&2; exit 1; }
python3 "$ROOT/tests/large_payload_candidate_gate.test.py"
if "$ROOT/deploy/large-payload-candidate-gate.sh" bad http://example.com x /tmp 1 /tmp /tmp/auth 2>/dev/null; then
  echo 'candidate gate accepted an invalid authority' >&2; exit 1
fi
echo 'large-payload candidate orchestration contract passed'
