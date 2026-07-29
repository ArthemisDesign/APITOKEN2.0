#!/usr/bin/env bash
set -euo pipefail

# One-time relocation of the original single-profile Codex home into the directory scanned by the
# authbot and OpenAI provider. The helper is deliberately metadata-only: it never reads auth.json
# or proxy.url, and it refuses to rename anything whose ownership, mode, type, or filesystem does
# not match the already-verified production shape.

CODEX_LEGACY_HOME=/srv/claude-api/data/codex/home
CODEX_HOMES_DIR=/srv/claude-api/data/codex-homes
CODEX_LEGACY_TARGET_NAME=mikala1158qqq-gmail-com

codex_migration_fail() {
  printf '[codex-homes-migrate] ERROR: %s\n' "$*" >&2
  return 1
}

codex_migration_path_exists() {
  [[ -e $1 || -L $1 ]]
}

codex_migration_identity() {
  local path=$1
  if stat -c '%u %g %a %d' -- "$path" >/dev/null 2>&1; then
    stat -c '%u %g %a %d' -- "$path"
  else
    stat -f '%u %g %Lp %d' -- "$path"
  fi
}

codex_migration_validate_secret() {
  local path=$1 expected_uid=$2 expected_gid=$3 identity uid gid mode device
  [[ -f $path && ! -L $path ]] \
    || { codex_migration_fail "secret marker is missing or unsafe: $path"; return 1; }
  identity=$(codex_migration_identity "$path") \
    || { codex_migration_fail "cannot inspect secret marker: $path"; return 1; }
  read -r uid gid mode device <<<"$identity"
  [[ $uid == "$expected_uid" && $gid == "$expected_gid" && $mode == 600 ]] \
    || { codex_migration_fail "secret marker has unexpected ownership or mode: $path"; return 1; }
}

codex_migration_validate_home() {
  local home=$1 expected_uid=$2 expected_gid=$3 identity uid gid mode device proxy
  [[ -d $home && ! -L $home ]] \
    || { codex_migration_fail "Codex home is missing or unsafe: $home"; return 1; }
  identity=$(codex_migration_identity "$home") \
    || { codex_migration_fail "cannot inspect Codex home: $home"; return 1; }
  read -r uid gid mode device <<<"$identity"
  [[ $uid == "$expected_uid" && $gid == "$expected_gid" && $mode == 700 ]] \
    || { codex_migration_fail "Codex home has unexpected ownership or mode: $home"; return 1; }
  codex_migration_validate_secret "$home/auth.json" "$expected_uid" "$expected_gid" || return 1
  proxy="$home/proxy.url"
  if codex_migration_path_exists "$proxy"; then
    codex_migration_validate_secret "$proxy" "$expected_uid" "$expected_gid" || return 1
  fi
}

codex_migrate_legacy_home() {
  local legacy_home=$1 homes_dir=$2 target_name=$3 action=$4 expected_uid=$5 expected_gid=$6
  local target legacy_identity homes_identity
  local legacy_uid legacy_gid legacy_mode legacy_device homes_uid homes_gid homes_mode homes_device

  [[ $action == check || $action == apply ]] \
    || { codex_migration_fail "action must be check or apply"; return 1; }
  [[ $legacy_home == /* && $homes_dir == /* ]] \
    || { codex_migration_fail "migration paths must be absolute"; return 1; }
  [[ $target_name =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]] \
    || { codex_migration_fail "target name is not a safe account slug"; return 1; }
  [[ $homes_dir != / ]] \
    || { codex_migration_fail "homes directory cannot be the filesystem root"; return 1; }
  homes_dir=${homes_dir%/}
  target="$homes_dir/$target_name"
  [[ $expected_uid =~ ^[0-9]+$ && $expected_gid =~ ^[0-9]+$ ]] \
    || { codex_migration_fail "expected owner identity is invalid"; return 1; }

  [[ -d $homes_dir && ! -L $homes_dir ]] \
    || { codex_migration_fail "homes directory is missing or unsafe: $homes_dir"; return 1; }
  homes_identity=$(codex_migration_identity "$homes_dir") \
    || { codex_migration_fail "cannot inspect homes directory: $homes_dir"; return 1; }
  read -r homes_uid homes_gid homes_mode homes_device <<<"$homes_identity"
  [[ $homes_uid == "$expected_uid" && $homes_gid == "$expected_gid" && $homes_mode == 700 ]] \
    || { codex_migration_fail "homes directory has unexpected ownership or mode"; return 1; }

  if codex_migration_path_exists "$legacy_home" && codex_migration_path_exists "$target"; then
    codex_migration_fail "legacy and migrated homes both exist; refusing an ambiguous move"
    return 1
  fi

  if ! codex_migration_path_exists "$legacy_home"; then
    codex_migration_path_exists "$target" \
      || { codex_migration_fail "neither legacy nor migrated home exists"; return 1; }
    codex_migration_validate_home "$target" "$expected_uid" "$expected_gid" || return 1
    printf '[codex-homes-migrate] legacy home already migrated to %s\n' "$target"
    return 0
  fi

  codex_migration_validate_home "$legacy_home" "$expected_uid" "$expected_gid" || return 1
  legacy_identity=$(codex_migration_identity "$legacy_home") \
    || { codex_migration_fail "cannot inspect legacy Codex home"; return 1; }
  read -r legacy_uid legacy_gid legacy_mode legacy_device <<<"$legacy_identity"
  [[ $legacy_device == "$homes_device" ]] \
    || { codex_migration_fail "legacy and destination homes are on different filesystems"; return 1; }

  if [[ $action == check ]]; then
    printf '[codex-homes-migrate] legacy home is safe to move to %s\n' "$target"
    return 0
  fi

  mv -T -- "$legacy_home" "$target"
  codex_migration_validate_home "$target" "$expected_uid" "$expected_gid" || return 1
  printf '[codex-homes-migrate] migrated legacy home to %s\n' "$target"
}

codex_homes_migrate_main() {
  local action expected_uid expected_gid
  [[ $# -eq 1 ]] \
    || { codex_migration_fail "usage: codex-homes-migrate.sh --check|--apply"; return 1; }
  [[ $1 == --check || $1 == --apply ]] \
    || { codex_migration_fail "usage: codex-homes-migrate.sh --check|--apply"; return 1; }
  action=${1#--}
  expected_uid=$(id -u deploy) \
    || { codex_migration_fail "production deploy user is missing"; return 1; }
  expected_gid=$(id -g deploy) \
    || { codex_migration_fail "production deploy group is missing"; return 1; }
  codex_migrate_legacy_home \
    "$CODEX_LEGACY_HOME" "$CODEX_HOMES_DIR" "$CODEX_LEGACY_TARGET_NAME" "$action" \
    "$expected_uid" "$expected_gid"
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  codex_homes_migrate_main "$@"
fi
