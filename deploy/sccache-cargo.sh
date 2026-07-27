#!/usr/bin/env bash
# Run a Cargo command through a repository-shared sccache.
#
# The pinned binary, sccache objects, and Cargo intermediate build directory live under the clone's
# git-common-dir. Every linked worktree reuses them while keeping its final target directory local.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
SCCACHE_VERSION=0.15.0

sc_log() { printf '[sccache-cargo] %s\n' "$*" >&2; }
sc_die() { sc_log "ERROR: $*"; exit 1; }

sc_platform() {
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
      printf '%s %s\n' \
        "sccache-v$SCCACHE_VERSION-aarch64-apple-darwin.tar.gz" \
        '430ef7b5f54256d3ed5bfe77e8b0afc51aa209aeebe4f95b69c3a52ce3acc6e9'
      ;;
    Darwin:x86_64)
      printf '%s %s\n' \
        "sccache-v$SCCACHE_VERSION-x86_64-apple-darwin.tar.gz" \
        'f8da93e0689122268f720ddb48c8357f3da18be8c88aff23a8e75a7a219367db'
      ;;
    Linux:aarch64|Linux:arm64)
      printf '%s %s\n' \
        "sccache-v$SCCACHE_VERSION-aarch64-unknown-linux-musl.tar.gz" \
        '3a6a3712b49da3d263bf2d30d702de4302793016019e800bfb81c0c69401d8f8'
      ;;
    Linux:x86_64|Linux:amd64)
      printf '%s %s\n' \
        "sccache-v$SCCACHE_VERSION-x86_64-unknown-linux-musl.tar.gz" \
        '782d2b5dd7ae0a55ebe368ab258114d0928d019ac2d949ab85d5d02f3926709e'
      ;;
    *)
      return 1
      ;;
  esac
}

sc_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 1
  fi
}

sc_install_pinned() (
  local install_dir=$1
  local platform asset expected url temp_dir archive actual extracted staged
  platform=$(sc_platform) || return 1
  asset=${platform%% *}
  expected=${platform#* }
  url="https://github.com/mozilla/sccache/releases/download/v$SCCACHE_VERSION/$asset"
  temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/claude-api-sccache.XXXXXX") || return 1
  archive="$temp_dir/$asset"
  trap 'rm -rf -- "$temp_dir"' EXIT

  sc_log "installing pinned sccache $SCCACHE_VERSION once for all worktrees"
  curl --proto '=https' --tlsv1.2 -fL --retry 3 --connect-timeout 15 --max-time 120 \
    -o "$archive" "$url" || return 1
  actual=$(sc_sha256 "$archive") || return 1
  if [[ $actual != "$expected" ]]; then
    sc_log "download checksum mismatch for $asset"
    return 1
  fi

  tar -xzf "$archive" -C "$temp_dir" || return 1
  extracted="$temp_dir/${asset%.tar.gz}/sccache"
  [[ -x $extracted ]] || return 1
  mkdir -p "$install_dir"
  staged="$install_dir/sccache.tmp.$$.$RANDOM"
  cp "$extracted" "$staged"
  chmod 0755 "$staged"
  # A concurrent worktree may install the same checksum-verified binary. Atomic replacement is safe.
  mv -f "$staged" "$install_dir/sccache"
  printf '%s\n' "$install_dir/sccache"
)

sc_resolve_binary() {
  local common_dir=$1
  local install_dir="$common_dir/codex-tools/sccache-v$SCCACHE_VERSION"
  local pinned="$install_dir/sccache"
  if [[ -n ${SCCACHE_BIN:-} ]]; then
    [[ -x $SCCACHE_BIN ]] || sc_die "SCCACHE_BIN is not executable: $SCCACHE_BIN"
    printf '%s\n' "$SCCACHE_BIN"
  elif [[ -x $pinned ]]; then
    printf '%s\n' "$pinned"
  elif sc_install_pinned "$install_dir"; then
    return 0
  elif command -v sccache >/dev/null 2>&1; then
    command -v sccache
  else
    return 1
  fi
}

sc_worktree_bases() {
  local line path separator=
  while IFS= read -r line; do
    [[ $line == 'worktree '* ]] || continue
    path=${line#worktree }
    printf '%s%s' "$separator" "$path"
    separator=:
  done < <(git -C "$ROOT" worktree list --porcelain)
  printf '\n'
}

sc_cargo_supports_shared_build_dir() {
  local version major minor
  version=$(cargo --version 2>/dev/null | awk '{print $2}')
  major=${version%%.*}
  version=${version#*.}
  minor=${version%%.*}
  [[ $major =~ ^[0-9]+$ && $minor =~ ^[0-9]+$ ]] || return 1
  (( major > 1 || (major == 1 && minor >= 91) ))
}

(( $# > 0 )) || sc_die 'pass the Cargo command to run'
if [[ ${SCCACHE_DISABLE:-0} == 1 ]]; then
  sc_log 'cache disabled explicitly'
  exec "$@"
fi

common_dir=$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) \
  || sc_die 'cannot resolve the repository git-common-dir'
if ! sccache_bin=$(sc_resolve_binary "$common_dir"); then
  sc_log 'WARNING: sccache is unavailable; continuing with the uncached Cargo command'
  exec "$@"
fi

cache_dir=${SCCACHE_DIR:-"$common_dir/codex-tools/sccache-cache"}
build_dir=${CARGO_BUILD_BUILD_DIR:-"$common_dir/codex-tools/cargo-build"}
base_dirs=${SCCACHE_BASEDIRS:-$(sc_worktree_bases)}
[[ -n $base_dirs ]] || base_dirs=$ROOT
server_id=$(printf '%s' "$base_dirs" | cksum | awk '{print $1}')
server_uds=${SCCACHE_SERVER_UDS:-"/tmp/claude-api-sccache-$(id -u)-$server_id.sock"}
build_env=()
mkdir -p "$cache_dir"
if sc_cargo_supports_shared_build_dir; then
  mkdir -p "$build_dir"
  build_env=("CARGO_BUILD_BUILD_DIR=$build_dir")
  sc_log "using $("$sccache_bin" --version) with shared compiler and Cargo build caches"
else
  sc_log "using $("$sccache_bin" --version); Cargo before 1.91 cannot share its build directory"
fi
exec env \
  RUSTC_WRAPPER="$sccache_bin" \
  CARGO_INCREMENTAL=0 \
  "${build_env[@]}" \
  SCCACHE_DIR="$cache_dir" \
  SCCACHE_BASEDIRS="$base_dirs" \
  SCCACHE_SERVER_UDS="$server_uds" \
  SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-10G}" \
  SCCACHE_IGNORE_SERVER_IO_ERROR="${SCCACHE_IGNORE_SERVER_IO_ERROR:-1}" \
  "$@"
