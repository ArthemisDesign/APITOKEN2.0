#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
grep -Fq "sed -n 's/^CLAUDE_API_KEYS=//p'" "$ROOT/deploy/install-watchdog.sh"
grep -Fq "install -o root -g root -m 0600 /dev/stdin" "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'unset canary_keys canary_key' "$ROOT/deploy/install-watchdog.sh"
! grep -Eq 'openssl rand.*payload.canary|sk-pool-%s' "$ROOT/deploy/install-watchdog.sh"
echo 'payload canary secret derivation contract passed'
