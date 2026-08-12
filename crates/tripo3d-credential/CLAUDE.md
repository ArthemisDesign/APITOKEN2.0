# CLAUDE.md — crates/tripo3d-credential

Local crate boundaries. General rules — the root `AGENTS.md` and `CLAUDE.md`; the credential
contract — `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §6; Tripo3D facts —
`docs/engine/TRIPO3D_PROVIDER.md`.

## What this is

Sealed AEAD envelopes of static Tripo3D (VAST / Holymolly) API-platform keys (`tsk_…`).
The crate is **pure**: XChaCha20-Poly1305, validation, normalization — and nothing else. It stands
OUTSIDE the layers `registry ← pool ← forward ← server`, like `glm-credential`,
`kimi-credential`, `gemini-credential` and `codex-credential`.

## Boundaries — do not violate

- **No network and no HTTP.** The crate does not call `api.tripo3d.ai`/`api.tripo3d.com` and does
  not poll the balance endpoint. It can only `seal`/`open`/`validate` over already-obtained
  material. Probing the key is done by `crates/authbot`, the runtime — by `crates/forward`.
- **No file I/O.** `0600`/`0700` permissions, atomic rename, fsync and roster publication are the
  responsibility of the calling producer, not of this crate.
- **No env.** The keyring arrives as a `kid:hex[,kid:hex]` string from
  `crates/server/src/config.rs`.
- **No key digest.** Tripo3D has no machine-readable subject (no `/me` exists); the dedup unit is
  the key itself. Comparison is performed by the caller (authbot) on open envelopes; do not add a
  hashing dependency (blake3 or similar) to the crate.
- **Dependencies are minimal.** Adding anything heavier than the current set requires
  justification in the commit.

## Invariants

1. **Secrets are never printed.** `Debug` for `Tripo3dCredential` and `CredentialKeyring` is
   written by hand and returns `REDACTED` for the key and the proxy. The
   `debug_never_prints_secrets` test pins that secrets leak neither into `Debug` nor into error
   `Display`. A derived `Debug` on `Tripo3dCredential` is forbidden.
2. **AAD binds the envelope to the profile id AND to the credential kind.** The envelope cannot be
   moved to a neighboring profile; the cleartext `kind` field is an AEAD input and after decryption
   is re-checked against the contents. There is one kind (`ApiKey`), but the invariant is kept for
   any future kinds.
3. **The key is static.** There is no refresh family, no expiry and no `rotate()`, and none will
   appear: rotation means reissuing the key in the console and re-running `seal` via Auth Bot. Do
   not add a refresh surface "for the future".
4. **Base URL — an allowlist of exactly two origins** (`https://api.tripo3d.ai` global,
   `https://api.tripo3d.com` CN), stored in canonical form without a trailing slash; keys are not
   interchangeable between the sites. A foreign host, a non-empty path, a query, a fragment or
   credentials in the URL — rejection at `seal`/`open`. The undocumented `apiv3` host
   (`https://openapi.tripo3d.ai`) is NOT in the allowlist. Calling `normalize_base_url` on input is
   the caller's duty. The only exception is the cargo feature `test-loopback-base-url`: plain HTTP
   on `127.0.0.1`/`localhost`/`[::1]` for mock upstreams in consumer tests. It is enabled only via
   dev-dependencies; in production binaries the allowlist stays strict.
5. **The `tsk_` prefix is enforced.** A `tcli_` Client ID is documented to answer 401 and fails
   closed at validation instead of burning a live request.
6. **The cohort is declared by the offer product** (e.g. "Tripo3D API $50") and stored
   lowercase-normalized (`normalize_cohort`); it is the calibration cohort key and matches the
   `cohort` column of `crates/registry/migrations_pg/0049_tripo3d_calibration.sql`. There is no
   plan ladder on the API side (prepaid credits), so unlike GLM there is no reviewed-plan table.
   A balance contradicting the declared cohort is the runtime's concern (admission probe), not
   this crate's.

## How to verify

```bash
cargo test -p tripo3d-credential
cargo test -p tripo3d-credential --features test-loopback-base-url
```

Tests must cover: roundtrip, moving an envelope to a foreign profile, `kind` substitution,
reading with the old key during online keyring rotation, unknown `kid`, ciphertext corruption,
the base_url allowlist (both origins; trailing-slash normalization; foreign
host/path/query/fragment/credentials — rejection; loopback only under the feature), the `tsk_`
prefix rule, cohort normalization and bounds, profile id and proxy boundaries, proxy userinfo
reconstruction, absence of secrets in `Debug` and in error `Display`.
