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
grep -Fq 'ExecStart=/usr/bin/caddy run' "$ROOT/systemd/apitoken-stage-caddy.service"
grep -Fq ':3900 {' "$ROOT/deploy/staging-Caddyfile"
grep -Fq 'bind 10.254.32.2' "$ROOT/deploy/staging-Caddyfile"
grep -Fq '@ready path /ready /ready/*' "$ROOT/deploy/staging-Caddyfile"
grep -Fxq 'Environment=HOME=/var/lib/apitoken-staging/caddy' "$ROOT/systemd/apitoken-stage-caddy.service"
! grep -Eq 'https://|tls |:443|0\.0\.0\.0' "$ROOT/deploy/staging-Caddyfile"
grep -Fq 'KEEP=3' "$ROOT/deploy/stage-gc.sh"
grep -Fq 'env: staging' "$ROOT/observability/prometheus/prometheus.yml"
grep -Fq 'sample_limit: 2000' "$ROOT/observability/prometheus/prometheus.yml"
for forbidden in content-studio crm suno tripo; do
  ! grep -Fq "$forbidden-stage.service" "$ROOT/deploy/staging-twin-inventory.json"
done
bash -n "$ROOT/deploy/staging-operator-env.sh" "$ROOT/deploy/install-staging-twin.sh"
grep -Fq 'Never copies production secrets' "$ROOT/deploy/staging-operator-env.sh"
grep -Fq 'Does not copy production secrets' "$ROOT/deploy/install-staging-twin.sh"
grep -Fq 'User=deploy-stage' "$ROOT/systemd/claude-api-anthropic-stage@.service"
grep -Fq 'Slice=staging.slice' "$ROOT/systemd/claude-api-anthropic-stage@.service"
grep -Fq 'NetworkNamespacePath=/run/netns/apitoken-stage' "$ROOT/systemd/apitoken-api-stage@.service"
grep -Fq 'ConditionPathExists=/etc/apitoken-staging/openai.live' "$ROOT/systemd/claude-api-openai-stage@.service"
grep -Fq 'ConditionPathExists=/etc/apitoken-staging/gemini.live' "$ROOT/systemd/claude-api-gemini-stage@.service"
grep -Fq 'ConditionPathExists=/etc/apitoken-staging/kimi.live' "$ROOT/systemd/claude-api-kimi-stage@.service"
grep -Fq ':8790 {' "$ROOT/deploy/staging-Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8787' "$ROOT/deploy/staging-Caddyfile"
grep -Fq 'ip daddr 10.254.32.2 accept' "$ROOT/deploy/install-staging-foundation.sh"
grep -Fq '127.0.0.2' "$ROOT/deploy/stage-loopback-pg.py"
grep -Fq '10.254.32.2:5433:5432' "$ROOT/deploy/staging-postgres.compose.yaml"
! grep -Fq '/etc/apitoken/api.env' "$ROOT/systemd/apitoken-api-stage@.service"
! grep -Fq '/srv/claude-api/releases' "$ROOT/systemd/claude-api-anthropic-stage@.service"
printf 'staging-twin.test: PASS\n'
