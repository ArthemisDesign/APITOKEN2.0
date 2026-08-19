# CLAUDE.md — crates/api-limits

Leaf contract for checked customer data-plane payload limits shared by `router`, `forward`, and
`server`.

## Boundaries

- No HTTP, environment reads, networking, async runtime, provider code, database access, or secrets.
- No dependencies on another workspace crate.
- Owns checked byte/admission units, strict decimal MiB/seconds parsers, fixed route classes,
  current production defaults, future hard compile ceilings, stable formatting, and relationship
  validation.
- Environment ownership remains in `crates/router/src/config.rs` and
  `crates/server/src/config.rs`; those layers map errors to startup failure.
- A hard ceiling is not an enabled public limit. Current defaults remain authoritative until the
  corresponding bounded storage, weighted admission, transport, and deployment stages are GREEN.
- Provider-owned narrower limits always override a common local envelope.

## Verification

```bash
cargo test -p api-limits
cargo build
```
