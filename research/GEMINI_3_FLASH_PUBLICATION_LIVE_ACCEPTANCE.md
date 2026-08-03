# Gemini 3 Flash Preview — production publication acceptance

Date: 2026-08-03

## Verdict

`gemini-3-flash-preview` passed the complete owned production live gate on both declared paid
subscription plans. The earlier withdrawal remains historical evidence for the failed candidates;
this fresh run authorizes a separate publication commit that adds the model to production defaults
and public catalog surfaces.

## Immutable implementation identity

- Runner SHA: `cc7e5bebc16ac720c909f221e6cfc9bd95070561`.
- Production runtime release SHA: `71213e6f8f036960885b1bd3a66781f6a07b6853`.
- `Cargo.toml`, `Cargo.lock`, and `crates/` were byte-identical between those two revisions, so the
  exercised runtime implementation is the runner implementation.
- Exercised `claude-api` binary SHA-256:
  `aea50c30dd53db78f892e2a63ff6d965518c88f220d20fb095609fa49e19107f`.
- This was a fresh run, not a resume or replay of an earlier partial matrix.

The raw live report remains an operator artifact and is intentionally not committed because it
contains profile/request identities. The evidence below is the sanitized aggregate required for
review and reproduction.

## Plans, budget, and completed matrix

- Plans: `google_ai_pro` and `google_ai_ultra`.
- Aggregate approved cap: `24,000,000,000 nanoUSD` (`$24`).
- Actual official-price spend: `49,232,500 nanoUSD` (`$0.0492325`).
- Paid turns: 22 total — 2 fresh, 8 thinking, 6 cache, 4 audio, and 2 forced-tool turns.
- Completion: `complete=true`, `pending_legs=[]`,
  `blocking_unavailable_capabilities=[]`.

Each plan independently proved:

- generation 2xx with visible non-thought output, terminal finish, authoritative usage, and public
  `modelVersion=gemini-3-flash-preview`;
- thinking levels `minimal`, `low`, `medium`, and `high`;
- incremental SSE with at least two candidate frames and terminal response/event usage parity;
- profile-local cache `write → prime → read`, with the terminal read accounting 8,170 cached input
  tokens;
- fresh and replayed strict PCM WAV accounting, each with 8 authoritative AUDIO tokens;
- a forced function call with exactly one returned function call.

Grounded Search was skipped before provider dispatch on both profiles as non-blocking. The public
control is priced per query and has no provider hard ceiling from which this bounded admission
runner could derive a safe worst-case hold; the skip spent no money and does not weaken the
generation/SSE/thinking/cache/audio/tool publication claims above.

## Route and cleanup

The accepted public route is `gemini-3-flash-preview → gemini-3-flash`. Quota admission joins the
observed private `gemini-3-flash` and `gemini-3-flash-agent` buckets, while native JSON and SSE
responses expose only the public model identity.

The isolated user-systemd canary was stopped after the run, its listener was confirmed closed, and
the stable production Gemini listener remained healthy. No live-runner process or alternate route
was left serving traffic.
