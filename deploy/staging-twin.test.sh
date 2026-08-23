#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
python3 - <<'PY' "$ROOT/deploy/staging-twin-inventory.json"
import json,sys
v=json.load(open(sys.argv[1]))
assert v['schema_version']==1 and len(v['components'])==18
assert not v['external_side_effects'] and not v['public_inbound'] and v['release_keep']==3
assert set(v['excluded']) == {'content-studio','crm','suno','tripo3d','glm-plane'}
PY
bash -n "$ROOT/deploy/stage-seed.sh" "$ROOT/deploy/stage-gc.sh"
python3 - "$ROOT/deploy/stage-stub-server.py" <<'PY'
import sys
compile(open(sys.argv[1]).read(), sys.argv[1], 'exec')
PY
grep -Fq 'external_side_effect": False' "$ROOT/deploy/stage-stub-server.py"
grep -Fq 'stage_safe_sink_up 1' "$ROOT/deploy/stage-stub-server.py"
grep -Fxq 'NetworkNamespacePath=/run/netns/apitoken-stage' "$ROOT/systemd/apitoken-stage-safe-sinks.service"
grep -Fxq 'User=deploy-stage' "$ROOT/systemd/apitoken-stage-caddy.service"
grep -Fq '10.254.32.2:3900' "$ROOT/deploy/staging-Caddyfile"
! grep -Eq 'https://|tls |:443|0\.0\.0\.0' "$ROOT/deploy/staging-Caddyfile"
grep -Fq 'KEEP=3' "$ROOT/deploy/stage-gc.sh"
grep -Fq 'env: staging' "$ROOT/observability/prometheus/prometheus.yml"
grep -Fq 'sample_limit: 2000' "$ROOT/observability/prometheus/prometheus.yml"
for forbidden in content-studio crm suno tripo; do
  ! grep -Fq "$forbidden-stage.service" "$ROOT/deploy/staging-twin-inventory.json"
done
printf 'staging-twin.test: PASS\n'
