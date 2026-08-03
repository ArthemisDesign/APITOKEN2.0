# Gemini 3 Flash optional tool-subset gate — live withdrawal — 2026-08-03

## Verdict

`gemini-3-flash-preview` remains dormant. The corrected per-profile cache runner passed both owned
paid plans through every thinking level, incremental SSE, cache write/read and exact PCM WAV
fresh/replay. The next forced-tool turn returned the required function call and complete accounting,
but the runner stopped because Google did not emit the optional `toolUsePromptTokenCount` subset.
This terminal run is not resumed and does not authorize publication.

## Immutable candidate and isolation

- Exact runner SHA: `b9d941c36eb9189f2d11ed4d0a6d3f5b225dd1d8`; trusted validation and production
  `deploy/watchdog` were GREEN.
- That commit changed only the runner and documentation. Its Cargo/Rust runtime source is
  byte-identical to production release `3212d0e9948a8cf0b24d28dbc11b33c393e93b93`, verified over
  `Cargo.toml`, `Cargo.lock` and `crates/`.
- Frozen runtime binary SHA-256:
  `c4e68bbc3d71a1e5aeaa54124fb172894e34ea2303a7e7f29a996fe1f610d11b`.
- The binary ran in an isolated user-systemd canary on loopback port `18895`, with production
  PostgreSQL, billing and immutable calibration authority. Stable Gemini traffic and public
  catalogs were unchanged. The canary was stopped after the verdict and the port was closed.

## Budget and passed evidence

The explicitly approved aggregate ceiling remained `$21`. Nineteen paid turns were durably
recorded for `37,240,500 nanoUSD` (`$0.0372405`); three legs were not dispatched. On both Pro and
Ultra the run proved:

- `minimal`, `low`, `medium` and `high` with public identity, visible output and terminal usage;
- incremental SSE with multiple candidate frames;
- profile-isolated cache write/read pairs;
- fresh and byte-identical replayed PCM WAV audio with exact AUDIO reconstruction;
- response usage equal to each immutable event.

## Blocking tool evidence

The first tool leg returned one `calibration_probe` function call, public
`modelVersion=gemini-3-flash-preview`, terminal finish/usage and exact response/event usage parity.
Its immutable vector contained 65 fresh input tokens, 79 total output tokens including 59 thinking
tokens, and zero `tool_prompt_tokens`. Official-rate accounting charged all 65 ordinary input tokens
once; no separately priced usage leg was absent.

The old harness nevertheless required positive `tool_prompt_tokens` and classified the turn as a
coverage failure. That requirement was too strong: `toolUsePromptTokenCount` is an optional subset
of ordinary input, unlike audio/cache/image/Search classes with distinct rates or billing units.
Absence must remain explicit zero evidence, but it cannot invalidate a forced function call whose
terminal total usage matches the immutable event exactly.

## Follow-up boundary

A later dormant runner may remove only the positive optional-subset requirement. It must continue
to require the forced function call, public model identity, terminal finish/usage, full
response/event vector equality and ordinary input billing exactly once. The terminal report is not
reclassified or resumed: a new runner SHA needs a wholly new run id and complete Pro+Ultra matrix.
Until that matrix is GREEN, production defaults, router presets, public catalogs, web/docs,
OpenKeys, admin and active pricing generations remain unchanged.

No credential, key, project, email, provider subject, raw profile id, capacity snapshot, generated
text or machine report is committed here.
