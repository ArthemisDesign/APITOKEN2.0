# CLAUDE.md — crates/kimi-credential

Local crate boundaries. General rules — the root `AGENTS.md` and `CLAUDE.md`; the provider
contract — `docs/engine/PROVIDER_ONBOARDING.md` §6; KIMI facts —
`docs/engine/KIMI_PROVIDER.md`.

## What this is

Sealed AEAD envelopes of KIMI (Kimi Code) subscription credentials. The crate is **pure**:
XChaCha20-Poly1305, validation, normalization — and nothing else. It stands OUTSIDE the layers
`registry ← pool ← forward ← server`, like `gemini-credential` and `codex-credential`.

## Boundaries — do not violate

- **No network and no HTTP.** The crate does not call `auth.kimi.com`, does not refresh tokens and
  does not poll `/me`/`/usages`. It can only `seal`/`open`/`validate`/`rotate` over already-obtained
  material. The OAuth exchange is done by `crates/authbot`, runtime refresh — by `crates/forward`.
- **No file I/O.** `0600`/`0700` permissions, atomic rename, fsync and roster publication are the
  responsibility of the calling producer, not of this crate.
- **No env.** The keyring arrives as a `kid:hex[,kid:hex]` string from
  `crates/server/src/config.rs`.
- **Dependencies are minimal.** Adding anything heavier than the current set requires justification
  in the commit.

## Invariants

1. **Secrets are never printed.** `Debug` for `KimiCredential` and `CredentialKeyring` is written by
   hand and returns `REDACTED` for tokens and the proxy. The `debug_never_prints_secrets` test pins
   this. Do not print `KimiCredential` via `{:#?}` of a derived `Debug` — derived is forbidden.
2. **AAD binds the envelope to the profile id AND to the credential kind.** The envelope cannot be
   moved to a neighboring profile and an OAuth envelope cannot be reinterpreted as a console key. The
   cleartext `kind` field is an AEAD input, so after decryption it is re-checked against the contents.
3. **The refresh family is rotating.** The provider issues a new `refresh_token` on every refresh and
   retires the previous one. `rotate()` refuses to accept a response without a new refresh token —
   otherwise we would silently persist a spent token and the subscription would die on next use.
   **The caller must hold a per-profile single-flight lock** from refresh until re-`seal`. A race
   between two blue-green generations: the loser re-reads the envelope once and takes the winner's
   token.
4. **The plan is part of the durable identity.** An empty `plan_name` is rejected: it would collapse
   different calibration cohorts into one. The plan comes from `/me` (`user_level_name`).
5. **Account status.** Only `USER_STATUS_NORMAL` is routed. Any other status is a rejection, not a
   warning.
6. **Tariff capabilities fail closed, but the ladder is populated.** `KIMI_REVIEWED_PLANS` carries
   every documented tier (Adagio, Andante, Moderato, Allegretto, Allegro, Vivace) with the
   capability set the provider publishes: `kimi-for-coding` at 256K for any member, `k3`/`k3-256k`
   from Moderato, the 1M window and highspeed from Allegretto. What the provider's sources actually
   contradict is the **price** ladder, not the capability mapping — the table was empty for months
   over that confusion, and an empty table treats the most expensive subscription exactly like the
   free tier. A plan outside the ladder still returns `None` and gets base capabilities only.
   Lookup ignores case and padding: a plan is an operator-visible label, and a subscription must not
   lose `k3` because `/me` spelled the tier differently. A new tier is added **only** together with
   dated evidence in `docs/engine/KIMI_PROVIDER.md`, and an unreviewed plan must stay operationally
   visible rather than silently degrading to base.

## How to verify

```bash
cargo test -p kimi-credential
```

Tests must cover: roundtrip, moving an envelope to a foreign profile, `kind` substitution, reading
with the old key during online rotation, unknown `kid`, ciphertext corruption, refusal of a
non-rotating refresh, prohibition of rotating a console key, profile id and proxy boundaries,
absence of secrets in `Debug`.
