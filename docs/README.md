# docs/ — индекс проектной документации

В корне репозитория остаются только точки входа: `AGENTS.md`, `CLAUDE.md`, `README.md`,
`CONTRIBUTING.md`, `BRANCHES.md`. Вся предметная документация — здесь, по доменам bounded
context'ов. Правила размещения и обновления — в разделе «Организация документации» и
«Документация — живой контракт» корневого `AGENTS.md`. При добавлении или переносе документа
обнови этот индекс.

## Связи и изменения (читай перед кросс-контекстной работой)

- [DEPENDENCIES.md](DEPENDENCIES.md) — карта всех связей проекта: производитель → контракт → потребители.
- [CHANGE_CHECKLISTS.md](CHANGE_CHECKLISTS.md) — чеклисты типовых кросс-функциональных изменений (модели, цены, провайдеры, контракты, оплаты, алерты).

## engine/ — Rust-движок (`crates/*`)

- [ARCHITECTURE.md](engine/ARCHITECTURE.md) — архитектура claude-api: слои, пул, ротация, affinity.
- [CONTROL_API.md](engine/CONTROL_API.md) — интеграционный гайд движка для бэкенда сайта и оплаты.
- [CODEX_PROVIDER.md](engine/CODEX_PROVIDER.md) — Codex (ChatGPT) OAuth subscription provider.
- [GEMINI_PROVIDER.md](engine/GEMINI_PROVIDER.md) — Gemini OAuth subscription provider.
- [KIMI_PROVIDER.md](engine/KIMI_PROVIDER.md) — KIMI (Moonshot) Kimi Code subscription provider: capability manifest, backend-only, не публикуется.
- [PROVIDER_WIRING_CHECKLIST.md](engine/PROVIDER_WIRING_CHECKLIST.md) — механическая карта подключения провайдера: точные файлы, символы, порядок и ловушки.
- [PROVIDER_ONBOARDING.md](engine/PROVIDER_ONBOARDING.md) — полный playbook добавления нового subscription-провайдера до production GA.
- [STAGE2_POSTGRES_AUTHORITY.md](engine/STAGE2_POSTGRES_AUTHORITY.md) — модель PostgreSQL authority и fencing Stage 2.
- [UNIFIED_ROUTER.md](engine/UNIFIED_ROUTER.md) — целевая архитектура единого endpoint для всех провайдеров (design).
- [ROUTING_FENCING.md](engine/ROUTING_FENCING.md) — детальный дизайн этапа 6 UNIFIED_ROUTER: routing с fallback-списками и attempt fencing (execution group / единственный billable winner).

## commerce/ — коммерция (`apps/api`, `apps/worker`, `packages/*`)

- [COMMERCIAL_BACKEND.md](commerce/COMMERCIAL_BACKEND.md) — карта и локальный запуск коммерческого бэкенда.
- [AUTHENTICATION.md](commerce/AUTHENTICATION.md) — аутентификация и авторизация.
- [PRICING.md](commerce/PRICING.md) — B2C 50%/overrides, B2B, OpenKeys, service, bonus и referral.
- [MULTI-DISCOUNT.md](commerce/MULTI-DISCOUNT.md) — итоговый контракт скидок и zero-downtime full-inventory cutover.
- [MULTI_DISCOUNT_STAGE5.md](commerce/MULTI_DISCOUNT_STAGE5.md) — Stage 5 v2: authoritative inventory и dormant target/recovery materialization.
- [MULTI_DISCOUNT_STAGE6.md](commerce/MULTI_DISCOUNT_STAGE6.md) — Stage 6: funding reconciliation.
- [MULTI_DISCOUNT_STAGE7.md](commerce/MULTI_DISCOUNT_STAGE7.md) — Stage 7: OpenKeys 1:1 cutover.
- [MULTI_DISCOUNT_STAGE9.md](commerce/MULTI_DISCOUNT_STAGE9.md) — Stage 9: zero-downtime atomic full-inventory cutover.
- [MULTI_DISCOUNT_CATALOG_GEN2.md](commerce/MULTI_DISCOUNT_CATALOG_GEN2.md) — catalog generation 2 (`claude-opus-5`, `claude-fable-5`): инертная доставка и активация.
- [CRYPTOMUS_INTEGRATION.md](commerce/CRYPTOMUS_INTEGRATION.md) — приём платежей через Cryptomus.
- [PLATEGA_INTEGRATION.md](commerce/PLATEGA_INTEGRATION.md) — приём платежей через Platega (дефолтный провайдер).
- [DIGISELLER_INTEGRATION.md](commerce/DIGISELLER_INTEGRATION.md) — DigiSeller: провайдер отключён, задел адаптера и условия включения.
- [EMAIL_INTEGRATION.md](commerce/EMAIL_INTEGRATION.md) — транзакционная почта и self-hosted SMTP.

## sales/ — партнёрское направление (`apps/sales-*`, `packages/sales-db`)

- [SALES_PORTAL.md](sales/SALES_PORTAL.md) — партнёрское направление (partners.apitoken.sale).
- [PARTNER_PROGRAM.md](sales/PARTNER_PROGRAM.md) — полное руководство партнёрской программы.
- [SALES_PAYOUT_PERIODS.md](sales/SALES_PAYOUT_PERIODS.md) — периоды и выплаты партнёрской программы.

## product/ — продуктовые витрины

- [OPENKEYS.md](product/OPENKEYS.md) — OpenKeys: предоплаченные ключи без регистрации (`apps/openkeys`).
- [PANEL.md](product/PANEL.md) — единая админ-панель admin.apitoken.sale (контракт).
- [ADMIN_PANEL.md](product/ADMIN_PANEL.md) — внутренняя админка `apps/admin` (Next.js).

## ops/ — эксплуатация

- [DEPLOYMENT.md](ops/DEPLOYMENT.md) — production deployment runbook (операторский).
- [INFRASTRUCTURE.md](ops/INFRASTRUCTURE.md) — production-инфраструктура и хосты.
- [MONITORING.md](ops/MONITORING.md) — мониторинг и runbook-анкоры алертов (`docs/ops/MONITORING.md#<alert>`).
- [DELETE_WORKTREE.md](ops/DELETE_WORKTREE.md) — постоянная fail-closed очистка замёрженных worktree и явно зарегистрированных клонов на macOS.
- [CLAUDE_CALIBRATION.md](ops/CLAUDE_CALIBRATION.md) — bounded live-прогон Claude: модели, token classes, sticky-подписки и жёсткий nanoUSD-бюджет.
- [GEMINI_CALIBRATION.md](ops/GEMINI_CALIBRATION.md) — exact-profile live-прогон Gemini: immutable backend evidence, capability matrix и общий лимит $40.
- [DEVBOT.md](ops/DEVBOT.md) — дизайн dev-бота Telegram (`apps/devbot`): топики, уведомления, источники событий (этапы 1–3 реализованы; этап 4 — бизнес-события — впереди).
- [FRONTEND_VISUAL_QA.md](ops/FRONTEND_VISUAL_QA.md) — визуальный QA фронтенда.
- [VERCEL_PRODUCT_ANALYTICS.md](ops/VERCEL_PRODUCT_ANALYTICS.md) — продуктовая аналитика Vercel.

## audits/ — аудиты (append-only, не редактируются задним числом)

- [AUDIT.md](audits/AUDIT.md) — архитектурный аудит claude-api.
- [FULL_AUDIT_M.md](audits/FULL_AUDIT_M.md) — полный аудит: движок, бэкенд, фронтенд, связи.
- [TESTS_AUDIT.md](audits/TESTS_AUDIT.md) — аудит полноты и достаточности тестов.
- [2026-08-01-AGENT_DOCS_AUDIT.md](audits/2026-08-01-AGENT_DOCS_AUDIT.md) — аудит системы координации агентов (AGENTS.md, DEPENDENCIES.md, чеклисты, docs-gate).
- [2026-08-03-UNIFIED_ROUTER_PRODUCTION_READINESS.md](audits/2026-08-03-UNIFIED_ROUTER_PRODUCTION_READINESS.md) — production-readiness аудит unified router: resource/auth, protocol parity, catalog, OpenCode и zero-downtime delivery.
- [2026-08-03-UNIFIED_ROUTER_REMEDIATION_CLOSEOUT.md](audits/2026-08-03-UNIFIED_ROUTER_REMEDIATION_CLOSEOUT.md) — closeout remediation unified router: production SHA, повторная live/negative/harness проверка и три внешних/GA остатка.
- [GEMINI_ROUTER_POOL_ACCEPTANCE_2026-08-03.md](audits/GEMINI_ROUTER_POOL_ACCEPTANCE_2026-08-03.md) — production acceptance Gemini pool через unified router: sticky/cache/SSE, ротация, FIFO, бюджет и fail-closed audio remediation.
- [2026-08-03-UNIFIED_ROUTER_RESILIENCE_AUDIT.md](audits/2026-08-03-UNIFIED_ROUTER_RESILIENCE_AUDIT.md) — повторный resilience/scale аудит: body admission, metadata authorities, Caddy fencing, startup probe, observability и честная image capability OpenCode.

## Рядом с кодом (не переносить сюда)

- `crates/<name>/CLAUDE.md` — локальные границы крейтов.
- `packages/db/MIGRATIONS.md` — правила миграций коммерции.
- `deploy/README.md`, `deploy/RELEASES.md` — контроллер доставки и релизы.
- `research/` — исследования и журналы (не инструкции).
