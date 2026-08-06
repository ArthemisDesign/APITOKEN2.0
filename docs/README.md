# docs/ — index of project documentation

Only the entry points remain in the repository root: `AGENTS.md`, `CLAUDE.md`, `README.md`,
`CONTRIBUTING.md`, `BRANCHES.md`. All subject documentation lives here, organized by bounded
context domains. The placement and update rules are in the "Documentation organization" and
"Documentation is a living contract" sections of the root `AGENTS.md`. When you add or move a
document, update this index.

## Relationships and changes (read before cross-context work)

- [DEPENDENCIES.md](DEPENDENCIES.md) — map of all project relationships: producer → contract → consumers.
- [CHANGE_CHECKLISTS.md](CHANGE_CHECKLISTS.md) — checklists for typical cross-functional changes (models, prices, providers, contracts, payments, alerts).

## engine/ — Rust engine (`crates/*`)

- [ARCHITECTURE.md](engine/ARCHITECTURE.md) — claude-api architecture: layers, pool, rotation, affinity.
- [CONTROL_API.md](engine/CONTROL_API.md) — engine integration guide for the site backend and payments.
- [CODEX_PROVIDER.md](engine/CODEX_PROVIDER.md) — Codex (ChatGPT) OAuth subscription provider.
- [CLAUDESTORE_FALLBACK.md](engine/CLAUDESTORE_FALLBACK.md) — dormant emergency Claude/GPT transports via ClaudeStore and their compliance/live gates.
- [GEMINI_PROVIDER.md](engine/GEMINI_PROVIDER.md) — Gemini OAuth subscription provider.
- [GLM_PROVIDER.md](engine/GLM_PROVIDER.md) — GLM (Zhipu / Z.ai) Coding Plan subscription provider: capability manifest, backend-only, not published.
- [KIMI_PROVIDER.md](engine/KIMI_PROVIDER.md) — KIMI (Moonshot) Kimi Code subscription provider: capability manifest, backend-only, not published.
- [PROVIDER_WIRING_CHECKLIST.md](engine/PROVIDER_WIRING_CHECKLIST.md) — mechanical provider wiring map: exact files, symbols, order, and pitfalls.
- [PROVIDER_ONBOARDING.md](engine/PROVIDER_ONBOARDING.md) — full playbook for adding a new subscription provider through production GA.
- [STAGE2_POSTGRES_AUTHORITY.md](engine/STAGE2_POSTGRES_AUTHORITY.md) — PostgreSQL authority model and Stage 2 fencing.
- [UNIFIED_ROUTER.md](engine/UNIFIED_ROUTER.md) — target architecture of the single endpoint for all providers (design).
- [ROUTING_FENCING.md](engine/ROUTING_FENCING.md) — detailed design of UNIFIED_ROUTER stage 6: routing with fallback lists and attempt fencing (execution group / single billable winner).

## commerce/ — commerce (`apps/api`, `apps/worker`, `packages/*`)

- [COMMERCIAL_BACKEND.md](commerce/COMMERCIAL_BACKEND.md) — map and local launch of the commercial backend.
- [AUTHENTICATION.md](commerce/AUTHENTICATION.md) — authentication and authorization.
- [PRICING.md](commerce/PRICING.md) — B2C 50%/overrides, B2B, OpenKeys, service, bonus, and referral.
- [MULTI-DISCOUNT.md](commerce/MULTI-DISCOUNT.md) — final discount contract and zero-downtime full-inventory cutover.
- [MULTI_DISCOUNT_STAGE5.md](commerce/MULTI_DISCOUNT_STAGE5.md) — Stage 5 v2: authoritative inventory and dormant target/recovery materialization.
- [MULTI_DISCOUNT_STAGE6.md](commerce/MULTI_DISCOUNT_STAGE6.md) — Stage 6: funding reconciliation.
- [MULTI_DISCOUNT_STAGE7.md](commerce/MULTI_DISCOUNT_STAGE7.md) — Stage 7: OpenKeys 1:1 cutover.
- [MULTI_DISCOUNT_STAGE9.md](commerce/MULTI_DISCOUNT_STAGE9.md) — Stage 9: zero-downtime atomic full-inventory cutover.
- [MULTI_DISCOUNT_CATALOG_GEN2.md](commerce/MULTI_DISCOUNT_CATALOG_GEN2.md) — catalog generation 2 (`claude-opus-5`, `claude-fable-5`): inert delivery and activation.
- [CRYPTOMUS_INTEGRATION.md](commerce/CRYPTOMUS_INTEGRATION.md) — accepting payments via Cryptomus.
- [PLATEGA_INTEGRATION.md](commerce/PLATEGA_INTEGRATION.md) — accepting payments via Platega (default provider).
- [DIGISELLER_INTEGRATION.md](commerce/DIGISELLER_INTEGRATION.md) — DigiSeller: provider disabled, adapter groundwork and enablement conditions.
- [EMAIL_INTEGRATION.md](commerce/EMAIL_INTEGRATION.md) — transactional email and self-hosted SMTP.

## sales/ — the affiliate arm (`apps/sales-*`, `packages/sales-db`)

- [SALES_PORTAL.md](sales/SALES_PORTAL.md) — the affiliate arm (partners.apitoken.sale).
- [PARTNER_PROGRAM.md](sales/PARTNER_PROGRAM.md) — complete partner program guide.
- [SALES_PAYOUT_PERIODS.md](sales/SALES_PAYOUT_PERIODS.md) — partner program periods and payouts.

## product/ — product storefronts

- [OPENKEYS.md](product/OPENKEYS.md) — OpenKeys: prepaid keys without registration (`apps/openkeys`).
- [PANEL.md](product/PANEL.md) — unified admin panel admin.apitoken.sale (contract).
- [ADMIN_PANEL.md](product/ADMIN_PANEL.md) — internal admin panel `apps/admin` (Next.js).

## ops/ — operations

- [DEPLOYMENT.md](ops/DEPLOYMENT.md) — production deployment runbook (operator-facing).
- [INFRASTRUCTURE.md](ops/INFRASTRUCTURE.md) — production infrastructure and hosts.
- [MONITORING.md](ops/MONITORING.md) — monitoring and alert runbook anchors (`docs/ops/MONITORING.md#<alert>`).
- [REDIS.md](ops/REDIS.md) — Redis topology (both instances), standing rules for new consumers, and the ranked map of where Redis pays off next.
- [DELETE_WORKTREE.md](ops/DELETE_WORKTREE.md) — permanent fail-closed cleanup of merged worktrees and explicitly registered clones on macOS.
- [CLAUDE_CALIBRATION.md](ops/CLAUDE_CALIBRATION.md) — bounded live calibration run of Claude: models, token classes, sticky subscriptions, and a hard nanoUSD budget.
- [GEMINI_CALIBRATION.md](ops/GEMINI_CALIBRATION.md) — exact-profile live calibration run of Gemini: immutable backend evidence, capability matrix, and a shared $40 limit.
- [KIMI_CALIBRATION.md](ops/KIMI_CALIBRATION.md) — dry-run-by-default live calibration run of KIMI: exact request-id attribution, $0.0001 aggregate cap, paid only with explicit permission.
- [GLM_CALIBRATION.md](ops/GLM_CALIBRATION.md) — fail-closed live calibration run of GLM Coding Plan directly against the provider: quota anchor, 3 models × stream/non-stream matrix, hard cap $0.05.
- [GPT_IMAGE_2_CANARY.md](ops/GPT_IMAGE_2_CANARY.md) — private dormant Codex OAuth GPT Image 2 canary: blocked dry-run, fail-closed execution until an enforceable worst-case charge bound exists, strict `0600` evidence design, and no publication surface.
- [DEVBOT.md](ops/DEVBOT.md) — design of the Telegram dev bot (`apps/devbot`): topics, notifications, event sources (stages 1–3 implemented; stage 4 — business events — ahead).
- [FRONTEND_VISUAL_QA.md](ops/FRONTEND_VISUAL_QA.md) — frontend visual QA.
- [VERCEL_PRODUCT_ANALYTICS.md](ops/VERCEL_PRODUCT_ANALYTICS.md) — Vercel product analytics.

## audits/ — audits (append-only, never edited retroactively)

- [AUDIT.md](audits/AUDIT.md) — architectural audit of claude-api.
- [FULL_AUDIT_M.md](audits/FULL_AUDIT_M.md) — full audit: engine, backend, frontend, relationships.
- [TESTS_AUDIT.md](audits/TESTS_AUDIT.md) — audit of test completeness and sufficiency.
- [2026-08-01-AGENT_DOCS_AUDIT.md](audits/2026-08-01-AGENT_DOCS_AUDIT.md) — audit of the agent coordination system (AGENTS.md, DEPENDENCIES.md, checklists, docs gate).
- [2026-08-03-UNIFIED_ROUTER_PRODUCTION_READINESS.md](audits/2026-08-03-UNIFIED_ROUTER_PRODUCTION_READINESS.md) — production-readiness audit of the unified router: resource/auth, protocol parity, catalog, OpenCode, and zero-downtime delivery.
- [2026-08-03-UNIFIED_ROUTER_REMEDIATION_CLOSEOUT.md](audits/2026-08-03-UNIFIED_ROUTER_REMEDIATION_CLOSEOUT.md) — unified router remediation closeout: production SHA, repeated live/negative/harness verification, and three external/GA leftovers.
- [GEMINI_ROUTER_POOL_ACCEPTANCE_2026-08-03.md](audits/GEMINI_ROUTER_POOL_ACCEPTANCE_2026-08-03.md) — production acceptance of the Gemini pool through the unified router: sticky/cache/SSE, rotation, FIFO, budget, and fail-closed audio remediation.
- [2026-08-03-UNIFIED_ROUTER_RESILIENCE_AUDIT.md](audits/2026-08-03-UNIFIED_ROUTER_RESILIENCE_AUDIT.md) — repeat resilience/scale audit: body admission, metadata authorities, Caddy fencing, startup probe, observability, and honest OpenCode image capability.
- [2026-08-04-BACKEND-POSTGRES-PERFORMANCE-AUDIT.md](audits/2026-08-04-BACKEND-POSTGRES-PERFORMANCE-AUDIT.md) — full PostgreSQL communications and performance audit across engine, commerce, Sales, OpenKeys, pooling, indexes, and observability.
- [2026-08-05-MULTI-DISCOUNT-IMPLEMENTATION-AUDIT.md](audits/2026-08-05-MULTI-DISCOUNT-IMPLEMENTATION-AUDIT.md) — implementation and production-readiness audit of the multi-provider discount contract, release-v2 rollout, customer/admin surfaces, OpenKeys, and referral attribution.
- [2026-08-05-SOURCE-CONTEXT-AUDIT.md](audits/2026-08-05-SOURCE-CONTEXT-AUDIT.md) — source concentration and large-file audit with a behavior-preserving decomposition roadmap for smaller agent navigation contexts.
- [2026-08-05-GEMINI-IMAGE-INTERACTIONS-CAPABILITY-AUDIT.md](audits/2026-08-05-GEMINI-IMAGE-INTERACTIONS-CAPABILITY-AUDIT.md) — capability audit of Gemini image generation across the current Interactions contract, the native core subset, multi-turn state, SDK methods, and OpenAI media conversion.
- [2026-08-06-MULTI-DISCOUNT-PRICES-AUDIT.md](audits/2026-08-06-MULTI-DISCOUNT-PRICES-AUDIT.md) — full contract audit of the live multi-discount pricing update: post-cutover B2B conversion and welcome-bonus gaps, settlement rounding, progressive UI leftovers, and the verified release-v2 economics.
- [2026-08-06-MULTI-DISCOUNT-DEEPSEEK-AUDIT.md](audits/2026-08-06-MULTI-DISCOUNT-DEEPSEEK-AUDIT.md) — full DeepSeek audit of the prices update: live release-v2 authority, B2C/B2B/OpenKeys/service economics, funding, welcome bonus, referral, progressive cleanup, and read-only production verification of the Definition of Done.

## Next to the code (do not move here)

- `crates/<name>/CLAUDE.md` — local crate boundaries.
- `packages/db/MIGRATIONS.md` — commerce migration rules.
- `deploy/README.md`, `deploy/RELEASES.md` — delivery controller and releases.
- `research/` — research and journals (not instructions), including resumable progress journals
  of provider wiring (`research/<PROVIDER>_PLANE_PROGRESS.md`).
