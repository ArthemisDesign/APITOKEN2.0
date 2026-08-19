# CLAUDE.md — crates/bounded-body

Dependency-light leaf primitives for the large-payload rollout. It owns fail-fast weighted budgets
and private memory-to-file body storage; it does not own HTTP, environment, route/provider policy,
metrics, billing, retry, or deployment paths.

## Boundaries and invariants

- Depends only on `api-limits` and the Rust standard library.
- Never waits for capacity: admission is atomic and fail-fast.
- `Reservation` is single-owner RAII. Growth reserves the delta before changing ownership; failed
  growth leaves the old reservation intact; drop releases exactly the owned units once.
- Storage and estimated-RSS use separate `Budget` instances. This crate accepts explicit weights and
  never invents an RSS coefficient.
- A chunk is checked against the request limit and reservations before copying or writing.
- Crossing the memory threshold persists the complete prefix to a private mode-0600 file before the
  in-memory allocation is dropped and its storage/RSS weight is shrunk.
- `StoredBody` and spool ownership are non-clone. Drop closes and removes named fallback files and
  releases all reservations. Errors and Debug output expose neither body bytes nor paths.
- OS/project quota, slot-specific roots, startup stale cleanup, HTTP streaming adapters, and public
  limits belong to later integration/deployment stages.

## Verification

```bash
cargo test -p bounded-body
cargo build
```
