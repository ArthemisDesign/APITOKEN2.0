#!/usr/bin/env bash

# Pure helpers shared by the automatic deploy watchdog and its operator tools.
# This file must remain safe to source: it performs no work at load time.

wd_log() {
  printf '[watchdog] %s\n' "$*"
}

wd_warn() {
  printf '[watchdog] WARNING: %s\n' "$*" >&2
}

wd_die() {
  printf '[watchdog] ERROR: %s\n' "$*" >&2
  exit 1
}

# Produce a GitHub-safe, bounded diagnostic from a private validator transcript. Only known
# failure markers are eligible; URLs/DSNs and common secret assignments are redacted defensively.
wd_validation_failure_summary() {
  [[ $# -eq 3 ]] || wd_die "validation failure summary requires log path, exit code, and phase"
  local log_file=$1 rc=$2 phase=$3 detail=''
  if [[ -f $log_file && ! -L $log_file ]]; then
    detail=$(grep -E '\[watchdog\] ERROR:|ERR_PNPM|ELIFECYCLE|test lane\(s\) failed|migration failed|candidate lane failed' \
      "$log_file" | tail -n 1 || true)
    detail=$(printf '%s' "$detail" | sed -E \
      $'s/\x1b\[[0-9;]*[[:alpha:]]//g; s#[A-Za-z][A-Za-z0-9+.-]*://[^[:space:]]+#URL_REDACTED#g; s/([A-Z_]*(TOKEN|KEY|SECRET|PASSWORD)[A-Z_]*)=[^[:space:]]+/\1=REDACTED/g; s/[[:cntrl:]]/ /g')
  fi
  [[ -n $detail ]] || detail="validator exited with code $rc"
  printf 'phase=%s; %.100s' "$phase" "$detail"
}

wd_validate_sha() {
  [[ ${1:-} =~ ^[0-9a-f]{40}$ ]] || wd_die "expected a full 40-character lowercase commit SHA"
}

# Run a command with bounded retries and linear backoff. Intended for transient network work such
# as the GitHub fetch, where a single DNS/TLS blip must not look like a code failure. The final
# attempt's exit status is returned unchanged so the caller can still fail closed.
wd_retry() {
  [[ $# -ge 3 ]] || wd_die "retry requires attempts, backoff seconds, and a command"
  local attempts=$1 backoff=$2 attempt=1 rc=0
  shift 2
  [[ $attempts =~ ^[1-9][0-9]*$ ]] || wd_die "retry attempts must be a positive integer"
  [[ $backoff =~ ^[0-9]+$ ]] || wd_die "retry backoff must be a non-negative integer"
  while :; do
    rc=0
    "$@" || rc=$?
    (( rc != 0 )) || return 0
    (( attempt < attempts )) || return "$rc"
    wd_warn "attempt $attempt of $attempts failed (exit $rc); retrying in $((backoff * attempt))s"
    sleep "$((backoff * attempt))"
    attempt=$((attempt + 1))
  done
}

# Print NUL-delimited immutable release directories that are safe to remove, newest-first retention.
# A release is protected when it is `current`/`previous`, within the newest `keep` by mtime, or
# named in the protected list (used for live PID-resolved releases and recorded component SHAs).
# Selection is deliberately pure; the caller performs the privileged deletion under the deploy lock.
wd_prunable_release_dirs() {
  [[ $# -ge 2 ]] || wd_die "release retention selection requires a release root and keep count"
  local root=$1 keep=$2 protected=() name link resolved canonical_root candidate kept=0
  shift 2
  protected=("$@")
  [[ $root == /* && -d $root && ! -L $root ]] \
    || wd_die "release root must be an absolute, non-symlink directory: $root"
  [[ $keep =~ ^[0-9]+$ ]] || wd_die "release retention keep count must be a non-negative integer"

  # Compare resolved link targets against the resolved root: a release root reached through a
  # symlinked parent must still recognise its own current/previous targets as in-root.
  canonical_root=$(readlink -f -- "$root")

  for link in current previous; do
    if [[ -L $root/$link ]]; then
      resolved=$(readlink -f -- "$root/$link" 2>/dev/null) || continue
      [[ ${resolved%/*} == "$canonical_root" ]] || continue
      protected+=("${resolved##*/}")
    fi
  done

  # Newest first by directory mtime, so `keep` always retains the most recent releases.
  while IFS= read -r candidate; do
    name=${candidate##*/}
    [[ $name =~ ^[0-9a-f]{40}$ ]] || continue
    [[ -d $candidate && ! -L $candidate ]] || continue
    if wd_list_contains "$name" "${protected[@]}"; then
      continue
    fi
    if (( kept < keep )); then
      kept=$((kept + 1))
      continue
    fi
    printf '%s\0' "$candidate"
  done < <(wd_dirs_newest_first "$root")
}

wd_list_contains() {
  local needle=$1 item
  shift
  for item in "$@"; do
    [[ $item == "$needle" ]] || continue
    return 0
  done
  return 1
}

wd_dirs_newest_first() {
  local root=$1 path modified
  while IFS= read -r -d '' path; do
    [[ -d $path && ! -L $path ]] || continue
    modified=$(wd_path_mtime_epoch "$path" 2>/dev/null) || continue
    [[ $modified =~ ^[0-9]+$ ]] || continue
    printf '%s\t%s\n' "$modified" "$path"
  done < <(find "$root" -mindepth 1 -maxdepth 1 -type d -print0) \
    | sort -rn -k1,1 | cut -f2-
}

# Print NUL-delimited pre-deploy dump files older than the newest `keep` per database. The hourly
# `<database>.dump` rotation artifacts are never selected: only the `<database>.pre-deploy-<sha>.dump`
# snapshots that accumulate once per deployment.
wd_prunable_predeploy_dumps() {
  [[ $# -eq 2 ]] || wd_die "dump retention selection requires a backup root and keep count"
  local root=$1 keep=$2 database dump name kept modified
  [[ $root == /* && -d $root && ! -L $root ]] \
    || wd_die "backup root must be an absolute, non-symlink directory: $root"
  [[ $keep =~ ^[0-9]+$ ]] || wd_die "dump retention keep count must be a non-negative integer"

  for database in commerce claude_engine sales apitoken_crm; do
    kept=0
    while IFS= read -r dump; do
      name=${dump##*/}
      [[ $name == "$database.pre-deploy-"*.dump ]] || continue
      [[ -f $dump && ! -L $dump ]] || continue
      if (( kept < keep )); then
        kept=$((kept + 1))
        continue
      fi
      printf '%s\0' "$dump"
    done < <(
      for dump in "$root/$database.pre-deploy-"*.dump; do
        [[ -f $dump && ! -L $dump ]] || continue
        modified=$(wd_path_mtime_epoch "$dump" 2>/dev/null) || continue
        [[ $modified =~ ^[0-9]+$ ]] || continue
        printf '%s\t%s\n' "$modified" "$dump"
      done | sort -rn -k1,1 | cut -f2-
    )
  done
}

wd_path_mtime_epoch() {
  local path=$1
  if stat -c '%Y' -- "$path" >/dev/null 2>&1; then
    stat -c '%Y' -- "$path"
  else
    stat -f '%m' -- "$path"
  fi
}

# Print NUL-delimited direct child directories that are safe watchdog candidates and whose
# directory mtime is strictly older than the supplied epoch. Selection is deliberately pure;
# watchdog.sh performs the privileged deletion only while holding the global watchdog lock.
wd_candidate_dirs_older_than() {
  [[ $# -eq 3 ]] || wd_die "candidate retention selection requires candidate/marker roots and a cutoff epoch"
  local root=$1 marker_root=$2 cutoff=$3 candidate name marker age_path modified
  [[ $root == /* && -d $root && ! -L $root ]] \
    || wd_die "candidate root must be an absolute, non-symlink directory: $root"
  [[ $marker_root == /* && -d $marker_root && ! -L $marker_root ]] \
    || wd_die "candidate marker root must be an absolute, non-symlink directory: $marker_root"
  [[ $cutoff =~ ^[0-9]+$ ]] || wd_die "candidate retention cutoff must be an epoch integer"

  while IFS= read -r -d '' candidate; do
    [[ -d $candidate && ! -L $candidate ]] || continue
    name=${candidate##*/}
    [[ $name =~ ^[0-9a-f]{40}$ ]] || continue
    marker="$marker_root/$name.tested"
    age_path=$candidate
    # Successful candidates receive their marker only after every selected validation lane. Give those
    # builds a full 24 hours from test completion; interrupted/failed builds fall back to the
    # workspace directory mtime.
    if [[ -f $marker && ! -L $marker ]]; then
      age_path=$marker
    fi
    if ! modified=$(wd_path_mtime_epoch "$age_path"); then
      wd_warn "cannot read candidate mtime; retention skipped $candidate"
      continue
    fi
    [[ $modified =~ ^[0-9]+$ ]] || {
      wd_warn "candidate has an invalid mtime; retention skipped $candidate"
      continue
    }
    if (( modified < cutoff )); then
      printf '%s\0' "$candidate"
    fi
  done < <(find "$root" -mindepth 1 -maxdepth 1 -type d -print0)
}

# A PostgreSQL-backed engine deployment has exactly one steady-state writer. Both slots may be
# ready briefly during a controlled cutover, but that overlap must never survive the controller.
# Arguments are active, ready, current-release, enabled for 8787 and 8788, then legacy active/enabled.
wd_engine_topology_is_steady() {
  [[ $# -eq 10 ]] || wd_die "engine topology check requires ten boolean values"
  local value
  for value in "$@"; do
    [[ $value == 0 || $value == 1 ]] || wd_die "engine topology values must be 0 or 1"
  done

  local slot_8787="$1:$2:$3:$4" slot_8788="$5:$6:$7:$8"
  local legacy_active=$9 legacy_enabled=${10}
  [[ $legacy_active == 0 && $legacy_enabled == 0 ]] || return 1
  [[ $slot_8787 == "1:1:1:1" && $slot_8788 == "0:0:0:0" ]] \
    || [[ $slot_8787 == "0:0:0:0" && $slot_8788 == "1:1:1:1" ]]
}

wd_read_line() {
  local path=$1
  [[ -f $path && ! -L $path ]] || return 1
  IFS= read -r REPLY <"$path" || return 1
  [[ -n $REPLY ]]
}

wd_read_sha() {
  local path=$1
  wd_read_line "$path" || return 1
  [[ $REPLY =~ ^[0-9a-f]{40}$ ]] || wd_die "invalid SHA state in $path"
  printf '%s\n' "$REPLY"
}

wd_atomic_write() {
  local path=$1 value=$2 mode=${3:-0640} temporary
  [[ $path == /* ]] || wd_die "state path must be absolute: $path"
  # Уникальность временного файла даёт mktemp, а не PID: `$$` одинаков во всех
  # асинхронных подоболочках, а BASHPID появился только в bash 4 — на macOS (bash 3.2)
  # параллельные лейны получали один и тот же путь и затирали друг друга.
  temporary=$(mktemp "${path}.tmp.XXXXXX") || wd_die "cannot create temporary state file for $path"
  printf '%s\n' "$value" >"$temporary"
  chmod "$mode" "$temporary"
  mv -f -- "$temporary" "$path"
}

wd_sha256_file() {
  local path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$path" | awk '{print $1}'
  else
    shasum -a 256 -- "$path" | awk '{print $1}'
  fi
}

wd_sha256_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

wd_commerce_release_bundle_digest() {
  local candidate=$1
  local digest_script=$candidate/deploy/release-tree-digest.mjs
  local bundle=$candidate/.deploy-artifacts/commerce-release
  [[ -f $digest_script && ! -L $digest_script ]] \
    || wd_die "commerce release digest helper is missing or unsafe"
  [[ -d $bundle && ! -L $bundle ]] \
    || wd_die "commerce release bundle is missing or unsafe"
  node "$digest_script" "$bundle"
}

wd_content_studio_runtime_directory() {
  local release=$1
  local app=$release/apps/content-studio
  local standalone=$app/.next/standalone/apps/content-studio
  if [[ -f $standalone/server.js && ! -L $standalone/server.js ]]; then
    printf '%s\n' "$standalone"
  else
    printf '%s\n' "$app"
  fi
}

# Hash the mandatory runtime entrypoints for one independently deployable TypeScript context. The
# candidate tree is frozen after validation; these digests prove each release promoter consumes the
# exact component build that passed the gate, without requiring unrelated component artifacts.
wd_typescript_component_artifact_digest() {
  local tree=$1 component=$2 relative
  local artifacts=()
  case "$component" in
    commerce)
      artifacts=(
        apps/api/dist/main.js
        apps/worker/dist/main.js
        apps/content-studio/.next/BUILD_ID
        packages/db/dist/migrate.js
      )
      ;;
    sales)
      artifacts=(
        apps/sales-api/dist/main.js
        apps/sales-web/.next/BUILD_ID
        packages/sales-db/dist/migrate.js
      )
      ;;
    openkeys)
      artifacts=(
        apps/openkeys/.next/BUILD_ID
        packages/openkeys-db/dist/migrate.js
      )
      ;;
    web)
      artifacts=(apps/web/.next/BUILD_ID)
      ;;
    *) wd_die "unknown TypeScript artifact component: $component" ;;
  esac
  for relative in "${artifacts[@]}"; do
    [[ -f $tree/$relative && ! -L $tree/$relative ]] \
      || wd_die "tested TypeScript artifact is missing or unsafe: $tree/$relative"
    printf '%s  %s\n' "$(wd_sha256_file "$tree/$relative")" "$relative"
  done | wd_sha256_stdin
}

# Legacy all-host-components digest. Keep its byte-for-byte definition while old tested markers and
# an older release promoter may still exist during the staged controller upgrade.
wd_typescript_artifact_digest() {
  local tree=$1 relative
  local artifacts=(
    apps/api/dist/main.js
    apps/worker/dist/main.js
    apps/content-studio/.next/BUILD_ID
    apps/sales-api/dist/main.js
    apps/sales-web/.next/BUILD_ID
    apps/openkeys/.next/BUILD_ID
    packages/db/dist/migrate.js
    packages/sales-db/dist/migrate.js
    packages/openkeys-db/dist/migrate.js
  )
  for relative in "${artifacts[@]}"; do
    [[ -f $tree/$relative && ! -L $tree/$relative ]] \
      || wd_die "tested TypeScript artifact is missing or unsafe: $tree/$relative"
    printf '%s  %s\n' "$(wd_sha256_file "$tree/$relative")" "$relative"
  done | wd_sha256_stdin
}

wd_typescript_component_list_contains() {
  local components=$1 expected=$2 component IFS=,
  [[ $expected == commerce || $expected == sales || $expected == openkeys || $expected == web ]] \
    || wd_die "unknown TypeScript component: $expected"
  for component in $components; do
    [[ $component == "$expected" ]] && return 0
  done
  return 1
}

wd_typescript_component_list_is_canonical() {
  case "$1" in
    commerce|sales|openkeys|web|\
    commerce,sales|commerce,openkeys|commerce,web|sales,openkeys|sales,web|openkeys,web|\
    commerce,sales,openkeys|commerce,sales,web|commerce,openkeys,web|sales,openkeys,web|\
    commerce,sales,openkeys,web)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_migration_manifest() {
  local tree=$1
  [[ -d $tree/packages/db/migrations ]] || wd_die "migration directory is missing from $tree"

  # Drizzle rewrites meta/_journal.json whenever it appends a migration. Hashing that file as one
  # immutable artifact would therefore reject every legitimate migration after the baseline. Build
  # a semantic manifest instead: existing SQL/snapshots remain byte-for-byte immutable, while each
  # canonical journal entry is an individually immutable, ordered record and new records may append.
  node - "$tree" <<'NODE'
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const tree = process.argv[2];
const root = path.join(tree, "packages/db/migrations");
const journalPath = path.join(root, "meta/_journal.json");

function fail(message) {
  process.stderr.write(`[watchdog] ERROR: invalid Drizzle migration history: ${message}\n`);
  process.exit(1);
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function walk(directory, prefix = "") {
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, "en"))) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    const absolute = path.join(directory, entry.name);
    if (!/^[A-Za-z0-9._/-]+$/.test(relative)) fail(`unsafe artifact path ${JSON.stringify(relative)}`);
    if (entry.isSymbolicLink()) fail(`symlink is forbidden: ${relative}`);
    if (entry.isDirectory()) files.push(...walk(absolute, relative));
    else if (entry.isFile()) files.push(relative);
    else fail(`non-regular artifact is forbidden: ${relative}`);
  }
  return files;
}

let journal;
try {
  journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
} catch (error) {
  fail(`cannot read meta/_journal.json (${error instanceof Error ? error.message : "unknown error"})`);
}
if (journal === null || typeof journal !== "object" || Array.isArray(journal)) fail("journal must be an object");
if (typeof journal.version !== "string" || journal.dialect !== "postgresql" || !Array.isArray(journal.entries)) {
  fail("journal header or entries are invalid");
}

const artifacts = walk(root);
const sqlFiles = new Set(artifacts.filter((file) => file.endsWith(".sql")));
const tags = new Set();
let previousWhen = -1;

process.stdout.write("format=apitoken-drizzle-manifest-v2\n");
process.stdout.write(`journal=${digest(canonical({ version: journal.version, dialect: journal.dialect }))}\n`);
for (let position = 0; position < journal.entries.length; position += 1) {
  const entry = journal.entries[position];
  if (entry === null || typeof entry !== "object" || Array.isArray(entry)) fail(`entry ${position} must be an object`);
  if (entry.idx !== position) fail(`entry ${position} has non-contiguous idx ${JSON.stringify(entry.idx)}`);
  if (!Number.isSafeInteger(entry.when) || entry.when <= previousWhen) fail(`entry ${position} has a non-monotonic timestamp`);
  if (typeof entry.tag !== "string" || !/^[A-Za-z0-9._-]+$/.test(entry.tag)) fail(`entry ${position} has an unsafe tag`);
  if (tags.has(entry.tag)) fail(`duplicate journal tag ${entry.tag}`);
  const sqlFile = `${entry.tag}.sql`;
  if (!sqlFiles.delete(sqlFile)) fail(`journal entry ${entry.tag} has no unique SQL artifact`);
  tags.add(entry.tag);
  previousWhen = entry.when;
  process.stdout.write(`entry=${String(position).padStart(8, "0")} ${digest(canonical(entry))} ${entry.tag}\n`);
}
if (sqlFiles.size !== 0) fail(`unjournaled SQL artifact(s): ${[...sqlFiles].sort().join(", ")}`);

for (const relative of artifacts.filter((file) => file !== "meta/_journal.json").sort()) {
  const contents = fs.readFileSync(path.join(root, relative));
  process.stdout.write(`file=${digest(contents)} packages/db/migrations/${relative}\n`);
}
NODE
}

wd_manifest_digest() {
  local manifest=$1
  [[ -f $manifest && ! -L $manifest ]] || wd_die "migration manifest is missing: $manifest"
  wd_sha256_stdin <"$manifest"
}

# Every previously applied migration artifact must still exist byte-for-byte. New files may only
# be appended. This rejects edited/deleted migration history before any production DB command runs.
wd_manifest_is_append_only() {
  local applied=$1 candidate=$2 line
  [[ -f $applied && -f $candidate ]] || return 1
  while IFS= read -r line; do
    grep -Fqx -- "$line" "$candidate" || return 1
  done <"$applied"
}

wd_marker_value() {
  local marker=$1 key=$2 line value found=0
  [[ -f $marker && ! -L $marker ]] || return 1
  while IFS= read -r line; do
    case "$line" in
      "$key="*)
        (( found == 0 )) || wd_die "duplicate $key in marker $marker"
        value=${line#*=}
        found=1
        ;;
    esac
  done <"$marker"
  (( found == 1 )) || return 1
  printf '%s\n' "$value"
}

wd_range_files() {
  local repo=$1 base=$2 target=$3
  if [[ -z $base || $base == "$target" ]]; then
    return 0
  fi
  # Disable rename collapsing so a move is classified as both a deletion from its old component
  # and an addition to its new one. Deletions must also trigger the lane that owned the removed path.
  git -C "$repo" diff --name-only --no-renames --diff-filter=ACDMRTUXB "$base..$target"
}

# Infrastructure scopes are canonical, ordered sets. Keeping one representation lets the
# unprivileged controller, fixed root bridge, final verifier, and regression tests agree on the
# exact transaction without accepting duplicate, reordered, or unknown scope names.
wd_infrastructure_scope_is_valid() {
  local scope=$1 token rank previous=0
  local tokens=()
  case "$scope" in
    none|full) return 0 ;;
    '') return 1 ;;
  esac
  IFS=+ read -r -a tokens <<<"$scope"
  for token in "${tokens[@]}"; do
    case "$token" in
      controller) rank=1 ;;
      caddy) rank=2 ;;
      systemd) rank=3 ;;
      monitoring) rank=4 ;;
      *) return 1 ;;
    esac
    (( rank > previous )) || return 1
    previous=$rank
  done
}

wd_infrastructure_scope_has() {
  local scope=$1 component=$2
  wd_infrastructure_scope_is_valid "$scope" || return 2
  case "$component" in
    controller|caddy|systemd|monitoring) ;;
    *) return 2 ;;
  esac
  [[ $scope == full ]] && return 0
  [[ "+$scope+" == *"+$component+"* ]]
}

# Return the canonical set of cross-component production checks required after the selected
# rollout. Component controllers already verify their own exact release before returning, so an
# unrelated deployment must not pay for every engine/Caddy smoke again. Systemd and risky full
# infrastructure changes retain the complete fence; monitoring-only checks only monitoring, while
# controller-only and documentation deliveries need no serving-runtime check at all.
wd_final_verification_plan() {
  local infrastructure_scope=$1 engine_changed=$2 backend_changed=$3
  local sales_changed=$4 openkeys_changed=$5
  local checks=()
  local broad_infrastructure=0 caddy_infrastructure=0 monitoring_infrastructure=0
  wd_infrastructure_scope_is_valid "$infrastructure_scope" || return 2
  local flag
  for flag in "$engine_changed" "$backend_changed" "$sales_changed" "$openkeys_changed"; do
    [[ $flag == 0 || $flag == 1 ]] || return 2
  done

  if [[ $infrastructure_scope == full ]] \
      || wd_infrastructure_scope_has "$infrastructure_scope" systemd; then
    broad_infrastructure=1
  fi
  wd_infrastructure_scope_has "$infrastructure_scope" caddy \
    && caddy_infrastructure=1
  wd_infrastructure_scope_has "$infrastructure_scope" monitoring \
    && monitoring_infrastructure=1

  if (( engine_changed == 1 || broad_infrastructure == 1 )); then
    checks+=(runtime panel)
  fi
  if (( caddy_infrastructure == 1 || broad_infrastructure == 1 )); then
    checks+=(routing)
  fi
  if (( engine_changed == 1 || backend_changed == 1 || sales_changed == 1 \
        || openkeys_changed == 1 || caddy_infrastructure == 1 \
        || monitoring_infrastructure == 1 || broad_infrastructure == 1 )); then
    checks+=(monitoring)
  fi
  if (( engine_changed == 1 || caddy_infrastructure == 1 \
        || broad_infrastructure == 1 )); then
    checks+=(codex)
  fi

  if (( ${#checks[@]} == 0 )); then
    printf 'none\n'
  else
    local IFS=,
    printf '%s\n' "${checks[*]}"
  fi
}

wd_verification_plan_has() {
  local verification_plan=$1 check=$2
  [[ ",$verification_plan," == *",$check,"* ]]
}

wd_path_is_engine() {
  case "$1" in
    crates/*|vendor/*|Cargo.toml|Cargo.lock|config.env.example|server.env.example|schema/*|tests/*|tools/refresh-fingerprint.sh|tools/codex-app-server/*)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_codex_tooling() {
  case "$1" in
    tools/codex-app-server/*)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# All pnpm workspace source and its shared build inputs belong to the TypeScript validation lane,
# including Vercel-only surfaces that do not map to a host deployment component.
wd_path_is_typescript() {
  case "$1" in
    apps/*|packages/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig*.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_web() {
  case "$1" in
    apps/web/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_backend() {
  case "$1" in
    # These database packages belong to independent bounded contexts. They still run in the
    # workspace-wide TypeScript validation lane, but must not roll the commerce backend.
    packages/sales-db/*|packages/openkeys-db/*)
      return 1
      ;;
    apps/api/*|apps/worker/*|apps/content-studio/*|packages/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# Партнёрский bounded context (partners.apitoken.sale). Отдельный жизненный цикл релизов
# (/opt/apitoken/sales-releases), НЕ на общем commerce-current. Shared build-файлы включены,
# чтобы бамп зависимостей тоже пере-собирал sales-релиз.
wd_path_is_sales() {
  case "$1" in
    apps/sales-api/*|apps/sales-web/*|packages/sales-db/*|packages/contracts/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# OpenKeys bounded context (openkeys.apitoken.sale). Свой релизный корень
# (/opt/apitoken/openkeys-releases) и своя БД, независимо от commerce и sales.
wd_path_is_openkeys() {
  case "$1" in
    apps/openkeys/*|packages/openkeys-db/*|packages/engine-client/*|packages/contracts/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_infrastructure() {
  case "$1" in
    deploy/*|systemd/*|observability/*|compose.yaml)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# Deployment tests and the contributor-side merge workflow must exercise the operational
# regression lane, but changing them cannot alter the installed production controller. New deploy
# files fail safe into installation until they are explicitly proven local-only here.
wd_path_requires_infrastructure_install() {
  case "$1" in
    deploy/*.md|deploy/*.test.sh|deploy/agent-merge.sh|deploy/agent-merge.suite.sh|\
    deploy/test-stage2-e2e.sh|deploy/sccache-cargo.sh|deploy/next-cache.sh|\
    deploy/typescript-scope.mjs|deploy/typescript-build-contexts.sh|\
    deploy/typescript-test-groups.sh|compose.yaml)
      return 1
      ;;
    deploy/*|systemd/*|observability/*|compose.yaml)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_caddy() {
  case "$1" in
    deploy/Caddyfile|deploy/install-caddy.sh|deploy/render-caddy.awk)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_systemd_definition() {
  case "$1" in
    systemd/apitoken-api@.service|\
    systemd/apitoken-deploy-watchdog.service|systemd/apitoken-deploy-watchdog.timer|\
    systemd/apitoken-candidate-validator.service|systemd/apitoken-candidate-validator.timer|\
    systemd/apitoken-sudoers-install.service|\
    systemd/apitoken-postgres.service|systemd/apitoken-affinity-redis.service|\
    systemd/apitoken-worker.service|systemd/apitoken-content-studio.service|\
    systemd/claude-api@.service|systemd/claude-api-backup.service|\
    systemd/claude-api-backup.timer|systemd/claude-api-fingerprint.service|\
    systemd/claude-api-fingerprint.timer|systemd/apitoken-sales-api.service|\
    systemd/apitoken-sales-web.service|systemd/claude-authbot.service|\
    systemd/apitoken-openkeys.service|systemd/apitoken-monitoring-collector.service|\
    systemd/apitoken-monitoring-collector.timer|systemd/journald-apitoken.conf)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_monitoring_definition() {
  case "$1" in
    observability/*|deploy/install-monitoring.sh|deploy/render-alertmanager.mjs|\
    deploy/collect-monitoring-metrics.sh)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# These files are copied into fixed, root-owned controller locations but do not define services,
# privileges, monitoring, secrets, or stateful infrastructure. Updating only this allowlist can
# therefore use the small controller installer. Unknown deploy paths deliberately fall through to
# the full installer.
wd_path_is_controller_definition() {
  case "$1" in
    deploy/watchdog.sh|deploy/watchdog-lib.sh|deploy/validation-plan.sh|\
    deploy/watchdog-test-db.sh|\
    deploy/watchdog-backup.sh|deploy/watchdog-migrate.sh|deploy/watchdog-infrastructure.sh|\
    deploy/watchdog-retention.sh|deploy/watchdog-codex-promote.sh|\
    deploy/watchdog-github.sh|deploy/watchdog-control.sh|\
    deploy/deploy.sh|deploy/lib.sh|deploy/commerce-release-bundle.sh|\
    deploy/release-tree-digest.mjs|deploy/content-studio-start.sh|\
    deploy/api-bluegreen.sh|deploy/engine-bluegreen.sh|\
    deploy/rollback.sh|deploy/sales-deploy.sh|deploy/openkeys-deploy.sh)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# Return the least expensive safe root-install transaction for an exact commit range. Independent
# narrow concerns compose in canonical order, for example `controller+caddy+monitoring`. Privileged
# or stateful definitions, unknown deployment files, and any deletion still select `full`.
#
# Deletions fail closed because a narrow copy-only transaction cannot remove a retired installed
# file safely. Rename detection stays disabled so a move includes that deletion.
wd_infrastructure_install_scope() {
  local repo=$1 base=$2 target=$3 entries status path
  local controller=0 caddy=0 systemd=0 monitoring=0
  local scopes=()
  [[ -n $base && $base != "$target" ]] || { printf 'none\n'; return 0; }
  entries=$(git -C "$repo" diff --name-status --no-renames --diff-filter=ACDMRTUXB \
    "$base..$target") || return 1
  while IFS=$'\t' read -r status path; do
    [[ -n $path ]] || continue
    wd_path_requires_infrastructure_install "$path" || continue
    if [[ $status == D* ]]; then
      printf 'full\n'
      return 0
    fi
    if wd_path_is_controller_definition "$path"; then
      controller=1
    elif wd_path_is_caddy "$path"; then
      caddy=1
    elif wd_path_is_systemd_definition "$path"; then
      systemd=1
    elif wd_path_is_monitoring_definition "$path"; then
      monitoring=1
    else
      printf 'full\n'
      return 0
    fi
  done <<<"$entries"
  (( controller == 0 )) || scopes+=(controller)
  (( caddy == 0 )) || scopes+=(caddy)
  (( systemd == 0 )) || scopes+=(systemd)
  (( monitoring == 0 )) || scopes+=(monitoring)
  if (( ${#scopes[@]} == 0 )); then
    printf 'none\n'
  else
    local IFS=+
    printf '%s\n' "${scopes[*]}"
  fi
}

wd_path_is_merge_workflow() {
  case "$1" in
    .claude/*|AGENTS.md|CLAUDE.md|BRANCHES.md|CONTRIBUTING.md)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_validation_neutral() {
  case "$1" in
    *.md|docs/*|.github/*|.gitignore|.gitattributes|LICENSE|LICENSE.*)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_range_has_class() {
  local repo=$1 base=$2 target=$3 classifier=$4 path
  while IFS= read -r path; do
    [[ -n $path ]] || continue
    if "$classifier" "$path"; then
      return 0
    fi
  done < <(wd_range_files "$repo" "$base" "$target")
  return 1
}

# Unknown paths fail safe into every expensive lane. This lets documentation-only commits stay fast
# without making a newly added code area silently untested.
wd_range_has_unknown_validation_path() {
  local repo=$1 base=$2 target=$3 path
  while IFS= read -r path; do
    [[ -n $path ]] || continue
    if wd_path_is_typescript "$path" \
      || wd_path_is_engine "$path" \
      || wd_path_is_infrastructure "$path" \
      || wd_path_is_merge_workflow "$path" \
      || wd_path_is_validation_neutral "$path"; then
      continue
    fi
    return 0
  done < <(wd_range_files "$repo" "$base" "$target")
  return 1
}

wd_range_changes_typescript_gate() {
  local repo=$1 base=$2 target=$3 path
  while IFS= read -r path; do
    case "$path" in
      deploy/typescript-scope.mjs|deploy/next-cache.sh|deploy/typescript-build-contexts.sh|\
      deploy/typescript-test-groups.sh|deploy/commerce-release-bundle.sh|\
      deploy/release-tree-digest.mjs)
        return 0
        ;;
    esac
  done < <(wd_range_files "$repo" "$base" "$target")
  return 1
}

# Shared compiler/package-manager inputs and deletions cannot be represented by a current-package
# closure. Force the complete workspace for those ranges; additions and edits inside a current
# package are resolved more narrowly by deploy/typescript-scope.mjs.
wd_range_requires_full_typescript_scope() {
  local repo=$1 base=$2 target=$3 path
  while IFS= read -r path; do
    case "$path" in
      package.json|pnpm-lock.yaml|pnpm-workspace.yaml|.node-version|tsconfig*.json|\
      deploy/typescript-scope.mjs|deploy/next-cache.sh|deploy/typescript-build-contexts.sh|\
      deploy/typescript-test-groups.sh|deploy/commerce-release-bundle.sh|\
      deploy/release-tree-digest.mjs)
        return 0
        ;;
    esac
  done < <(wd_range_files "$repo" "$base" "$target")
  while IFS= read -r path; do
    case "$path" in
      apps/*|packages/*) return 0 ;;
    esac
  done < <(git -C "$repo" diff --name-only --no-renames --diff-filter=D "$base..$target")
  return 1
}

# Print the canonical comma-separated runtime contexts whose complete artifacts are required for an
# exact TypeScript validation range. A full/unknown scope fails closed to every context. Component
# baselines may be older than the processed SHA, so derive this from the same exact validation range
# rather than only from the newest commit's changed paths.
wd_typescript_components_for_range() {
  local repo=$1 base=$2 target=$3 force_full=$4
  local components=()
  [[ $force_full == 0 || $force_full == 1 ]] \
    || wd_die "TypeScript full-scope flag must be 0 or 1"
  if (( force_full == 1 )); then
    printf 'commerce,sales,openkeys,web\n'
    return 0
  fi
  wd_range_has_class "$repo" "$base" "$target" wd_path_is_backend \
    && components+=(commerce)
  wd_range_has_class "$repo" "$base" "$target" wd_path_is_sales \
    && components+=(sales)
  wd_range_has_class "$repo" "$base" "$target" wd_path_is_openkeys \
    && components+=(openkeys)
  wd_range_has_class "$repo" "$base" "$target" wd_path_is_web \
    && components+=(web)
  if (( ${#components[@]} == 0 )); then
    # A TypeScript lane with no known owner can be a newly introduced workspace surface. Building
    # everything is the only safe default until its bounded context is explicitly classified.
    printf 'commerce,sales,openkeys,web\n'
    return 0
  fi
  local joined IFS=,
  joined="${components[*]}"
  printf '%s\n' "$joined"
}

wd_require_ancestor() {
  local repo=$1 base=$2 target=$3 label=$4
  [[ -n $base ]] || wd_die "$label baseline is not initialized"
  git -C "$repo" cat-file -e "$base^{commit}" 2>/dev/null \
    || wd_die "$label baseline $base is unavailable in the source repository"
  git -C "$repo" merge-base --is-ancestor "$base" "$target" \
    || wd_die "$label baseline $base is not an ancestor of candidate $target; automatic history rewrites are forbidden"
}
