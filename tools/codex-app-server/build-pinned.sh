#!/usr/bin/env bash
set -euo pipefail

umask 077

usage() {
  echo "usage: $0 --install-dir /absolute/path" >&2
  exit 2
}

install_dir=""
while (($#)); do
  case "$1" in
    --install-dir)
      (($# >= 2)) || usage
      install_dir="$2"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

case "$install_dir" in
  /*) ;;
  *) usage ;;
esac
if [[ "$install_dir" == "/" ]]; then
  echo "refusing broad install directory /" >&2
  exit 2
fi

script_dir="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=UPSTREAM.pin
source "$script_dir/UPSTREAM.pin"
patch_path="$script_dir/$CODEX_PATCH"

for command_name in git cargo rustc install mktemp; do
  command -v "$command_name" >/dev/null || {
    echo "required command is missing: $command_name" >&2
    exit 1
  }
done

sha256_file() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

actual_patch_sha="$(sha256_file "$patch_path")"
if [[ "$actual_patch_sha" != "$CODEX_PATCH_SHA256" ]]; then
  echo "Codex patch digest mismatch" >&2
  exit 1
fi

build_root="$(mktemp -d "${TMPDIR:-/tmp}/apitoken-codex-build.XXXXXXXX")"
case "$build_root" in
  "${TMPDIR:-/tmp}"/apitoken-codex-build.*) ;;
  *)
    echo "unexpected temporary directory: $build_root" >&2
    exit 1
    ;;
esac
cleanup() {
  rm -rf -- "$build_root"
}
trap cleanup EXIT

source_dir="$build_root/source"
git init -q "$source_dir"
git -C "$source_dir" remote add origin "$CODEX_GIT_URL"
git -C "$source_dir" -c protocol.version=2 fetch -q --depth=1 origin \
  "refs/tags/$CODEX_GIT_TAG:refs/tags/$CODEX_GIT_TAG"
tag_commit="$(git -C "$source_dir" rev-parse "refs/tags/$CODEX_GIT_TAG^{commit}")"
if [[ "$tag_commit" != "$CODEX_GIT_COMMIT" ]]; then
  echo "Codex tag resolved to unexpected commit: $tag_commit" >&2
  exit 1
fi
git -C "$source_dir" switch -q --detach "$CODEX_GIT_COMMIT"

original_lock="$source_dir/codex-rs/Cargo.lock"
if [[ "$(sha256_file "$original_lock")" != "$CODEX_ORIGINAL_LOCK_SHA256" ]]; then
  echo "upstream Cargo.lock digest mismatch" >&2
  exit 1
fi
git -C "$source_dir" apply --check "$patch_path"
git -C "$source_dir" apply "$patch_path"
git -C "$source_dir" diff --check

export CARGO_NET_GIT_FETCH_WITH_CLI=true
export CARGO_TARGET_DIR="$build_root/target"
(
  cd "$source_dir/codex-rs"
  # The release tag intentionally checks in workspace packages as 0.0.0. Cargo performs one
  # deterministic local-version normalization; the resulting lock is attested before any build.
  cargo metadata --format-version=1 >/dev/null
)
if [[ "$(sha256_file "$original_lock")" != "$CODEX_NORMALIZED_LOCK_SHA256" ]]; then
  echo "normalized Cargo.lock digest mismatch" >&2
  exit 1
fi

(
  cd "$source_dir/codex-rs"
  # These two patch proofs are library unit tests. Restrict Cargo to that target: an unfiltered
  # release test invocation also starts unrelated integration-test constructors, whose upstream
  # arg0 safety guard deliberately rejects their temporary CODEX_HOME on Linux.
  cargo test --locked --release -p codex-core --lib \
    apitoken_openai_compat -- --nocapture
  cargo test --locked --release -p codex-app-server --lib \
    apitoken_openai_compat -- --nocapture
  # The official app-server integration harness deliberately passes a debug-only isolation flag
  # to the spawned production binary. Running this one test with --release removes that flag from
  # clap and fails before the request-capture assertion is reached.
  cargo test --locked -p codex-app-server \
    thread_start_omits_empty_instruction_overrides_from_model_request -- --nocapture
  cargo build --locked --release -p codex-cli --bin codex
)

built_binary="$CARGO_TARGET_DIR/release/codex"
actual_version="$("$built_binary" --version)"
if [[ "$actual_version" != "$CODEX_EXPECTED_VERSION" ]]; then
  echo "Codex version mismatch: $actual_version" >&2
  exit 1
fi
binary_sha="$(sha256_file "$built_binary")"
install -d -m 0755 "$install_dir"
versioned_binary="$install_dir/codex-$CODEX_GIT_COMMIT-$binary_sha"
if [[ -e "$versioned_binary" ]]; then
  if [[ "$(sha256_file "$versioned_binary")" != "$binary_sha" ]]; then
    echo "existing versioned Codex binary has a different digest" >&2
    exit 1
  fi
else
  install -m 0555 "$built_binary" "$versioned_binary"
fi

link_tmp="$install_dir/.codex-link.$$"
ln -s "$(basename -- "$versioned_binary")" "$link_tmp"
mv -f -- "$link_tmp" "$install_dir/codex"

echo "CODEX_BINARY=$install_dir/codex"
echo "CODEX_BINARY_SHA256=$binary_sha"
echo "CODEX_VERSION=$actual_version"
echo "CODEX_SOURCE_COMMIT=$CODEX_GIT_COMMIT"
