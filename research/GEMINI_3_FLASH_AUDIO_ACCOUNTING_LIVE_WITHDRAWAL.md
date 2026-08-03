# Gemini 3 Flash audio-accounting candidate — live withdrawal — 2026-08-03

## Verdict

`gemini-3-flash-preview` remains dormant. The exact audio-accounting implementation passed public
identity, visible output, terminal usage and all four thinking levels plus incremental SSE on both
owned paid plans, but the controlled matrix then failed on the Google AI Ultra cache-write turn.
The paid response had no visible non-thought output and did not match its immutable usage vector.
Per the publication gate, the run stopped and cannot be resumed or used to publish the model.

## Immutable candidate and isolation

- Exact implementation SHA: `4b0c6443b55eb1839bdd9ccbe1cc8e5bb1cc8214`.
- Frozen engine SHA-256:
  `84f7249bf9026d1cd27307b3848afe690beaa18cd1d16db1e86215f8f06ee4bd`.
- Plans: one owned Google AI Pro profile and one owned Google AI Ultra profile, selected only by
  opaque admin calibration targets.
- The exact release binary ran in an isolated user-systemd canary on loopback port `18895`, with
  production PostgreSQL, billing and immutable calibration authority. The stable Gemini plane and
  public catalogs were not changed.
- The canary was stopped after the verdict and port `18895` was confirmed closed.

## Budget and dispatch

The runner used the explicitly approved aggregate `$21` ceiling. Twelve paid turns were durably
recorded before the blocking result; actual official-rate spend was `19,886,500 nanoUSD`
(`$0.0198865`). Ten legs remained pending and were not dispatched. The report is terminal with
`complete=false`, `resume_safe=false` and one blocking unavailable capability.

## Evidence that passed

On both Pro and Ultra, the following produced public
`modelVersion=gemini-3-flash-preview`, visible non-thought output, terminal finish and usage, and an
exact response-to-immutable-event usage match:

- `minimal`, `low`, `medium` and `high` thinking levels;
- incremental SSE with two candidate-bearing frames.

The Pro cache-write turn also returned visible output and exact response/event usage parity. These
successes do not override the later required-capability failure.

## Blocking cache evidence

The Ultra cache-write turn returned a terminal one-frame response with the correct public model id,
but zero visible text, zero function calls and zero inline media. Its immutable event contained
`4,167` fresh input tokens, `8,170` cache-read tokens and zero output tokens. The response usage did
not match that immutable vector. No cache-read, audio, tool or Search generation was sent after this
failure.

The nominal Ultra write observing cache-read tokens exposed a runner isolation defect: matrix order
is leg-first, then profile, while the cache key contained only run and model identity. The preceding
Pro write could therefore warm the same provider cache lineage before Ultra's supposed first write.
The 128-token cache output ceiling was independently too small for a reliable Flash Preview gate:
this turn ended without visible output, and the prior withdrawn audio turn had already consumed
119 of 128 output tokens. Neither fact may be reinterpreted as successful capability evidence.

## Follow-up candidate boundary

A later dormant candidate may:

1. derive a deterministic cache scope from the stable profile ordering, without placing the raw
   profile id or provider identity in the provider payload;
2. preserve byte-identical write/read requests inside one profile while making different profiles'
   cache and audio lineages distinct;
3. use `maxOutputTokens=512` for cache and audio legs. With the existing full-input-context reserve,
   the complete two-plan Flash matrix remains bounded at `20,999,168,000 nanoUSD`, below `$21`;
4. run a wholly new exact-SHA matrix with a new run id. The failed turn is never replayed and this
   terminal report is not resumed.

Only a later complete GREEN run may be followed by a separate publication commit. Until then the
model stays absent from production defaults, router presets, public contracts/catalogs, web docs,
OpenKeys, admin product surfaces and active pricing generations.

## Secret hygiene

No credential, key, project, email, provider subject, raw profile id, capacity snapshot or generated
text is recorded here. The sensitive machine report remains local under `/tmp` and is not committed.
