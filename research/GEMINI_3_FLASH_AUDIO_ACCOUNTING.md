# Gemini 3 Flash Preview — bounded audio-accounting proof — 2026-08-03

## Verdict

The previous publication withdrawal remains in force, but its audio-accounting blocker has a
bounded exact implementation path. The dormant `gemini-3-flash-preview` route may reconstruct a
missing generation `promptTokensDetails[AUDIO]` only for inline PCM WAV whose duration maps to an
integer under Google's official 32-audio-tokens-per-second contract. This implementation must pass
a new controlled Pro+Ultra exact-SHA gate before any production default or public catalog changes.

## Authority and observed gap

- Google audio documentation lists WAV, MP3, AIFF, AAC, OGG Vorbis and FLAC and states exactly
  `32 tokens per second of audio (1 minute = 1,920 tokens)`:
  <https://ai.google.dev/gemini-api/docs/audio> (reviewed 2026-08-03).
- The official v1beta discovery schema defines `UsageMetadata.promptTokensDetails` as the output-only
  list of modalities processed in request input and `cacheTokensDetails` as the corresponding cached
  modality list:
  <https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta> (reviewed 2026-08-03).
- The withdrawn exact-SHA run sent a valid 250 ms mono PCM WAV. Generation returned
  `promptTokenCount=55` without either modality list; private `countTokens` returned 4091 and also
  omitted the lists. Therefore countTokens is not request-matching generation authority.
- 250 ms is exactly 8 audio tokens under the published rate. Because this duration has no fractional
  token, reconstructing that one class does not require an undocumented rounding rule. The remaining
  47 aggregate prompt tokens stay in the generic non-audio class; no hidden-prompt estimate is used.

## Dormant implementation contract

The fallback is deliberately narrower than Google's full audio format list:

1. It is active only for generation on public `gemini-3-flash-preview` routed to private
   `gemini-3-flash`.
2. Input must be inline `audio/wav`. A strict Apache-2.0 `hound` parser validates RIFF length,
   PCM/IEEE-float format, channel alignment and the complete sample stream. `fileData` and compressed
   audio are rejected before upstream admission because their exact duration is not locally proven.
3. For each WAV, `frames × 32` must divide exactly by `sample_rate`. Fractional results are rejected;
   no floor, ceil or nearest rounding is invented. Multiple WAV parts sum with checked integer
   arithmetic. Channels do not multiply duration because Google combines channels.
4. Any provider-supplied `promptTokensDetails[AUDIO]`, including an explicit zero, remains authority.
   Reconstruction happens only when the AUDIO row is absent and the derived count fits the aggregate
   prompt total.
5. Cache attribution is accepted only when it is mathematically determined: cached count is zero,
   cached count equals the whole prompt, or `cacheTokensDetails[AUDIO]` is explicit. A partial cache
   without an AUDIO row fails closed because text and audio cache rates differ.
6. The exact reconstructed row is inserted before both public response serialization and
   `metering::gemini` parsing, so native JSON, SSE, settlement and immutable event evidence share one
   token vector. An unprovable non-stream response is not delivered; after an already-emitted SSE
   byte, the existing conservative-hold path applies without a fake usage event.

## Test and publication boundary

Runtime coverage follows the existing Gemini pattern and lives in Rust next to the implementation:
request parsing, duration/channel arithmetic, reserve estimation, unsupported media rejection,
fresh/full/explicit-partial/ambiguous cache cases, provider-authority precedence, public JSON/SSE
reconstruction and a mock-upstream endpoint test. `tools/gemini_calibration/test_run_live.py`
continues to test only the Python live runner; it is not the model implementation test suite.

No production model list, systemd default, router preset, web catalog, OpenKeys catalog or active
pricing generation changes belong in this dormant fix. Publication requires a new exact-SHA run of
all thinking levels, incremental SSE, text cache, fresh/replayed PCM WAV, forced tool and every other
dispatchable claimed control on each owned paid plan. Any mismatch keeps the withdrawal in force.

## Secret hygiene

This document contains only public contracts and already-sanitized aggregate evidence. It contains
no profile id, account/project identity, credential, raw capacity snapshot, prompt response text or
customer data.
