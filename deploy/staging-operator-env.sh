#!/usr/bin/env bash
set -euo pipefail
# Generate stage-only operator env files. Never copies production secrets. Idempotent:
# an existing non-empty bundle is reused; empty placeholders are filled.
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'staging-operator-env: root required' >&2; exit 1; }

CONFIG=/etc/apitoken-staging
DATA=/srv/claude-api-staging/data
BUNDLE=$CONFIG/.operator-bundle
umask 077

rand_hex() { openssl rand -hex "${1:-32}"; }
rand_b64url32() {
  python3 -c 'import secrets,base64; print(base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip("="))'
}

install -d -o root -g deploy-stage -m 0750 "$CONFIG"
install -d -o deploy-stage -g deploy-stage -m 0750 "$DATA"

if [[ ! -s $BUNDLE ]]; then
  tmp=$(mktemp)
  redis_password=
  if [[ -s $CONFIG/redis.env ]]; then
    # shellcheck disable=SC1091
    redis_password=$(awk -F= '/^STAGE_REDIS_PASSWORD=/{print $2; exit}' "$CONFIG/redis.env")
  fi
  [[ -n $redis_password ]] || redis_password=$(rand_hex 32)
  cat >"$tmp" <<EOF
CONTROL_KEY=$(rand_hex 32)
PANEL_KEY=$(rand_hex 32)
ENGINE_ADMIN_KEY=$(rand_hex 32)
SALES_CONTROL_KEY=$(rand_hex 32)
COMMERCIAL_ADMIN_KEY=$(rand_hex 32)
SALES_ADMIN_KEY=$(rand_hex 32)
AUTH_TOKEN_ENCRYPTION_KEY=$(rand_b64url32)
SALES_TOKEN_ENCRYPTION_KEY=$(rand_b64url32)
OPENKEYS_SESSION_SECRET=$(rand_hex 32)
OPENKEYS_ADMIN_USER=stage-admin
OPENKEYS_ADMIN_PASSWORD=$(rand_hex 16)
CONTENT_STUDIO_ENGINE_KEY=$(rand_hex 24)
AFFINITY_SECRET=$(rand_hex 32)
COMMERCE_DB_PASSWORD=$(rand_hex 32)
ENGINE_DB_PASSWORD=$(rand_hex 32)
SALES_DB_PASSWORD=$(rand_hex 32)
OPENKEYS_DB_PASSWORD=$(rand_hex 32)
STAGE_REDIS_PASSWORD=$redis_password
EOF
  chown root:root "$tmp"
  chmod 0600 "$tmp"
  mv -f "$tmp" "$BUNDLE"
fi

# shellcheck disable=SC1090
source "$BUNDLE"
[[ -n ${CONTROL_KEY:-} && -n ${COMMERCE_DB_PASSWORD:-} ]] || {
  echo 'staging-operator-env: bundle is incomplete' >&2
  exit 1
}

write_env() {
  local path=$1
  shift
  local tmp
  tmp=$(mktemp)
  printf '%s\n' "$@" >"$tmp"
  chown root:root "$tmp"
  chmod 0600 "$tmp"
  mv -f "$tmp" "$path"
}

pg() {
  printf 'postgresql://%s:%s@10.254.32.2:5433/%s?sslmode=disable' "$1" "$2" "$3"
}

redis_url() {
  printf 'redis://:%s@10.254.32.2:%s/0' "$STAGE_REDIS_PASSWORD" "$1"
}

write_env "$CONFIG/anthropic.env" \
  'CLAUDE_API_PROVIDER=anthropic' \
  'CLAUDE_API_KIMI_ENABLED=0' \
  'CLAUDE_API_GLM_ENABLED=0'

write_env "$CONFIG/openai.env" \
  'CLAUDE_API_PROVIDER=openai' \
  'CLAUDE_API_CODEX_ENABLED=0'

write_env "$CONFIG/gemini.env" \
  'CLAUDE_API_PROVIDER=gemini' \
  'CLAUDE_API_GEMINI_BATCH_ENABLED=0'

write_env "$CONFIG/kimi.env" \
  'CLAUDE_API_PROVIDER=kimi' \
  'CLAUDE_API_KIMI_ENABLED=0'

write_env "$CONFIG/router.env" \
  'CLAUDE_ROUTER_HOST=127.0.0.1' \
  'CLAUDE_ROUTER_KIMI_ORIGIN=http://127.0.0.1:8804'

write_env "$CONFIG/api.env" \
  'NODE_ENV=development' \
  'HOST=127.0.0.1' \
  'PORT=3000' \
  "DATABASE_URL=$(pg commerce "$COMMERCE_DB_PASSWORD" commerce)" \
  'ENGINE_BASE_URL=http://127.0.0.1:8787' \
  "ENGINE_CONTROL_KEY=$CONTROL_KEY" \
  'ENGINE_TIMEOUT_MS=10000' \
  'PUBLIC_API_BASE_URL=http://10.254.32.2:8791' \
  'PUBLIC_APP_BASE_URL=http://10.254.32.2:3900' \
  "AUTH_TOKEN_ENCRYPTION_KEY=$AUTH_TOKEN_ENCRYPTION_KEY" \
  'EMAIL_VERIFICATION_REQUIRED=false' \
  "COMMERCIAL_ADMIN_KEY=$COMMERCIAL_ADMIN_KEY" \
  "SALES_CONTROL_KEY=$SALES_CONTROL_KEY" \
  'SALES_API_URL=http://127.0.0.1:3100' \
  "CONTENT_STUDIO_ENGINE_KEY=$CONTENT_STUDIO_ENGINE_KEY"

write_env "$CONFIG/worker.env" \
  'NODE_ENV=development' \
  "DATABASE_URL=$(pg commerce "$COMMERCE_DB_PASSWORD" commerce)" \
  'ENGINE_BASE_URL=http://127.0.0.1:8787' \
  "ENGINE_CONTROL_KEY=$CONTROL_KEY" \
  "AUTH_TOKEN_ENCRYPTION_KEY=$AUTH_TOKEN_ENCRYPTION_KEY" \
  'EMAIL_DELIVERY_MODE=disabled' \
  'PUBLIC_APP_BASE_URL=http://10.254.32.2:3900' \
  'PUBLIC_API_BASE_URL=http://10.254.32.2:8791'

write_env "$CONFIG/sales-api.env" \
  'NODE_ENV=development' \
  'HOST=127.0.0.1' \
  'PORT=3100' \
  "SALES_DATABASE_URL=$(pg sales "$SALES_DB_PASSWORD" sales)" \
  "SALES_TOKEN_ENCRYPTION_KEY=$SALES_TOKEN_ENCRYPTION_KEY" \
  "SALES_ADMIN_KEY=$SALES_ADMIN_KEY" \
  "SALES_CONTROL_KEY=$SALES_CONTROL_KEY" \
  'COMMERCE_BASE_URL=http://127.0.0.1:3000' \
  'PUBLIC_SALES_BASE_URL=http://10.254.32.2:3200' \
  'PUBLIC_MAIN_SITE_URL=http://10.254.32.2:3900' \
  'EMAIL_DELIVERY_MODE=log' \
  'PAYOUT_READ_RPC_URLS=http://127.0.0.1:3901'

write_env "$CONFIG/sales-web.env" \
  'NODE_ENV=development' \
  'HOST=127.0.0.1' \
  'PORT=3200'

write_env "$CONFIG/openkeys.env" \
  'NODE_ENV=development' \
  "OPENKEYS_DATABASE_URL=$(pg openkeys "$OPENKEYS_DB_PASSWORD" openkeys)" \
  'ENGINE_BASE_URL=http://127.0.0.1:8787' \
  "ENGINE_CONTROL_KEY=$CONTROL_KEY" \
  'ENGINE_PUBLIC_BASE_URL=https://api.apitoken.sale' \
  'ENGINE_OPENAI_PUBLIC_BASE_URL=https://openai.api.apitoken.sale' \
  "OPENKEYS_ADMIN_USER=$OPENKEYS_ADMIN_USER" \
  "OPENKEYS_ADMIN_PASSWORD=$OPENKEYS_ADMIN_PASSWORD" \
  "OPENKEYS_SESSION_SECRET=$OPENKEYS_SESSION_SECRET" \
  'OPENKEYS_PUBLIC_BASE_URL=https://openkeys.apitoken.sale'

write_env "$CONFIG/admin.env" \
  'NODE_ENV=development' \
  'HOST=127.0.0.1' \
  'PORT=3700'

write_env "$CONFIG/authbot.env" \
  'AUTH_BOT_MOCK=1'

write_env "$CONFIG/devbot.env" \
  'DEVBOT_LOG_SINK=1' \
  'DEVBOT_ENGINE_BASE_URL=http://127.0.0.1:8787'

write_env "$CONFIG/sinks.env" \
  'STAGE_SAFE_SINK=1'

write_env "$DATA/server.env" \
  'CLAUDE_API_HOST=127.0.0.1' \
  "CLAUDE_API_CONTROL_KEY=$CONTROL_KEY" \
  "CLAUDE_API_PANEL_KEY=$PANEL_KEY" \
  "CLAUDE_API_KEYS=$ENGINE_ADMIN_KEY" \
  'CLAUDE_API_UPSTREAM=http://127.0.0.1:9080' \
  'CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1' \
  'CLAUDE_API_BILLING=1' \
  'CLAUDE_API_TRUST_LOOPBACK=0' \
  'CLAUDE_API_CODEX_ENABLED=0' \
  'CLAUDE_API_GLM_ENABLED=0' \
  'CLAUDE_API_KIMI_ENABLED=0' \
  'CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0' \
  'CLAUDE_API_GEMINI_BATCH_ENABLED=0' \
  "CLAUDE_API_REDIS_URL=$(redis_url 6379)" \
  "CLAUDE_API_AFFINITY_REDIS_URL=$(redis_url 6380)" \
  "CLAUDE_API_AFFINITY_SECRET=$AFFINITY_SECRET" \
  "SUB_CFG_DIR=$DATA"

write_env "$DATA/engine-postgres.env" \
  "CLAUDE_API_DATABASE_URL=postgresql://claude_engine:${ENGINE_DB_PASSWORD}@127.0.0.2:5433/claude_engine"

write_env "$DATA/db-roles.env" \
  "COMMERCE_DB_PASSWORD=$COMMERCE_DB_PASSWORD" \
  "ENGINE_DB_PASSWORD=$ENGINE_DB_PASSWORD" \
  "SALES_DB_PASSWORD=$SALES_DB_PASSWORD" \
  "OPENKEYS_DB_PASSWORD=$OPENKEYS_DB_PASSWORD"

if [[ ! -s $CONFIG/redis.env ]]; then
  write_env "$CONFIG/redis.env" "STAGE_REDIS_PASSWORD=$STAGE_REDIS_PASSWORD"
  chown root:deploy-stage "$CONFIG/redis.env"
  chmod 0640 "$CONFIG/redis.env"
fi

printf 'staging-operator-env: stage-only env files are present\n'
