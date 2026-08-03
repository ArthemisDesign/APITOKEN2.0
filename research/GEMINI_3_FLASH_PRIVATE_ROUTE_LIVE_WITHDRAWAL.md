# Gemini 3 Flash private route — exact-SHA publication withdrawal — 2026-08-03

## Verdict

`gemini-3-flash-preview` remains dormant and must not be added to production defaults, the public
model catalog, router presets, the site, OpenKeys or an active pricing generation. The private
`gemini-3-flash` route generated real responses on both owned paid plans, but the exact-SHA gate
failed closed on authoritative audio accounting: a successful audio turn had no audio token class,
and the free token counter supplied no modality breakdown that could repair that omission.

This is a publication withdrawal, not a rollback of the dormant implementation. The reviewed
public-to-private mapping, public `modelVersion` rewrite, quota-row join and official rate card stay
available for a future controlled re-probe. They are not production availability evidence.

## Immutable identities

- Public model: `gemini-3-flash-preview`.
- Private generation model: `gemini-3-flash`.
- Quota rows: `gemini-3-flash` plus `gemini-3-flash-agent`.
- Exact dormant implementation SHA: `22636b1c8f2ef4c1ff9ec52646d511e160eeef05`.
- Frozen engine SHA-256:
  `db9cb793ca7974694f724d9cf11aa13b6d22bb19d7d7f55753b6722873f727ec`.
- Live-runner correction SHA: `7ab01feb0a2e50bf522fee8c7d52ed90bcc4ae24`; its full local gate,
  trusted-host validation and production `deploy/watchdog` were GREEN before the resumed run.
- Plans: one owned Google AI Pro profile and one owned Google AI Ultra profile, addressed only by
  opaque exact-profile targets through the admin-only calibration route.

The canary ran the frozen implementation binary on an isolated loopback port while inheriting the
production PostgreSQL/billing authority. Its executable, candidate HEAD and SHA-256 were checked
before start. The canary was terminated after the verdict, its port was closed, and the stable
Gemini plane remained healthy.

## Admission and budget

The free preflight completed `22/22` `countTokens` calls across both plans. Twenty generation legs
had a proved aggregate dispatch ceiling of `20,989,952,000 nanoUSD` under the explicitly approved
`$21` cap. Two per-query Search legs had no provider-published hard fanout ceiling and were therefore
not dispatchable. No Search generation was sent.

The run stopped after 15 immutable records with total actual spend `30,329,500 nanoUSD`
(`$0.0303295`). Every paid request had exactly one dispatch. The already completed Pro `minimal`
turn was retained across resume and was not repeated. Seven matrix legs remained pending after the
blocking miss; none was sent afterward.

## Evidence that passed

Both Pro and Ultra produced public `modelVersion=gemini-3-flash-preview`, visible non-thought text,
terminal finish/usage and response-to-immutable-event usage parity for:

- `minimal`, `low`, `medium` and `high` thinking levels;
- incremental SSE with two candidate-bearing frames;
- cache write followed by a read with an observed cached-input token class.

`minimal` produced zero thinking tokens on both plans while still returning complete visible output
and terminal evidence. This is valid dynamic minimal behavior. `low`, `medium` and `high` each
reported a positive thinking token class on both plans.

These successes do not override a later blocking capability/accounting failure. Forced tool calls,
audio replay and Search generation were not reached after the runner stopped.

## Blocking audio evidence

The Pro `audio-fresh` request contained a valid 250 ms mono PCM WAV plus a short text instruction.
Generation returned 2xx with:

- public `modelVersion=gemini-3-flash-preview`;
- six visible non-thought characters and terminal finish/usage;
- exact response/event usage parity;
- `promptTokenCount=55`, output `119`, thoughts `118`;
- `audio_input_tokens=0` and `cached_audio_input_tokens=0`;
- actual recorded cost `384,500 nanoUSD`, with all 55 prompt tokens indistinguishable from the
  lower-priced generic text input class.

The runner therefore emitted the blocking reason `audio input token class was not observed`,
persisted the already incurred spend and stopped before any further paid request.

There is no parser-divergence evidence that can authorize publication. Rust metering and the
independent live-runner parser both derive audio only from the reviewed public field
`usageMetadata.promptTokensDetails[modality=AUDIO]`; their complete vectors matched exactly. Raw
generation metadata is deliberately not retained, so the report cannot distinguish an upstream
omission from an unreviewed private metadata spelling. Either case lacks authoritative public
contract evidence. A subsequent free-only diagnostic repeated `countTokens` for the identical
audio body on both Pro and Ultra. Each returned `totalTokens=4091` while omitting
`promptTokensDetails`, `cacheTokensDetails` and `cachedContentTokenCount` entirely. Besides lacking
the required modality, that count differs radically from the generation's authoritative prompt
total of 55, so it cannot be substituted into settlement.

Treating all 55 generation prompt tokens as audio would invent evidence and overprice the mixed
text/audio request. Treating them as text underprices the official `$1/M` audio input rate. Either
choice violates the exact integer API-cost authority. Publication is therefore unsafe even though
the model understood the clip and generated a valid answer.

## Withdrawal consequences

- Keep the model absent from both Gemini systemd default lists and the stable production allowlist.
- Keep `google/gemini-3-flash-preview` absent from router presets and the public web/docs catalog.
- Do not create or activate a new pricing capability/catalog generation for this model; immutable
  rejected generation 4 remains historical and unchanged.
- Keep OpenKeys and admin product catalogs unchanged.
- Preserve the dormant mapping and tests so a future upstream change can be re-probed without
  guessing the private route again.

A future reconsideration needs new provider evidence that exact generation usage exposes a positive
audio modality class (or another authoritative, request-matching breakdown), followed by a fresh
exact-SHA controlled matrix on every claimed paid plan. The failed audio generation itself must
never be replayed.

## Secret hygiene

- No bearer, refresh token, API/control key, project, email or provider subject was written here.
- Opaque profile IDs and raw capacity snapshots remain only in the local `/tmp` report and are not
  committed.
- Generated response text was never persisted; only bounded counters and booleans were retained.
- Production customer traffic, stable slots and public catalogs were not changed.
