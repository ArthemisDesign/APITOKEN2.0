# CLAUDE.md — crates/suno-credential

Local crate boundaries. General rules — the root `AGENTS.md` and `CLAUDE.md`; the credential
contract — `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §6; Suno facts —
`docs/engine/SUNO_PROVIDER.md`.

## What this is

Sealed AEAD envelopes of Suno (suno.com) paid-subscription session material: the full browser
cookie string (the critical entry is the Clerk `__client` cookie) plus the discovered session id.
The crate is **pure**: XChaCha20-Poly1305, validation, normalization — and nothing else. It stands
OUTSIDE the layers `registry ← pool ← forward ← server`, like `glm-credential`,
`kimi-credential`, `gemini-credential` and `codex-credential`.

## Boundaries — do not violate

- **No network and no HTTP.** The crate does not call `auth.suno.com` or
  `studio-api.prod.suno.com`, does not mint JWTs and does not merge `set-cookie`. It can only
  `seal`/`open`/`validate` over already-obtained material. Session discovery, JWT minting and the
  per-profile single-flight from mint through envelope re-seal are the runtime's concern
  (`crates/forward`); probing at intake is done by `crates/authbot`.
- **No file I/O.** `0600`/`0700` permissions, atomic rename, fsync and roster publication are the
  responsibility of the calling producer, not of this crate.
- **No env.** The keyring arrives as a `kid:hex[,kid:hex]` string from
  `crates/server/src/config.rs`.
- **Dependencies are minimal.** Adding anything heavier than the current set requires
  justification in the commit.

## Invariants

1. **Secrets are never printed.** `Debug` for `SunoCredential` and `CredentialKeyring` is written
   by hand and returns `REDACTED` for the cookie, the session id and the proxy. The
   `debug_never_prints_secrets` test pins that secrets leak neither into `Debug` nor into error
   `Display`. A derived `Debug` on `SunoCredential` is forbidden.
2. **AAD binds the envelope to the profile id AND to the credential kind.** The envelope cannot be
   moved to a neighboring profile; the cleartext `kind` field is an AEAD input and after decryption
   is re-checked against the contents. There is one kind (`SessionCookie`), but the invariant is
   kept for any future kinds.
3. **The cookie must carry a non-empty `__client` entry.** Without it the material cannot mint a
   JWT, so its absence fails closed at seal. The cookie string is bounded; `session_id` is optional
   (rediscoverable via `SUNO_CLIENT_PATH`) and bounded when present.
4. **No base-url field, by design.** One platform (`suno.com`) with fixed hosts: a host-override
   knob could only smuggle the session to a foreign origin, so hosts and paths are crate
   constants. All wire hosts/paths are `oss-hypothesis` (gcui-art/suno-api, read 2026-08-12) and
   fail closed until a live run proves them; `SUNO_SESSION_TOKENS_PATH` carries a `{sid}`
   placeholder substituted by the caller.
5. **Plan is declared by the offer** and normalized to exactly `Pro`/`Premier`
   (`SunoPlan::parse`); the Free tier is excluded by design and anything else fails closed. The
   canonical labels match the `plan IN ('Pro', 'Premier')` CHECK of
   `crates/registry/migrations_pg/0050_suno_window_calibration.sql`. `SUNO_REVIEWED_PLANS` pins the
   published monthly credits (Pro 2 500, Premier 10 000, reviewed 2026-08-12). An observed
   `monthly_limit` contradicting the declared plan is the runtime's concern (admission probe), not
   this crate's.
6. **There is no `rotate()` surface in this crate.** A mint response may rotate the Clerk token;
   the winner of the runtime's single-flight re-seals via this crate before releasing the lock.
   That protocol lives in the caller.

## How to verify

```bash
cargo test -p suno-credential
```

Tests must cover: roundtrip, moving an envelope to a foreign profile, `kind` substitution,
reading with the old key during online keyring rotation, unknown `kid`, ciphertext corruption,
the `__client=` cookie requirement, optional-but-bounded session id, plan parse/normalize and the
published monthly credits, endpoint constants, profile id and proxy boundaries, proxy userinfo
reconstruction, absence of secrets in `Debug` and in error `Display`.
