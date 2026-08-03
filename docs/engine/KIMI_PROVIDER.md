# KIMI (Moonshot AI) — provider capability manifest

Статус интеграции: **default-off backend preview runtime, mock-verified и ещё не live-verified**.
Дата ревью источников — **2026-08-03**.

Документ создан по `docs/engine/PROVIDER_ONBOARDING.md` §3.3 и является capability manifest
плоскости KIMI. Он фиксирует, что доказано, чем именно доказано и что остаётся `unknown`.
Каждое утверждение ниже помечено по иерархии evidence из §3.1: `official`, `live`,
`oss-hypothesis`, `decision`, `unknown`, `not-applicable`.

## 0. Область и намеренные ограничения

Плоскость KIMI строится **только как backend**: engine runtime, метеринг, калибровка, Auth Bot
и внутренний live-runner. Провайдер **не публикуется** в публичный каталог, в `/v1/models`
роутера, в commerce/OpenKeys прайсинг, на сайт и в клиентскую документацию. Это осознанное
решение владельца продукта: интеграция нужна для внутренних тестов и калибровочных прогонов,
а не для продажи.

- `decision` Провайдер остаётся за выключенным switch'ем; ни одна публичная поверхность не
  получает строку KIMI. `docs/CHANGE_CHECKLISTS.md` чеклист «новый провайдер» применяется
  частично, публикационные пункты помечены неприменимыми с этой причиной.
- `decision` Этот файл — внутренняя инженерная инструкция (обязательна по `AGENTS.md`,
  «Документация — живой контракт»), а не публичная витрина.

Пока публикации нет, GA-критерий §1 `PROVIDER_ONBOARDING.md` **не заявляется**. Терминальное
состояние этой работы — verified **preview**: runtime и калибровка доказаны на mock-гейтах,
живые гейты выполняются отдельно на принадлежащей нам подписке.

## 1. Product / plan

`official` Kimi разводит три независимые системы доступа; ключи и base URL между ними
не взаимозаменяемы (Kimi Code docs, error-reference).

| Плоскость | Назначение | Base URL | Биллинг |
|---|---|---|---|
| Kimi Open Platform | pay-per-token developer API | `https://api.moonshot.ai/v1` (int.), `https://api.moonshot.cn/v1` (CN) | по токенам |
| Kimi Code (подписка) | subscription coding plan | `https://api.kimi.com/coding/v1` (OpenAI), `https://api.kimi.com/coding/` (Anthropic) | из квоты подписки |
| Kimi web/app chat | потребительский чат | — | подписка, **API не даёт** |

`decision` Наш провайдер — **только Kimi Code**. Open Platform используется исключительно как
authority официального прайсинга (replacement cost), но не как источник ёмкости.

### 1.1 Тарифные планы

`unknown` **Точный набор и цены планов не зафиксированы.** Источники противоречат друг другу:
help-центр Kimi перечисляет CNY-лестницу (Adagio ¥0, Andante ¥49, Moderato ¥99, Allegretto ¥199,
Allegro ¥699), международные страницы — USD-лестницу (Adagio free, Moderato $19, Allegretto $39,
Allegro $99, Vivace $199), а с 2026-07-20 сообщается о разделении общего членства и отдельного
Coding Plan с другими именами тиров. Ни одно из этих чисел не подтверждено provider-owned
страницей в момент ревью.

`decision` Цена подписки **не участвует** в расчётах: калибровка отвечает на вопрос «сколько
official API replacement cost помещается в окно», а не «сколько стоила подписка»
(`PROVIDER_ONBOARDING.md` §10). Поэтому неизвестная цена **не блокирует** интеграцию.

`official` Что зафиксировано твёрдо — **имя плана является машиночитаемым**: эндпоинт `/me`
возвращает `user_level` (int) и `user_level_name` (строка, напр. `"Vivace"`). Это и есть
authoritative paid plan identity для когорт калибровки. Маркетинговая цена для этого не нужна.

`official` Гейтинг возможностей по тирам (Kimi Code docs, models):

| Возможность | Требуемый тир |
|---|---|
| `kimi-for-coding` (256K) | любой член |
| `k3`, `k3-256k` (256K) | Moderato и выше |
| `k3` полный 1M контекст | Allegretto и выше |
| `kimi-for-coding-highspeed` | Allegretto и выше |

`official` Запрос возможности выше плана возвращает `401` (models docs) либо `403`
(error-reference). Расхождение источников — `unknown`; обработчик обязан классифицировать
оба как «capability не разрешена планом», не как auth-смерть.

`official` **Community Guidelines ограничивают подписку «personal interactive use only».**

> `decision` Это ограничение — реальный compliance-риск для перепродажи ёмкости. Именно оно
> дополнительно обосновывает backend-only режим без публикации. Любое расширение до продажи
> требует отдельного юридического ревью и в этой работе не выполняется.

## 2. Credential

`official` + `oss-hypothesis` Официальный CLI `github.com/MoonshotAI/kimi-code`, MIT,
pinned SHA `75395f6abb17f83f30d16b51f4e060a639f43622` (2026-08-03), `packages/oauth/src`.

| Поле | Значение | Метка |
|---|---|---|
| Grant | OAuth 2.0 Device Authorization Grant (RFC 8628) | `oss-hypothesis` |
| OAuth host | `https://auth.kimi.com` | `oss-hypothesis` |
| Device authorization | `POST /api/oauth/device_authorization`, form, `client_id` | `oss-hypothesis` |
| Token | `POST /api/oauth/token`, form | `oss-hypothesis` |
| Device grant type | `urn:ietf:params:oauth:grant-type:device_code` | `oss-hypothesis` |
| Refresh grant type | `refresh_token` | `oss-hypothesis` |
| Official client id | `17e5f671-d194-4dfb-9706-5516cb48c098` | `oss-hypothesis` |
| PKCE | `not-applicable` (device flow) | `decision` |
| Scopes | ответ содержит `scope`, конкретные значения не задокументированы | `unknown` |
| Refresh rotation | **ротирующая семья**: `refresh_token` обязателен в ответе refresh | `oss-hypothesis` |
| Alt-credential | статический API key из Kimi Code Console | `official` |

`decision` Ротирующая семья refresh-токенов означает обязательный per-profile single-flight
re-seal по `PROVIDER_ONBOARDING.md` §6: победитель атомарно перезапечатывает конверт до снятия
lock, проигравший один раз перечитывает конверт. Бесконтрольный reuse старого refresh-токена
убивает подписку.

`decision` Device flow идеально ложится на Auth Bot: продавец получает `user_code` и
`verification_uri_complete`, подтверждает в браузере, бот поллит `/api/oauth/token`. Продавец
никогда не передаёт пароль, 2FA или сам токен.

### 2.1 Identity — `GET {base}/me`

`oss-hypothesis` Заголовок `Authorization: Bearer <access_token>`. Payload:

| Поле | Роль |
|---|---|
| `user_id` | **stable provider subject** — authority квоты и dedup |
| `user_level`, `user_level_name` | **authoritative paid plan identity** для когорт |
| `status` (`USER_STATUS_NORMAL`) | состояние аккаунта |
| `region` (`REGION_CN`) | inference geography |
| `email`, `phone`, `nickname`, `avatar` | **PII — запечатывается, наружу не публикуется** |

`decision` Наружу (admin projection, метрики, логи) отдаётся только opaque id, производный от
`user_id`, и `user_level_name`. `email`/`phone`/`nickname` не покидают конверт — §12 запрещает
full email и external account id в projection.

## 3. Model admission

`official` Подписочные id (Kimi Code docs, models) и их соответствие официальному прайс-листу
Open Platform (`platform.kimi.ai/docs/pricing/*`).

| Подписочная модель | Официальная модель (rate card) | Контекст | Тир | Non-stream | Incremental stream | Usage | Quota | Решение |
|---|---|---|---|---|---|---|---|---|
| `kimi-for-coding` | `kimi-k2.7-code` | 262 144 | все | `unknown` | `unknown` | `unknown` | `official` | preview, за switch |
| `kimi-for-coding-highspeed` | `kimi-k2.7-code-highspeed` | 262 144 | Allegretto+ | `unknown` | `unknown` | `unknown` | `official` | preview, за switch |
| `k3-256k` | `kimi-k3` | 262 144 | Moderato+ | `unknown` | `unknown` | `unknown` | `official` | preview, за switch |
| `k3` | `kimi-k3` | до 1 048 576 | Moderato+ (1M — Allegretto+) | `unknown` | `unknown` | `unknown` | `official` | preview, за switch |

`official` `k3[1m]` — это **не отдельная модель**, а Claude-Code-специфичная форма записи,
включающая 1M-окно. В обычных API-вызовах используется `k3`. Наш канонический id — `k3`
с явным context mode; скобочная форма принимается как алиас.

`official` **Reasoning-контролы различаются по семействам:**

- `k3`: всегда рассуждает, `reasoning_effort` ∈ {`low`, `high`, `max`}, default `high`.
  Нормализация алиасов: `null`/`undefined`→`high`, `ultra`/`max`/`xhigh`→`max`,
  `high`/`medium`→`high`, `low`/`minimum`/`light`→`low`, `none`→thinking off,
  прочее → HTTP 400.
- `kimi-for-coding` / `-highspeed`: `Thinking: ON`.

`official` **Критично для денег: отключение thinking маршрутизирует запрос на K2.6.**
«Disabling thinking routes both K3 and K2.7 Code to K2.6.» То есть **served model ≠ requested
model**, и у K2.6 другой rate card (cache hit $0.16 против $0.30 у K3).

> `decision` Поэтому immutable turn event обязан хранить requested и served model **раздельно**
> (`PROVIDER_ONBOARDING.md` §10.2), а тарификация идёт по **served** модели, взятой из ответа
> провайдера, а не по запрошенной. Если провайдер не вернул served model, а thinking выключен —
> модель считается неизвестной и биллинг fail closed до reserve.

`official` `kimi-for-coding-highspeed` при опечатке молча деградирует до `kimi-for-coding`
без ошибки. `decision` → served model всегда берётся из ответа, никогда из запроса.

`official` `k3` на 1M «потребляет примерно вдвое больше квоты», чем `k3-256k`;
highspeed — «6× скорость, 3× расход квоты». Это подтверждает, что нативная квота —
**взвешенный кредит**, а не счётчик запросов.

`unknown` Точные веса квоты (2×, 3×) не являются публичным нормативным контрактом с единицей
измерения. Их семантика доказывается только live-прогоном.

## 4. Wire

| Операция | URL | Заголовки | Тело | Framing | Usage | Ошибки |
|---|---|---|---|---|---|---|
| Generation (Anthropic) | `POST https://api.kimi.com/coding/v1/messages` | `Authorization: Bearer` (CLI) либо `x-api-key` (Claude Code) | Anthropic Messages | SSE | `unknown` | см. §4.2 |
| Generation (OpenAI) | `POST https://api.kimi.com/coding/v1/chat/completions` | `Authorization: Bearer` | OpenAI Chat | SSE | `unknown` | см. §4.2 |
| Catalogue | `GET /coding/v1/models` | `Authorization: Bearer` | — | JSON | — | **негейтед** |
| Identity | `GET /coding/v1/me` | `Authorization: Bearer` | — | JSON | — | 401 |
| Quota | `GET /coding/v1/usages` | `Authorization: Bearer` | — | JSON | — | 401/404 |

`decision` **Anthropic-совместимый транспорт — решающее архитектурное преимущество.** Нативный
протокол движка уже Anthropic (`CLAUDE.md`, инвариант «Прозрачность»). Плоскость KIMI поэтому не
нуждается в трансляции протокола масштаба `crates/forward/src/gemini/` (schema/stream/skin —
около 10 000 строк); она переиспользует существующий Anthropic-путь и добавляет только
credential, транспорт, пул, метеринг и калибровку.

`official` `GET /v1/models` **не проверяет авторизацию** — он отвечает 200 на невалидный ключ,
а последующий generation даёт 403. `decision` → readiness-проба плоскости обязана бить в
`/messages` минимальным запросом либо в `/me`, но **никогда** в `/models`. Ложноположительный
health по `/models` прямо запрещён.

`unknown` Точный auth-заголовок Anthropic-маршрута (`Authorization: Bearer` против `x-api-key`)
не подтверждён нормативной страницей. Официальный CLI использует Bearer для `/me`, `/usages` и
чата; документация Claude Code задаёт `ANTHROPIC_API_KEY`, что даёт `x-api-key`.
`decision` → реализуем Bearer как проверенный источником вариант, `x-api-key` — как
конфигурируемую альтернативу; выбор фиксируется первым же live-прогоном.

`unknown` Форма terminal usage на Anthropic-маршруте (поля `usage`, наличие
`cache_read_input_tokens` / `cache_creation_input_tokens`) не подтверждена. Без authoritative
usage биллинг fail closed — settlement по консервативному hold.

### 4.1 Реализованный backend gateway

`decision` Точные reviewed Kimi Code aliases диспетчеризуются внутри Anthropic
`POST /v1/messages` после общей авторизации и bounded-чтения тела, но до Claude-specific identity,
pricing и pool mutation. Alias никогда не уходит в Claude upstream: выключенная плоскость,
повреждённый initial roster и cold roster дают fail-closed ответ KIMI-пути, не fallback.

`decision` Реализация в `crates/forward/src/kimi/gateway.rs` mock-доказывает следующие локальные
инварианты, но не снимает provider-owned `unknown` из §6:

- non-stream response и SSE-байты проходят без протокольной трансляции; usage extractor умеет
  собирать split SSE frames, но settlement признаёт authoritative только terminal event;
- retry/rotation разрешены только до первого публичного байта; после него upstream дренируется
  даже при downstream disconnect, а shutdown ждёт stream finalizer;
- metered turn проходит customer reserve → durable delivering marker → terminal settlement;
  actual charge берётся по **served model**, а отсутствие terminal usage сохраняет полный hold;
- immutable turn evidence доставляется через bounded FIFO; `/usages` не выполняется при pending
  head, а после HTTP-снимка writer повторно дренирует FIFO, читает durable cumulative spend и
  завершает immutable observation/CAS до публикации quota steering;
- poll берёт только idle profile snapshot и не вводит customer semaphore: generation, начавшаяся
  во время HTTP, меняет monotonic epoch и целиком инвалидирует снимок. После финальной epoch-check
  новый turn может идти параллельно, но его enqueue удерживается FIFO-барьером до записи более
  раннего quota snapshot;
- OAuth refresh держит per-profile single-flight, требует новую rotating refresh family и атомарно
  re-seal'ит envelope до снятия lock; blue-green loser перечитывает shared disk authority;
- readiness проверяет только authenticated `/me`; первый 401 заставляет один forced refresh/retry;
- server каждые 15 секунд ищет новую атомарную публикацию roster; неизменённый profile сохраняет
  тот же runtime `Arc` с health/client/in-flight state, а новый или изменённый credential проходит
  `/me` до публикации всей generation;
- malformed/decrypt/client/probe failure и исчезнувший `profiles.json` сохраняют last-good
  capacity. Намеренное удаление флота выражается только валидным пустым roster; удалённый profile
  сразу закрыт для новых запросов, но уже выданный in-flight lease живёт до своего natural drop;
- перед atomic swap gateway берёт affected refresh locks и повторно читает roster: snapshot,
  устаревший из-за параллельной rotating refresh/re-seal, не может стать новой in-memory authority;
- bearer redirect запрещён, неизвестные tool/media surfaces и неподдержанный reasoning fail closed;
- синтетические ошибки проходят общий Anthropic-compatible sanitizer и не раскрывают клиенту имя
  внутреннего backend, roster, subscription или provider body;
- непроверенный plan получает только базовый `kimi-for-coding`; reviewed tier allowlist остаётся
  authority расширенных aliases.

### 4.2 Классы ошибок

`official` (Kimi Code error-reference):

| Статус | Смысл | Наша реакция |
|---|---|---|
| 401 | auth не прошла, либо возможность выше плана | refresh + retry на том же профиле; повтор → auth quarantine |
| 402 | «unable to verify your membership benefits», обычно временная | transport-класс, bounded rotation |
| 403 | identity верна, но: тир не даёт возможность / аккаунт закрыт / **квота исчерпана** | quota wall → cooling до reset, без transport-бюджета |
| 429/5xx | перегрузка инференса — «retry directly» | bounded transport rotation |

`official` Провайдер сам разделяет «перегрузка движка (повтор осмысленен)» и «квота аккаунта
(повтор бесполезен)». `decision` → это ровно разделение осей §8.4: quota bucket отдельно от
transport health.

`unknown` 403 совмещает исчерпание квоты и отсутствие возможности у плана. Различение возможно
только по телу ошибки; до live-подтверждения обработчик fail closed классифицирует
неразличимый 403 как quota wall (консервативно — профиль выводится из ротации до reset,
а не помечается мёртвым).

## 5. Money / quota

### 5.1 Официальный rate card (replacement cost)

`official` `platform.kimi.ai/docs/pricing/*`, ревью 2026-08-03, USD, до налогов,
«1M = 1 000 000». Тарифных ступеней по длине контекста нет — ставка плоская на всём окне.

| Официальная модель | Cache hit / 1M | Cache miss / 1M | Output / 1M | Контекст |
|---|---|---|---|---|
| `kimi-k3` | $0.30 | $3.00 | $15.00 | 1 048 576 |
| `kimi-k2.7-code` | $0.19 | $0.95 | $4.00 | 262 144 |
| `kimi-k2.7-code-highspeed` | $0.38 | $1.90 | $8.00 | 262 144 |
| `kimi-k2.6` | $0.16 | $0.95 | $4.00 | 262 144 |

`official` Отдельной цены cache **write**/storage не публикуется — только hit и miss. Кеширование
описано как автоматическое. `decision` → leg cache-write отсутствует, а не считается нулём
молча; ноль здесь — задокументированный факт отсутствия платного leg'а.

`official` Reasoning-токены тарифицируются по output-ставке (K3: «reasoning output consumes
tokens billed at the output rate»). `decision` → reasoning — **subset output**, отдельным leg'ом
не тарифицируется; двойного счёта нет.

`official` Web search на платформе «currently being updated», использовать не рекомендуется,
документация устарела. `decision` → **capability записывается как unavailable, бюджет не
тратится** (`SKILL.md`: платный tool/search диспатчится только когда доказан конечный per-request
ceiling). Отдельной per-call цены в актуальных прайс-страницах нет.

`official` Устаревшие/снятые: серия `kimi-k2-*` снята 2026-05-25, `kimi-latest` — 2026-01-28,
`kimi-thinking-preview` — 2025-11-11, `moonshot-v1-*` и `kimi-k2.5` — sunset 31 августа.
`decision` → в rate card не вносятся: подписка их не отдаёт.

### 5.2 Нативная квота — `GET /coding/v1/usages`

`oss-hypothesis` Схема (официальный CLI, `packages/oauth/src/managed-usage.ts`):

```json
{
  "usage":  { "used": "40", "limit": "1000", "resetTime": "2026-08-03T05:20:51Z" },
  "limits": [
    { "name": "...",
      "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
      "detail": { "used": "1", "limit": "100", "resetTime": "..." } }
  ],
  "boosterWallet": {
    "balance": { "type": "BOOSTER", "amount": <fp>, "amountLeft": <fp> },
    "monthlyChargeLimitEnabled": false,
    "monthlyChargeLimit": { "priceInCents": <int>, "currency": "USD" },
    "monthlyUsed": { "priceInCents": <int>, "currency": "USD" }
  }
}
```

Существенные свойства:

- `used`/`limit` — **целые десятичные строки**, не проценты. `decision` → парсим в integer,
  не через float; доля считается как `used * FRACTION_SCALE / limit`.
- **Resolution окна = `FRACTION_SCALE / limit`.** Это принципиально лучше, чем whole-percent у
  Claude: при `limit=1000` разрешение равно 0.1 %. `decision` → resolution вычисляется из
  фактического `limit` каждого окна и хранится вместе с наблюдением (§10.3), а не задаётся
  константой.
- `usage` — **недельное** окно; backend не присылает `window`, CLI синтезирует `1 week`.
  `decision` → мы **не синтезируем** окно молча: недельная семантика подтверждается
  `official` («refreshes automatically every 7 days»), и это фиксируется как явный маппинг
  с указанием источника.
- `limits[]` — окна с явной длительностью; 5-часовое приходит как `duration: 300`,
  `TIME_UNIT_MINUTE`. `decision` → длительность нормализуется в секунды и хранится точно;
  окна 5ч и 7д живут **независимо** (§10.3).
- `resetTime` — RFC3339, прямое reset evidence для машины состояний интервала.
  `decision` → отсутствующий, невозможный или нестрогий timestamp отклоняет весь snapshot;
  нормализация в Unix seconds выполняется до durable write, без локального `now + duration`.
- `boosterWallet` — Extra Usage, реальные деньги в fixed-point (делитель 1 000 000 → центы),
  с валютой USD/CNY. `decision` → это **третий, отдельный** ledger: он не смешивается ни с
  нативной квотой, ни с API-долларами.

`unknown` **Единица `used` не доказана.** Косвенно она взвешенная (K3@1M ≈ 2× к `k3-256k`,
highspeed = 3×), но нормативного определения нет: это может быть кредит, токеновый эквивалент
или взвешенный запрос. По `SKILL.md` («provider buckets publish amounts with unclear semantics»)
величина сохраняется как **raw quota evidence** до live-доказательства и **не делится** на
токеновую цену ради выдумывания ёмкости.

`official` Монтхли-потолок членства общий: «PPT, Agent Cluster, Kimi Code и т.д. делят общий
месячный лимит», при его исчерпании Kimi Code заморожен даже при остатке недельной квоты.
`decision` → это третье, **внешнее** окно; исчерпание видно как 403 при непустой недельной
квоте. Обработчик обязан не считать такой профиль здоровым только потому, что недельная доля < 1.

`official` Квота общая для всех устройств и API-ключей аккаунта. `decision` → authority квоты —
`user_id`, а не ключ; несколько ключей одной подписки — один subject.

### 5.3 Выбор ledger-модели

`decision` По §10.1 KIMI **не является** GPT-подобным dual-ledger провайдером, несмотря на
наличие нативных единиц. Решающее различие: GPT публикует нативное списание **на каждый turn**,
а KIMI отдаёт нативный расход **только агрегатом окна** в `/usages`. Построить независимый
per-turn нативный ledger не из чего, и выводить его делением API-долларов на цену токена прямо
запрещено.

Отсюда фактическая модель — **Claude-подобная по форме, но с гораздо лучшим разрешением**:

1. **API-nanoUSD ledger** — точный, per-turn, из официального rate card §5.1 по **served**
   модели. Кумулятивная сумма по subject.
2. **Нативная доля окна** — `used/limit` из `/usages`, играет роль claude-подобной quota
   fraction, но приходит целыми числами с разрешением `FRACTION_SCALE / limit` вместо
   целых процентов.
3. **Booster wallet** — реальные деньги (центы), отдельный третий ledger, ни с чем не смешивается.

`decision` **Нативную ёмкость окна оценивать не нужно — она опубликована.** `limit` и есть полный
размер окна в нативных единицах, а `limit - used` — точный нативный остаток. Оценке подлежит
только одно: сколько official API replacement cost помещается в окно при наблюдённой нагрузке.
Поэтому в схеме нет ни оценки нативной ёмкости, ни per-turn нативного leg'а — есть точный
`native_limit_units` и обычная формула §10.5 для `capacity_nanoUSD`.

`unknown` Единица `used` по-прежнему не доказана. Это не мешает считать долю (доля безразмерна),
но означает, что нативный остаток нельзя переводить в токены или деньги до live-доказательства.

`decision` Когорты (§10.6) объединяются только по точному `user_level_name` + точной
длительности окна. `unknown` план блокирует агрегацию когорты — в отличие от Claude, здесь план
машиночитаем, поэтому блокировка ожидается редкой.

### 5.4 Runtime ordering

`decision` Server запускает первый бесплатный `/usages` anchor сразу после `/me` preflight, затем
повторяет его с `CLAUDE_API_KIMI_QUOTA_POLL_SECS`; roster discovery остаётся независимым
15-секундным tick. Poll идёт последовательно по snapshot текущей whole-generation roster и не
возвращает удалённый profile обратно в generation.

Для каждого subject порядок load-bearing:

1. профиль должен быть idle; poll запоминает monotonic generation-start epoch;
2. известный bounded turn FIFO полностью дренируется, иначе HTTP вообще не выполняется;
3. после `/usages` epoch и in-flight проверяются снова; любой turn, стартовавший во время GET,
   инвалидирует весь snapshot без локальной очереди или ограничения customer concurrency;
4. под FIFO-барьером выполняется ещё один drain, serial PostgreSQL writer читает cumulative
   official API spend и для каждого независимого окна делает immutable observation + estimator
   CAS; conflict одного turn quarantines только этот event, transient head удерживается;
5. runtime публикует tightest used fraction и full-window cooling до exact reset только после
   durable успеха **всех** окон. DB/CAS/parser/upstream failure сохраняет last-good quota.

Shutdown закрывает admission и steady maintenance, ждёт stream finalizers, затем повторяет тот же
turn-before-quota порядок. Финальный provider read ограничен уже существующим process deadline;
его отмена не позволяет старому maintenance task записывать данные после общего billing flush.
Под deadline не начинается rotating OAuth refresh: финальный poll использует только ещё валидный
access token, а refresh/reseal остаётся неделимой steady-state операцией.

## 6. Что остаётся недоказанным

Ниже — полный список `unknown`, каждый из которых fail closed и снимается только контролируемым
live-прогоном на принадлежащей нам подписке:

1. Точный auth-заголовок Anthropic-маршрута.
2. Форма terminal usage и наличие cache-legs в ответе.
3. Реальная инкрементальность SSE (буферизованный единственный кадр ≠ stream).
4. Единица измерения `used` в `/usages`.
5. Различение 401/403 для «возможность выше плана» и «квота исчерпана».
6. Набор и цены тарифных планов.
7. Поведение при исчерпании общего месячного потолка членства.
8. Существование и стоимость платных tool/search-единиц на подписочном маршруте.

Ни один из них не блокирует сборку runtime, метеринга, credential и калибровочной схемы —
блокируются только соответствующие live-гейты (`PROVIDER_ONBOARDING.md` §2).

## 7. Состояние доставки

Текущая цепочка продолжается producer-first checkpoint'ами от `master`. Плоскость остаётся
default-off и backend-only: ни одна публичная поверхность не содержит строку KIMI. Server уже
композирует gateway, но production activation и live-доказательства ещё не заявляются.

| Этап | Артефакт | Состояние |
|---|---|---|
| research / capability manifest | этот файл | готово |
| официальный rate card | `crates/metering/src/kimi.rs` | готово, 18 тестов |
| calibration authority (schema) | `crates/registry/migrations_pg/0027_kimi_window_calibration.sql` | готово, expand-only, 2 теста |
| типы наблюдений | `crates/registry/src/kimi_calibration.rs` | готово, 10 тестов |
| credential | `crates/kimi-credential` | готово, 18 тестов |
| calibration estimator | `crates/forward/src/kimi_calibration.rs` | готово, 19 тестов |
| Auth Bot: device-code протокол | `crates/authbot/src/kimi_oauth.rs` | готово, 14 тестов |
| Auth Bot: мастер продавца | `crates/authbot/src/{bot,kimi_roster}.rs` | готово, device flow → atomic roster до выплаты |
| transport / pool primitives | `crates/forward/src/kimi/**` | готовы roster/client/selection/refresh/error/attempt/FIFO/config |
| durable read/write калибровки в PostgreSQL | `crates/registry` | готово; real-PG replay/conflict/CAS/history matrix зелёная |
| server: env/config | `crates/server/src/config.rs` | готово: strict default-off input → typed config |
| server/forward: gateway + readiness | `crates/{server,forward}` | готово на mock-гейтах: exact internal dispatch, `/me`, refresh, rotation, stream lifecycle, reserve/delivering/settlement/FIFO |
| last-good roster reload | `crates/{server,forward}` | готово на mock-гейтах: 15-секундное discovery, whole-generation validation, `/me` admission, exact-Arc reuse, refresh-race verification, safe removal |
| quota observations | `crates/{server,forward}` | готово на mock/real-PG гейтах: idle `/usages`, generation-epoch rejection, turn-before-quota drain, exact spend read, independent-window immutable write/CAS, publish-after-durable и bounded shutdown |
| observability, alerts, blue-green | `observability/**`, `systemd/**` | **не сделано** |
| безопасный live-runner | `tools/kimi_calibration/` | **не сделано** |
| live-матрица на нашей подписке | — | **не сделано, нужна подписка** |

Следующий producer-first шаг — bounded-cardinality observability и admin-only operational
projection для runtime/delivery/calibration evidence. Затем следуют blue-green wiring,
live-runner и контролируемый живой прогон. Публикация не планируется вовсе (см. §0).

## 8. Источники

Все ссылки просмотрены 2026-08-03.

- `https://platform.kimi.ai/docs/pricing/chat`, `.../chat-k3`, `.../chat-k27-code`, `.../chat-k26`
- `https://platform.kimi.ai/docs/models`
- `https://www.kimi.com/code/docs/en/kimi-code/models.html`
- `https://www.kimi.com/code/docs/en/kimi-code/membership.html`
- `https://www.kimi.com/code/docs/en/kimi-code/error-reference.html`
- `https://www.kimi.com/code/docs/en/third-party-tools/claude-code.html`
- `https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/providers.html`
- `github.com/MoonshotAI/kimi-code` @ `75395f6abb17f83f30d16b51f4e060a639f43622`, MIT
  (только чтение; временный клон удалён после исследования)
