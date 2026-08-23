#!/usr/bin/env bash
set -euo pipefail
ROOT=/var/lib/apitoken-staging
KEEP=3
find "$ROOT/backups" -mindepth 1 -maxdepth 1 -type f -name '*.dump' -print0 2>/dev/null \
  | xargs -0 ls -1t 2>/dev/null | tail -n +$((KEEP + 1)) | xargs -r rm -f --
find /srv/claude-api-staging/releases /opt/apitoken-staging/releases -mindepth 1 -maxdepth 1 -type d \
  -name '[0-9a-f]*' -print0 2>/dev/null | xargs -0 ls -1dt 2>/dev/null \
  | tail -n +$((KEEP + 1)) | xargs -r rm -rf --
printf 'stage-gc: KEEP=%s applied inside loopback roots\n' "$KEEP"
