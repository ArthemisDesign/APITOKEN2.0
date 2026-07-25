#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

# Candidate retention selects only direct, real SHA directories strictly older than the cutoff.
# It must not follow symlinks or touch malformed entries.
candidate_root="$TEMP/candidates"
mkdir -p "$candidate_root"
old_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
boundary_sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
new_sha=cccccccccccccccccccccccccccccccccccccccc
symlink_sha=dddddddddddddddddddddddddddddddddddddddd
marked_new_sha=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
marked_old_sha=ffffffffffffffffffffffffffffffffffffffff
mkdir -p "$candidate_root/$old_sha" "$candidate_root/$boundary_sha" \
  "$candidate_root/$new_sha" "$candidate_root/$marked_new_sha" \
  "$candidate_root/$marked_old_sha" "$candidate_root/not-a-sha" "$TEMP/outside-candidate"
ln -s -- "$TEMP/outside-candidate" "$candidate_root/$symlink_sha"
touch "$TEMP/$marked_new_sha.tested" "$TEMP/$marked_old_sha.tested"
candidate_cutoff=1800000000
node - "$candidate_root/$old_sha" "$candidate_root/$boundary_sha" \
  "$candidate_root/$new_sha" "$candidate_root/$marked_new_sha" \
  "$candidate_root/$marked_old_sha" "$candidate_root/not-a-sha" "$TEMP/outside-candidate" \
  "$TEMP/$marked_new_sha.tested" "$TEMP/$marked_old_sha.tested" "$candidate_cutoff" <<'NODE'
const fs = require("node:fs");
const [oldPath, boundaryPath, newPath, markedNewPath, markedOldPath, malformedPath, outsidePath,
  markedNewMarker, markedOldMarker, cutoffText] = process.argv.slice(2);
const cutoff = Number(cutoffText);
fs.utimesSync(oldPath, cutoff - 1, cutoff - 1);
fs.utimesSync(boundaryPath, cutoff, cutoff);
fs.utimesSync(newPath, cutoff + 1, cutoff + 1);
fs.utimesSync(markedNewPath, cutoff - 1, cutoff - 1);
fs.utimesSync(markedOldPath, cutoff + 1, cutoff + 1);
fs.utimesSync(malformedPath, cutoff - 1, cutoff - 1);
fs.utimesSync(outsidePath, cutoff - 1, cutoff - 1);
fs.utimesSync(markedNewMarker, cutoff + 1, cutoff + 1);
fs.utimesSync(markedOldMarker, cutoff - 1, cutoff - 1);
NODE
expired_candidates=()
while IFS= read -r -d '' expired_candidate; do
  expired_candidates+=("$expired_candidate")
done < <(wd_candidate_dirs_older_than "$candidate_root" "$TEMP" "$candidate_cutoff")
[[ ${#expired_candidates[@]} -eq 2 ]] \
  || wd_die "candidate retention selected an unsafe or non-expired directory"
printf '%s\n' "${expired_candidates[@]}" | grep -Fxq "$candidate_root/$old_sha" \
  || wd_die "candidate retention did not select an expired untested workspace"
printf '%s\n' "${expired_candidates[@]}" | grep -Fxq "$candidate_root/$marked_old_sha" \
  || wd_die "candidate retention did not use the test-completion marker age"

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

# Bounded retry: transient failures are absorbed, permanent ones still surface their exit status.
retry_attempts_file="$TEMP/retry-attempts"
printf '0\n' >"$retry_attempts_file"
flaky_command() {
  local count
  count=$(<"$retry_attempts_file")
  count=$((count + 1))
  printf '%s\n' "$count" >"$retry_attempts_file"
  (( count >= 3 ))
}
wd_retry 3 0 flaky_command || wd_die "retry did not absorb a transient failure"
[[ $(<"$retry_attempts_file") == 3 ]] || wd_die "retry did not stop at the first success"
if wd_retry 2 0 false; then
  wd_die "retry reported success for a permanently failing command"
fi

# Release retention: current/previous and explicitly protected SHAs survive regardless of age, the
# newest `keep` are retained, and only genuine SHA directories are ever selected.
release_root="$TEMP/releases"
mkdir -p "$release_root"
release_shas=(
  1111111111111111111111111111111111111111
  2222222222222222222222222222222222222222
  3333333333333333333333333333333333333333
  4444444444444444444444444444444444444444
  5555555555555555555555555555555555555555
)
for release_sha in "${release_shas[@]}"; do
  mkdir -p "$release_root/$release_sha"
done
mkdir -p "$release_root/not-a-release"
ln -s "$release_root/${release_shas[4]}" "$release_root/current"
ln -s "$release_root/${release_shas[3]}" "$release_root/previous"
node - "$release_root" "${release_shas[@]}" <<'NODE'
const fs = require("node:fs");
const [root, ...shas] = process.argv.slice(2);
// Oldest first, so index 0 is the least recently modified release.
shas.forEach((sha, index) => {
  const when = 1700000000 + index;
  fs.utimesSync(`${root}/${sha}`, when, when);
});
NODE

# keep=1 retains the newest unprotected release plus current/previous. Of the three unprotected
# releases (…111, …222, …333), the newest (…333) is kept and the two oldest are selected.
prunable_releases=()
while IFS= read -r -d '' prunable_release; do
  prunable_releases+=("${prunable_release##*/}")
done < <(wd_prunable_release_dirs "$release_root" 1)
[[ ${#prunable_releases[@]} -eq 2 ]] \
  || wd_die "release retention selected ${#prunable_releases[@]} directories, expected 2"
printf '%s\n' "${prunable_releases[@]}" | grep -Fxq "${release_shas[0]}" \
  || wd_die "release retention did not select the oldest release"
printf '%s\n' "${prunable_releases[@]}" | grep -Fxq "${release_shas[1]}" \
  || wd_die "release retention did not select the second-oldest release"
for protected_release in "${release_shas[4]}" "${release_shas[3]}" "${release_shas[2]}" not-a-release; do
  if printf '%s\n' "${prunable_releases[@]}" | grep -Fxq "$protected_release"; then
    wd_die "release retention selected a protected or non-release entry: $protected_release"
  fi
done

# An explicitly protected SHA (a live PID's release, or a recorded component baseline) must survive
# even when retention counting would otherwise reach it.
protected_selection=()
while IFS= read -r -d '' prunable_release; do
  protected_selection+=("${prunable_release##*/}")
done < <(wd_prunable_release_dirs "$release_root" 1 "${release_shas[0]}")
if printf '%s\n' "${protected_selection[@]}" | grep -Fxq "${release_shas[0]}"; then
  wd_die "release retention removed an explicitly protected live release"
fi

# keep=0 with no protected list still must not touch current/previous.
zero_keep_selection=()
while IFS= read -r -d '' prunable_release; do
  zero_keep_selection+=("${prunable_release##*/}")
done < <(wd_prunable_release_dirs "$release_root" 0)
[[ ${#zero_keep_selection[@]} -eq 3 ]] \
  || wd_die "keep=0 retention must still protect current and previous"

# Pre-deploy dump retention is per-database and must never select the hourly rotation artifact.
dump_root="$TEMP/backups"
mkdir -p "$dump_root"
for database in commerce claude_engine; do
  : >"$dump_root/$database.dump"
  for index in 1 2 3; do
    dump_sha=$(printf '%040d' "$index")
    : >"$dump_root/$database.pre-deploy-$dump_sha.dump"
    node -e 'const fs=require("node:fs");const w=1700000000+Number(process.argv[2]);fs.utimesSync(process.argv[1],w,w);' \
      "$dump_root/$database.pre-deploy-$dump_sha.dump" "$index"
  done
done
: >"$dump_root/commerce-pre-offboard-20260715T102931Z.dump"

prunable_dumps=()
while IFS= read -r -d '' prunable_dump; do
  prunable_dumps+=("${prunable_dump##*/}")
done < <(wd_prunable_predeploy_dumps "$dump_root" 2)
# Two databases, three snapshots each, keep the newest two: exactly one per database is selected.
[[ ${#prunable_dumps[@]} -eq 2 ]] \
  || wd_die "dump retention selected ${#prunable_dumps[@]} files, expected 2"
for retained_dump in commerce.dump claude_engine.dump commerce-pre-offboard-20260715T102931Z.dump; do
  if printf '%s\n' "${prunable_dumps[@]}" | grep -Fxq "$retained_dump"; then
    wd_die "dump retention selected a non-pre-deploy artifact: $retained_dump"
  fi
done
oldest_dump_sha=$(printf '%040d' 1)
printf '%s\n' "${prunable_dumps[@]}" | grep -Fxq "commerce.pre-deploy-$oldest_dump_sha.dump" \
  || wd_die "dump retention did not select the oldest commerce snapshot"

# Path classifiers: sales vs backend/engine/infra separation.
wd_path_is_sales apps/sales-api/src/main.ts || wd_die "sales-api not classified as sales"
wd_path_is_sales apps/sales-web/src/app/page.tsx || wd_die "sales-web not classified as sales"
wd_path_is_sales packages/sales-db/src/schema.ts || wd_die "sales-db not classified as sales"
wd_path_is_sales apps/api/src/main.ts && wd_die "commerce api wrongly classified as sales"
wd_path_is_sales crates/server/src/http.rs && wd_die "engine wrongly classified as sales"
wd_path_is_backend packages/sales-db/src/schema.ts || wd_die "sales-db should also be backend class (shared packages)"
wd_path_is_backend apps/content-studio/src/app/page.tsx || wd_die "content studio must trigger commerce deployment"
wd_path_is_engine tools/codex-app-server/build-pinned.sh \
  || wd_die "pinned Codex tooling must trigger an engine deployment"

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
wd_path_is_infrastructure deploy/affinity-redis.compose.yaml
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

# Shared cache affinity must remain host-local, durable enough for cache continuity, and optional
# for engine availability. PostgreSQL continues to own all financial/capacity correctness.
grep -Fq '127.0.0.1:6379:6379' "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq 'redis:7.4.2-alpine@sha256:02419de7eddf55aa5bcf49efb74e88fa8d931b4d77c07eff8a6b2144472b6952' \
  "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq -- '--appendonly' "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq 'everysec' "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq 'Wants=network-online.target apitoken-affinity-redis.service' \
  "$ROOT/systemd/claude-api@.service"
! grep -Fq 'Requires=apitoken-affinity-redis.service' "$ROOT/systemd/claude-api@.service"
grep -Fq 'CLAUDE_API_AFFINITY_SECRET' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'apitoken-affinity-redis.service' "$ROOT/deploy/install-watchdog.sh"
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
# Both slots of each pair stay listed as upstreams while exactly one runs, so the active health
# checker fails against a deliberately stopped address once every two seconds forever. Excluding
# that logger is what keeps the journal and the Grafana error panel readable; losing the line
# silently reintroduces roughly one junk entry per second.
[[ $(grep -Fc 'exclude http.handlers.reverse_proxy.health_checker.active' "$ROOT/deploy/Caddyfile") == 1 ]]
grep -Fq 'COMMERCE_BASE_URL=http://127.0.0.1:8791' "$ROOT/apps/sales-api/.env.example"
grep -Fq 'COMMERCE_BALANCER_URL=${COMMERCE_BALANCER_URL:-http://127.0.0.1:8791}' "$ROOT/deploy/sales-deploy.sh"
grep -Fq 'configure_commerce_balancer' "$ROOT/deploy/sales-deploy.sh"
grep -Fq 'COMMERCE_BALANCER_READY_URL=${COMMERCE_BALANCER_READY_URL:-http://127.0.0.1:8791/v1/ready}' "$ROOT/deploy/api-bluegreen.sh"
[[ $(grep -Fc 'balancer_is_ready' "$ROOT/deploy/api-bluegreen.sh") -ge 6 ]]
# The disposable test database publishes a fixed host port. It must sit below the kernel ephemeral
# range, or an unrelated outbound connection can be assigned that exact source port and the container
# fails to bind — which quarantines a healthy candidate for reasons that have nothing to do with it.
test_db_port=$(sed -n 's/^PORT=${WATCHDOG_POSTGRES_PORT:-\([0-9]*\)}$/\1/p' "$ROOT/deploy/watchdog-test-db.sh")
[[ -n $test_db_port ]] \
  || wd_die "could not read the disposable test database port"
(( test_db_port < 32768 )) \
  || wd_die "test database port $test_db_port is inside the ephemeral range and will collide"

# The authbot produces the subscriptions the engine serves from, so it must ship from the same
# tested, immutable release rather than a hand-built scratch binary that can drift for months.
grep -Fq 'cargo build --locked --release -p authbot' "$ROOT/deploy/deploy.sh" \
  || wd_die "the release does not build the authbot"
grep -Fq '"$ENGINE_STAGE/authbot"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the authbot binary is not installed into the engine release"
grep -Fq 'staged authbot binary is missing' "$ROOT/deploy/deploy.sh" \
  || wd_die "a release without an authbot binary must fail closed"
grep -Fq 'ExecStart=/srv/claude-api/releases/current/authbot' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot unit must run the binary from the current release"
grep -Fq 'claude-authbot.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "the authbot unit is never installed"
grep -Fq '/usr/bin/systemctl restart claude-authbot.service' "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "the deploy user cannot restart the authbot"
# Restarting on every engine deploy would kill a device authorization the bot is walking a seller
# through, so the restart must stay conditional on the binary actually changing.
grep -Fq 'cmp -s "$previous/authbot" "$current/authbot"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the authbot restart must be conditional on a changed binary"

grep -Fq 'final_verify_admin_panel' "$ROOT/deploy/watchdog.sh"
# The panel check runs immediately after cutover, while the stable listener still round-robins the
# retiring slot. It must require a streak of current answers rather than accepting the first one,
# and its window must stay well above Caddy's 2s active-health convergence: a one-second window
# quarantined a correct promotion on 2026-07-25.
grep -Fq 'streak >= 3' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the admin-panel check must require consecutive current answers, not a single one"
grep -Fq 'for _ in $(seq 1 20); do' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the admin-panel convergence window must outlast blue-green cutover and health checks"
grep -Fq 'monitoring-config.test.sh' "$ROOT/deploy/watchdog.sh"
grep -Fq 'TEST_SALES_DATABASE_URL=' "$ROOT/deploy/watchdog.sh"
grep -Fq 'CANDIDATE_RETENTION_SECONDS=$((24 * 60 * 60))' "$ROOT/deploy/watchdog.sh"
grep -Fq 'prune_expired_candidates' "$ROOT/deploy/watchdog.sh"

# Retention, retry, and post-admission recovery must stay wired into the watchdog itself.
grep -Fq 'prune_expired_releases "$ENGINE_RELEASE_ROOT" engine' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'engine release retention is not wired into the watchdog cycle'
grep -Fq 'prune_expired_releases "$COMMERCE_RELEASE_ROOT" commerce' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'commerce release retention is not wired into the watchdog cycle'
grep -Fq 'prune_expired_dumps' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'pre-deploy dump retention is not wired into the watchdog cycle'
grep -Fq 'wd_retry 3 5 git -C "$SOURCE_REPO" fetch' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the GitHub fetch is not retried before failing a cycle'
grep -Fq 'rollback_engine' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'engine post-admission rollback is not wired into the watchdog'
grep -Fq 'rollback_backend' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'backend post-admission rollback is not wired into the watchdog'
# Rollback recovery requires the controller to be installed alongside the blue-green scripts.
grep -Fq 'controller/rollback.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the rollback controller is not installed for automatic recovery'
grep -Fq 'watchdog-retention.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the dump retention helper is not installed'

# A pre-candidate failure must never quarantine a commit: no SHA has been evaluated at that point.
grep -Fq 'no commit was evaluated' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'pre-candidate failures are not separated from candidate quarantine'

# `wd_die` terminates with `exit`, which does not run an ERR trap. Without EXIT in the trap list,
# every wd_die validation failure would fail closed but silently: no quarantine, no red status.
grep -Eq '^trap fail ERR EXIT INT TERM$' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the watchdog failure handler must be registered on EXIT so wd_die quarantines'
# ...but an EXIT trap also fires on success, so it must return early on a zero status or every
# successful cycle would report itself as a failure.
grep -Fq '(( rc == 0 ))' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the failure handler does not exempt successful exits from quarantine'

# Behavioural check of the exact handler shape, rather than trusting the greps above. Subshells must
# not inherit the trap, or the post-admission rollback path (which runs its verifier in a subshell)
# would quarantine from inside the subshell instead of recovering.
trap_fixture="$TEMP/trap-fixture.sh"
cat >"$trap_fixture" <<'FIXTURE'
#!/usr/bin/env bash
set -euo pipefail
set -E
wd_die(){ printf 'ERROR: %s\n' "$*" >&2; exit 1; }
fail() {
  local rc=$?
  trap - ERR EXIT INT TERM
  if (( rc == 0 )); then return 0; fi
  if [[ -z ${CANDIDATE_SHA:-} ]]; then printf 'PRECANDIDATE\n'; exit "$rc"; fi
  printf 'QUARANTINED\n'
  exit "$rc"
}
trap fail ERR EXIT INT TERM
case "$1" in
  success) CANDIDATE_SHA=abc; printf 'OK\n' ;;
  exit0) CANDIDATE_SHA=abc; printf 'OK\n'; exit 0 ;;
  die_precandidate) wd_die 'missing required file' ;;
  die_candidate) CANDIDATE_SHA=abc; wd_die 'candidate checkout mismatch' ;;
  command_failure) CANDIDATE_SHA=abc; false ;;
  subshell) CANDIDATE_SHA=abc
    verify(){ wd_die 'verification failed'; }
    if ! ( verify ); then printf 'RECOVERED\n'; fi
    printf 'CONTINUED\n' ;;
esac
FIXTURE
chmod +x "$trap_fixture"

trap_case_output() { "$trap_fixture" "$1" 2>/dev/null || true; }
[[ $(trap_case_output success) == OK ]] \
  || wd_die 'a successful cycle must not report a failure'
[[ $(trap_case_output exit0) == OK ]] \
  || wd_die 'a deliberate exit 0 (already processed / quarantined) must not report a failure'
[[ $(trap_case_output die_precandidate) == PRECANDIDATE ]] \
  || wd_die 'wd_die before a candidate is selected must not quarantine'
[[ $(trap_case_output die_candidate) == QUARANTINED ]] \
  || wd_die 'wd_die after a candidate is selected must quarantine it'
[[ $(trap_case_output command_failure) == QUARANTINED ]] \
  || wd_die 'a failing command must still quarantine the candidate'
# The subshell must be caught by its caller so recovery runs; it must NOT quarantine from inside.
subshell_result=$(trap_case_output subshell)
[[ $subshell_result == $'RECOVERED\nCONTINUED' ]] \
  || wd_die "a wd_die inside a condition subshell must not quarantine (got: $subshell_result)"


# Operator visibility: sales has an independent release lifecycle and must appear in status.
grep -Fq 'for entry in processed engine backend sales rejected pending-migration' \
  "$ROOT/deploy/watchdog-control.sh" || wd_die 'watchdog status does not report the sales baseline'

# The least-privilege sudo policy must exist, deny the reporting credential, and be installed by a
# validating installer rather than hand-edited.
sudoers_policy="$ROOT/deploy/sudoers.d/95-apitoken-deploy"
[[ -f $sudoers_policy ]] || wd_die 'the least-privilege sudo policy is missing'
if grep -Eq '^[^#]*NOPASSWD:[[:space:]]*ALL' "$sudoers_policy"; then
  wd_die 'the sudo policy grants unrestricted NOPASSWD:ALL'
fi
grep -Fq 'visudo -c -f' "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not validate before installing'
grep -Fq 'github-watchdog.env' "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not verify the reporting credential stays unreadable'
# The policy must permit re-running its own installer. Without this, removing the unrestricted
# grant is irreversible without console access.
grep -Fq '/usr/local/lib/apitoken-watchdog/install-sudoers.sh' "$sudoers_policy" \
  || wd_die 'the sudo policy is not self-repairable: the installer path is not permitted'
grep -Fq 'APITOKEN_POLICY' "$sudoers_policy" \
  || wd_die 'the policy self-management alias is missing'
# Operator tooling must survive the restriction.
grep -Fq '/usr/local/bin/apitoken-watchdog status' "$sudoers_policy" \
  || wd_die 'the sudo policy breaks apitoken-watchdog status'
# Every Cmnd_Alias must be referenced by a grant line, or the privilege is silently not granted.
while IFS= read -r declared_alias; do
  grep -Fq "$declared_alias" <<<"$(grep -E '^deploy ALL=' -A2 "$sudoers_policy")" \
    || wd_die "sudo policy declares unused alias $declared_alias"
done < <(grep -oE '^Cmnd_Alias [A-Z_]+' "$sudoers_policy" | awk '{print $2}')
# The installer and its policy are delivered together with the other operational definitions.
grep -Fq 'install-sudoers.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the sudoers installer is not delivered to the host'
# install-watchdog.sh must never re-add apitoken-ci to the deploy group: that would silently undo
# the isolation fix on the next infrastructure install, and the two installers would fight.
if grep -Eq 'usermod -a -G deploy apitoken-ci' "$ROOT/deploy/install-watchdog.sh"; then
  wd_die 'the watchdog installer re-adds apitoken-ci to the deploy group'
fi
grep -Fq 'gpasswd -d apitoken-ci deploy' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the watchdog installer does not enforce apitoken-ci group isolation'

grep -Fq 'sales-dsn)' "$ROOT/deploy/watchdog-test-db.sh"
grep -Fq 'require_retired_vhost panel.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq 'require_admin_auth_vhost content-studio.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq 'require_admin_auth_vhost monitoring.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq "''|000|404|421" "$ROOT/deploy/watchdog.sh"
grep -Fq 'data-admin-panel-version="9"' "$ROOT/crates/server/src/admin-panel.html"
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

printf 'watchdog retention, migration, and engine topology tests passed\n'
