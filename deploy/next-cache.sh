#!/usr/bin/env bash
set -uo pipefail

operation=${1:-}
candidate_argument=${2:-}
cache_root=${NEXT_CACHE_ROOT:-}

next_cache_log() {
  printf '[next-cache] %s\n' "$*"
}

next_cache_warn() {
  printf '[next-cache] WARNING: %s\n' "$*" >&2
}

if [[ $operation != restore && $operation != save ]] || [[ -z $candidate_argument ]]; then
  printf 'usage: NEXT_CACHE_ROOT=/absolute/path next-cache.sh <restore|save> <candidate>\n' >&2
  exit 2
fi
[[ $cache_root == /* ]] || {
  next_cache_warn 'NEXT_CACHE_ROOT must be an absolute path; continuing without cache'
  exit 0
}
candidate=$(cd -- "$candidate_argument" 2>/dev/null && pwd -P) || {
  next_cache_warn "candidate directory is unavailable: $candidate_argument"
  exit 0
}
[[ -d $candidate && ! -L $candidate ]] || {
  next_cache_warn "candidate is not a regular directory: $candidate"
  exit 0
}
if ! mkdir -p -- "$cache_root" || [[ ! -d $cache_root || -L $cache_root ]]; then
  next_cache_warn "persistent cache root is unavailable: $cache_root"
  exit 0
fi

next_apps=(
  apps/web
  apps/content-studio
  apps/sales-web
  apps/openkeys
)

archive_is_safe() {
  local archive=$1 listing entry found=0
  [[ -f $archive && ! -L $archive ]] || return 1
  listing=$(tar -tf "$archive" 2>/dev/null) || return 1
  while IFS= read -r entry; do
    [[ -n $entry ]] || continue
    found=1
    case "$entry" in
      cache|cache/|cache/*) ;;
      *) return 1 ;;
    esac
    case "/$entry/" in
      */../*|*/./*) return 1 ;;
    esac
  done <<<"$listing"
  (( found == 1 ))
}

restore_app_cache() {
  local app=$1 key
  key=${app//\//-}
  local archive="$cache_root/$key.tar"
  local app_root="$candidate/$app"
  local snapshot="$candidate/.next-cache-$key.${BASHPID:-$$}.${RANDOM}.tar"

  [[ -d $app_root && ! -L $app_root ]] || return 0
  [[ -f $archive && ! -L $archive ]] || {
    next_cache_log "miss: $app"
    return 0
  }
  if ! cp -f -- "$archive" "$snapshot"; then
    next_cache_warn "could not snapshot $app cache; treating it as a miss"
    return 0
  fi
  if ! archive_is_safe "$snapshot"; then
    next_cache_warn "ignored invalid $app cache archive"
    rm -f -- "$snapshot"
    return 0
  fi
  if ! mkdir -p -- "$app_root/.next" \
    || ! tar -xf "$snapshot" -C "$app_root/.next"; then
    next_cache_warn "could not restore $app cache; Next.js will rebuild it"
    rm -f -- "$snapshot"
    return 0
  fi
  rm -f -- "$snapshot"
  next_cache_log "restored: $app"
}

save_app_cache() {
  local app=$1 key
  key=${app//\//-}
  local source="$candidate/$app/.next/cache"
  local archive="$cache_root/$key.tar"
  local temporary="$cache_root/.$key.tmp.${BASHPID:-$$}.${RANDOM}.tar"

  [[ -d $source && ! -L $source ]] || return 0
  if [[ -n $(find "$source" -type l -print -quit 2>/dev/null) ]]; then
    next_cache_warn "refusing to persist $app cache because it contains a symlink"
    return 0
  fi
  if ! tar -cf "$temporary" -C "$candidate/$app/.next" cache \
    || ! archive_is_safe "$temporary"; then
    next_cache_warn "could not create a valid $app cache archive"
    rm -f -- "$temporary"
    return 0
  fi
  chmod 0600 "$temporary" 2>/dev/null || true
  if ! mv -f -- "$temporary" "$archive"; then
    next_cache_warn "could not publish $app cache archive"
    rm -f -- "$temporary"
    return 0
  fi
  next_cache_log "saved: $app"
}

for next_app in "${next_apps[@]}"; do
  if [[ $operation == restore ]]; then
    restore_app_cache "$next_app"
  else
    save_app_cache "$next_app"
  fi
done
