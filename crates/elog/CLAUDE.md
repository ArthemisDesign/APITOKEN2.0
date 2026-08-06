# crates/elog — unified error logging

`elog` is the single place where every runtime diagnostic line of the engine is emitted.
It is a leaf crate: **no dependencies**, and none may be added. Every layer that logs
(`forward`, `server`, `router`, `authbot`, `registry`, `pool`) depends on it directly;
`metering` stays pure and does not.

## Contract

- All runtime error/warn/info lines go through `elog::error`, `elog::warn`, `elog::info`
  — never raw `eprintln!` in production code (tests may keep `eprintln!`/capture).
- Line format is fixed and greppable: `[LEVEL][category] message` on stderr, e.g.
  `[ERROR][forward] subs admin query failed: ...`.
- Every line passes through `elog::scrub_secrets` — bearer tokens, `sk-*` keys and
  Google OAuth tokens are masked centrally. A secret that reaches `elog` cannot reach the
  log; compose messages without relying on that, though.
- The message argument is `impl Display`: compose with `format!` at the call site; anyhow
  chains keep working via `{e:#}`.
- Category is a short kebab-case domain name (`forward`, `billing`, `codex`, `gemini`,
  `router-auth`, `server-poller`, ...) — see the per-crate migration for the stable set.

## Levels

- `error` — a failure or invariant violation; an operator should look at it.
- `warn` — degradation, retry, rotation, best-effort fallback; the system still serves.
- `info` — lifecycle and state transitions (startup, shutdown, refresh success).

## Invariants

- Panic-free: `elog` must never panic, so a logging bug cannot take down a request.
- No global state: the crate is stateless, so it is safe from any thread and does not
  serialize the engine.
- Never log credentials: if a message must contain identity material (email, profile id)
  that is the caller's decision; tokens are scrubbed here regardless.

## Tests

`cargo test -p elog` — scrubber cases are the regression suite (masking, delimiters,
false-positive guard for words like `task-ant`). A new secret shape must arrive with a
test here before it is logged anywhere.
