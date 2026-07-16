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
  local tree=$1 path hash
  [[ -d $tree/packages/db/migrations ]] || wd_die "migration directory is missing from $tree"

  while IFS= read -r path; do
    [[ $path != *$'\n'* ]] || wd_die "migration path contains a newline"
    hash=$(wd_sha256_file "$tree/$path")
    printf '%s  %s\n' "$hash" "$path"
  done < <(find "$tree/packages/db/migrations" -type f -print \
    | sed "s#^$tree/##" | LC_ALL=C sort)
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
    apps/api/*|apps/worker/*|packages/*|package.json|pnpm-lock.yaml|pnpm-workspace.yaml|tsconfig.base.json|.node-version)
      return 0
      ;;
    *) return 1 ;;
  esac
}

wd_path_is_infrastructure() {
  case "$1" in
    deploy/*|systemd/*|compose.yaml|.github/*)
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

wd_require_ancestor() {
  local repo=$1 base=$2 target=$3 label=$4
  [[ -n $base ]] || wd_die "$label baseline is not initialized"
  git -C "$repo" cat-file -e "$base^{commit}" 2>/dev/null \
    || wd_die "$label baseline $base is unavailable in the source repository"
  git -C "$repo" merge-base --is-ancestor "$base" "$target" \
    || wd_die "$label baseline $base is not an ancestor of candidate $target; automatic history rewrites are forbidden"
}
