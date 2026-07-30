#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-codex-promote.sh
source "$ROOT/deploy/watchdog-codex-promote.sh"

TEMP=$(mktemp -d "${TMPDIR:-/tmp}/apitoken-codex-promote-test.XXXXXXXX")
trap 'rm -rf -- "$TEMP"' EXIT

CODEX_PROMOTE_STATE_ROOT="$TEMP/state"
CODEX_PROMOTE_CANDIDATE_ROOT="$CODEX_PROMOTE_STATE_ROOT/candidates"
CODEX_PROMOTE_BIN_ROOT="$TEMP/data/codex/bin"
CODEX_PROMOTE_CONFIG_ENV="$TEMP/data/config.env"
CODEX_PROMOTE_ATTESTATION="$CODEX_PROMOTE_BIN_ROOT/.promoted"
CODEX_PROMOTE_EXPECTED_CANDIDATE_UID=$(id -u)
mkdir -p "$CODEX_PROMOTE_CANDIDATE_ROOT" "$TEMP/data"

source_repo="$TEMP/source"
git init --quiet "$source_repo"
git -C "$source_repo" config user.name test
git -C "$source_repo" config user.email test@example.invalid
mkdir -p "$source_repo/tools/codex-app-server"
printf 'fixture\n' >"$source_repo/tracked"
printf '1\n' >"$source_repo/tools/codex-app-server/PROMOTION_ATTESTATION_FORMAT"
git -C "$source_repo" add tracked tools/codex-app-server/PROMOTION_ATTESTATION_FORMAT
git -C "$source_repo" commit --quiet -m fixture
sha=$(git -C "$source_repo" rev-parse HEAD)
candidate="$CODEX_PROMOTE_CANDIDATE_ROOT/$sha"
mv "$source_repo" "$candidate"

mkdir -p "$candidate/.deploy-artifacts/codex"
artifact="$candidate/.deploy-artifacts/codex/codex"
printf '#!/bin/sh\nexit 0\n' >"$artifact"
chmod 0555 "$artifact"
digest=$(wd_sha256_file "$artifact")
tree=$(git -C "$candidate" rev-parse 'HEAD^{tree}')
tooling_tree=$(git -C "$candidate" rev-parse 'HEAD:tools/codex-app-server')
source_commit=25af12f7e61572b0bc18ddb1008be543b91519b0
marker="$CODEX_PROMOTE_STATE_ROOT/$sha.tested"
cat >"$marker" <<EOF
sha=$sha
tree=$tree
codex_artifacts=1
codex_binary_sha256=$digest
codex_source_commit=$source_commit
codex_version=codex-cli 0.145.0
EOF
printf 'SECRET_FIXTURE=preserved\nCLAUDE_API_CODEX_BIN=/old/codex\n' \
  >"$CODEX_PROMOTE_CONFIG_ENV"
chmod 0640 "$CODEX_PROMOTE_CONFIG_ENV"

promote_codex_candidate "$sha"

versioned="$CODEX_PROMOTE_BIN_ROOT/codex-$source_commit-$digest"
[[ -f $versioned && ! -L $versioned && -x $versioned ]] \
  || wd_die "Codex promotion did not install an immutable executable"
[[ $(wd_sha256_file "$versioned") == "$digest" ]] \
  || wd_die "promoted Codex digest changed"
[[ $(readlink "$CODEX_PROMOTE_BIN_ROOT/codex") == "$(basename -- "$versioned")" ]] \
  || wd_die "stable Codex link does not select the promoted binary"
[[ -f $CODEX_PROMOTE_ATTESTATION && ! -L $CODEX_PROMOTE_ATTESTATION ]] \
  || wd_die "Codex promotion did not publish a regular runtime attestation"
grep -Fxq 'format=1' "$CODEX_PROMOTE_ATTESTATION" \
  || wd_die "Codex promotion attestation has the wrong format"
grep -Fxq "candidate_sha=$sha" "$CODEX_PROMOTE_ATTESTATION" \
  || wd_die "Codex promotion attestation lost the candidate SHA"
grep -Fxq "tooling_tree=$tooling_tree" "$CODEX_PROMOTE_ATTESTATION" \
  || wd_die "Codex promotion attestation lost the tooling tree"
grep -Fxq "source_commit=$source_commit" "$CODEX_PROMOTE_ATTESTATION" \
  || wd_die "Codex promotion attestation lost the upstream commit"
grep -Fxq "binary_sha256=$digest" "$CODEX_PROMOTE_ATTESTATION" \
  || wd_die "Codex promotion attestation lost the binary digest"
grep -Fxq 'version=codex-cli 0.145.0' "$CODEX_PROMOTE_ATTESTATION" \
  || wd_die "Codex promotion attestation lost the tested version"
grep -Fxq 'SECRET_FIXTURE=preserved' "$CODEX_PROMOTE_CONFIG_ENV" \
  || wd_die "Codex promotion did not preserve unrelated config"
grep -Fxq "CLAUDE_API_CODEX_BIN=$versioned" "$CODEX_PROMOTE_CONFIG_ENV" \
  || wd_die "Codex promotion did not select the immutable binary"
grep -Fxq "CLAUDE_API_CODEX_BIN_SHA256=$digest" "$CODEX_PROMOTE_CONFIG_ENV" \
  || wd_die "Codex promotion did not attest the immutable binary"
grep -Fxq 'CLAUDE_API_CODEX_VERSION=codex-cli 0.145.0' "$CODEX_PROMOTE_CONFIG_ENV" \
  || wd_die "Codex promotion did not record the tested version"
[[ $(grep -Ec '^CLAUDE_API_CODEX_BIN=' "$CODEX_PROMOTE_CONFIG_ENV") == 1 ]] \
  || wd_die "Codex promotion left duplicate config keys"

chmod u+w "$artifact"
printf '\n' >>"$artifact"
if ( promote_codex_candidate "$sha" >/dev/null 2>&1 ); then
  wd_die "Codex promotion accepted an artifact changed after the test marker"
fi

printf 'watchdog Codex promotion tests passed\n'
