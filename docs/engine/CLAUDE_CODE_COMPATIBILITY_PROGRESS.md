# Claude Code compatibility remediation progress

This is the implementation journal for findings `CC-01`, `CC-02`, `CC-03`, `CC-05`, `CC-06`, and `CC-08` from the append-only audit [`docs/audits/2026-08-21-CLAUDE-CODE-COMPATIBILITY.md`](../audits/2026-08-21-CLAUDE-CODE-COMPATIBILITY.md).

- **Started:** 2026-08-22
- **Baseline:** `d135e69dbb96ec41b54ac1db175632433b85a569`
- **Branch:** `fix/claude-code-compat-remediation`
- **Scope:** exact stable/latest offline acceptance, exact full Claude Code fingerprint, discovery display names, synthetic Anthropic error identity/shape, explicit subscription-persona attribution boundary, and stale documentation cleanup.
- **Excluded:** audit finding `CC-04` (discovery deadline) and `CC-07` (new non-Claude control translations), because the requested remediation list was 1, 2, 3, 5, 6, and 8.
- **Safety boundary:** no production access, subscription credential, or paid request is needed for the implementation gate. Any later production acceptance remains a separate controlled step.

## Status

| Finding | Status | Planned result |
|---|---|---|
| CC-01 exact acceptance | implemented, local GREEN | Integrity-pinned real native clients for npm stable/latest; two loopback cases per channel; no ambient `claude` authority |
| CC-02 fingerprint | implemented, focused GREEN | Full captured `cc_version` preserved; guessed per-profile build suffix removed from Claude and GLM |
| CC-03 discovery name | implemented, focused GREEN | Additive `display_name` published while retaining existing `name` |
| CC-05 synthetic errors | implemented, focused GREEN | Canonical `req_…` in `request-id` and body `request_id`; Messages 413 uses `request_too_large` |
| CC-06 attribution boundary | implemented in code/docs, focused GREEN | Typed helper seams make the subscription persona rewrite explicit; later system blocks stay ordered; API-key fallback clone remains pre-persona |
| CC-08 docs | implemented, pending full gate | Discovery contains-filter, UA `|` delimiter, dormant refresh, full version, and attribution wording corrected |

## Work log

### 2026-08-22 — Audit publication

- Published the dated audit to `master` as `d135e69dbb96ec41b54ac1db175632433b85a569`.
- Trusted candidate validation and final `deploy/watchdog` were GREEN.
- Created this remediation branch from that exact production baseline.

### 2026-08-22 — Remediation implementation

- Added integrity-pinned native package manifests for macOS/Linux x64/arm64 and exact npm stable/latest versions.
- Added a credential-blind loopback mock, bounded evidence assertion, and runner. The runner downloads only when its cache misses, validates SHA-512 before extraction, confirms the binary's exact version, and runs a current-control/structured-output case plus a discovery case for each channel. A regular `claude-api` Rust integration test composes the native Anthropic engine and mock upstream, then replays each stable/latest exact main request through that runtime; no delivery-controller change is needed. The paid live harness remains separate.
- Preserved the configured `cc_version` exactly and removed synthetic build suffix generation from Claude and GLM subscription personas. Existing three-component production values remain valid during blue-green overlap and are no longer expanded; the next reviewed capture atomically replaces the same key with its full suffix. Fingerprint env updates now publish once with one final rename and never restart the legacy singleton.
- Added `display_name` to model discovery without removing the existing `name` field.
- Unified engine/router synthetic Anthropic identities: one `req_…` appears in `request-id` and body `request_id`; Messages request-size errors now use `request_too_large`.
- Updated living contracts to describe the subscription OAuth persona rewrite honestly and to retain response/SSE compatibility as the transparent boundary.

## Verification ledger

- `bash tools/refresh-fingerprint.test.sh` — GREEN; all four observed version suffix forms preserved.
- Focused `claude-router`, `forward`, GLM, and server config tests — GREEN.
- `CLAUDE_CODE_COMPAT_CACHE_ROOT=/tmp/claude-code-compat-cache bash tests/claude_code_compat_matrix.sh` — GREEN for 2.1.231 stable and 2.1.239 latest, basic+discovery cases.
- `cargo test -p claude-router` — 137 passed.
- Focused subscription-attribution, synthetic-error, GLM full-version, and server full-version tests — GREEN. One broad parallel forward filter hit the pre-existing temporary SQLite name collision; the affected test passed immediately with `--test-threads=1`.
- `bash deploy/agent-merge.suite.sh` and `bash deploy/watchdog-lib.test.sh` — GREEN before the final design moved exact acceptance into ordinary Rust integration coverage.
- `cargo build` — GREEN for the full Rust workspace.
- `cargo test -p claude-api --test claude_code_compat` — GREEN: both exact channels replayed through the native Anthropic engine and mock upstream with SSE completion.
- The first full candidate gate exposed two stale server assertions after additive `request_id`; both were fixed and pass. A later trusted-host TypeScript failure came from changing the delivery controller itself; those controller changes were removed so the standard Rust lane owns this test. Paid/live evidence is never inferred from local tests.

## Completion criteria

1. The repository can resolve and execute real Claude Code npm `stable` and `latest` artifacts without relying on the ambient installed version, and fail closed on version/integrity mismatch.
2. Modern full `cc_version` values remain byte-identical in the emitted subscription persona; no synthetic `.dNN` suffix is appended.
3. Unified discovery returns both existing `name` and Claude Code `display_name`.
4. Every gateway-generated Anthropic error has one canonical `request-id` header and the same `request_id` in its body; request-size failures use `request_too_large`.
5. Code and documentation explicitly identify the subscription-persona rewrite as an OAuth transport boundary, distinct from router response passthrough.
6. The affected focused suites, workspace Rust gate, shell checks, docs check, and merge pipeline are GREEN on the final SHA.
