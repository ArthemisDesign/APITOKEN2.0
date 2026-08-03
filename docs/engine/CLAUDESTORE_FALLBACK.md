# ClaudeStore — аварийный fallback Claude- и GPT-plane

Статус Claude transport: **implemented / default-off / production live pending**. Статус GPT
transport: **implemented / default-off / blocked до отдельного Codex-tier credential и live gate**.
Оба transport не являются частью локальных subscription-пулов, не публикуются как отдельный
провайдер или модель и по умолчанию выключены. Их единственная роль — последний совместимый attempt
после terminal результата штатной локальной pre-byte ротации своей provider plane.

## Граница применения

- Клиентский запрос, модель, тариф и один внутренний money/request identity остаются в исходной
  Claude или GPT plane; новый публичный provider/model/catalog не появляется.
- Локальные подписки всегда имеют приоритет. Единичный account/network/5xx/429 продолжает штатную
  локальную rotation/retry policy и сам по себе не разрешает внешний вызов.
- Внешний attempt допустим только до первого публичного байта и только после результата
  `local pool exhausted/unavailable`. После первого байта replay или смена transport запрещены.
- Authoritative terminal Anthropic/OpenAI usage тарифицируется существующим exact metering своей
  plane и завершает исходный reserve ровно один раз. GPT transport fail closed при нулевом или
  внутренне противоречивом terminal usage.
- Внешний turn не создаёт local subscription quota/calibration observation, affinity или health
  attribution конкретному локальному аккаунту/profile.
- Secret читается только `crates/server/src/config.rs`, не возвращается через API/метрики/логи и
  хранится только в production secret env. Production URL compile-fixed; произвольный upstream из
  окружения задать нельзя.
- Каждый switch принимается только как strict `0|1|false|true`; enabled без своего валидного
  `sk-cs4-*` secret или не на своей fixed provider plane останавливает startup. Claude использует
  `CLAUDE_API_CLAUDESTORE_{FALLBACK_ENABLED,API_KEY}`, GPT — отдельные
  `CLAUDE_API_CLAUDESTORE_CODEX_{FALLBACK_ENABLED,API_KEY}`. Ключи нельзя переиспользовать:
  ClaudeStore переключает universal key между Basic/Claude и Codex tier.

## Claude Messages capability manifest

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

## Claude Messages wire и ошибки

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

## GPT/Codex capability manifest

Полный dated evidence dossier: [`research/CLAUDESTORE_GPT_FALLBACK_EVIDENCE.md`](../../research/CLAUDESTORE_GPT_FALLBACK_EVIDENCE.md).

| Поле | Evidence | Состояние / решение |
|---|---|---|
| Credential | `official`, 2026-08-03 | Отдельный `sk-cs4-*` universal key, переключённый в dashboard на ClaudeStore Codex tier; Basic/Claude key непригоден |
| Native endpoint | `official` + unauthenticated live, 2026-08-03 | `POST https://api3.claudestore.store/v1/responses`, Bearer auth; без ключа endpoint и `/v1/models` возвращают bounded 401 |
| Public adapters | `implementation` | Исходные `/v1/responses`, `/v1/chat/completions` и Anthropic skin GPT-plane сходятся во внутренний Responses turn; внешний transport всегда использует только `/v1/responses` |
| Models | `official`; authenticated live `unknown` | Compile-fixed allowlist: `gpt-5.5`, `gpt-5.4`; live `/v1/models` не расширяет его автоматически |
| Streaming | `official`; authenticated live `unknown` | Документация заявляет Responses SSE; mock проверяет декодирование и отсутствие replay, но настоящий incremental stream ещё не доказан |
| Usage | `official`; authenticated live `unknown` | Требуется terminal OpenAI `usage` с ненулевым input/total и согласованной суммой; иначе attempt считается failed |
| Tools/reasoning/structured output/Fast | `official` частично; live `unknown` | Dormant transport сохраняет внутренний Responses body; включение блокируется до controlled matrix всех реально продаваемых controls |
| Local identity | `implementation` | Не отправляются `chatgpt-account-id`, `originator`, `client_metadata`, OAuth credential, proxy или private local upstream slug; публичный model id восстанавливается перед send |
| Accounting | `implementation` | Существующий Codex reserve/settlement и локальный OpenAI tariff; ClaudeStore turn не пишет ChatGPT quota, affinity или calibration evidence |
| Rollback | `decision` | Выключить Codex switch/удалить отдельный secret; локальный ChatGPT pool продолжает работать |

## GPT/Codex wire и ошибки

| Операция | Контракт | Решение runtime |
|---|---|---|
| Responses | `POST /v1/responses`, `Authorization: Bearer`, JSON Responses body, SSE | Максимум один attempt после terminal локальной rotation policy; compile-fixed origin и model allowlist |
| Chat Completions / Anthropic skin | Публичные адаптеры APIToken.sale | Используют общий внутренний turn; отдельные вызовы ClaudeStore `/chat/completions` или `/messages` не выполняются |
| 400/401/402/403/429/5xx/network | External terminal failure | Не повторять, не менять local home health/quota; вернуть исходный локальный status с bounded body и без `not_started` proof |
| Output начался | Responses SSE delta | Ни локальный, ни внешний attempt больше не запускается; post-byte replay запрещён |
| Terminal usage отсутствует/нулевой | Недостаточно authority для exact settlement | Не считать success и не писать calibration; activation gate обязан доказать nonzero usage до включения |

GPT fallback намеренно не заменяет Codex provider при startup с пустым sealed roster: это аварийный
transport действующего subscription-пула, а не самостоятельная provider plane. Конструктор OpenAI
runtime по-прежнему требует хотя бы один валидный local profile.

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

- [LLM-readable service index](https://claudestore.store/llms.txt) и
  [full reference](https://claudestore.store/llms-full.txt) — canonical API3 base URL, разделение
  Basic/Claude и Codex tier, Anthropic/OpenAI surfaces и pay-as-you-go product identity.
- [Messages API](https://claudestore.store/docs/api-reference/messages/) — request/response fields,
  `x-api-key`, usage и заявленная Anthropic SDK compatibility.
- [Streaming](https://claudestore.store/docs/api-reference/streaming/) — Anthropic SSE event shape.
- [Errors](https://claudestore.store/docs/api-reference/errors/) и
  [Rate limits](https://claudestore.store/docs/guides/rate-limits/) — 4xx/5xx/529 и 429
  `Retry-After`; стабильный RPM/TPM не публикуется.
- [OpenAI & Codex endpoints](https://claudestore.store/docs/api-reference/codex/) — отдельный
  Codex-tier key, Bearer auth, `/v1/responses`, `/v1/chat/completions`, `/v1/models`, модели
  `gpt-5.5`/`gpt-5.4`, заявленные SSE и terminal OpenAI usage.
- [Privacy Policy](https://claudestore.store/legal/privacy/) `v3.0-2026-07-23` — request-cycle
  content handling и 12-month usage metadata retention.
- Сайт ссылается на GitHub `zerofeesclub/claudestore`, но на дату проверки ссылка отвечает
  `Repository not found`. Поэтому независимого inspectable implementation SHA нет: official docs
  остаются wire-authority, а расхождение считается явным evidence conflict, не подтверждением кода.

До serving остаются обязательными:

1. plane-specific secret provisioning вне git с подтверждёнными owner/mode и kill switch; GPT
   требует нового отдельного key на Codex tier, которого в текущей задаче нет;
2. bounded authenticated live matrix для каждого transport: supported model list, minimal
   non-stream generation с terminal usage, настоящий incremental SSE, deterministic 4xx,
   insufficient-balance/429 и secret/privacy scan; GPT дополнительно проверяет tools, reasoning,
   structured output и Fast либо явно исключает недоказанные controls;
3. post-deploy smoke на exact watchdog-green SHA с проверкой единственного settlement и нулевой
   local subscription calibration attribution.

Claude mock-матрица уже фиксирует: healthy local → 0 external attempts; local retry success → 0;
empty pool → ровно 1; external 5xx → локальный terminal + refund; успешный ответ → customer
settlement без local subscription attribution; post-byte SSE failure → error tail без replay.
GPT mock-матрица фиксирует: healthy local home → 0 external attempts; terminal local pool → ровно
один `/v1/responses`; local identity не выходит; `gpt-5.5`/`gpt-5.4` allowlist; terminal usage
обязателен; local calibration не меняется; failed external attempt сохраняет локальный HTTP status,
но снимает `not_started`. Эти тесты, сборка и merge сами по себе не закрывают authenticated
live-гейты и не означают GA.
