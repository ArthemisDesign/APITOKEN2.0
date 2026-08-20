#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
bash -n "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- '--sizes-mib "8,32,64" --concurrency 4' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- 'large_payload_candidate_gate.py' "$ROOT/deploy/large-payload-candidate-gate.sh"
grep -Fq -- 'verdict.tmp' "$ROOT/deploy/large-payload-candidate-gate.sh"
if "$ROOT/deploy/large-payload-candidate-gate.sh" bad http://example.com x /tmp 1 /tmp 2>/dev/null; then
  echo 'candidate gate accepted an invalid authority' >&2; exit 1
fi
echo 'large-payload candidate orchestration contract passed'
