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
- [CODEX_FLEET_FAILURE_ISOLATION.md](engine/CODEX_FLEET_FAILURE_ISOLATION.md) — accepted implementation plan preventing one request/model failure from poisoning Codex shared health, exhausting the roster, or producing synthetic quota errors.
- [CLAUDESTORE_FALLBACK.md](engine/CLAUDESTORE_FALLBACK.md) — dormant emergency Claude/GPT transports via ClaudeStore and their compliance/live gates.
- [GEMINI_PROVIDER.md](engine/GEMINI_PROVIDER.md) — Gemini OAuth subscription provider.
- [GEMINI_BATCH_MODE_PLAN.md](engine/GEMINI_BATCH_MODE_PLAN.md) — implementation plan for a durable, subscription-distributed Gemini-compatible batch mode without an extra batch discount.
- [GEMINI_BATCH_MODE_JOURNAL.md](engine/GEMINI_BATCH_MODE_JOURNAL.md) — append-only execution journal for Gemini Batch Mode stages 1–6.
- [GLM_PROVIDER.md](engine/GLM_PROVIDER.md) — GLM (Zhipu / Z.ai) Coding Plan subscription provider: capability manifest, backend-only, not published.
- [KIMI_PROVIDER.md](engine/KIMI_PROVIDER.md) — KIMI (Moonshot) Kimi Code subscription provider: capability manifest, backend-only, not published.
- [TRIPO3D_PROVIDER.md](engine/TRIPO3D_PROVIDER.md) — Tripo3D (VAST / Holymolly) 3D generation API provider: capability manifest, backend-only, not published.
- [SUNO_PROVIDER.md](engine/SUNO_PROVIDER.md) — Suno music generation subscription provider: capability manifest, backend-only, not published.
- [PROVIDER_WIRING_CHECKLIST.md](engine/PROVIDER_WIRING_CHECKLIST.md) — mechanical provider wiring map: exact files, symbols, order, and pitfalls.
- [PROVIDER_ONBOARDING.md](engine/PROVIDER_ONBOARDING.md) — full playbook for adding a new subscription provider through production GA.
- [STAGE2_POSTGRES_AUTHORITY.md](engine/STAGE2_POSTGRES_AUTHORITY.md) — PostgreSQL authority model and Stage 2 fencing.
- [UNIFIED_ROUTER.md](engine/UNIFIED_ROUTER.md) — target architecture of the single endpoint for all providers (design).
- [CLAUDE_CODE_COMPATIBILITY_PROGRESS.md](engine/CLAUDE_CODE_COMPATIBILITY_PROGRESS.md) — implementation journal for the accepted Claude Code stable/latest compatibility remediation linked to the 2026-08-21 audit.
- [LARGE_PAYLOAD_SCALING_PLAN.md](engine/LARGE_PAYLOAD_SCALING_PLAN.md) — staged train for raising model API payload and concurrency limits; commits 1–8 landed (router/Gemini/OpenAI 256 MiB, Anthropic request 32 MiB, translated response 256 MiB).
- [QUOTA_DISTRIBUTION_ANALYSIS.md](engine/QUOTA_DISTRIBUTION_ANALYSIS.md) — принятый план эффективности трат подписочных квот (Claude 5h/7d, Codex weekly, Gemini per-model buckets, Kimi/GLM 5h+weekly): подтверждённые проблемы, принятые правки R2/R7/R1 без слома cache-аффинити, отклонённые предложения с причинами, порядок внедрения.
- [QUOTA_DISTRIBUTION_PROGRESS.md](engine/QUOTA_DISTRIBUTION_PROGRESS.md) — рабочий журнал внедрения правок R2/R7/R1 и измерения (шаг 0) по плану QUOTA_DISTRIBUTION_ANALYSIS.
- [ROUTING_FENCING.md](engine/ROUTING_FENCING.md) — detailed design of UNIFIED_ROUTER stage 6: routing with fallback lists and attempt fencing (execution group / single billable winner).
- [REQUEST_OBSERVABILITY.md](engine/REQUEST_OBSERVABILITY.md) — owner-approved v1 request-lifecycle decision record, privacy boundary, exact producer/read/metrics scope, ordered rollout, and finite Definition of Done (first Codex count-token producer only; implementation incomplete).
- [ELOG.md](../crates/elog/CLAUDE.md) — unified error logging contract (crate-level instruction: format, levels, scrubbing).

## commerce/ — commerce (`apps/api`, `apps/worker`, `packages/*`)

- [COMMERCIAL_BACKEND.md](commerce/COMMERCIAL_BACKEND.md) — map and local launch of the commercial backend.
- [AUTHENTICATION.md](commerce/AUTHENTICATION.md) — authentication and authorization.
- [PRICING.md](commerce/PRICING.md) — customer-pricing entry point and retirement boundary; the live detailed contract is PRICING_MODEL.
- [PRICING_MODEL.md](commerce/PRICING_MODEL.md) — LIVE pricing contract: one balance, one discount, per-provider overrides.
- [MULTI-DISCOUNT.md](commerce/MULTI-DISCOUNT.md) — retired 2026-08-09; history of the catalog/switch/release design and why it was withdrawn.
- [MULTI_DISCOUNT_STAGE5.md](commerce/MULTI_DISCOUNT_STAGE5.md) — historical Stage 5 protocol; all producers and consumers are removed.
- [MULTI_DISCOUNT_STAGE6.md](commerce/MULTI_DISCOUNT_STAGE6.md) — historical Stage 6 funding-reconciliation protocol; all producers and consumers are removed.
- [MULTI_DISCOUNT_STAGE7.md](commerce/MULTI_DISCOUNT_STAGE7.md) — historical Stage 7 OpenKeys cutover protocol; all producers and consumers are removed.
- [MULTI_DISCOUNT_STAGE9.md](commerce/MULTI_DISCOUNT_STAGE9.md) — historical Stage 9 cutover protocol; all producers and consumers are removed.
- [MULTI_DISCOUNT_CATALOG_GEN2.md](commerce/MULTI_DISCOUNT_CATALOG_GEN2.md) — catalog generation 2 (`claude-opus-5`, `claude-fable-5`): inert delivery and activation (historical; the gen2 library/CLI are deleted).
- [CRYPTOMUS_INTEGRATION.md](commerce/CRYPTOMUS_INTEGRATION.md) — accepting payments via Cryptomus.
- [PLATEGA_INTEGRATION.md](commerce/PLATEGA_INTEGRATION.md) — accepting payments via Platega (default provider).
- [DIGISELLER_INTEGRATION.md](commerce/DIGISELLER_INTEGRATION.md) — DigiSeller: provider disabled, adapter groundwork and enablement conditions.
- [EMAIL_INTEGRATION.md](commerce/EMAIL_INTEGRATION.md) — transactional email and self-hosted SMTP.
- [CRM_BRIDGE.md](commerce/CRM_BRIDGE.md) — live least-privilege CRM referral link, registration, pricing and money bridge.

## sales/ — the affiliate arm (`apps/sales-*`, `packages/sales-db`)

- [SALES_PORTAL.md](sales/SALES_PORTAL.md) — the affiliate arm (partners.apitoken.sale).
- [PARTNER_PROGRAM.md](sales/PARTNER_PROGRAM.md) — complete partner program guide.
- [SALES_PAYOUT_PERIODS.md](sales/SALES_PAYOUT_PERIODS.md) — partner program periods and payouts.

## product/ — product storefronts

- [OPENKEYS.md](product/OPENKEYS.md) — OpenKeys: prepaid keys without registration (`apps/openkeys`).
- [PANEL.md](product/PANEL.md) — unified admin panel admin.apitoken.sale (contract).
- [ADMIN_PANEL.md](product/ADMIN_PANEL.md) — internal admin panel `apps/admin` (Next.js).

## ops/ — operations

- [DEPLOYMENT.md](ops/DEPLOYMENT.md) — production deployment runbook (operator-facing; agents deliver via `agent-merge.sh` and SSH only as `observe`).
- [INFRASTRUCTURE.md](ops/INFRASTRUCTURE.md) — production infrastructure and hosts (`deploy` watchdog identity vs `observe` agent SSH).
- [MODEL_RELEASE_CYCLE.md](ops/MODEL_RELEASE_CYCLE.md) — adding a model: dormant implementation, exact-SHA live proof, compiled/hot tariff verification, then separate public discovery and storefront publication; no policy or release-head advance.
- [PRICING_RELEASE_BACKFILL.md](ops/PRICING_RELEASE_BACKFILL.md) — retired tombstone; its strict-chain sweep, knobs and endpoint no longer exist and must not be run.
- [PRICING_RETIREMENT.md](ops/PRICING_RETIREMENT.md) — fail-closed retirement of the dead pricing policy/release/funding schema: exact manifests, retention/rollback gates and staged drop order.
- [MONITORING.md](ops/MONITORING.md) — monitoring and alert runbook anchors (`docs/ops/MONITORING.md#<alert>`).
- [INCIDENT_POSTMORTEMS.md](ops/INCIDENT_POSTMORTEMS.md) — threshold, template and executable-guardrail standard for production incidents and escaped near misses.
- [STAGING_ENVIRONMENT.md](ops/STAGING_ENVIRONMENT.md) — staging implementation plan (v8, owner-approved 2026-08-22, not implemented): co-located twin in a network namespace on the production VPS; `staging.slice` 32G/4 CPU; 80G loopback; rootless Docker; serial SHA promotion after explicit operator attest; host-global installers never from a stage candidate; first code is a production `contour-config` extract.
- [HOST_IMAGE_GATE.md](ops/HOST_IMAGE_GATE.md) — disposable Ubuntu 24.04 host-image for installer proofs (`useradd`, visudo, tmpfiles, ProtectSystem); merge-blocking on Ubuntu-host paths, not a production apply.
- [REDIS.md](ops/REDIS.md) — Redis topology (both instances), standing rules for new consumers, and the ranked map of where Redis pays off next.
- [DELETE_WORKTREE.md](ops/DELETE_WORKTREE.md) — permanent fail-closed cleanup of merged worktrees and explicitly registered clones on macOS.
- [CLAUDE_CALIBRATION.md](ops/CLAUDE_CALIBRATION.md) — bounded live calibration run of Claude: models, token classes, sticky subscriptions, and a hard nanoUSD budget.
- [GEMINI_CALIBRATION.md](ops/GEMINI_CALIBRATION.md) — exact-profile live calibration run of Gemini: immutable backend evidence, capability matrix, and a shared $40 limit.
- [GEMINI_BATCH_STAGE5_CANARY.md](ops/GEMINI_BATCH_STAGE5_CANARY.md) — dry-run-by-default, SSH credential-safe controlled Stage 5 Batch runner with an original $10 aggregate nanoUSD checkpoint and nonresumable paid-create boundary.
- [KIMI_CALIBRATION.md](ops/KIMI_CALIBRATION.md) — dry-run-by-default live calibration run of KIMI: exact request-id attribution, $0.0001 aggregate cap, paid only with explicit permission.
- [GLM_CALIBRATION.md](ops/GLM_CALIBRATION.md) — fail-closed live calibration run of GLM Coding Plan directly against the provider: quota anchor, 3 models × stream/non-stream matrix, hard cap $0.05.
- [TRIPO3D_CALIBRATION.md](ops/TRIPO3D_CALIBRATION.md) — dry-run-by-default live calibration run of the Tripo3D plane: single-profile no-spill attribution, version/option/refund matrix, explicit budget with a $5.00 hard cap.
- [SUNO_CALIBRATION.md](ops/SUNO_CALIBRATION.md) — dry-run-by-default live calibration run of the Suno plane: single-profile no-spill attribution, song/extend/lyrics/stems matrix with reserve-fallback settlement recording, explicit budget with a $1.00 hard cap.
- [GPT_IMAGE_2_CANARY.md](ops/GPT_IMAGE_2_CANARY.md) — GPT Image 2 canary and publication journal: private exact-profile generation/edit proofs, the fenced public preflight/paid-smoke gate chain, and the GREEN one-shot that authorized publication (the model is now published).
- [DEVBOT.md](ops/DEVBOT.md) — design of the Telegram dev bot (`apps/devbot`): topics, notifications, event sources (stages 1–3 implemented; stage 4 — business events — ahead).
- [FRONTEND_VISUAL_QA.md](ops/FRONTEND_VISUAL_QA.md) — frontend visual QA.
- [VERCEL_PRODUCT_ANALYTICS.md](ops/VERCEL_PRODUCT_ANALYTICS.md) — Vercel product analytics.
- [VERCEL.md](ops/VERCEL.md) — Vercel deployment runbook for `apps/web`: trigger model, state triage via the GitHub API without Vercel access, failure signatures, access policy.

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
- [2026-08-06-kimi-pool-review.md](audits/2026-08-06-kimi-pool-review.md) — historical KIMI pool audit covering routing, cooling, auth, billing, delivery and observability.
- [2026-08-07-kimi-production-readiness.md](audits/2026-08-07-kimi-production-readiness.md) — historical review of the gaps between the backend-only KIMI plane and public sale.
- [2026-08-07-pricing-cross-engine-review.md](audits/2026-08-07-pricing-cross-engine-review.md) — historical cross-engine pricing handover and failure-mode review.
- [2026-08-09-kimi-production-parity.md](audits/2026-08-09-kimi-production-parity.md) — historical production-parity acceptance of the KIMI plane.
- [2026-08-12-PRICING-ROLLBACK-REMEDIATION-CLOSEOUT.md](audits/2026-08-12-PRICING-ROLLBACK-REMEDIATION-CLOSEOUT.md) — resumable finding→SHA→production-evidence closeout for pricing rollback remediation, with the exact retention, payout-funding, debt-decision and external failure-domain gates still preventing final completion.
- [2026-08-12-PRICING-ROLLBACK-EXTERNAL-GATES-CLOSEOUT.md](audits/2026-08-12-PRICING-ROLLBACK-EXTERNAL-GATES-CLOSEOUT.md) — append-only continuation with exact debt causality, payout funding requirement, Borg contents, live off-host incident/recovery evidence and the four remaining external/retention gates.
- [2026-08-21-CLAUDE-CODE-COMPATIBILITY.md](audits/2026-08-21-CLAUDE-CODE-COMPATIBILITY.md) — exact stable/latest npm and paired-wire audit of Claude Code 2.1.231/2.1.239 against the current Messages/router paths, including acceptance, fingerprint, discovery, error-shape and attribution gaps.
- [2026-08-21-CODEX-CLI-0.149-COMPATIBILITY.md](audits/2026-08-21-CODEX-CLI-0.149-COMPATIBILITY.md) — exact-source and paired-wire audit of Codex CLI 0.149.0 against the current GPT/Codex custom-provider path, including the 128-byte MCP-name delta, preserved correctness gaps and the controlled baseline-upgrade plan.
- [2026-08-21-CODEX-CLI-0.149-LIVE-ACCEPTANCE.md](audits/2026-08-21-CODEX-CLI-0.149-LIVE-ACCEPTANCE.md) — exact 0.149 `--profile apitoken` live acceptance, terminal usage and cost evidence, and the public-vs-native proof boundary at that checkpoint.
- [2026-08-21-CODEX-NATIVE-0.149-PIN-ADMISSION.md](audits/2026-08-21-CODEX-NATIVE-0.149-PIN-ADMISSION.md) — official device-flow private ChatGPT models/usage/Responses proof admitting the internal 0.149 wire identity pin.

## Next to the code (do not move here)

- `crates/<name>/CLAUDE.md` — local crate boundaries.
- `packages/db/MIGRATIONS.md` — commerce migration rules.
- `deploy/README.md`, `deploy/RELEASES.md` — delivery controller and releases.
- `research/` — research and journals (not instructions), including resumable progress journals
  of provider wiring (`research/<PROVIDER>_PLANE_PROGRESS.md`).
