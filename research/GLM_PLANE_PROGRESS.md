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
| Скелет capability manifest + ledger | `docs/engine/GLM_PROVIDER.md`, этот файл | _pending first commit_ |

## Открытые швы

- Весь research по GLM Coding Plan — только стартовые факты, детали `unknown`.

## Следующее действие (ровно одно)

Deep research: официальные страницы Z.ai/bigmodel.cn (pricing, coding plan docs), OSS-клиенты
(claude-code-router и аналоги), точные эндпоинты/квоты/usage → дописать манифест.

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

## Заблокировано человеком

- Живая GLM Coding Plan подписка для live-гейтов (preview GA blocked).
