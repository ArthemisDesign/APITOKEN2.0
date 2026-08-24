#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
bash -n "$ROOT/deploy/stage-live-control.sh"
python3 - "$ROOT/deploy/stage-live-proxy.py" "$ROOT/deploy/stage-live-host-proxy.py" <<'PY'
import sys
for p in sys.argv[1:]: compile(open(p).read(),p,'exec')
PY
grep -Fq 'MAX_TOKENS = 64' "$ROOT/deploy/stage-live-proxy.py"
grep -Fq 'MAX_BODY = 1_048_576' "$ROOT/deploy/stage-live-proxy.py"
grep -Fq 'UPSTREAM_HOST = "10.254.32.1"' "$ROOT/deploy/stage-live-proxy.py"
grep -Fq "conn.request('POST','/v1/messages'" "$ROOT/deploy/stage-live-host-proxy.py"
! grep -Eq 'api\.anthropic\.com|api\.openai\.com|generativelanguage\.googleapis\.com' "$ROOT/deploy/stage-live-proxy.py" "$ROOT/deploy/stage-live-host-proxy.py"
grep -Fq 'cap >= 100000 && cap <= 100000000' "$ROOT/deploy/stage-live-control.sh"
grep -Fq 'spend_limit_nano' "$ROOT/deploy/stage-live-control.sh"
grep -Fq 'source /srv/claude-api/data/server.env' "$ROOT/deploy/stage-live-control.sh"
grep -Fq 'expires_ts' "$ROOT/deploy/stage-live-control.sh"
grep -Fq 'iptables -I INPUT 1 -i veth-stage-host -s 10.254.32.2 -d 10.254.32.1 -p tcp --dport 9081 -j ACCEPT' "$ROOT/deploy/stage-live-control.sh"
grep -Fq 'iptables -D INPUT -i veth-stage-host -s 10.254.32.2 -d 10.254.32.1 -p tcp --dport 9081 -j ACCEPT' "$ROOT/deploy/stage-live-control.sh"
grep -Fq 'probe already consumed' "$ROOT/deploy/stage-live-control.sh"
grep -Fq 'body=$(mktemp -p /var/lib/apitoken-staging/watchdog); chown deploy-stage:deploy-stage "$body"; chmod 0600 "$body"' "$ROOT/deploy/stage-live-control.sh"
grep -Fq "rm -f -- /etc/apitoken-staging/stage-live.enabled" "$ROOT/deploy/stage-live-control.sh"
grep -Fxq 'NetworkNamespacePath=/run/netns/apitoken-stage' "$ROOT/systemd/apitoken-stage-live-client.service"
grep -Fxq 'User=nobody' "$ROOT/systemd/apitoken-stage-live-host-proxy.service"
grep -Fq 'ConditionPathExists=/etc/apitoken-staging/stage-live.enabled' "$ROOT/systemd/apitoken-stage-live-client.service"
grep -Fq 'stage-live-control.sh enable' "$ROOT/deploy/sudoers.d/96-apitoken-stage"
! grep -Fq 'CONTROL_KEY' "$ROOT/systemd/apitoken-stage-live-client.service"
printf 'staging-phase8.test: PASS\n'
