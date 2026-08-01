# Codex native wire facts (live probe record)

> Статус: **ПОДТВЕРЖДЕНО живыми прогонами 2026-07-31 и 2026-08-01** на четырёх
> production-профилях (ChatGPT Pro).
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
  `User-Agent: codex_cli_rs/0.146.0 (Linux; x86_64) codex_cli_rs`, `version`, а для turn также
  `session-id`, `thread-id`, `x-client-request-id`, `x-codex-window-id` и
  `x-codex-turn-metadata` — приняты, 200. Root `session-id == thread-id`; `turn_id` уникален для
  turn, installation/thread/window стабильны. Legacy `OpenAI-Beta: responses=experimental`,
  underscore `session_id` и отдельный installation header официальный 0.146 wire не посылает.
- Тело: `model, instructions:"", input, tools:[], store:false, stream:true`,
  `include:["reasoning.encrypted_content"]` и зеркальный `client_metadata` — принято.

## Fast / service tier (полный прогон 2026-08-01)

Проверены все четыре pool-профиля с официальной идентичностью Codex 0.146 и модели
`gpt-5.4`, `gpt-5.5`, `gpt-5.6-sol`, `gpt-5.6-terra`:

- `/codex/models?client_version=0.145.0` и `0.146.0` возвращают одинаковый каталог; у моделей
  одновременно есть текущий `service_tiers[].id = "priority"` и legacy
  `additional_speed_tiers = ["fast"]`;
- `POST /responses` с `service_tier: "priority"` принимает запрос (HTTP 200), но
  `response.completed.response.service_tier` на каждом аккаунте равен `default`;
- literal `service_tier: "fast"` backend отклоняет: HTTP 400 `Unsupported service_tier`;
- результат одинаков для HTTP+SSE и WebSocket v2, для минимального тела и для полной формы
  официального клиента (`tools/tool_choice/parallel_tool_calls`, `reasoning`,
  `include: ["reasoning.encrypted_content"]`, `prompt_cache_key`, `client_metadata`, session/thread/
  installation/window headers);
- `/wham/usage` не содержит Fast-полей, `has_credits: false`.

Исходники официального `rust-v0.146.0` подтверждают: `ServiceTier::Fast.request_value()` — ровно
`"priority"`; `/fast` меняет клиентскую настройку и не вызывает отдельный endpoint активации.
Официальная документация описывает Fast как 1.5x режим для GPT-5.4/5.5/5.6, а config reference
явно говорит, что пользовательский `fast` отображается в request value `priority`:

- https://learn.chatgpt.com/docs/agent-configuration/speed#fast-mode
- https://learn.chatgpt.com/docs/config-file/config-reference#configtoml

Финальный `service_tier=default` для ChatGPT-auth не является downgrade-вердиктом. Это независимо
воспроизведено в `openai/codex#14204`, `#30413` и `#32191`; maintainer в
https://github.com/openai/codex/issues/14204#issuecomment-4033184620 подтвердил, что Fast в этом
режиме маршрутизируется сервером, а поле финального ответа не годится для end-to-end проверки.

Production A/B на одинаковом длинном выводе `gpt-5.5` подтвердил реальный Fast при reported
`default`: медиана Standard `67.36 output tok/s`, priority `102.02 output tok/s`, то есть `1.514x`;
медианное полное время `29.807s` против `20.985s`. Четыре priority-turn были успешно обслужены
четырьмя разными pool-профилями. Поэтому успешный принятый `priority` — effective Fast для
публичного ответа, settlement, ledger и калибровочного spend; completed tier хранится отдельно как
`provider_reported_tier` только для wire-диагностики. Fast-routing опирается на capability каталога
и не демотирует профиль из-за reported `default`.

## Открытые вопросы (не блокеры)

1. Появляется ли `codex.rate_limits` в стриме при исчерпании/429 — парсер принимает обе формы.
2. Формат `/wham/usage` на Plus/Business (структура та же, окна могут отличаться).
3. `metered_feature`-семейства (codex_bengalfox) — если добавим Spark-модель в каталог,
   потребуется учитывать её отдельный лимит в selection.
