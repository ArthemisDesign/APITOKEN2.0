#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || exit 1
mem_kib=$(awk '/^MemAvailable:/ {print $2}' /proc/meminfo)
[[ $mem_kib =~ ^[0-9]+$ ]] || exit 1
if (( mem_kib < 12582912 )); then
  systemctl stop staging.slice
  loginctl terminate-user deploy-stage >/dev/null 2>&1 || true
  echo 'stage-emergency-guard: stopped stage below 12G MemAvailable'
  exit 0
fi
# Production readiness failure also stops stage. Hotfix admission never depends on stage state.
for url in http://127.0.0.1:8790/ready http://127.0.0.1:8791/v1/ready \
  http://127.0.0.1:8792/ready http://127.0.0.1:8794/ready http://127.0.0.1:8802/ready; do
  if ! curl -fsS -m 2 "$url" >/dev/null; then
    systemctl stop staging.slice
    loginctl terminate-user deploy-stage >/dev/null 2>&1 || true
    echo "stage-emergency-guard: stopped stage after production SLO red: $url"
    exit 0
  fi
done
