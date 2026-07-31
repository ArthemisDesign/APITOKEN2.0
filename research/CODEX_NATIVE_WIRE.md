# Codex native wire facts (live probe record)

> Статус: **ПОДТВЕРЖДЕНО живым прогоном 2026-07-31** на production-профиле (ChatGPT Pro).
> Прогон: read-only + один tiny turn, без rotation-теста (rotation на проде не проводилась —
> строгая reuse-инвалидация убила бы токен; логика покрыта кодом и тестами).

## Endpoint'ы (все живы, HTTPS+SSE работает)

| Что | URL | Статус |
|---|---|---|
| Генерация (Responses API, SSE) | `POST https://chatgpt.com/backend-api/codex/responses` | ✅ 200, полный SSE-стрим |
| План и окна лимитов | `GET https://chatgpt.com/backend-api/wham/usage` | ✅ 200 |
| Каталог моделей | `GET https://chatgpt.com/backend-api/codex/models?client_version=<ver>` | ✅ 200; `client_version` ОБЯЗАТЕЛЕН (без него 400) |
| OAuth refresh | `POST https://auth.openai.com/oauth/token` | не тестировался (rotation намеренно не гнали на проде) |

## SSE-последовательность (живая запись)

`response.created, response.in_progress, response.output_item.added, response.output_item.done,
response.content_part.added, response.output_text.delta ×N, response.output_text.done,
response.content_part.done, response.output_item.done, response.completed`

События `codex.rate_limits` в этом стриме не было — снапшоты берём из заголовков.

## Rate-limit заголовки (подтверждены дословно)

- `x-codex-primary-used-percent: "100"`, `x-codex-primary-window-minutes: "10080"`
- `x-codex-primary-reset-after-seconds: "383412"`, `x-codex-primary-reset-at: "1785912728"`
- `x-codex-secondary-used-percent`, `x-codex-secondary-window-minutes`,
  `x-codex-secondary-reset-after-seconds`, `x-codex-secondary-reset-at` (пустые для этого плана)
- `x-codex-plan-type: "pro"`, `x-codex-active-limit: "premium"`
- `x-codex-credits-has-credits/balance/unlimited`, `x-codex-bengalfox-*` (отдельное семейство
  лимитов GPT-5.3-Codex-Spark; наши pin'ованные модели им не пользуются)

## `/wham/usage` — точная схема

```json
{
 "plan_type": "pro",
 "rate_limit": {
  "allowed": true,
  "limit_reached": false,
  "primary_window": {
   "used_percent": 100,
   "limit_window_seconds": 604800,
   "reset_after_seconds": 383348,
   "reset_at": 1785912728
  },
  "secondary_window": null
 },
 "additional_rate_limits": [
  {"limit_name": "GPT-5.3-Codex-Spark", "metered_feature": "codex_bengalfox",
   "rate_limit": {"allowed": true, "limit_reached": false,
    "primary_window": {"used_percent": 0, "limit_window_seconds": 604800, ...}}}
 ],
 "credits": {...}, "spend_control": {...}, "rate_limit_reached_type": null
}
```

## Ключевой семантический факт (меняет политику отбора)

**`used_percent=100` при `allowed: true` — ОБСЛУЖИВАЕТ.** На проверенном профиле недельное окно
(единственное, 604800s — 5h-окна у GPT сейчас нет) стоит на 100%, и turn проходит успешно.
Поэтому hard-exclusion делается ТОЛЬКО по явному вердикту провайдера (`limit_reached` /
`allowed: false` / HTTP 429), а рулят отводом трафика от почти-полных окон мягкие reserve-кепки
(98% короткое / 97% weekly). Проценты могут включать расход вне нашего шлюза.

## OAuth / идентичность (подтверждено)

- `Authorization: Bearer`, `ChatGPT-Account-ID`, `originator: codex_cli_rs`,
  `User-Agent: codex_cli_rs/0.145.0 (Linux; x86_64) codex_cli_rs`,
  `OpenAI-Beta: responses=experimental`, `session_id` — приняты, 200.
- Тело: `model, instructions:"", input, tools:[], store:false, stream:true` — принято.

## Открытые вопросы (не блокеры)

1. Появляется ли `codex.rate_limits` в стриме при исчерпании/429 — парсер принимает обе формы.
2. WS-транспорт: HTTP+SSE жив, WS не требуется (проверено на текущую дату).
3. Формат `/wham/usage` на Plus/Business (структура та же, окна могут отличаться).
4. `metered_feature`-семейства (codex_bengalfox) — если добавим Spark-модель в каталог,
   потребуется учитывать её отдельный лимит в selection.
