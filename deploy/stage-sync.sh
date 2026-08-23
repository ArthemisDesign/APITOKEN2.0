#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 2 && $1 == --after-hotfix && $2 =~ ^[0-9a-f]{40}$ ]] || {
  echo 'usage: stage-sync.sh --after-hotfix <master-sha>' >&2; exit 2;
}
exec ssh -o BatchMode=yes stage-ctl@84.32.48.2 -- sync "$2"
