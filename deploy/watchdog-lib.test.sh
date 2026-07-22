#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT
for fixture in baseline appended tampered; do
  mkdir -p "$TEMP/$fixture/packages/db"
  cp -R -- "$ROOT/packages/db/migrations" "$TEMP/$fixture/packages/db/migrations"
done

wd_migration_manifest "$TEMP/baseline" >"$TEMP/baseline.manifest"

node - "$TEMP/appended/packages/db/migrations/meta/_journal.json" "$TEMP/appended/packages/db/migrations/0012_watchdog_manifest_test.sql" <<'NODE'
const fs = require("node:fs");
const [journalPath, sqlPath] = process.argv.slice(2);
const journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
const previous = journal.entries.at(-1);
journal.entries.push({
  idx: journal.entries.length,
  version: journal.version,
  when: previous.when + 1,
  tag: "0012_watchdog_manifest_test",
  breakpoints: true,
});
fs.writeFileSync(journalPath, `${JSON.stringify(journal, null, 2)}\n`);
fs.writeFileSync(sqlPath, "CREATE TABLE watchdog_manifest_test (id integer PRIMARY KEY);\n");
NODE
wd_migration_manifest "$TEMP/appended" >"$TEMP/appended.manifest"
wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/appended.manifest"
[[ $(wd_manifest_digest "$TEMP/baseline.manifest") != $(wd_manifest_digest "$TEMP/appended.manifest") ]]

printf '\n-- forbidden historical edit\n' >>"$TEMP/tampered/packages/db/migrations/0000_curved_skrulls.sql"
wd_migration_manifest "$TEMP/tampered" >"$TEMP/tampered.manifest"
if wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/tampered.manifest"; then
  wd_die "manifest accepted an edited historical SQL migration"
fi

node - "$TEMP/tampered/packages/db/migrations/meta/_journal.json" <<'NODE'
const fs = require("node:fs");
const journalPath = process.argv[2];
const journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
journal.entries[0].when += 1;
fs.writeFileSync(journalPath, JSON.stringify(journal));
NODE
wd_migration_manifest "$TEMP/tampered" >"$TEMP/tampered-journal.manifest"
if wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/tampered-journal.manifest"; then
  wd_die "manifest accepted an edited historical journal entry"
fi

# Path classifiers: sales vs backend/engine/infra separation.
wd_path_is_sales apps/sales-api/src/main.ts || wd_die "sales-api not classified as sales"
wd_path_is_sales apps/sales-web/src/app/page.tsx || wd_die "sales-web not classified as sales"
wd_path_is_sales packages/sales-db/src/schema.ts || wd_die "sales-db not classified as sales"
wd_path_is_sales apps/api/src/main.ts && wd_die "commerce api wrongly classified as sales"
wd_path_is_sales crates/server/src/http.rs && wd_die "engine wrongly classified as sales"
wd_path_is_backend packages/sales-db/src/schema.ts || wd_die "sales-db should also be backend class (shared packages)"
wd_path_is_backend apps/content-studio/src/app/page.tsx || wd_die "content studio must trigger commerce deployment"

wd_engine_topology_is_steady 1 1 1 1 0 0 0 0 0 0
wd_engine_topology_is_steady 0 0 0 0 1 1 1 1 0 0
for invalid_topology in \
  "1 1 1 1 1 1 1 1 0 0" \
  "1 1 1 0 0 0 0 1 0 0" \
  "1 0 1 1 0 0 0 0 0 0" \
  "1 1 0 1 0 0 0 0 0 0" \
  "1 1 1 1 0 0 0 0 1 0" \
  "1 1 1 1 0 0 0 0 0 1"; do
  # shellcheck disable=SC2086 -- each fixture intentionally expands to ten arguments.
  if wd_engine_topology_is_steady $invalid_topology; then
    wd_die "engine topology accepted an invalid steady state: $invalid_topology"
  fi
done

wd_path_is_infrastructure deploy/watchdog.sh
wd_path_is_infrastructure systemd/apitoken-deploy-watchdog.service
wd_path_is_infrastructure compose.yaml
wd_path_is_infrastructure observability/prometheus/prometheus.yml
if wd_path_is_infrastructure .github/workflows/indexnow.yml; then
  wd_die "GitHub-only workflow changes must not require a production-host infrastructure install"
fi
wd_path_is_caddy deploy/Caddyfile
if wd_path_is_caddy deploy/watchdog.sh; then
  wd_die "non-Caddy infrastructure change requested a Caddy reload"
fi

grep -Fq 'admin.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'admin.partners.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'crm.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'content-studio.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'monitoring.apitoken.sale {' "$ROOT/deploy/Caddyfile"
! grep -Fq 'panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"
! grep -Fq 'partners.panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"
! grep -Fq 'crm.panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'import managed_admin_auth' "$ROOT/deploy/Caddyfile") -ge 5 ]]
grep -Fq 'forward_auth 127.0.0.1:8791' "$ROOT/deploy/Caddyfile"
grep -Fq 'order request_header before forward_auth' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up Host 127.0.0.1:8791' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-Admin-Key "<ADMIN_AUTH_KEY_PLACEHOLDER>"' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-Admin-Domain {http.request.host}' "$ROOT/deploy/Caddyfile"
grep -Fq '@commerce_admin path /admin/*' "$ROOT/deploy/Caddyfile"
grep -Fq 'handle_path /partner-admin/*' "$ROOT/deploy/Caddyfile"
grep -Fq 'copy_headers X-Admin-Actor X-Admin-Account-Id' "$ROOT/deploy/Caddyfile"
! grep -Fq 'header_up x-admin-actor' "$ROOT/deploy/Caddyfile"
! grep -Fq 'header_up x-admin-account-id' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'reverse_proxy 127.0.0.1:3000 127.0.0.1:3001' "$ROOT/deploy/Caddyfile") == 1 ]]
[[ $(grep -Fc 'reverse_proxy 127.0.0.1:8787 127.0.0.1:8788' "$ROOT/deploy/Caddyfile") == 1 ]]
! grep -Fq 'unhealthy_status 503' "$ROOT/deploy/Caddyfile"
! grep -Fq 'fail_duration' "$ROOT/deploy/Caddyfile"
! grep -Fq 'max_fails' "$ROOT/deploy/Caddyfile"
grep -Fq 'request>headers>X-Admin-Key replace REDACTED' "$ROOT/deploy/Caddyfile"
grep -Fq 'COMMERCE_BASE_URL=http://127.0.0.1:8791' "$ROOT/apps/sales-api/.env.example"
grep -Fq 'COMMERCE_BALANCER_URL=${COMMERCE_BALANCER_URL:-http://127.0.0.1:8791}' "$ROOT/deploy/sales-deploy.sh"
grep -Fq 'configure_commerce_balancer' "$ROOT/deploy/sales-deploy.sh"
grep -Fq 'COMMERCE_BALANCER_READY_URL=${COMMERCE_BALANCER_READY_URL:-http://127.0.0.1:8791/v1/ready}' "$ROOT/deploy/api-bluegreen.sh"
[[ $(grep -Fc 'balancer_is_ready' "$ROOT/deploy/api-bluegreen.sh") -ge 6 ]]
grep -Fq 'final_verify_admin_panel' "$ROOT/deploy/watchdog.sh"
grep -Fq 'monitoring-config.test.sh' "$ROOT/deploy/watchdog.sh"
grep -Fq 'TEST_SALES_DATABASE_URL=' "$ROOT/deploy/watchdog.sh"
grep -Fq 'sales-dsn)' "$ROOT/deploy/watchdog-test-db.sh"
grep -Fq 'require_retired_vhost panel.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq 'require_admin_auth_vhost content-studio.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq 'require_admin_auth_vhost monitoring.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq "''|000|404|421" "$ROOT/deploy/watchdog.sh"
grep -Fq 'data-admin-panel-version="4"' "$ROOT/crates/server/src/admin-panel.html"
[[ ! -e "$ROOT/crates/server/src/panel.html" ]]

render_live="$TEMP/live.Caddyfile"
rendered_once="$TEMP/rendered-once.Caddyfile"
rendered_twice="$TEMP/rendered-twice.Caddyfile"
{
  printf '(panel_admins) {\n\tbasic_auth {\n\t\tadmin $2y$12$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q\n\t}\n}\n'
  printf '(crm_admins) {\n\tbasic_auth {\n\t\tcrm $2a$14$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q\n\t}\n}\n'
  printf 'admin.apitoken.sale {\n'
  printf '\theader_up x-api-key "test-control-secret"\n'
  printf '\theader_up x-admin-key "test-commerce-secret"\n'
  printf '\theader_up x-sales-admin-key "test-sales-secret"\n}\n'
} >"$render_live"
awk -f "$ROOT/deploy/render-caddy.awk" "$render_live" "$ROOT/deploy/Caddyfile" >"$rendered_once"
awk -f "$ROOT/deploy/render-caddy.awk" "$rendered_once" "$ROOT/deploy/Caddyfile" >"$rendered_twice"
for rendered in "$rendered_once" "$rendered_twice"; do
  ! grep -Fq 'basic_auth' "$rendered"
  ! grep -Fq '$2y$' "$rendered"
  grep -Fq 'forward_auth 127.0.0.1:8791' "$rendered"
  [[ $(grep -Fc 'header_up x-api-key "test-control-secret"' "$rendered") == 1 ]]
  [[ $(grep -Fc 'header_up x-admin-key "test-commerce-secret"' "$rendered") == 2 ]]
  [[ $(grep -Fc 'header_up X-Admin-Key "test-commerce-secret"' "$rendered") == 1 ]]
  [[ $(grep -Fc 'header_up x-sales-admin-key "test-sales-secret"' "$rendered") == 1 ]]
  if grep -Eq '<[A-Z_]*PLACEHOLDER>' "$rendered"; then
    wd_die "rendered Caddy fixture retained a secret placeholder"
  fi
done

legacy_export="$TEMP/legacy-admins.json"
awk -f "$ROOT/deploy/export-legacy-admins.awk" "$render_live" >"$legacy_export"
node - "$legacy_export" <<'NODE'
const fs = require("node:fs");
const value = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
if (value.accounts.length !== 2) process.exit(1);
const panel = value.accounts.find((account) => account.username === "admin");
const crm = value.accounts.find((account) => account.username === "crm");
if (!panel || panel.domains.length !== 4 || !panel.domains.includes("admin.apitoken.sale") ||
    !panel.domains.includes("monitoring.apitoken.sale")) process.exit(1);
if (!crm || crm.domains.length !== 1 || crm.domains[0] !== "crm.apitoken.sale") process.exit(1);
NODE

watchdog_writable_paths=$(sed -n 's/^ReadWritePaths=//p' "$ROOT/systemd/apitoken-deploy-watchdog.service")
for required_path in \
  /var/lib/apitoken/watchdog /opt/apitoken /srv/claude-api/releases /run/lock \
  /usr/local/lib/apitoken-watchdog /usr/local/bin /etc/systemd/system /etc/caddy \
  /etc/apitoken /srv/claude-api/data /var/lib/apitoken/monitoring; do
  if ! tr ' ' '\n' <<<"$watchdog_writable_paths" | grep -Fxq "$required_path"; then
    wd_die "watchdog service cannot update required operational path: $required_path"
  fi
done

for cache_environment in \
  'Environment=CARGO_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/cargo' \
  'Environment=XDG_CACHE_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/xdg-cache' \
  'Environment=XDG_CONFIG_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/xdg-config' \
  'Environment=XDG_DATA_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/xdg-data'; do
  grep -Fxq "$cache_environment" "$ROOT/systemd/apitoken-deploy-watchdog.service" \
    || wd_die "watchdog service is missing writable build cache environment: $cache_environment"
done
grep -Fq 'DEPLOY_BUILD_CACHE_ROOT=/var/lib/apitoken/watchdog/deploy-build-cache' \
  "$ROOT/deploy/deploy.sh" || wd_die 'release builder does not pin the writable build cache'
grep -Fq '/var/lib/apitoken/watchdog/deploy-build-cache/cargo' \
  "$ROOT/deploy/install-watchdog.sh" || wd_die 'watchdog installer does not create the release build cache'
grep -Fq 'tokio-postgres-rustls' "$ROOT/crates/registry/Cargo.toml" \
  || wd_die 'engine PostgreSQL transport must use rustls alongside the BoringSSL forward transport'
if grep -Eq '^[[:space:]]*(postgres-native-tls|native-tls)[[:space:]]*=' \
  "$ROOT/crates/registry/Cargo.toml"; then
  wd_die 'OpenSSL-compatible PostgreSQL TLS cannot be linked with the BoringSSL forward transport'
fi

printf 'watchdog migration and engine topology tests passed\n'
