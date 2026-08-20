#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${CLAUDE_API_TEST_DATABASE_URL:-}" ]]; then
  printf '%s\n' 'CLAUDE_API_TEST_DATABASE_URL is required for the Stage 5 real-PostgreSQL gate.' >&2
  exit 64
fi

case "${CLAUDE_API_TEST_DATABASE_URL}" in
  *localhost*|*127.0.0.1*|*'[::1]'*) ;;
  *)
    printf '%s\n' 'Refusing a non-loopback PostgreSQL URL for the Stage 5 destructive test gate.' >&2
    exit 64
    ;;
esac

export GEMINI_BATCH_STAGE5_LOAD_ITEMS="${GEMINI_BATCH_STAGE5_LOAD_ITEMS:-1000}"
export GEMINI_BATCH_STAGE5_LOGICAL_JSONL_BYTES="${GEMINI_BATCH_STAGE5_LOGICAL_JSONL_BYTES:-2147483000}"

cargo test -p registry pg::gemini_batch_tests::stage5_resilience_postgres_matrix -- --exact --nocapture
cargo test -p registry pg::gemini_batch_tests::stage5_postgres_load_and_fairness -- --exact --nocapture
cargo test -p forward gemini::batch_handlers::tests::stage5_synthetic_near_2gb_jsonl_is_streamed_in_order -- --exact --nocapture
CLAUDE_API_GEMINI_BATCH_HTTP_LIFECYCLE=1 cargo test -p forward gemini::api::tests::gemini_batch_public_handlers_postgres_lifecycle_files_and_account_isolation -- --exact --nocapture
