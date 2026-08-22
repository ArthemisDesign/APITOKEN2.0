#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'staging-pressure-proof: root required' >&2; exit 1; }
[[ $(systemctl show staging.slice -p MemoryMax --value) == 34359738368 ]] || exit 1
[[ $(systemctl show staging.slice -p MemoryHigh --value) == 30064771072 ]] || exit 1
[[ $(systemctl show staging.slice -p CPUQuotaPerSecUSec --value) == 4s ]] || exit 1
# Bounded proof children. They never target production units and self-expire.
systemd-run --quiet --wait --collect --unit=staging-proof-pids --slice=staging.slice \
  -p TasksMax=64 -p RuntimeMaxSec=10 /bin/bash -c 'for i in $(seq 1 96); do sleep 5 & done; wait' \
  >/dev/null 2>&1 && { echo 'PID pressure did not hit the child bound' >&2; exit 1; } || true
systemd-run --quiet --wait --collect --unit=staging-proof-memory --slice=staging.slice \
  -p MemoryMax=512M -p RuntimeMaxSec=10 /usr/bin/python3 -c 'x=bytearray(700*1024*1024); print(len(x))' \
  >/dev/null 2>&1 && { echo 'memory pressure did not hit the child bound' >&2; exit 1; } || true
systemd-run --quiet --wait --collect --unit=staging-proof-cpu --slice=staging.slice \
  -p CPUQuota=400% -p RuntimeMaxSec=8 /bin/bash -c 'for i in $(seq 1 8); do sha256sum /dev/zero & done; sleep 3' \
  >/dev/null 2>&1 || true
for url in http://127.0.0.1:8790/ready http://127.0.0.1:8791/v1/ready \
  http://127.0.0.1:8792/ready http://127.0.0.1:8794/ready http://127.0.0.1:8802/ready; do
  curl -fsS -m 3 "$url" >/dev/null || { echo "production readiness failed after stage pressure: $url" >&2; exit 1; }
done
printf 'staging-pressure-proof: PASS\n'
