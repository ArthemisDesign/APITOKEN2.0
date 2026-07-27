#!/usr/bin/env bash
set -euo pipefail

APP_ROOT=${CONTENT_STUDIO_APP_ROOT:-/opt/apitoken/releases/current/apps/content-studio}
NODE_BIN=${CONTENT_STUDIO_NODE_BIN:-/usr/bin/node}
HOST=${HOSTNAME:-127.0.0.1}
PORT_NUMBER=${PORT:-3500}
STANDALONE_SERVER=$APP_ROOT/.next/standalone/apps/content-studio/server.js

if [[ -f $STANDALONE_SERVER && ! -L $STANDALONE_SERVER ]]; then
  export HOSTNAME=$HOST
  export PORT=$PORT_NUMBER
  exec "$NODE_BIN" "$STANDALONE_SERVER"
fi

# Releases built before the compact-bundle format remain valid rollback anchors.
LEGACY_NEXT=$APP_ROOT/node_modules/.bin/next
[[ -x $LEGACY_NEXT ]] || {
  printf 'content-studio-start: no standalone or legacy Next.js runtime in %s\n' "$APP_ROOT" >&2
  exit 1
}
exec "$LEGACY_NEXT" start -H "$HOST" -p "$PORT_NUMBER"
