# Gemini 3 Flash cache-liveness gate — live withdrawal — 2026-08-03

## Verdict

`gemini-3-flash-preview` remains dormant. The corrected optional tool-subset candidate passed both
owned paid plans through all four thinking levels and incremental SSE, then produced successful
profile-isolated cache writes. The first byte-identical cache replay returned entirely fresh input,
so the runner stopped before audio, tools and Search. This terminal run is not resumed and does not
authorize publication.

## Immutable candidate and isolation

- Exact implementation/runner SHA: `d0a9fb4052773517e987d1a79664965a131ef1ac`; trusted validation
  and production `deploy/watchdog` were GREEN.
- Frozen runtime binary SHA-256:
  `316f6727f8b9e9fc1c744fc9c33362b15c9d730c4f971a0e8467b5181ee2f877`.
- Plans: one owned Google AI Pro profile and one owned Google AI Ultra profile, addressed only by
  opaque exact-profile calibration targets.
- The binary ran in an isolated user-systemd canary on loopback port `18895`, with production
  PostgreSQL, billing and immutable calibration authority. Stable Gemini traffic and public
  catalogs were unchanged. The canary was stopped after the verdict, port `18895` was confirmed
  closed and the stable Gemini plane remained healthy.

## Budget and passed evidence

The explicitly approved aggregate ceiling remained `$21`. Thirteen paid turns were durably
recorded for `34,320,500 nanoUSD` (`$0.0343205`); nine legs were not dispatched. On both Pro and
Ultra the run proved:

- `minimal`, `low`, `medium` and `high`, with public model identity, visible output, terminal usage
  and exact response/event parity;
- incremental SSE with two candidate-bearing frames;
- profile-isolated cache-write payloads with 12,343 fresh input tokens and terminal visible output.

## Blocking cache evidence

The first profile's replay used a request body byte-identical to its write and the same stable
profile scope. Generation returned public `modelVersion=gemini-3-flash-preview`, visible output,
terminal finish/usage and exact response/event parity. Its immutable vector nevertheless contained
12,343 fresh input tokens, 512 output tokens and zero cache-read tokens. The official-rate charge
was therefore the full `7,707,500 nanoUSD`, exactly the same as a fresh turn. The runner preserved
that spend, emitted the blocking reason `cached input token class was not observed` and sent no
later paid leg.

The report cannot establish why Google's implicit cache skipped this replay. It does establish one
avoidable harness interval: coverage was scheduled leg-first, so the other paid profile's cache
write and mandatory immutable/quota propagation wait occurred between this profile's write and
read. The payload isolation fix prevented cross-profile warmth but did not make each replay pair
temporally local.

## Follow-up boundary

A later dormant runner may schedule each byte-identical cache/audio pair consecutively within one
profile before moving to the next profile. This removes the controllable cross-profile liveness gap
without weakening proof: the adjacent read must still expose a positive authoritative cache class,
and a miss remains terminal rather than being retried or repriced. The failed request and this run
are never resumed. A wholly fresh exact-SHA Pro+Ultra matrix with a new run id is required.

Until that matrix is complete and GREEN, production defaults, router presets, public
contracts/catalogs, web/docs, OpenKeys, admin and active pricing generations remain unchanged.

No credential, key, project, email, provider subject, raw profile id, capacity snapshot, generated
text or machine report is committed here.
