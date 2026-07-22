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

wd_validate_sha() {
  [[ ${1:-} =~ ^[0-9a-f]{40}$ ]] || wd_die "expected a full 40-character lowercase commit SHA"
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
    # Successful candidates receive their marker only after the complete test gate. Give those
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
  local path=$1 value=$2 temporary
  temporary="${path}.tmp.$$"
  [[ $path == /* ]] || wd_die "state path must be absolute: $path"
  printf '%s\n' "$value" >"$temporary"
  chmod 0640 "$temporary"
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
  git -C "$repo" diff --name-only --diff-filter=ACMRTUXB "$base..$target"
}

wd_path_is_engine() {
  case "$1" in
    crates/*|vendor/*|Cargo.toml|Cargo.lock|config.env.example|server.env.example|schema/*|tests/*|tools/refresh-fingerprint.sh)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_backend() {
  case "$1" in
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

wd_path_is_infrastructure() {
  case "$1" in
    deploy/*|systemd/*|observability/*|compose.yaml)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_caddy() {
  [[ $1 == deploy/Caddyfile ]]
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

wd_require_ancestor() {
  local repo=$1 base=$2 target=$3 label=$4
  [[ -n $base ]] || wd_die "$label baseline is not initialized"
  git -C "$repo" cat-file -e "$base^{commit}" 2>/dev/null \
    || wd_die "$label baseline $base is unavailable in the source repository"
  git -C "$repo" merge-base --is-ancestor "$base" "$target" \
    || wd_die "$label baseline $base is not an ancestor of candidate $target; automatic history rewrites are forbidden"
}
