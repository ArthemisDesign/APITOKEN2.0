# Staging hotfix drill — 2026-08-23

- Drill type: offline host-owned attestation contract test. No production hotfix was pushed.
- Fixture: one exact local Git SHA and matching deployed-stage marker.
- Action: issue `mode=hotfix` with mandatory reason `emergency`.
- Expected result: record binds commit, tree, policy digest, Unix identity `deploy`, actor, 24-hour TTL, reason, candidate marker, and record digest.
- Observed result: the contract test accepted the exact marker and rejected it after marker movement.
- Production effect: none. Phase 6 remains dry-run and ordinary merges are not blocked.
- Recovery/lock order: marker is read before atomic record replacement; marker movement invalidates a later issue. Phase 7 adds enforced sync and production admission locks.
