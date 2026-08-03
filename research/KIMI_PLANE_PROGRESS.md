# KIMI plane — журнал прогресса (resumable)

**Назначение.** Этот файл существует, чтобы работа над плоскостью KIMI не терялась при сбросе
контекста агента. Он — источник истины о том, что сделано и что делать следующим. После любого
компакта/рестарта агент читает **сначала его**, а не восстанавливает план по памяти.

Правила ведения:

- Обновляется **в том же коммите**, что и сам шаг. Отдельный «коммит про прогресс» отстаёт от
  реальности и хуже отсутствующего.
- Строка «сделано» обязана нести SHA. Без SHA утверждение непроверяемо.
- Раздел «следующее действие» всегда содержит ровно одну конкретную задачу, а не список пожеланий.

Контракты: `docs/engine/PROVIDER_ONBOARDING.md` (что доказать),
`docs/engine/PROVIDER_WIRING_CHECKLIST.md` (что редактировать),
`docs/engine/KIMI_PROVIDER.md` (факты о провайдере и открытые `unknown`).

## Терминальное состояние (когда работа считается законченной)

1. Оператор создаёт оффер KIMI, продавец проходит путь целиком, профиль публикуется в roster.
2. Движок читает roster, профиль становится маршрутизируемой ёмкостью и отдаёт трафик.
3. Трафик тарифицируется по официальному rate card **по served-модели**.
4. Калибровка пишет immutable evidence из реального движения квоты `/usages`.
5. Точный production SHA зелёный, live-матрица пройдена на нашей подписке.

Пункты 1–4 достижимы без живой подписки (моки/гейты). Пункт 5 без неё заблокирован.

## Сделано

| Шаг | Артефакт | SHA |
|---|---|---|
| research / capability manifest | `docs/engine/KIMI_PROVIDER.md` | `dfb6ff52` |
| официальный rate card | `crates/metering/src/kimi.rs` | `f9234643` |
| calibration authority (schema) | `migrations_pg/0027_kimi_window_calibration.sql` | `2afdd183` |
| credential | `crates/kimi-credential` | `b895587a` |
| типы наблюдений + estimator | `registry/src/kimi_calibration.rs`, `forward/src/kimi_calibration.rs` | `2ff081a9` |
| Auth Bot: device-code протокол | `crates/authbot/src/kimi_oauth.rs` | `f95f9f05` |
| Auth Bot: продукт и кнопки тарифов | `crates/authbot/src/bot.rs` | `4ed97c07` |
| механическая карта подключения | `docs/engine/PROVIDER_WIRING_CHECKLIST.md` | `98ab7093` |
| **всё вышеперечисленное в master, watchdog GREEN** | — | `62c49ab6` |
| правило завершённости в скилле и карте | `SKILL.md`, чеклист | ветка |
| Auth Bot: публикация roster | `crates/authbot/src/kimi_roster.rs` | ветка |

## Открытые швы (выглядит подключённым, не работает)

1. **`km_ready` не обработан.** Оффер KIMI создаётся и принимается, но сделка не завершается:
   device-код не выдаётся, `kimi_oauth` и `kimi_roster` никто не вызывает.
2. **Профиль в roster не читается никем.** Плоскости `crates/forward/src/kimi/` нет, поэтому
   опубликованная подписка не становится ёмкостью и трафик по ней не идёт.

Оба шва обязаны быть закрыты до любого заявления о готовности.

## Следующее действие

Обработчик `km_ready` в `crates/authbot/src/bot.rs`: выдать device-код и
`verification_uri_complete`, перевести продавца в `km_wait`, поллить
`/api/oauth/token` под generation guard, получить `/me`, опубликовать через `kimi_roster::publish`
**до** завершения выплаты. Образец — Codex `cx_wait` (тоже device-flow) и generation-fencing
Gemini. Требования: cancel/retry/restart не платят и не двигают чужую сделку; истёкший или
отказанный flow не оставляет ни конверта, ни строки roster.

## Очередь после этого

1. `crates/forward/src/kimi/` — roster loader, транспорт поверх `api.kimi.com/coding`
   (Anthropic-нативный, трансляционный слой не нужен), липкий пул, single-flight refresh
   ротирующей семьи, оси здоровья, стрим, reserve/settle.
2. Durable-калибровка: read/write поверх таблиц `0027` + поллер `/usages`, дренаж turn-FIFO перед
   каждым опросом квоты.
3. `crates/server`: env, wiring плоскости, readiness. **Пробу нельзя вешать на `/v1/models`** —
   он негейтед и отвечает 200 на мёртвый ключ; бить в `/messages` или `/me`.
4. `tools/kimi_calibration/run_live.py` — dry-run по умолчанию, целочисленный бюджет, точная
   атрибуция по immutable request id.
5. Observability, blue-green, live-матрица.

## Заблокировано человеком

- **Живая подписка Kimi Code.** Без неё не снимаются 8 `unknown` из §6 манифеста: auth-заголовок
  Anthropic-маршрута, форма terminal usage, реальная инкрементальность SSE, единица `used`,
  различение 401/403, набор планов, поведение месячного потолка, платные tool/search-единицы.
- **Красный master.** На 2026-08-03 `628b941e` (*observability: Redis pressure / Codex history*,
  чужая работа) уронил `deploy/watchdog` в фазе `verifying`. Мёржить поверх нельзя; ветка
  `feat/kimi-plane-completion` ждёт зелёного master.
