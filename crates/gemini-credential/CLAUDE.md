# `gemini-credential` — local contract

This crate owns only the format and verification of encrypted Gemini OAuth envelopes, pending
secret envelopes, and proxy canonicalization. There is no HTTP, external network, env, DB, roster I/O or
Auth Bot/runtime logic here; the producer and consumers pass in ready-made values.

Critical invariants:

1. Google identity, email, project, OAuth material and the authenticated proxy exist only inside an
   XChaCha20-Poly1305 envelope. `Debug`, errors and test snapshots must not reveal plaintext.
2. Version, `kid`, profile/context id in AAD, pinned OAuth identity/token endpoint, bounded fields,
   key rotation and zeroization remain fail-closed. Changing the wire format requires an explicit version.
3. A plan is accepted only on reviewed tier evidence. An exact known tier ID is authoritative and
   survives a display-name change; an exact known name of a different plan conflicts and is rejected.
   An unknown ID or a familiar substring (`Pro`, `Ultra`) alone grants no access; an exact
   standalone name remains legacy evidence for already-compatible sealed credentials.
4. The proxy is canonicalized reversibly: percent-encoded userinfo is decoded once and encoded into
   the unreserved set. Never log it, return it in an error, or weaken the origin/path check.
5. File atomicity, permissions, symlink/path guards and roster publication are implemented by the I/O
   owners (`authbot`/runtime); this crate provides pure encode/decode/validate primitives.

Verification: `cargo test -p gemini-credential`. When plan/tier validation changes, you must also
run `cargo test -p authbot`, because Auth Bot uses this allowlist before publication.
