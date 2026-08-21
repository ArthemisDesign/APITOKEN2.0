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

## Две независимые шкалы стоимости (проверено 2026-08-01)

Официальная документация публикует отдельные тарифы для API и для расхода ChatGPT Codex credits.
Это не две записи одной и той же цены: API nanoUSD отвечает «сколько стоили бы эти токены через
API», а credits отвечают «какую нормализованную долю подписочной квоты съела нагрузка». Поэтому
одинаковые Pro-подписки нельзя сравнивать по единственному USD total, если на них попадала разная
смесь моделей, cache hit и output.

Подписочная credit-карта на 1M токенов (input / cached input / output):

| Модель | Input credits | Cached credits | Output credits |
|---|---:|---:|---:|
| GPT-5.6 Sol / GPT-5.5 | 125 | 12.5 | 750 |
| GPT-5.6 Terra | 50 | 5 | 300 |
| GPT-5.6 Luna | 5 | 0.5 | 30 |
| GPT-5.4 | 62.5 | 6.25 | 375 |

Reasoning уже входит в `output_tokens`, cached input — подмножество `input_tokens`: прибавление
этих полей второй раз завысило бы расход. В credit-карте нет отдельного cache-write тарифа или
long-context множителя, поэтому они допустимы в API-USD расчёте, но не выдумываются для credits.

С 2026-07-30 API-тариф Terra равен `$2 / $0.20 / $2.50 / $12`, Luna —
`$0.20 / $0.02 / $0.25 / $1.20` на 1M fresh / cached / cache-write / output. API Fast для
GPT-5.6 стал `2x`, но подписочный Fast по-прежнему списывает `2.5x` credits; для GPT-5.4 обе
официальные карты используют `2x`. Каталог хранит эти величины раздельно и effective-dated, чтобы
новая цена не переписывала исторические turn.

Источники:

- https://learn.chatgpt.com/docs/pricing#what-are-tokens-and-credits
- https://learn.chatgpt.com/docs/agent-configuration/speed#fast-mode
- https://developers.openai.com/api/docs/changelog#july-2026

## Codex CLI 0.149 custom-provider live acceptance — 2026-08-21

Exact `@openai/codex@0.149.0 --profile apitoken` completed one ephemeral read-only turn through
`router.apitoken.sale` on `gpt-5.6-luna`:

- no tool call and no filesystem mutation;
- one assistant item with the expected `OK` text;
- authoritative terminal usage: input `14157`, cached input `0`, cache-write input `0`, output `5`,
  reasoning output `0`;
- temporary workdir was deleted after the process exited.

This is GREEN evidence for the **public named custom-provider boundary**.

## Native ChatGPT identity 0.149 proof — 2026-08-21

A separate official `@openai/codex@0.149.0 login --device-auth` completed in an isolated mode-0700
`CODEX_HOME` on a throwaway ChatGPT Pro account. The credential was read only inside the proof
process and deleted immediately after the run; refresh-family rotation and WebSocket were not run.

Direct private-backend evidence under exact `originator: codex_cli_rs`, User-Agent/version `0.149.0`:

- `GET /backend-api/codex/models?client_version=0.149.0` → HTTP 200, 9 entries;
  `gpt-5.6-luna` advertises current `priority` and legacy `fast`;
- `GET /backend-api/wham/usage` → HTTP 200, paid Pro plan, `allowed:true`, no reached quota wall;
- one `POST /backend-api/codex/responses` on Luna → HTTP 200, incremental SSE sequence
  `created → in_progress → output item/content/text deltas/done → completed`;
- exact request controls accepted: `parallel_tool_calls:false`, reasoning effort `low`,
  summary `auto`, context `all_turns`, `store:false`, `stream:true`, no tools;
- authoritative terminal usage: input `14`, cached `0`, output `6`, reasoning `0`, total `20`;
- response headers contained the reviewed `x-codex-*` quota families and `x-codex-turn-state`;
- requested priority reported completed tier `default`, the already documented ChatGPT-auth diagnostic
  behavior and not proof of a Fast downgrade.

This closes native identity admission for `CODEX_CLI_VERSION=0.149.0`. The earlier direct Python
device-flow HTTP 530 was a fingerprint limitation of the hand-written login request, not a backend
wire refusal; official CLI device auth succeeded.

The paid-turn authorization target was `100000 nanoUSD` (`$0.0001`), but actual Codex system/tool
context produced 14157 input tokens. At the current Luna official card that is approximately
`$0.00284` before discount and `$0.00142` at the current 50% B2C multiplier. The probe cannot enforce
a pre-dispatch nanoUSD cap because it does not know the final Codex-composed prompt before submission.
Its flags are now documented as an operator authorization target, not a hard cost ceiling. No second
paid turn was run.

## Открытые вопросы (не блокеры)

1. Появляется ли `codex.rate_limits` в стриме при исчерпании/429 — парсер принимает обе формы.
2. Формат `/wham/usage` на Plus/Business (структура та же, окна могут отличаться).
3. `metered_feature`-семейства (codex_bengalfox) — если добавим Spark-модель в каталог,
   потребуется учитывать её отдельный лимит в selection.
