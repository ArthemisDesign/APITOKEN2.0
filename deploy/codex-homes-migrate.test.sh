#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/codex-homes-migrate.sh
source "$ROOT/deploy/codex-homes-migrate.sh"

TEMP=$(mktemp -d "${TMPDIR:-/tmp}/apitoken-codex-homes-migrate-test.XXXXXXXX")
trap 'rm -rf -- "$TEMP"' EXIT

# Production uses GNU mv -T so a concurrently created target can never turn the rename into a
# nested move. macOS contributors have BSD mv; emulate only that already-preflighted test rename.
if ! mv --help 2>&1 | grep -q -- '-T'; then
  mv() {
    [[ ${1:-} == -T && ${2:-} == -- ]] || command mv "$@"
    shift 2
    [[ ! -e $2 && ! -L $2 ]] || return 1
    command mv -- "$1" "$2"
  }
fi

fail() {
  printf '[codex-homes-migrate-test] ERROR: %s\n' "$*" >&2
  exit 1
}

make_fixture() {
  local root=$1
  mkdir -p "$root/codex/home" "$root/codex-homes"
  chmod 0700 "$root/codex/home" "$root/codex-homes"
  : >"$root/codex/home/auth.json"
  chmod 0600 "$root/codex/home/auth.json"
}

safe_root="$TEMP/safe"
make_fixture "$safe_root"
codex_migrate_legacy_home \
  "$safe_root/codex/home" "$safe_root/codex-homes" mikala1158qqq-gmail-com check
[[ -d $safe_root/codex/home && ! -e $safe_root/codex-homes/mikala1158qqq-gmail-com ]] \
  || fail "check mode changed the filesystem"
codex_migrate_legacy_home \
  "$safe_root/codex/home" "$safe_root/codex-homes" mikala1158qqq-gmail-com apply
[[ ! -e $safe_root/codex/home \
  && -f $safe_root/codex-homes/mikala1158qqq-gmail-com/auth.json ]] \
  || fail "apply mode did not move the authenticated home"
codex_migrate_legacy_home \
  "$safe_root/codex/home" "$safe_root/codex-homes" mikala1158qqq-gmail-com apply

ambiguous_root="$TEMP/ambiguous"
make_fixture "$ambiguous_root"
mkdir "$ambiguous_root/codex-homes/mikala1158qqq-gmail-com"
chmod 0700 "$ambiguous_root/codex-homes/mikala1158qqq-gmail-com"
: >"$ambiguous_root/codex-homes/mikala1158qqq-gmail-com/auth.json"
chmod 0600 "$ambiguous_root/codex-homes/mikala1158qqq-gmail-com/auth.json"
if codex_migrate_legacy_home \
    "$ambiguous_root/codex/home" "$ambiguous_root/codex-homes" \
    mikala1158qqq-gmail-com apply >/dev/null 2>&1; then
  fail "migration accepted simultaneous legacy and destination homes"
fi
[[ -d $ambiguous_root/codex/home \
  && -d $ambiguous_root/codex-homes/mikala1158qqq-gmail-com ]] \
  || fail "ambiguous migration changed either home"

unsafe_root="$TEMP/unsafe"
make_fixture "$unsafe_root"
chmod 0644 "$unsafe_root/codex/home/auth.json"
if codex_migrate_legacy_home \
    "$unsafe_root/codex/home" "$unsafe_root/codex-homes" \
    mikala1158qqq-gmail-com apply >/dev/null 2>&1; then
  fail "migration accepted an exposed auth store"
fi
[[ -d $unsafe_root/codex/home && ! -e $unsafe_root/codex-homes/mikala1158qqq-gmail-com ]] \
  || fail "unsafe migration changed the filesystem"

symlink_root="$TEMP/symlink"
make_fixture "$symlink_root"
rm "$symlink_root/codex/home/auth.json"
: >"$symlink_root/elsewhere-auth.json"
chmod 0600 "$symlink_root/elsewhere-auth.json"
ln -s "$symlink_root/elsewhere-auth.json" "$symlink_root/codex/home/auth.json"
if codex_migrate_legacy_home \
    "$symlink_root/codex/home" "$symlink_root/codex-homes" \
    mikala1158qqq-gmail-com apply >/dev/null 2>&1; then
  fail "migration accepted a symlinked auth store"
fi
[[ -d $symlink_root/codex/home && ! -e $symlink_root/codex-homes/mikala1158qqq-gmail-com ]] \
  || fail "symlink refusal changed the filesystem"

printf 'Codex homes migration tests passed\n'
