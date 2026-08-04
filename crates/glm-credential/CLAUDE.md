# CLAUDE.md — crates/glm-credential

Local crate boundaries. General rules — the root `AGENTS.md` and `CLAUDE.md`; the credential
contract — `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §6; GLM facts —
`docs/engine/GLM_PROVIDER.md`.

## What this is

Sealed AEAD envelopes of static GLM Coding Plan (Zhipu AI / Z.ai) API keys.
The crate is **pure**: XChaCha20-Poly1305, validation, normalization — and nothing else. It stands
OUTSIDE the layers `registry ← pool ← forward ← server`, like `kimi-credential`, `gemini-credential`
and `codex-credential`.

## Boundaries — do not violate

- **No network and no HTTP.** The crate does not call `api.z.ai`/`open.bigmodel.cn` and does not poll
  the quota endpoint. It can only `seal`/`open`/`validate` over already-obtained material.
  Probing the key is done by `crates/authbot`, the runtime — by `crates/forward`.
- **No file I/O.** `0600`/`0700` permissions, atomic rename, fsync and roster publication are the
  responsibility of the calling producer, not of this crate.
- **No env.** The keyring arrives as a `kid:hex[,kid:hex]` string from
  `crates/server/src/config.rs`.
- **No key digest.** GLM has no machine-readable subject (`/me` does not exist);
  the dedup unit is the key itself. Comparison is performed by the caller (authbot) on open
  envelopes; do not add a hashing dependency (blake3 or similar) to the crate.
- **Dependencies are minimal.** Adding anything heavier than the current set requires
  justification in the commit.

## Invariants

1. **Secrets are never printed.** `Debug` for `GlmCredential` and `CredentialKeyring` is written
   by hand and returns `REDACTED` for the key and the proxy. The `debug_never_prints_secrets` test
   pins that secrets leak neither into `Debug` nor into error `Display`. A derived
   `Debug` on `GlmCredential` is forbidden.
2. **AAD binds the envelope to the profile id AND to the credential kind.** The envelope cannot be
   moved to a neighboring profile; the cleartext `kind` field is an AEAD input and after decryption
   is re-checked against the contents. There is one kind (`PlanKey`), but the invariant is kept for
   any future kinds.
3. **The key is static.** There is no refresh family, no expiry and no `rotate()`, and none will
   appear: rotation means reissuing the key in the console and re-running `seal` via Auth Bot. Do not
   add a refresh surface "for the future".
4. **Base URL — an allowlist of exactly two origins** (`https://api.z.ai`,
   `https://open.bigmodel.cn`), stored in canonical form without a trailing slash; int/CN keys
   are incompatible between the sites. A foreign host, a non-empty path, a query, a fragment or
   credentials in the URL — rejection at `seal`/`open`. Calling `normalize_base_url` on input is the
   caller's duty. The only exception is the cargo feature
   `test-loopback-base-url`: plain HTTP on `127.0.0.1`/`localhost`/`[::1]` for mock upstreams
   in consumer tests. It is enabled only via dev-dependencies (`forward`); in
   production binaries the allowlist stays strict.
5. **The plan is declared by the offer** and normalized to `lite|pro|max` (`GlmPlan::parse`);
   Team and legacy prompts plans fail closed. An observed quota window-limit contradicting the
   declared plan is the runtime's concern (profile out of rotation), not this crate's.
6. **Window credits are officially published** (docs.z.ai/devpack/overview, reviewed
   2026-08-03): lite 2000/5h + 10000/7d, pro 12000 + 60000, max 28000 + 140000 — which is why
   `GLM_REVIEWED_PLANS`, unlike KIMI's, is not empty. Rate-limit/concurrency differences between
   tiers are NOT encoded: they are dynamic and undocumented (`unknown`). All three models
   (`GLM_PLAN_MODELS`) are available on all plans — also official.

## How to verify

```bash
cargo test -p glm-credential
```

Tests must cover: roundtrip, moving an envelope to a foreign profile, `kind` substitution,
reading with the old key during online keyring rotation, unknown `kid`, ciphertext corruption,
the base_url allowlist (both origins; foreign host/path/credentials — rejection; canonical form
mandatory), normalization and unknown plan, official window credits, profile id and proxy
boundaries, proxy userinfo reconstruction, absence of secrets in `Debug` and in error `Display`.
