#!/usr/bin/env bash
set -euo pipefail

# Promote only a Codex binary that was built inside the immutable watchdog candidate. The helper
# runs as root because the production binary directory is root-owned and config.env may contain
# secrets owned by another service account. It never prints or sources that file.
umask 077

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
LIB=$SCRIPT_DIR/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is unavailable\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

CODEX_PROMOTE_STATE_ROOT=/var/lib/apitoken/watchdog
CODEX_PROMOTE_CANDIDATE_ROOT=$CODEX_PROMOTE_STATE_ROOT/candidates
CODEX_PROMOTE_BIN_ROOT=/srv/claude-api/data/codex/bin
CODEX_PROMOTE_CONFIG_ENV=/srv/claude-api/data/config.env
CODEX_PROMOTE_ATTESTATION=$CODEX_PROMOTE_BIN_ROOT/.promoted
CODEX_PROMOTE_EXPECTED_CANDIDATE_UID=0

codex_stat_identity() {
  local path=$1
  if stat -c '%u %g %a' -- "$path" >/dev/null 2>&1; then
    stat -c '%u %g %a' -- "$path"
  else
    stat -f '%u %g %Lp' -- "$path"
  fi
}

codex_replace_config() (
  local binary=$1 digest=$2 version=$3
  local config=$CODEX_PROMOTE_CONFIG_ENV identity uid gid mode temporary key count

  [[ -f $config && ! -L $config ]] || wd_die "Codex config target is missing or unsafe: $config"
  for key in CLAUDE_API_CODEX_BIN CLAUDE_API_CODEX_BIN_SHA256 CLAUDE_API_CODEX_VERSION; do
    count=$(grep -Ec "^${key}=" "$config" 2>/dev/null || true)
    (( count <= 1 )) || wd_die "duplicate $key in $config"
  done

  identity=$(codex_stat_identity "$config") || wd_die "cannot inspect $config"
  read -r uid gid mode <<<"$identity"
  [[ $uid =~ ^[0-9]+$ && $gid =~ ^[0-9]+$ && $mode =~ ^[0-7]{3,4}$ ]] \
    || wd_die "cannot preserve config ownership and mode"

  temporary="${config}.codex.tmp.$$"
  [[ ! -e $temporary && ! -L $temporary ]] || wd_die "unsafe temporary config path: $temporary"
  trap 'rm -f -- "${temporary:-}"' EXIT
  awk \
    -v binary="$binary" \
    -v digest="$digest" \
    -v version="$version" \
    '
      !/^CLAUDE_API_CODEX_BIN=/ &&
      !/^CLAUDE_API_CODEX_BIN_SHA256=/ &&
      !/^CLAUDE_API_CODEX_VERSION=/ { print }
      END {
        print "CLAUDE_API_CODEX_BIN=" binary
        print "CLAUDE_API_CODEX_BIN_SHA256=" digest
        print "CLAUDE_API_CODEX_VERSION=" version
      }
    ' "$config" >"$temporary"
  chown "$uid:$gid" "$temporary"
  chmod "$mode" "$temporary"
  mv -f -- "$temporary" "$config"
  trap - EXIT
)

promote_codex_candidate() {
  local sha=$1 candidate marker marker_sha marker_tree candidate_sha candidate_tree
  local artifact expected_hash actual_hash source_commit version versioned temporary_link
  local bin_owner tooling_tree attestation_format temporary_attestation

  wd_validate_sha "$sha"
  candidate="$CODEX_PROMOTE_CANDIDATE_ROOT/$sha"
  marker="$CODEX_PROMOTE_STATE_ROOT/$sha.tested"
  artifact="$candidate/.deploy-artifacts/codex/codex"

  [[ -d $candidate && ! -L $candidate ]] || wd_die "tested candidate directory is missing"
  [[ $(codex_stat_identity "$candidate" | awk '{print $1}') == "$CODEX_PROMOTE_EXPECTED_CANDIDATE_UID" ]] \
    || wd_die "tested candidate has the wrong owner"
  [[ -f $marker && ! -L $marker ]] || wd_die "candidate test-success marker is missing"
  marker_sha=$(wd_marker_value "$marker" sha) || wd_die "candidate marker has no SHA"
  marker_tree=$(wd_marker_value "$marker" tree) || wd_die "candidate marker has no tree"
  [[ $marker_sha == "$sha" ]] || wd_die "candidate marker SHA mismatch"
  candidate_sha=$(git -c safe.directory="$candidate" -C "$candidate" rev-parse 'HEAD^{commit}')
  candidate_tree=$(git -c safe.directory="$candidate" -C "$candidate" rev-parse 'HEAD^{tree}')
  [[ $candidate_sha == "$sha" && $candidate_tree == "$marker_tree" ]] \
    || wd_die "tested candidate identity changed after validation"
  [[ -z $(git -c safe.directory="$candidate" -C "$candidate" \
    status --porcelain --untracked-files=no) ]] || wd_die "tested candidate has tracked modifications"

  [[ $(wd_marker_value "$marker" codex_artifacts) == 1 ]] \
    || wd_die "candidate has no tested Codex artifact"
  expected_hash=$(wd_marker_value "$marker" codex_binary_sha256) \
    || wd_die "candidate marker has no Codex digest"
  source_commit=$(wd_marker_value "$marker" codex_source_commit) \
    || wd_die "candidate marker has no Codex source commit"
  version=$(wd_marker_value "$marker" codex_version) \
    || wd_die "candidate marker has no Codex version"
  [[ $expected_hash =~ ^[0-9a-f]{64}$ ]] || wd_die "candidate Codex digest is malformed"
  [[ $source_commit =~ ^[0-9a-f]{40}$ ]] || wd_die "candidate Codex source commit is malformed"
  [[ $version =~ ^codex-cli\ [0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || wd_die "candidate Codex version is malformed"
  [[ -f $artifact && ! -L $artifact && -x $artifact ]] \
    || wd_die "tested Codex artifact is missing or unsafe"
  actual_hash=$(wd_sha256_file "$artifact")
  [[ $actual_hash == "$expected_hash" ]] || wd_die "tested Codex artifact digest changed"
  tooling_tree=$(git -c safe.directory="$candidate" -C "$candidate" \
    rev-parse 'HEAD:tools/codex-app-server')
  [[ $tooling_tree =~ ^[0-9a-f]{40}$ ]] || wd_die "candidate Codex tooling tree is malformed"
  attestation_format=$(<"$candidate/tools/codex-app-server/PROMOTION_ATTESTATION_FORMAT")
  [[ $attestation_format == 1 ]] || wd_die "candidate Codex attestation format is unsupported"

  if [[ -e $CODEX_PROMOTE_BIN_ROOT || -L $CODEX_PROMOTE_BIN_ROOT ]]; then
    [[ -d $CODEX_PROMOTE_BIN_ROOT && ! -L $CODEX_PROMOTE_BIN_ROOT ]] \
      || wd_die "Codex binary root is unsafe"
    bin_owner=$(codex_stat_identity "$CODEX_PROMOTE_BIN_ROOT" | awk '{print $1}')
    [[ $bin_owner == "$CODEX_PROMOTE_EXPECTED_CANDIDATE_UID" ]] \
      || wd_die "Codex binary root has the wrong owner"
  else
    install -d -m 0755 -- "$CODEX_PROMOTE_BIN_ROOT"
  fi

  versioned="$CODEX_PROMOTE_BIN_ROOT/codex-$source_commit-$expected_hash"
  if [[ -e $versioned || -L $versioned ]]; then
    [[ -f $versioned && ! -L $versioned ]] || wd_die "versioned Codex target is unsafe"
    [[ $(wd_sha256_file "$versioned") == "$expected_hash" ]] \
      || wd_die "existing versioned Codex binary has a different digest"
  else
    install -m 0555 -- "$artifact" "$versioned"
  fi

  # Engine slots use the immutable versioned path. Updating config first is safe for the currently
  # running slot (it already loaded its environment), while a later slot can never observe a
  # symlink/hash mismatch.
  codex_replace_config "$versioned" "$expected_hash" "$version"

  temporary_link="$CODEX_PROMOTE_BIN_ROOT/.codex-link.$$"
  [[ ! -e $temporary_link && ! -L $temporary_link ]] \
    || wd_die "unsafe temporary Codex link path"
  if [[ -e $CODEX_PROMOTE_BIN_ROOT/codex || -L $CODEX_PROMOTE_BIN_ROOT/codex ]]; then
    [[ ! -d $CODEX_PROMOTE_BIN_ROOT/codex ]] || wd_die "stable Codex path is a directory"
  fi
  ln -s -- "$(basename -- "$versioned")" "$temporary_link"
  mv -f -- "$temporary_link" "$CODEX_PROMOTE_BIN_ROOT/codex"

  # This public, root-owned state is the desired/runtime fence used by the watchdog. Git history is
  # not runtime state: a failed candidate may already have promoted a different binary while the
  # last-green component SHA correctly remains unchanged. Write the attestation last so any crash
  # before the complete config+symlink promotion is observed as drift and retried fail-closed.
  temporary_attestation="${CODEX_PROMOTE_ATTESTATION}.tmp.$$"
  [[ ! -e $temporary_attestation && ! -L $temporary_attestation ]] \
    || wd_die "unsafe temporary Codex attestation path"
  trap 'rm -f -- "${temporary_attestation:-}"' EXIT
  {
    printf 'format=%s\n' "$attestation_format"
    printf 'candidate_sha=%s\n' "$sha"
    printf 'tooling_tree=%s\n' "$tooling_tree"
    printf 'source_commit=%s\n' "$source_commit"
    printf 'binary_sha256=%s\n' "$expected_hash"
    printf 'version=%s\n' "$version"
  } >"$temporary_attestation"
  chmod 0444 "$temporary_attestation"
  mv -f -- "$temporary_attestation" "$CODEX_PROMOTE_ATTESTATION"
  trap - EXIT

  wd_log "promoted tested Codex $version ($expected_hash) from candidate $sha"
}

codex_promote_main() {
  [[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "Codex promotion must run as root"
  [[ $# -eq 1 ]] || wd_die "usage: watchdog-codex-promote.sh <tested-full-sha>"
  promote_codex_candidate "$1"
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  codex_promote_main "$@"
fi
