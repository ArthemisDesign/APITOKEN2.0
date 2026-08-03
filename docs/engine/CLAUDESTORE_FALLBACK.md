# ClaudeStore — аварийный fallback Claude-plane

Статус: **implemented / default-off / production live pending**. Этот transport не является частью
локального Claude subscription-пула, не публикуется как отдельный провайдер или модель и по
умолчанию выключен. Его единственная роль — последний Anthropic-compatible attempt после
доказанного исчерпания всей локальной pre-byte ротации.

## Граница применения

- Клиентский запрос, модель, тариф и один внутренний money/request identity остаются Claude-plane.
- Локальные подписки всегда имеют приоритет. Единичный account/network/5xx/429 продолжает штатную
  локальную ротацию и сам по себе не разрешает внешний вызов.
- Внешний attempt допустим только до первого публичного байта и только после результата
  `local pool exhausted/unavailable`. После первого байта replay или смена transport запрещены.
- Authoritative terminal Anthropic usage тарифицируется существующим exact metering и завершает
  исходный reserve ровно один раз.
- Внешний turn не создаёт Claude subscription quota/calibration observation, affinity или health
  attribution конкретному локальному аккаунту.
- Secret читается только `crates/server/src/config.rs`, не возвращается через API/метрики/логи и
  хранится только в production secret env. Production URL compile-fixed; произвольный upstream из
  окружения задать нельзя.
- Switch принимается только как strict `0|1|false|true`; enabled без валидного `sk-cs4-*` secret
  или на fixed OpenAI/Gemini plane останавливает startup.

## Capability manifest

| Поле | Evidence | Состояние / решение |
|---|---|---|
| Продукт | `official`, 2026-08-03 | ClaudeStore — независимый pay-as-you-go API gateway, не Claude subscription provider |
| Credential | `official`, 2026-08-03 | API key в `x-api-key`; plaintext допускается только в secret env |
| Native endpoint | `official` + unauthenticated live, 2026-08-03 | `POST https://api3.claudestore.store/v1/messages`; unauthenticated `/v1/models` отвечает bounded 401 `Missing API key` |
| Anthropic version | `official`, 2026-08-03 | `anthropic-version: 2023-06-01` |
| Non-stream | `official`; authenticated live `unknown` | Anthropic Messages JSON с terminal `usage.input_tokens/output_tokens` |
| Streaming | `official`; authenticated live `unknown` | SSE `message_start` → deltas → `message_stop`; mock подтверждает отсутствие post-byte replay, incrementality и terminal usage живого сервиса ещё не проверены |
| Tools | `official`; authenticated live `unknown` | Документация заявляет стандартный Anthropic `tools`; fail-closed сохраняется текущей wire-валидацией |
| Models | `official` catalogue; key-scoped live `unknown` | Fallback не переписывает model id; неизвестная/недоступная модель завершает единственный внешний attempt, а наружу остаётся sanitized local terminal response |
| Upstream quota | `official` | 429 + `Retry-After`; не используется как Claude subscription quota evidence |
| Billing | `official`; authenticated live `unknown` | ClaudeStore списывает Anthropic-equivalent credits; customer settlement остаётся по локальному Anthropic rate card и terminal usage |
| Data | `official`, policy v3.0 | Prompt/response content заявлен как не сохраняемый после request cycle; usage metadata хранится 12 месяцев |
| Rollback | `decision` | Удалить/очистить secret env или выключить strict boolean; локальный пул продолжает работать без внешней зависимости |

## Wire и ошибки

| Операция | Контракт | Решение runtime |
|---|---|---|
| Messages | `POST /v1/messages`, `x-api-key`, `anthropic-version`, исходный Anthropic body | Forward байтов без OpenAI-transliteration; private локальные subscription headers не отправляются |
| Stream | Anthropic SSE | Использовать существующий `TeeMeter`; до первого публичного байта возможен только один внешний attempt, после него replay запрещён |
| 400/401/403/402 | terminal client/credential/balance failure | Не повторять; скрыть ClaudeStore credential/balance details и вернуть уже вычисленный локальный terminal response |
| 429 | external capacity/rate limit, optional `Retry-After` | Не повторять и не записывать Claude subscription cooling/quota; наружу остаётся локальный terminal response |
| 5xx/network до bytes | внешний transport fault | Не повторять; наружу остаётся локальный terminal response, каскад на другие внешние сервисы отсутствует |
| malformed/EOF после bytes | post-byte stream failure | Не replay; settlement следует существующей conservative missing-usage политике |

После любого начатого внешнего `send` неуспех считается execution-ambiguous: клиент получает
санитизированный локальный terminal status/body и refund, но `x-apitoken-execution-state:
not_started` снимается. Поэтому router не может начать ещё одну billable continuation по ложному
доказательству; только полный pre-external local terminal сохранял бы этот proof.

## Письменное разрешение

Действующие [Terms and Conditions](https://claudestore.store/terms-and-conditions/) версии
`v3.0-2026-07-23`, пункт 8.2, запрещают resell/redistribute/sublicense API access третьим лицам без
явного письменного согласия ClaudeStore. Клиентский fallback-трафик попадает в эту зону.

3 августа 2026 оператор получил от администратора ClaudeStore явное письменное разрешение
APIToken.sale использовать ключ ClaudeStore как резервный upstream для обработки клиентских
запросов и redistribution API-доступа. Оригинал переписки и identity отправителя хранит оператор
вне Git; screenshot, персональные данные и credential в репозиторий не копируются. Этот grant
снимает blocker пункта 8.2 для заявленного сценария, но не заменяет технические live-гейты ниже.

## Evidence и незакрытые live-гейты

Официальные источники, просмотрены 2026-08-03:

- [LLM-readable service index](https://claudestore.store/llms.txt) — canonical API3 base URL,
  Anthropic/OpenAI surfaces и pay-as-you-go product identity.
- [Messages API](https://claudestore.store/docs/api-reference/messages/) — request/response fields,
  `x-api-key`, usage и заявленная Anthropic SDK compatibility.
- [Streaming](https://claudestore.store/docs/api-reference/streaming/) — Anthropic SSE event shape.
- [Errors](https://claudestore.store/docs/api-reference/errors/) и
  [Rate limits](https://claudestore.store/docs/guides/rate-limits/) — 4xx/5xx/529 и 429
  `Retry-After`; стабильный RPM/TPM не публикуется.
- [Privacy Policy](https://claudestore.store/legal/privacy/) `v3.0-2026-07-23` — request-cycle
  content handling и 12-month usage metadata retention.
- Сайт ссылается на GitHub `zerofeesclub/claudestore`, но на дату проверки ссылка отвечает
  `Repository not found`. Поэтому независимого inspectable implementation SHA нет: official docs
  остаются wire-authority, а расхождение считается явным evidence conflict, не подтверждением кода.

До serving остаются обязательными:

1. secret provisioning вне git с подтверждёнными owner/mode и kill switch;
2. bounded authenticated live matrix: supported model list, minimal non-stream generation с
   terminal usage, настоящий incremental SSE, deterministic 4xx, insufficient-balance/429 и
   secret/privacy scan;
3. post-deploy smoke на exact watchdog-green SHA с проверкой единственного settlement и нулевой
   local subscription calibration attribution.

Mock-матрица уже фиксирует: healthy local → 0 external attempts; local retry success → 0; empty
pool → ровно 1; external 5xx → локальный terminal + refund; успешный ответ → customer settlement
без local subscription attribution; post-byte SSE failure → error tail без replay. Эти тесты,
сборка и merge сами по себе не закрывают authenticated live-гейты и не означают GA.
