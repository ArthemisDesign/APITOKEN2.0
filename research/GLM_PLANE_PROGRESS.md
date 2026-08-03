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
| Migration 0029 + registry observation types | `crates/registry/{migrations_pg/0029_glm_window_calibration.sql,src/glm_calibration.rs,src/pg.rs}` | _этот коммит_ |

## Открытые швы

- Нет. Authority запечатана expand-only миграцией 0029 (schema 28→29), стоит рядом с 0019
  и 0027, ничего чужого не трогает. Зависимый код (credential/estimator/runtime) мёржится
  только после зелёных deploy/migration + deploy/watchdog на SHA миграции.

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

## Следующее действие (ровно одно)

Мёрж metering + migration через `git push -u origin HEAD && ./deploy/agent-merge.sh`;
дождаться зелёного deploy/watchdog (в т.ч. deploy/migration на SHA миграции). Затем
credential crate `crates/glm-credential` по образцу `crates/kimi-credential`.

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
