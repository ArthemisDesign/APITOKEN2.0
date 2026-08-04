# GLM (Zhipu / Z.ai) plane — resumable progress ledger

> Обязательный ledger по `.claude/skills/provider-onboarding/SKILL.md` и
> `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §0. Читать ПЕРВЫМ после любого сброса контекста.
> Каждая строка «сделано» обязана ссылаться на SHA, которым она приземлена.

## Терминальное состояние

Оператор покупает GLM Coding Plan через Auth Bot (device-free: подписка GLM работает по
статическому API-ключу из консоли Z.ai / bigmodel.cn — продавец присылает только ключ,
бот валидирует его через identity/quota endpoint, запечатывает AEAD-конверт и атомарно
публикует roster). Профиль становится маршрутизируемой ёмкостью в липком параллельном пуле,
трафик тарифицируется по официальному rate card Z.ai в integer nanoUSD, калибровка пишет
immutable evidence из реального движения квоты (prompt-based окна). Плоскость default-off,
backend-only (по образцу KIMI, `docs/engine/KIMI_PROVIDER.md` §0) до отдельного решения
владельца о публикации.

Модель бизнеса (аудит реселлеров): китайские реселлеры покупают GLM Coding Plan
($3+/мес, prompt-quota окна), получают plan-bound API key и продают Anthropic-/OpenAI-
совместимый API-доступ поверх квоты подписки — ровно та же модель, что наш Claude-пул
(подписка Max/Pro → API, неотличимый от api.anthropic.com).

## Сделано (SHA на строку)

| Шаг | Артефакт | SHA |
|---|---|---|
| Скелет capability manifest + ledger | `docs/engine/GLM_PROVIDER.md`, этот файл | 03cf2582 |
| Шаблон цепочки + pre-flight в ledger | этот файл | faf8c25a |
| Полный research + capability manifest | `docs/engine/GLM_PROVIDER.md` | 2938d61e |
| Metering: API rate card + credit schedule | `crates/metering/src/glm.rs` | 9cc4a955 |
| Migration 0029 + registry observation types | `crates/registry/{migrations_pg/0029_glm_window_calibration.sql,src/glm_calibration.rs,src/pg.rs}` | 40b6d866 |
| **Мёрж в master, deploy/watchdog GREEN** | master `e9810ef9c6e2a000edb8fac9ad8997d04c8dad9b` (trusted host validation GREEN) | e9810ef9 |
| Credential crate | `crates/glm-credential` (+ корневой Cargo.toml/lock) | 0363581a |
| Calibration estimator | `crates/forward/src/glm_calibration.rs` | 3825be3b |
| Auth Bot: протокол + roster (dormant) | `crates/authbot/src/{glm_key,glm_roster}.rs`, `main.rs`, CLAUDE.md | 3378eb06 |
| Auth Bot: мастер продавца | `crates/authbot/src/{bot,db,main}.rs`, CLAUDE.md | 093c05c2 |
| Runtime примитивы | `crates/forward/src/glm/{config,transport,roster,client,selection,pool,queue,mod}.rs` | 9ce68f55 |
| Gateway + dispatch + billing writer | `crates/forward/src/glm/gateway.rs`, `proxy.rs`, `state.rs`, `billing.rs` | 884b8869 |
| Server wiring | `crates/server/src/{config,main,poller}.rs`, server CLAUDE.md | 08baad97 |
| Docs: причины отложенных строк §8 | `docs/engine/GLM_PROVIDER.md` | 0869040a |
| **Мёрж runtime-цепочки в master, deploy/watchdog GREEN** | master `1c8b7fc1eda45025bc1e04dc917a0cf7087aba45` (trusted host validation GREEN, deployment 5736492612; ребейз на 54fb2220: landed SHAs bc4c23ce, 1d279145, d42e916b, 2525973b, b4eab375, edb126f2, 36dc9a5d, 1c8b7fc1) | 1c8b7fc1 |
| Observability: operational status, admin-only `GET /glm-subs` (+fleet `window_totals`), aggregate метрики, `glm-provider` алерты + runbook + consistency-пины | `crates/{forward,server}`, `observability/**`, `deploy/**`, `docs/**` | _этот коммит_ |

## Рамка от владельца (2026-08-04)

**Только backend, тестовый режим.** НИКАКИХ изменений сайта (`apps/web`), публичных
docs/док-портала, фронта (`apps/admin`, `apps/openkeys`), каталога роутера и любых
публичных поверхностей. Внутренние инженерные доки репо (манифест, CLAUDE.md крейтов,
ledger) — рабочий журнал, не публикация. Совпадает с backend-only решением манифеста §0.

## Инцидент и восстановление (2026-08-04)

После мёржа `feat/glm-provider` в master LaunchAgent DELETE_WORKTREE снёс worktree как
clean+merged (штатный критерий) вместе с локальной веткой. Восстановлено: worktree
пересоздан на remote-ветке (== e9810ef9) и залочен (`git worktree lock`; снять lock
перед финальным `agent-worktree.sh finish`). Estimator был построен в scratch-дереве
`~/wt/glm-forward-calibration` (от master) и перенесён сюда; scratch-дерево убирается
через `agent-worktree.sh finish`. Урок: после промежуточного мёржа держать на ветке
новый коммит (или lock), чтобы reaper не считал дерево завершённым.

## Открытые швы

- Нет. Цепочка от credential до observability замкнута: плоскость композируется сервером,
  default-off, пять env-ключей CLAUDE_API_GLM_* (fleet BASE_URL отклоняется fail closed),
  engine-side admin projection и алерты на месте. За рамкой backend-ядра: same-origin admin UI
  consumer `/glm-subs` (`apps/admin`, отдельный checkpoint), live-runner
  `tools/glm_calibration/` и live-матрица (нужна подписка — блокирует человек).

## Следующее действие (ровно одно)

Backend-цепочка полностью в master с зелёным deploy/watchdog (`1c8b7fc1`). Дальше — только
человек/оператор: (1) provision host env движка `CLAUDE_API_GLM_ENABLED=true`,
`CLAUDE_API_GLM_ROSTER_DIR=/srv/claude-api/data/glm`, `CLAUDE_API_GLM_CREDENTIAL_KEYS`,
`CLAUDE_API_GLM_CREDENTIAL_ACTIVE_KID` (опционально `CLAUDE_API_GLM_AUTH_SCHEME=bearer`,
`CLAUDE_API_GLM_QUOTA_POLL_SECS=300`) и симметрично `AUTH_BOT_GLM_*` для authbot;
(2) первая живая GLM Coding Plan подписка через Auth Bot → live-гейты из манифеста §6
(usage-форма, SSE-инкрементальность, единицы quota endpoint, quota wall на живом аккаунте)
→ live-runner `tools/glm_calibration/` → same-origin admin UI consumer `/glm-subs`
(`apps/admin`, отдельный checkpoint) → verified preview report.

Ключевые факты research (дата ревью 2026-08-03): credits-система с 2026-07-30
(Lite 2000/5ч+10000/нед, Pro 12000/60000, Max 28000/140000); формула кредитов
`(in×m_in + cached×m_c + out×m_out)/10000` с мультипликаторами 5.2=6.9/1.7/24,
5-Turbo=5.7/1.5/21, 4.7=4.6/1.2/16, off-peak ×0.5 (пн–пт 14:00–18:00 UTC+8); endpoint'ы
`api.z.ai/api/anthropic` + `api.z.ai/api/coding/paas/v4` (Bearer); quota endpoint
`GET /api/monitor/usage/quota/limit` (Authorization без Bearer, HTTP 200 + code:401 =
невалидный ключ); коды 1308=5ч wall, 1310=weekly wall, 1309=план истёк, 1311=модель не в
плане, 1313=fair-use; только 3 модели на плане (glm-5.2 1M, glm-5-turbo 200K, glm-4.7 200K);
reroute glm-5.1/5→5.2 (billing по served); rate card 5.2=$1.40/0.26/4.40,
5-turbo=$1.20/0.24/4.00, 4.7=$0.60/0.11/2.20; ToS запрещает resale/proxy → backend-only.

## Очередь

1. research / capability manifest (`docs/engine/GLM_PROVIDER.md`)
2. metering rate card (`crates/metering/src/glm.rs`)
3. additive migration + registry observation types (`crates/registry`)
4. credential crate (`crates/glm-credential`)
5. calibration estimator (`crates/forward/src/glm_calibration.rs`)
6. Auth Bot: протокол + мастер продавца (`crates/authbot/src/glm_*.rs`, `bot.rs`)
7. transport/pool/gateway (`crates/forward/src/glm/**`)
8. server wiring (`crates/server/src/config.rs` + composition)
9. observability/admin projection
10. безопасный live-runner (`tools/glm_calibration/`)
11. live-матрица на собственной подписке — **заблокировано человеком (нужна GLM-подписка)**

Шаблон цепочки снят с git-истории KIMI (b0937339 manifest → f5fbc9c2 metering →
140b67d9 migration → ce9cd625 credential → fafb8682 estimator → 9d357dfb/5dd973cf/dc175204
authbot → d8a37422…23e7baba runtime). Отличие GLM от KIMI на входе: статический API key
из консоли вместо OAuth device flow — acquisition ближе к Claude setup-token ветке
(продавец присылает ключ, бот валидирует probe'ом, seal, atomic roster), но без
single-flight refresh-контура.

Проверено заранее: baseline `cargo build --locked` зелёный; `git push -u origin HEAD`
работает (ветка на origin); следов прежнего GLM в репо нет.

## Заблокировано человеком

- Живая GLM Coding Plan подписка для live-гейтов (preview GA blocked).
