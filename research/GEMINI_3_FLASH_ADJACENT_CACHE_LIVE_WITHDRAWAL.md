# Gemini 3 Flash adjacent-cache gate — live withdrawal — 2026-08-03

## Verdict

`gemini-3-flash-preview` remains dormant. The profile-local scheduler again passed both owned paid
plans through all four thinking levels and incremental SSE. Its adjacent cache replay observed the
required cache class on Pro, but the same fixed write/read sequence remained entirely fresh on
Ultra. The runner stopped before audio, tools and Search. This terminal run is not resumed and does
not authorize publication.

## Immutable candidate and isolation

- Exact runner SHA: `a4eed55b03835fb0a2b9d360b7c07ca37fe389b6`; trusted validation and production
  `deploy/watchdog` were GREEN.
- The deployed runtime source (`Cargo.toml`, `Cargo.lock`, `crates/`) is byte-identical to parent
  release `9bd234b1ee30221c1f4a6eddb8f9878b00335e9e` because this candidate changes only the
  calibration runner and documentation.
- Frozen runtime binary SHA-256:
  `aea50c30dd53db78f892e2a63ff6d965518c88f220d20fb095609fa49e19107f`.
- Plans: one owned Google AI Pro profile and one owned Google AI Ultra profile, addressed only by
  opaque exact-profile calibration targets.
- The binary ran in an isolated user-systemd canary on loopback port `18895`, with production
  PostgreSQL, billing and immutable calibration authority. Stable Gemini traffic and public
  catalogs were unchanged. The canary was stopped after the verdict, port `18895` was confirmed
  closed and the active stable Gemini slot was healthy.

## Budget and passed evidence

The explicitly approved aggregate ceiling remained `$21`. Fourteen paid turns were durably
recorded for `37,985,500 nanoUSD` (`$0.0379855`); eight legs were not dispatched. On both Pro and
Ultra the run proved `minimal`, `low`, `medium` and `high`, plus incremental SSE, with public model
identity, visible output, terminal usage and exact response/event parity.

Both profile-local cache writes contained 12,342 fresh input tokens and 512 output tokens. The Pro
read followed its write without another profile's generation in between and exposed 8,170 cached
plus 4,172 fresh input tokens. Its exact official-rate charge fell from `7,707,000` to
`4,030,500 nanoUSD`, proving the intended cache accounting branch under the adjacent schedule.

## Blocking Ultra evidence

Ultra used its own stable profile scope and the same profile-local write/read ordering. The read
body was byte-identical to its write and returned public model identity, visible output, terminal
finish/usage and exact response/event parity. It nevertheless exposed all 12,342 input tokens as
fresh, zero as cached and cost the full `7,707,000 nanoUSD`. The runner preserved that successful
turn and spend, emitted `cached input token class was not observed` and sent no later paid leg.

This isolates the previous cross-profile gap as a real harness defect but not the complete source of
implicit-cache liveness. A single successful warm-up is not deterministic across both owned plans.
Treating Ultra's turn as cached would invent evidence; retrying this terminal run would violate its
fixed matrix.

## Follow-up boundary and budget

A later dormant runner may define a fixed three-turn profile-local group: `write → prime → read`.
The middle request is a planned successful generation with its own request id, immutable evidence
and charge, not a retry after transport ambiguity or failed generation. Only the final read may
satisfy the cache capability, and it must still expose a positive authoritative cache class.

The existing full-context pre-dispatch ceiling makes the complete Pro+Ultra matrix
`23,099,392,000 nanoUSD`. It therefore requires an explicit `$24` aggregate authorization; the
prior `$21` approval cannot be reused or rounded up. After that authorization, a wholly fresh
exact-SHA run id is required. Until it is complete and GREEN, production defaults, router presets,
public contracts/catalogs, web/docs, OpenKeys, admin and active pricing generations remain
unchanged.

No credential, key, project, email, provider subject, raw profile id, capacity snapshot, generated
text or machine report is committed here.
