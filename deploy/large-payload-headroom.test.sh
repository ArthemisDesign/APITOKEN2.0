#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
HEADROOM=$ROOT/deploy/large-payload-headroom.sh
bash -n "$HEADROOM"

grep -Fq 'DISK_SPOOL_PREFIX=/var/lib/apitoken/spool/' "$HEADROOM" \
  || { echo 'headroom lost the disk-backed spool prefix' >&2; exit 1; }
grep -Fq 'RUN_ANTHROPIC_PREFIX=/run/claude-api-anthropic-' "$HEADROOM" \
  || { echo 'headroom lost the Anthropic /run prefix' >&2; exit 1; }
grep -Fq '17179869184' "$HEADROOM" \
  || { echo 'disk-backed headroom is not 16 GiB' >&2; exit 1; }
grep -Fq '8589934592' "$HEADROOM" \
  || { echo 'Anthropic /run headroom is not 8 GiB' >&2; exit 1; }
grep -Fq 'REJECT_VOLATILE_FS=1' "$HEADROOM" \
  || { echo 'disk-backed headroom does not reject tmpfs' >&2; exit 1; }
grep -Fq 'REJECT_VOLATILE_FS=0' "$HEADROOM" \
  || { echo 'Anthropic /run headroom no longer allows tmpfs' >&2; exit 1; }
grep -Fq '[[ $REJECT_VOLATILE_FS == 1 ]] && command -v findmnt' "$HEADROOM" \
  || { echo 'tmpfs reject is not gated on the disk-backed policy' >&2; exit 1; }

grep -Fq '"$HEADROOM_HELPER" "$CONTOUR_ROOTS_ANTHROPIC_RUNTIME_PREFIX-$TARGET_PORT"' \
  "$ROOT/deploy/engine-bluegreen.sh" \
  && grep -Fq '"$CONTOUR_UNITS_ANTHROPIC_SLICE"' "$ROOT/deploy/engine-bluegreen.sh" \
  || { echo 'Anthropic cutover no longer gates /run RuntimeDirectory' >&2; exit 1; }
grep -Fq '"$HEADROOM_HELPER" "$CONTOUR_ROOTS_SPOOL/gemini-$GEMINI_TARGET_PORT" "$CONTOUR_UNITS_GEMINI_SLICE"' \
  "$ROOT/deploy/engine-bluegreen.sh" \
  || { echo 'Gemini cutover no longer gates the disk-backed spool' >&2; exit 1; }
grep -Fq '"$HEADROOM_HELPER" "$CONTOUR_ROOTS_SPOOL/openai-$OPENAI_TARGET_PORT" "$CONTOUR_UNITS_OPENAI_SLICE"' \
  "$ROOT/deploy/engine-bluegreen.sh" \
  || { echo 'OpenAI cutover no longer gates the disk-backed spool' >&2; exit 1; }
grep -Fq '"$HEADROOM_HELPER" "$CONTOUR_ROOTS_SPOOL/router-$TARGET_PORT"' \
  "$ROOT/deploy/router-bluegreen.sh" \
  && grep -Fq '"$CONTOUR_UNITS_ROUTER_SLICE"' "$ROOT/deploy/router-bluegreen.sh" \
  || { echo 'router cutover no longer gates the disk-backed spool' >&2; exit 1; }
grep -Fq 'large-payload-headroom.sh /var/lib/apitoken/spool/router-[0-9]* claude-router.slice' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || { echo 'sudoers lost the disk-backed router headroom command' >&2; exit 1; }
grep -Fq 'large-payload-headroom.sh /var/lib/apitoken/spool/gemini-[0-9]* claude-api-gemini.slice' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || { echo 'sudoers lost the disk-backed Gemini headroom command' >&2; exit 1; }
grep -Fq 'large-payload-headroom.sh /var/lib/apitoken/spool/openai-[0-9]* claude-api-openai.slice' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || { echo 'sudoers lost the disk-backed OpenAI headroom command' >&2; exit 1; }
grep -Fq 'large-payload-headroom.sh /run/claude-api-anthropic-[0-9]* claude-api-anthropic.slice' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || { echo 'sudoers lost the Anthropic /run headroom command' >&2; exit 1; }

echo 'large-payload headroom path policy passed'
