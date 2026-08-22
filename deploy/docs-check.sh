#!/usr/bin/env bash
# Stable entry point for the exact-range living-documentation gate.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
exec python3 "$ROOT/deploy/docs-check.py" "$@"
