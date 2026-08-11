# Pricing release backfill — retired

This runbook is closed and must not be executed. The worker sweep, environment knobs, OpenKeys
`POST /api/internal/admin/strict-backfill` route, strict-policy/key-ACK chain and release-v2
opt-out APIs it previously operated were removed from source and production on 2026-08-09.
Keeping runnable instructions here would recreate the withdrawn pricing authority.

Current account pricing is the scalar/provider model in `docs/commerce/PRICING_MODEL.md`. Existing
account and OpenKeys reconciliation uses those live scalar controls only. The retired database
objects remain immutable incident evidence until every time, rollback, cursor, dependency, backup
and health gate in `docs/ops/PRICING_RETIREMENT.md` passes.

Historical commands and evidence remain available in Git history. They are not a rollback path;
rollback compatibility is enforced by `deploy/engine-commerce-compatibility.contract` and
`deploy/RELEASES.md`.
