#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d)
CACHE_ROOT="$TEMP/cache"
trap 'rm -rf -- "$TEMP"' EXIT

apps=(
  apps/web
  apps/content-studio
  apps/sales-web
  apps/openkeys
  apps/admin
)

new_candidate() {
  local candidate=$1 value=$2 app
  for app in "${apps[@]}"; do
    mkdir -p "$candidate/$app/.next/cache"
    printf '%s:%s\n' "$app" "$value" >"$candidate/$app/.next/cache/sentinel"
    printf 'not cache\n' >"$candidate/$app/.next/not-cache"
  done
}

save_cache() {
  NEXT_CACHE_ROOT="$CACHE_ROOT" bash "$ROOT/deploy/next-cache.sh" save "$1"
}

restore_cache() {
  NEXT_CACHE_ROOT="$CACHE_ROOT" bash "$ROOT/deploy/next-cache.sh" restore "$1"
}

first="$TEMP/first"
new_candidate "$first" first
save_cache "$first"
for app in "${apps[@]}"; do
  key=${app//\//-}
  [[ -f $CACHE_ROOT/$key.tar && ! -L $CACHE_ROOT/$key.tar ]] \
    || { printf 'cache archive missing for %s\n' "$app" >&2; exit 1; }
  if tar -tf "$CACHE_ROOT/$key.tar" | grep -Fq 'not-cache'; then
    printf 'archive for %s escaped .next/cache\n' "$app" >&2
    exit 1
  fi
done

second="$TEMP/second"
for app in "${apps[@]}"; do mkdir -p "$second/$app"; done
restore_cache "$second"
for app in "${apps[@]}"; do
  [[ $(cat "$second/$app/.next/cache/sentinel") == "$app:first" ]] \
    || { printf 'cache did not restore for %s\n' "$app" >&2; exit 1; }
done

# Publishing is an atomic last-writer-wins replacement: concurrent candidates may race, but readers
# must always receive one complete archive rather than a mix of both.
concurrent_a="$TEMP/concurrent-a"
concurrent_b="$TEMP/concurrent-b"
new_candidate "$concurrent_a" concurrent-a
new_candidate "$concurrent_b" concurrent-b
save_cache "$concurrent_a" >/dev/null &
pid_a=$!
save_cache "$concurrent_b" >/dev/null &
pid_b=$!
wait "$pid_a"
wait "$pid_b"
concurrent_restore="$TEMP/concurrent-restore"
for app in "${apps[@]}"; do mkdir -p "$concurrent_restore/$app"; done
restore_cache "$concurrent_restore" >/dev/null
for app in "${apps[@]}"; do
  value=$(cat "$concurrent_restore/$app/.next/cache/sentinel")
  [[ $value == "$app:concurrent-a" || $value == "$app:concurrent-b" ]] \
    || { printf 'concurrent archive was incomplete for %s: %s\n' "$app" "$value" >&2; exit 1; }
done

# A corrupt archive is a cache miss, never a failed build prerequisite.
printf 'not a tar archive\n' >"$CACHE_ROOT/apps-web.tar"
corrupt_restore="$TEMP/corrupt-restore"
for app in "${apps[@]}"; do mkdir -p "$corrupt_restore/$app"; done
restore_cache "$corrupt_restore" >/dev/null
[[ ! -e $corrupt_restore/apps/web/.next/cache/sentinel ]] \
  || { printf 'corrupt cache was restored\n' >&2; exit 1; }

# Candidate-created symlinks are never copied into the persistent cache.
safe="$TEMP/safe"
new_candidate "$safe" safe
save_cache "$safe" >/dev/null
ln -s "$TEMP" "$safe/apps/web/.next/cache/escape"
printf 'poisoned\n' >"$safe/apps/web/.next/cache/sentinel"
save_cache "$safe" >/dev/null
symlink_restore="$TEMP/symlink-restore"
for app in "${apps[@]}"; do mkdir -p "$symlink_restore/$app"; done
restore_cache "$symlink_restore" >/dev/null
[[ $(cat "$symlink_restore/apps/web/.next/cache/sentinel") == 'apps/web:safe' ]] \
  || { printf 'symlink-containing cache replaced the last safe archive\n' >&2; exit 1; }
[[ ! -L $symlink_restore/apps/web/.next/cache/escape ]] \
  || { printf 'cache restore created a symlink\n' >&2; exit 1; }

miss="$TEMP/miss"
mkdir -p "$miss/apps/web"
empty_root="$TEMP/empty-cache"
NEXT_CACHE_ROOT="$empty_root" bash "$ROOT/deploy/next-cache.sh" restore "$miss" >/dev/null
[[ ! -e $miss/apps/web/.next/cache ]] || { printf 'cache miss created output\n' >&2; exit 1; }

if NEXT_CACHE_ROOT=relative bash "$ROOT/deploy/next-cache.sh" save "$first" >/dev/null 2>&1; then
  :
else
  printf 'an unavailable optional cache must not fail validation\n' >&2
  exit 1
fi
if NEXT_CACHE_ROOT="$CACHE_ROOT" bash "$ROOT/deploy/next-cache.sh" invalid "$first" \
  >/dev/null 2>&1; then
  printf 'invalid cache operation was accepted\n' >&2
  exit 1
fi

printf 'next-cache.test: ok\n'
