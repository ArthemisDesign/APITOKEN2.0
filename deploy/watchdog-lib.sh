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
  WD_LAST_ERROR=$*
  printf '[watchdog] ERROR: %s\n' "$*" >&2
  exit 1
}

# Stream redaction for public GitHub text. URLs/DSNs, secret assignments, PATs, and auth headers
# are stripped; control characters cannot create a second line in a commit-status description.
wd_redact_public_stream() {
  sed -E \
    $'s/\x1b\[[0-9;]*[[:alpha:]]//g; s#[A-Za-z][A-Za-z0-9+.-]*://[^[:space:]]+#URL_REDACTED#g; s/([A-Za-z_]*(TOKEN|KEY|SECRET|PASSWORD)[A-Za-z_]*)=[^[:space:]]+/\\1=REDACTED/g; s/(sk-[A-Za-z0-9_-]{8,})/TOKEN_REDACTED/g; s/github_pat_[A-Za-z0-9_]{20,}/TOKEN_REDACTED/g; s/[Aa]uthorization:.*$/Authorization: REDACTED/g; s/[Bb]earer[[:space:]]+[A-Za-z0-9._~+\/=-]{8,}/Bearer REDACTED/g; s/[[:cntrl:]]/ /g'
}

wd_redact_public_detail() {
  printf '%s' "${1:-}" | wd_redact_public_stream
}

# Exact-SHA payload-canary reason line written by large-payload-candidate-gate.sh. Content-free:
# statuses, oom/spool flags, or a load-driver class. Never a body, credential, or host path.
wd_payload_canary_reason() {
  local sha=${1:-${CANDIDATE_SHA:-}}
  local file=${WD_PAYLOAD_EVIDENCE_DIR:-/var/lib/apitoken/watchdog/large-payload}/$sha.reason
  local reason
  [[ $sha =~ ^[0-9a-f]{40}$ && -f $file && ! -L $file ]] || return 1
  reason=$(head -n 1 "$file" 2>/dev/null || true)
  reason=${reason%$'\r'}
  [[ $reason == payload-canary:* ]] || return 1
  printf '%s\n' "$reason"
}

# Wrapper messages that hide the inner compiler/controller cause. A transcript marker wins over these.
wd_error_is_generic() {
  local msg=${1:-}
  [[ $msg == *'failed (exit '* || $msg == *'lanes failed'* || $msg == *'controller failed'* ]]
}

# First/last known failure marker from a private transcript. Prefer a concrete Rust cause over
# Cargo's trailing rerun hint; operational markers remain last-wins. Only the last 4000 lines are
# scanned so a huge cargo/pnpm log cannot stall fail-closed reporting.
wd_failure_marker_line() {
  local log_file=$1 window detail=''
  [[ -f $log_file && ! -L $log_file ]] || return 0
  window=$(tail -n 4000 "$log_file" 2>/dev/null || true)
  [[ -n $window ]] || return 0
  detail=$(printf '%s\n' "$window" | grep -E 'panicked at|error\[E[0-9]+\]|Connection refused|No such file or directory' \
    | head -n 1 || true)
  [[ -n $detail ]] || detail=$(printf '%s\n' "$window" | grep -E \
    '\[watchdog\] ERROR:|ERR_PNPM|ELIFECYCLE|test lane\(s\) failed|migration failed|candidate lane failed|test result: FAILED|error: test failed|error: could not compile|payload-canary:|Job for .+\.service failed|AssertionError' \
    | tail -n 1 || true)
  detail=$(wd_redact_public_detail "$detail")
  detail=$(printf '%s' "$detail" | sed -E 's/^[[:space:]]+//;s/[[:space:]]+$//')
  printf '%s' "$detail"
}

# GitHub commit/deployment descriptions are capped at 140 characters. Prefer the payload-canary
# reason file, then a concrete transcript marker, then the last wd_die message. Never publish a
# bash line number from `wait`.
wd_github_failure_description() {
  [[ $# -eq 2 ]] || wd_die "github failure description requires phase and exit code"
  local phase=$1 rc=$2 detail='' marker='' out
  detail=$(wd_payload_canary_reason 2>/dev/null || true)
  if [[ -z $detail && -n ${WD_CYCLE_LOG:-} ]]; then
    marker=$(wd_failure_marker_line "$WD_CYCLE_LOG")
  fi
  if [[ -z $detail ]]; then
    if [[ -n ${WD_LAST_ERROR:-} ]] && ! wd_error_is_generic "$WD_LAST_ERROR"; then
      detail=$WD_LAST_ERROR
    elif [[ -n $marker ]]; then
      detail=$marker
    else
      detail=${WD_LAST_ERROR:-}
    fi
  fi
  detail=$(wd_redact_public_detail "$detail")
  detail=$(printf '%s' "$detail" | sed -E 's/^[[:space:]]+//;s/[[:space:]]+$//')
  [[ -n $detail ]] || detail="exit $rc; candidate quarantined"
  printf -v out 'phase=%s; %s' "$phase" "$detail"
  if (( ${#out} > 140 )); then
    out=${out:0:140}
  fi
  printf '%s' "$out"
}

# Produce a GitHub-safe, bounded diagnostic from a private validator transcript. Only known
# failure markers are eligible; URLs/DSNs and common secret assignments are redacted defensively.
wd_validation_failure_summary() {
  [[ $# -eq 3 ]] || wd_die "validation failure summary requires log path, exit code, and phase"
  local log_file=$1 rc=$2 phase=$3 detail=''
  detail=$(wd_failure_marker_line "$log_file")
  [[ -n $detail ]] || detail="validator exited with code $rc"
  printf 'phase=%s; %.100s' "$phase" "$detail"
}

# Private per-SHA directory for redacted failure reports. Not world-readable: the public channel
# is the GitHub check run, not this host path.
wd_failure_report_dir() {
  printf '%s' "${WD_FAILURE_DIR:-${STATE_ROOT:-/var/lib/apitoken/watchdog}/failures}"
}

wd_prepare_cycle_log() {
  local sha=$1 dir file
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || wd_die "cycle log requires a full SHA"
  dir=$(wd_failure_report_dir)
  mkdir -p -- "$dir"
  chmod 0700 "$dir" 2>/dev/null || true
  file=$dir/$sha.cycle.log
  rm -f -- "$file"
  : >"$file"
  chmod 0600 "$file"
  WD_CYCLE_LOG=$file
  printf '%s' "$file"
}

# Redacted excerpt for a GitHub check-run text field: matching markers with one line of context,
# plus the last 40 lines. Hard-capped at 24 KiB so the Checks API limit cannot be exceeded.
wd_extract_failure_excerpt() {
  local log_file=$1 window marks tail_lines
  [[ -f $log_file && ! -L $log_file ]] || return 1
  window=$(tail -n 4000 "$log_file" 2>/dev/null | tr -d '\000' || true)
  [[ -n $window ]] || return 1
  marks=$(printf '%s\n' "$window" | grep -E -C1 -- \
    'panicked at|error\[E[0-9]+\]|Connection refused|No such file or directory|\[watchdog\] ERROR:|ERR_PNPM|ELIFECYCLE|test lane\(s\) failed|migration failed|candidate lane failed|test result: FAILED|error: test failed|error: could not compile|payload-canary:|Job for .+\.service failed|AssertionError' \
    | head -n 160 || true)
  tail_lines=$(printf '%s\n' "$window" | tail -n 40 || true)
  {
    [[ -z $marks ]] || printf '%s\n' "$marks"
    printf '\n--- last 40 lines ---\n'
    printf '%s\n' "$tail_lines"
  } | wd_redact_public_stream | head -c 24000
}

# Write the exact files the root GitHub helper will upload. Paths are SHA-keyed under the failure
# directory; the helper refuses symlinks and any path outside that directory.
wd_write_failure_report() {
  [[ $# -eq 4 ]] || wd_die "failure report requires sha, phase, exit code, and headline"
  local sha=$1 phase=$2 rc=$3 headline=$4
  local dir summary_file text_file excerpt='' marker='' previous_umask
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || wd_die "failure report requires a full SHA"
  dir=$(wd_failure_report_dir)
  mkdir -p -- "$dir"
  chmod 0700 "$dir" 2>/dev/null || true
  summary_file=$dir/$sha.summary.md
  text_file=$dir/$sha.text
  headline=$(wd_redact_public_detail "$headline")
  if [[ -n ${WD_CYCLE_LOG:-} ]]; then
    excerpt=$(wd_extract_failure_excerpt "$WD_CYCLE_LOG" || true)
    marker=$(wd_failure_marker_line "$WD_CYCLE_LOG")
  fi
  previous_umask=$(umask)
  umask 077
  cat >"$summary_file" <<EOF
## Deploy failure

- **SHA:** \`$sha\`
- **Phase:** \`$phase\`
- **Exit:** \`$rc\`
- **Headline:** \`$headline\`
EOF
  if [[ -n $marker ]]; then
    printf '\nConcrete marker: `%s`\n' "$marker" >>"$summary_file"
  fi
  printf '\nRedacted cycle excerpt is attached as check-run text. Bodies, credentials, and DSNs are stripped.\n' \
    >>"$summary_file"
  if [[ -n $excerpt ]]; then
    printf '%s\n' "$excerpt" >"$text_file"
  else
    printf '(no cycle transcript; headline only)\n' >"$text_file"
  fi
  chmod 0600 "$summary_file" "$text_file"
  umask "$previous_umask"
}

wd_discard_cycle_log() {
  local log=${WD_CYCLE_LOG:-}
  [[ -n $log && -f $log && ! -L $log ]] || return 0
  rm -f -- "$log"
}

# Leftover unbounded cycle transcripts from a killed watchdog must not fill the state volume.
wd_prune_failure_cycle_logs() {
  local dir
  dir=$(wd_failure_report_dir)
  [[ -d $dir && ! -L $dir ]] || return 0
  find "$dir" -maxdepth 1 -type f -name '*.cycle.log' -mtime +0 -delete 2>/dev/null || true
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
# An optional `--pattern <ERE>` selects prefixed lane names (the CRM root uses `crm-<sha>`); the
# default pattern admits only plain 40-character SHA names. A `crm-` prefix is normalized before
# SHA validation and before protected-name comparison, so `current`/`previous` and the protected
# list stay plain-SHA based regardless of the root's naming scheme.
# Selection is deliberately pure; the caller performs the privileged deletion under the deploy lock.
wd_prunable_release_dirs() {
  [[ $# -ge 2 ]] || return 2
  local root=$1 keep=$2 protected=() name_pattern='^[0-9a-f]{40}$' name sha link resolved
  local canonical_root candidate kept=0 ordered
  shift 2
  if [[ ${1:-} == --pattern && $# -ge 2 ]]; then
    name_pattern=$2
    shift 2
  fi
  protected=("$@")
  [[ $root == /* && -d $root && ! -L $root ]] || return 1
  [[ $keep =~ ^[0-9]+$ ]] || return 2

  # Compare resolved link targets against the resolved root: a release root reached through a
  # symlinked parent must still recognise its own current/previous targets as in-root.
  canonical_root=$(readlink -f -- "$root") || return 1

  for link in current previous; do
    if [[ -L $root/$link ]]; then
      resolved=$(readlink -f -- "$root/$link" 2>/dev/null) || return 1
      [[ ${resolved%/*} == "$canonical_root" ]] || continue
      name=${resolved##*/}
      name=${name#crm-}
      [[ $name =~ ^[0-9a-f]{40}$ ]] || return 1
      protected+=("$name")
    fi
  done

  # Materialize the complete ordering before emitting any NUL candidate. A failed enumeration or
  # mtime read therefore cannot expose a partial deletion stream to the caller.
  ordered=$(wd_dirs_newest_first "$root") || return 1
  while IFS= read -r candidate; do
    [[ -n $candidate ]] || continue
    name=${candidate##*/}
    [[ $name =~ $name_pattern ]] || continue
    sha=${name#crm-}
    [[ $sha =~ ^[0-9a-f]{40}$ ]] || continue
    [[ -d $candidate && ! -L $candidate ]] || return 1
    if wd_list_contains "$sha" "${protected[@]}"; then
      continue
    fi
    if (( kept < keep )); then
      kept=$((kept + 1))
      continue
    fi
    printf '%s\0' "$candidate"
  done <<<"$ordered"
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
  local root=$1 path modified listing rows
  listing=$(mktemp) || return 1
  rows=$(mktemp) || { rm -f -- "$listing"; return 1; }
  if ! find "$root" -mindepth 1 -maxdepth 1 -type d -print0 >"$listing"; then
    rm -f -- "$listing" "$rows"
    return 1
  fi
  while IFS= read -r -d '' path; do
    [[ -d $path && ! -L $path ]] || { rm -f -- "$listing" "$rows"; return 1; }
    modified=$(wd_path_mtime_epoch "$path" 2>/dev/null) \
      || { rm -f -- "$listing" "$rows"; return 1; }
    [[ $modified =~ ^[0-9]+$ ]] || { rm -f -- "$listing" "$rows"; return 1; }
    printf '%s\t%s\n' "$modified" "$path" >>"$rows"
  done <"$listing"
  rm -f -- "$listing"
  if ! sort -rn -k1,1 "$rows" | cut -f2-; then
    rm -f -- "$rows"
    return 1
  fi
  rm -f -- "$rows"
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
    admin)
      artifacts=(apps/admin/.next/BUILD_ID)
      ;;
    devbot)
      artifacts=(apps/devbot/dist/main.js)
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
  [[ $expected == commerce || $expected == sales || $expected == openkeys || $expected == web \
    || $expected == admin || $expected == devbot ]] || wd_die "unknown TypeScript component: $expected"
  for component in $components; do
    [[ $component == "$expected" ]] && return 0
  done
  return 1
}

# A canonical component list is non-empty, contains only known components, and orders them by
# their fixed rank (commerce < sales < openkeys < web < admin < devbot). Checking the rank order
# programmatically keeps the validator exact for any number of components without enumerating
# every combination.
wd_typescript_component_list_is_canonical() {
  local components=$1 component rank previous_rank=0 IFS=,
  [[ -n $components ]] || return 1
  # Word splitting drops a trailing empty field, so reject empty fields explicitly.
  [[ $components != ,* && $components != *, && $components != *,,* ]] || return 1
  for component in $components; do
    case "$component" in
      commerce) rank=1 ;;
      sales) rank=2 ;;
      openkeys) rank=3 ;;
      web) rank=4 ;;
      admin) rank=5 ;;
      devbot) rank=6 ;;
      *) return 1 ;;
    esac
    (( rank > previous_rank )) || return 1
    previous_rank=$rank
  done
  return 0
}

wd_drizzle_migration_manifest() {
  [[ $# -eq 3 ]] || wd_die "usage: wd_drizzle_migration_manifest <tree> <relative-root> <format>"
  local tree=$1 relative_root=$2 format=$3
  [[ $relative_root =~ ^[A-Za-z0-9._/-]+$ && $relative_root != /* && $relative_root != *..* ]] \
    || wd_die "unsafe migration root: $relative_root"
  [[ $format =~ ^[A-Za-z0-9._-]+$ ]] || wd_die "unsafe migration manifest format: $format"
  [[ -d $tree/$relative_root ]] || wd_die "migration directory is missing from $tree: $relative_root"

  # Drizzle rewrites meta/_journal.json whenever it appends a migration. Hashing that file as one
  # immutable artifact would therefore reject every legitimate migration after the baseline. Build
  # a semantic manifest instead: existing SQL/snapshots remain byte-for-byte immutable, while each
  # canonical journal entry is an individually immutable, ordered record and new records may append.
  node - "$tree" "$relative_root" "$format" <<'NODE'
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const tree = process.argv[2];
const relativeRoot = process.argv[3];
const format = process.argv[4];
const root = path.join(tree, relativeRoot);
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

process.stdout.write(`format=${format}\n`);
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
  process.stdout.write(`file=${digest(contents)} ${relativeRoot}/${relative}\n`);
}
NODE
}

wd_migration_manifest() {
  wd_drizzle_migration_manifest "$1" packages/db/migrations apitoken-drizzle-manifest-v2
}

wd_sales_migration_manifest() {
  wd_drizzle_migration_manifest "$1" packages/sales-db/migrations apitoken-sales-drizzle-manifest-v1
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

# Identify the one pricing-retirement contraction whose newly added immutable migration must be
# verified after deployment. The processed production SHA is the only valid base: if a failed
# forward-only contraction remains unprocessed, its follow-up fix still re-runs this proof. Adding
# both contraction paths in one range is deliberately invalid because the runbook requires two
# independent production failure domains.
wd_pricing_retirement_postdrop_stage() {
  [[ $# -eq 3 ]] || return 2
  local repo=$1 base=$2 target=$3 added path commerce=0 engine=0
  [[ -d $repo ]] || return 2
  [[ $base =~ ^[0-9a-f]{40}$ && $target =~ ^[0-9a-f]{40}$ ]] || return 2
  added=$(git -C "$repo" diff --name-only --no-renames --diff-filter=A "$base..$target" -- \
    packages/db/migrations/0049_retire_pricing_schema.sql \
    crates/registry/migrations_pg/0049_retire_pricing_schema.sql) || return 1
  while IFS= read -r path; do
    [[ -n $path ]] || continue
    case $path in
      packages/db/migrations/0049_retire_pricing_schema.sql) commerce=1 ;;
      crates/registry/migrations_pg/0049_retire_pricing_schema.sql) engine=1 ;;
      *) return 2 ;;
    esac
  done <<<"$added"
  (( commerce == 0 || engine == 0 )) || return 3
  if (( commerce == 1 )); then
    printf 'commerce\n'
  elif (( engine == 1 )); then
    printf 'engine\n'
  else
    printf 'none\n'
  fi
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
  local sales_changed=$4 openkeys_changed=$5 admin_changed=$6
  local checks=()
  local broad_infrastructure=0 caddy_infrastructure=0 monitoring_infrastructure=0
  wd_infrastructure_scope_is_valid "$infrastructure_scope" || return 2
  local flag
  for flag in "$engine_changed" "$backend_changed" "$sales_changed" "$openkeys_changed" \
    "$admin_changed"; do
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
    # The "panel" lane is final_verify_admin_data: the UI moved to the standalone Next.js admin
    # app, so the engine-side smoke now verifies the admin data routes the app polls.
    checks+=(runtime panel)
  fi
  if (( caddy_infrastructure == 1 || broad_infrastructure == 1 )); then
    checks+=(routing)
  fi
  if (( engine_changed == 1 || backend_changed == 1 || sales_changed == 1 \
        || openkeys_changed == 1 || admin_changed == 1 || caddy_infrastructure == 1 \
        || monitoring_infrastructure == 1 || broad_infrastructure == 1 )); then
    checks+=(monitoring)
  fi
  if (( engine_changed == 1 || caddy_infrastructure == 1 \
        || broad_infrastructure == 1 )); then
    checks+=(codex gemini kimi)
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
    crates/*|vendor/*|Cargo.toml|Cargo.lock|config.env.example|server.env.example|schema/*|tests/*|tools/refresh-fingerprint.sh|tools/codex-native/*|systemd/claude-api.service|systemd/claude-api@.service|systemd/claude-api-anthropic@.service|systemd/claude-api-openai.service|systemd/claude-api-openai@.service|systemd/claude-api-gemini.service|systemd/claude-api-gemini@.service|systemd/claude-api-kimi.service|systemd/claude-api-kimi@.service|systemd/claude-router.service|systemd/claude-router@.service|deploy/router-bluegreen.sh|deploy/router-promote.sh|deploy/engine-commerce-compatibility.contract)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# The assembled Control API acceptance owns both sides of the engine↔TypeScript seam. A client or
# shared schema edit therefore needs a production engine artifact even when no Rust path changed;
# the trusted host runs the built package against that exact binary and disposable PostgreSQL.
wd_path_requires_control_api_acceptance() {
  case "$1" in
    crates/server/src/admin.rs|crates/server/src/http.rs|crates/server/src/config.rs|\
    crates/server/src/main.rs|crates/registry/src/*|crates/registry/migrations_pg/*|\
    packages/engine-client/*|packages/contracts/src/index.ts|\
    tests/control_api_engine_client_acceptance.sh)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_requires_router_engine_replay() {
  case "$1" in
    crates/router/*|crates/server/*|crates/forward/*|crates/pool/*|Cargo.toml|Cargo.lock|\
    tests/router_engine_replay.py|tests/router_engine_replay_mock.py|\
    tests/router_engine_replay_semantics.test.py|tests/fixtures/router-engine-replay-v1.json)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_codex_tooling() {
  case "$1" in
    tools/codex-native/*)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# All pnpm workspace source and its shared build inputs belong to the TypeScript validation lane,
# including Vercel-only surfaces that do not map to a host deployment component.
wd_path_is_typescript() {
  case "$1" in
    apps/*|packages/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig*.json|.node-version|deploy/engine-commerce-compatibility.contract)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_web() {
  case "$1" in
    apps/web/*|packages/opencode-router-plugin/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_backend() {
  case "$1" in
    # These packages belong to independent client/database contexts. They still run in the
    # workspace-wide TypeScript validation lane, but must not roll the commerce backend.
    packages/sales-db/*|packages/openkeys-db/*|packages/opencode-router-plugin/*)
      return 1
      ;;
    apps/api/*|apps/worker/*|apps/content-studio/*|packages/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version|deploy/engine-commerce-compatibility.contract)
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

# Admin panel bounded context (admin.apitoken.sale). Свой релизный корень
# (/opt/apitoken/admin-releases), без собственной БД и без workspace-зависимостей,
# поэтому shared build-файлы включены, как у apps/web: бамп зависимостей
# пере-собирает admin-релиз.
wd_path_is_admin() {
  case "$1" in
    apps/admin/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# Devbot bounded context (apps/devbot, dev-уведомления в Telegram). Свой релизный корень
# (/opt/apitoken/devbot-releases) и свой юнит. Изменения юнита и deploy-скрипта тоже катят
# lane, чтобы disabled-until-provisioned контур проходился end-to-end; shared build-файлы
# включены по той же логике, что у admin: бамп зависимостей пере-собирает релиз.
wd_path_is_devbot() {
  case "$1" in
    apps/devbot/*|systemd/apitoken-devbot.service|deploy/devbot-deploy.sh|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
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
    deploy/*.md|deploy/*.test.sh|deploy/test-fixtures/*|deploy/agent-merge.sh|deploy/agent-merge.suite.sh|\
    deploy/test-stage2-e2e.sh|deploy/sccache-cargo.sh|deploy/agent-worktree.sh|\
    deploy/DELETE_WORKTREE.sh|deploy/prune-merged.sh|deploy/next-cache.sh|\
    deploy/typescript-scope.mjs|deploy/typescript-build-contexts.sh|\
    deploy/typescript-test-groups.sh|deploy/local-test-databases.sh|\
    deploy/local-postgres-init.sql|deploy/host-image-gate.sh|deploy/host-image/*|\
    compose.yaml)
      return 1
      ;;
    deploy/*|systemd/*|observability/*|compose.yaml)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# Paths whose correctness depends on Ubuntu userland or systemd namespaces.
# A match selects the disposable host-image gate on the merge client. It does
# not install anything on the production host.
wd_path_depends_on_ubuntu_host() {
  case "$1" in
    systemd/*|deploy/install-*.sh|deploy/watchdog-infrastructure.sh|\
    deploy/sudoers.d/*|deploy/apitoken-observe.sh|\
    deploy/affinity-redis.compose.yaml|deploy/commerce-postgres.compose.yaml|\
    deploy/Caddyfile|deploy/render-caddy.awk|deploy/host-image-gate.sh|\
    deploy/host-image/*)
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
    systemd/apitoken-sudoers-install.service|systemd/apitoken-tmpfiles-install.service|\
    systemd/apitoken-sysctl-install.service|systemd/apitoken-observe-install.service|\
    systemd/apitoken-postgres.service|systemd/apitoken-affinity-redis.service|\
    systemd/apitoken-worker.service|systemd/apitoken-content-studio.service|\
    systemd/claude-api.service|systemd/claude-api@.service|systemd/claude-api-anthropic@.service|systemd/claude-api-openai.service|systemd/claude-api-openai@.service|\
    systemd/claude-api-gemini.service|systemd/claude-api-gemini@.service|\
    systemd/claude-api-kimi.service|systemd/claude-api-kimi@.service|\
    systemd/claude-api-backup.service|\
    systemd/claude-api-backup.timer|systemd/claude-api-fingerprint.service|\
    systemd/claude-api-fingerprint.timer|systemd/apitoken-sales-api.service|\
    systemd/apitoken-sales-web.service|systemd/claude-authbot.service|\
    systemd/claude-router.service|systemd/claude-router@.service|\
    systemd/claude-router.slice|systemd/claude-api-anthropic.slice|systemd/claude-api-openai.slice|systemd/claude-api-gemini.slice|\
    systemd/apitoken-openkeys.service|systemd/apitoken-admin.service|\
    systemd/apitoken-monitoring-collector.service|\
    systemd/apitoken-monitoring-collector.timer|systemd/journald-apitoken.conf|\
    systemd/apitoken-tmpfiles.conf|systemd/sysctl-apitoken-redis.conf|\
    deploy/install-tmpfiles.sh|deploy/install-sysctl.sh|deploy/install-observe.sh|\
    deploy/apitoken-observe.sh)
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
    deploy/contour-config.sh|deploy/contour-config.py|deploy/contour-config.schema.json|\
    deploy/contour-production.json|\
    deploy/gpt-image-2-live-gate.sh|deploy/gpt-image-2-public-smoke-gate.sh|\
    deploy/gpt-image-2-public-preflight-gate.sh|deploy/gpt-image-2-public-preflight-v2-gate.sh|\
    deploy/gpt-image-2-public-preflight-v3-gate.sh|deploy/gpt-image-2-public-paid-smoke-gate.sh|\
    deploy/gpt-image-2-public-paid-smoke-v2-gate.sh|deploy/gpt-image-2-public-paid-smoke-v3-gate.sh|\
    deploy/gpt-image-2-public-paid-inspect-gate.sh|\
    deploy/gpt-image-2-surface-probe-gate.sh|\
    deploy/watchdog-test-db.sh|deploy/watchdog-backup.sh|deploy/pricing-retirement-admission.sh|\
    deploy/pricing-retirement-postdrop.sh|deploy/pricing-retired-schema-manifest.sh|deploy/watchdog-migrate.sh|\
    deploy/watchdog-infrastructure.sh|deploy/watchdog-retention.sh|\
    deploy/watchdog-github.sh|deploy/watchdog-control.sh|\
    deploy/deploy.sh|deploy/authbot-runtime-state.sh|deploy/lib.sh|deploy/commerce-release-bundle.sh|\
    deploy/release-tree-digest.mjs|deploy/content-studio-start.sh|\
    deploy/api-bluegreen.sh|deploy/engine-bluegreen.sh|deploy/router-bluegreen.sh|deploy/router-promote.sh|deploy/engine-migrate.sh|deploy/codex-homes-migrate.sh|\
    deploy/rollback.sh|deploy/sales-deploy.sh|deploy/openkeys-deploy.sh|\
    deploy/admin-deploy.sh)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_gpt_image_2_live_gate_trigger() {
  [[ $1 == deploy/gpt-image-2-live-gate.sh ]]
}

wd_path_is_gpt_image_2_public_smoke_gate_trigger() {
  [[ $1 == deploy/gpt-image-2-public-smoke-gate.sh ]]
}

wd_path_is_gpt_image_2_public_preflight_gate_trigger() {
  [[ $1 == deploy/gpt-image-2-public-preflight-gate.sh ]]
}

wd_path_is_gpt_image_2_public_preflight_v2_gate_trigger() {
  # This one-shot root is permanently fenced after its delivery; retained controller changes may
  # only be inspected as history and must never execute it again.
  return 1
}

wd_path_is_gpt_image_2_public_preflight_v3_gate_trigger() {
  [[ $1 == deploy/gpt-image-2-public-preflight-v3-gate.sh ]]
}

wd_path_is_gpt_image_2_public_paid_smoke_gate_trigger() {
  # The paid output root is permanently fenced after its first delivery. Never dispatch from it again.
  return 1
}

wd_path_is_gpt_image_2_public_paid_smoke_v2_gate_trigger() {
  # The v2 paid output root recorded generation and is permanently fenced. Never dispatch from it again.
  return 1
}

wd_path_is_gpt_image_2_public_paid_smoke_v3_gate_trigger() {
  [[ $1 == deploy/gpt-image-2-public-paid-smoke-v3-gate.sh ]]
}

wd_path_is_gpt_image_2_public_paid_inspect_gate_trigger() {
  [[ $1 == deploy/gpt-image-2-public-paid-inspect-gate.sh ]]
}

wd_path_is_gpt_image_2_surface_probe_gate_trigger() {
  [[ $1 == deploy/gpt-image-2-surface-probe-gate.sh ]]
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
    .claude/*|.cursor/rules/*|AGENTS.md|CLAUDE.md|BRANCHES.md|CONTRIBUTING.md|\
    deploy/change-plan.sh|deploy/change-plan.test.sh|\
    deploy/repository-invariants.py|deploy/repository-invariants.test.sh)
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
    printf 'commerce,sales,openkeys,web,admin,devbot\n'
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
  wd_range_has_class "$repo" "$base" "$target" wd_path_is_admin \
    && components+=(admin)
  wd_range_has_class "$repo" "$base" "$target" wd_path_is_devbot \
    && components+=(devbot)
  if (( ${#components[@]} == 0 )); then
    # A TypeScript lane with no known owner can be a newly introduced workspace surface. Building
    # everything is the only safe default until its bounded context is explicitly classified.
    printf 'commerce,sales,openkeys,web,admin,devbot\n'
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
