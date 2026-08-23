#!/usr/bin/env bash
set -euo pipefail
# Trusted master-sourced mock twin: stage-only env, databases, release copies, units, mock
# upstream. Does not copy production secrets. Does not start live provider planes.
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'install-staging-twin: root required' >&2; exit 1; }

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [[ ! -f $ROOT/deploy/staging-operator-env.sh ]]; then
  ROOT=/usr/local/lib/apitoken-watchdog
fi
ENV_SCRIPT=$ROOT/deploy/staging-operator-env.sh
[[ -x $ENV_SCRIPT ]] || ENV_SCRIPT=$ROOT/staging-operator-env.sh
[[ -x $ENV_SCRIPT ]] || { echo 'install-staging-twin: env generator missing' >&2; exit 1; }

ip netns list | awk '{print $1}' | grep -Fxq apitoken-stage \
  || { echo 'install-staging-twin: stage netns missing' >&2; exit 1; }
mountpoint -q /var/lib/apitoken-staging \
  || { echo 'install-staging-twin: staging loopback is not mounted' >&2; exit 1; }

DOCKER=(runuser -u deploy-stage -- env DOCKER_HOST=unix:///run/apitoken-staging/docker.sock docker)
pg_exec() {
  "${DOCKER[@]}" exec -i apitoken-postgres-stage psql -U stage -d postgres -v ON_ERROR_STOP=1 "$@"
}

wait_pg() {
  local _
  for _ in $(seq 1 60); do
    "${DOCKER[@]}" exec apitoken-postgres-stage pg_isready -U stage -d stage_empty >/dev/null 2>&1 && return 0
    sleep 1
  done
  echo 'install-staging-twin: postgres-stage is not ready' >&2
  exit 1
}

copy_release() {
  local src=$1 dst_root=$2
  local sha current
  [[ -L $src/current || -d $src/current ]] || return 0
  current=$(readlink -f "$src/current")
  [[ -d $current ]] || return 0
  sha=$(basename "$current")
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || return 0
  install -d -o deploy-stage -g deploy-stage -m 0750 "$dst_root"
  if [[ ! -d $dst_root/$sha ]]; then
    cp -a "$current" "$dst_root/$sha"
    chown -R deploy-stage:deploy-stage "$dst_root/$sha"
  fi
  ln -sfn "$dst_root/$sha" "$dst_root/current"
  chown -h deploy-stage:deploy-stage "$dst_root/current"
}

ensure_role_db() {
  local role=$1 password=$2 db=$3
  pg_exec -c "DO \$\$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='$role') THEN CREATE ROLE $role LOGIN; END IF; END \$\$;"
  pg_exec -c "ALTER ROLE $role NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD '$password';"
  if [[ $(pg_exec -Atqc "SELECT 1 FROM pg_database WHERE datname='$db'") != 1 ]]; then
    pg_exec -c "CREATE DATABASE $db OWNER $role;"
  fi
  pg_exec -c "ALTER DATABASE $db OWNER TO $role;"
  pg_exec -c "REVOKE ALL ON DATABASE $db FROM PUBLIC;"
  pg_exec -c "GRANT CONNECT,TEMPORARY ON DATABASE $db TO $role;"
}

install_unit() {
  local unit=$1
  local src=$ROOT/systemd/$unit
  [[ -f $src ]] || src=$ROOT/$unit
  if [[ ! -f $src ]]; then
    [[ -f /etc/systemd/system/$unit ]] && return 0
    echo "install-staging-twin: missing unit $unit" >&2
    exit 1
  fi
  install -o root -g root -m 0644 "$src" "/etc/systemd/system/$unit"
}

"$ENV_SCRIPT"
wait_pg

# shellcheck disable=SC1091
source /srv/claude-api-staging/data/db-roles.env

ensure_role_db commerce "$COMMERCE_DB_PASSWORD" commerce
ensure_role_db claude_engine "$ENGINE_DB_PASSWORD" claude_engine
ensure_role_db sales "$SALES_DB_PASSWORD" sales
ensure_role_db openkeys "$OPENKEYS_DB_PASSWORD" openkeys

install -d -o deploy-stage -g deploy-stage -m 0750 \
  /srv/claude-api-staging/data \
  /var/lib/apitoken-staging/{logs,spool,cache} \
  /var/lib/apitoken-staging/spool/router-8800 \
  /var/lib/apitoken-staging/spool/router-8801 \
  /opt/apitoken-staging/{releases,sales-releases,openkeys-releases,admin-releases,devbot-releases} \
  /srv/claude-api-staging/releases

copy_release /srv/claude-api/releases /srv/claude-api-staging/releases
copy_release /opt/apitoken/releases /opt/apitoken-staging/releases
copy_release /opt/apitoken/sales-releases /opt/apitoken-staging/sales-releases
copy_release /opt/apitoken/openkeys-releases /opt/apitoken-staging/openkeys-releases
copy_release /opt/apitoken/admin-releases /opt/apitoken-staging/admin-releases

if [[ -f $ROOT/deploy/stage-loopback-pg.py ]]; then
  install -o root -g root -m 0755 "$ROOT/deploy/stage-loopback-pg.py" \
    /usr/local/lib/apitoken-watchdog/stage-loopback-pg.py
elif [[ -f $ROOT/stage-loopback-pg.py ]]; then
  install -o root -g root -m 0755 "$ROOT/stage-loopback-pg.py" \
    /usr/local/lib/apitoken-watchdog/stage-loopback-pg.py
fi
if [[ -f $ROOT/tests/mock_upstream.py ]]; then
  install -o root -g root -m 0755 "$ROOT/tests/mock_upstream.py" \
    /usr/local/lib/apitoken-watchdog/mock_upstream.py
elif [[ -f $ROOT/mock_upstream.py ]]; then
  install -o root -g root -m 0755 "$ROOT/mock_upstream.py" \
    /usr/local/lib/apitoken-watchdog/mock_upstream.py
fi

if [[ -f $ROOT/deploy/staging-Caddyfile ]]; then
  install -o root -g deploy-stage -m 0640 "$ROOT/deploy/staging-Caddyfile" \
    /etc/apitoken-staging/caddy/Caddyfile
elif [[ -f $ROOT/staging-Caddyfile ]]; then
  install -o root -g deploy-stage -m 0640 "$ROOT/staging-Caddyfile" \
    /etc/apitoken-staging/caddy/Caddyfile
fi

ip netns exec apitoken-stage nft list chain inet apitoken_stage output 2>/dev/null \
  | grep -Fq 'ip daddr 10.254.32.2 accept' \
  || ip netns exec apitoken-stage nft add rule inet apitoken_stage output ip daddr 10.254.32.2 accept

for unit in \
  apitoken-stage-pg-loopback.service \
  apitoken-stage-mock-upstream.service \
  claude-api-anthropic-stage@.service \
  claude-api-openai-stage@.service \
  claude-api-gemini-stage@.service \
  claude-api-kimi-stage@.service \
  claude-router-stage@.service \
  apitoken-api-stage@.service \
  apitoken-worker-stage.service \
  apitoken-sales-api-stage.service \
  apitoken-sales-web-stage.service \
  apitoken-openkeys-stage.service \
  apitoken-admin-stage.service \
  apitoken-staging-twin-install.service
 do
  install_unit "$unit"
done
systemctl daemon-reload
systemctl start apitoken-stage-pg-loopback.service

migrate_node() {
  local name=$1 url=$2 script=$3 resolved
  [[ -f $script ]] || return 0
  resolved=$(readlink -f "$script")
  case "$name" in
    commerce) DATABASE_URL=$url /usr/bin/node "$resolved" ;;
    sales) SALES_DATABASE_URL=$url /usr/bin/node "$resolved" ;;
    openkeys) OPENKEYS_DATABASE_URL=$url /usr/bin/node "$resolved" ;;
    *) echo "install-staging-twin: unknown migrate target $name" >&2; return 1 ;;
  esac
}

# shellcheck disable=SC1091
source /etc/apitoken-staging/.operator-bundle
if [[ -x /srv/claude-api-staging/releases/current/claude-api ]]; then
  ip netns exec apitoken-stage setpriv --reuid="$(id -u deploy-stage)" --regid="$(id -g deploy-stage)" --init-groups --no-new-privs \
    env HOME=/home/deploy-stage \
    CLAUDE_API_DATABASE_URL="postgresql://claude_engine:${ENGINE_DB_PASSWORD}@127.0.0.2:5433/claude_engine" \
    SUB_CFG_DIR=/srv/claude-api-staging/data \
    /srv/claude-api-staging/releases/current/claude-api db migrate-engine \
    || echo 'install-staging-twin: engine migrate deferred' >&2
fi
migrate_node commerce "postgresql://commerce:${COMMERCE_DB_PASSWORD}@10.254.32.2:5433/commerce" \
  /opt/apitoken-staging/releases/current/packages/db/dist/migrate.js
migrate_node sales "postgresql://sales:${SALES_DB_PASSWORD}@10.254.32.2:5433/sales" \
  /opt/apitoken-staging/sales-releases/current/packages/sales-db/dist/migrate.js
migrate_node openkeys "postgresql://openkeys:${OPENKEYS_DB_PASSWORD}@10.254.32.2:5433/openkeys" \
  /opt/apitoken-staging/openkeys-releases/current/packages/openkeys-db/dist/migrate.js

systemctl restart apitoken-stage-caddy.service
systemctl start --no-block apitoken-stage-mock-upstream.service \
  claude-api-anthropic-stage@8787.service \
  claude-router-stage@8800.service \
  apitoken-api-stage@3000.service \
  apitoken-worker-stage.service \
  apitoken-sales-api-stage.service \
  apitoken-sales-web-stage.service \
  apitoken-openkeys-stage.service \
  apitoken-admin-stage.service

if [[ -x /srv/claude-api-staging/releases/current/claude-api ]]; then
  seed_dir=/srv/claude-api-staging/data
  install -d -o deploy-stage -g deploy-stage -m 0700 "$seed_dir"
  if [[ ! -s $seed_dir/token-mock ]]; then
    printf 'faketoken-stage-mock-aaaaaaaa\n' >"$seed_dir/token-mock"
    chown deploy-stage:deploy-stage "$seed_dir/token-mock"
    chmod 0600 "$seed_dir/token-mock"
  fi
  ip netns exec apitoken-stage setpriv --reuid="$(id -u deploy-stage)" --regid="$(id -g deploy-stage)" --init-groups --no-new-privs \
    env HOME=/home/deploy-stage SUB_CFG_DIR=$seed_dir \
    CLAUDE_API_DATABASE_URL="postgresql://claude_engine:${ENGINE_DB_PASSWORD}@127.0.0.2:5433/claude_engine" \
    /srv/claude-api-staging/releases/current/claude-api sub add-file 'stage-mock@local' --token-file "$seed_dir/token-mock" \
    >/dev/null 2>&1 || true
  ip netns exec apitoken-stage setpriv --reuid="$(id -u deploy-stage)" --regid="$(id -g deploy-stage)" --init-groups --no-new-privs \
    env HOME=/home/deploy-stage SUB_CFG_DIR=$seed_dir \
    /srv/claude-api-staging/releases/current/claude-api sub set-plan 'stage-mock@local' max20 \
    >/dev/null 2>&1 || true
fi

printf 'install-staging-twin: mock twin inventory installed\n'
