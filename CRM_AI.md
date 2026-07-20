# CRM_AI.md — устройство AI-CRM (crm.panel.apitoken.sale)

Внутренняя CRM «CRM & Parsing». Главный принцип: **на каждом этапе решения принимает нейронка,
а не хардкод**. Код отвечает только за хранение, транспорт и исполнение решений AI. Модель
вызывается через НАШ движок (`https://api.apitoken.sale/v1/messages`) по ключу «CRM & Parsing»
(env `CRM_ENGINE_KEY`; сам ключ — в `secrets/CRM.md`, в репозиторий и браузер не попадает).

Формат входа и инструкция для парсеров — **`CRM_PARSING_SPEC.md`** (единственный контракт
между парсерами и CRM). Каркас/деплой/учётки — `CRM_PORTAL.md`.

## Конвейер (что происходит с контактом)

```
парсер (+свой AI-классификатор по CRM_PARSING_SPEC.md)
   │  конверт apitoken.crm/contact@v1, максимум признаков + confidence + evidence
   ▼
POST /v1/ingest/contacts  (crm-api :3400, ключ x-crm-ingest-key)
   │ 1. строгая проверка МИНИМУМА (конверт + ≥1 канал связи); остальное — открытое
   │ 2. не влезло в конверт → AI-адаптер: нейронка сама мапит чужой формат в конверт,
   │    отклонение пишется в drift-лог ингеста (envelope drift) — спека растёт от жизни
   │ 3. upsert по каналу (telegram/gmail/…): контакт един, признаки сливаются
   │ 4. незнакомые ключи признаков → attribute registry со статусом proposed
   ▼
AI-куратор признаков (POST /v1/registry/curate)
   │ смотрит registry + примеры значений: описывает новые ключи, мержит синонимы
   │ (tg_handle ≈ telegram_username), нормализует значения; решения — в ai_audit
   ▼
AI-сегментатор (POST /v1/views/refresh)
   │ смотрит распределения признаков по корпусу и САМ придумывает smart views:
   │ название, зачем сегмент, фильтр-DSL; пересчитывает размеры; создаёт новые
   │ view при появлении новых признаков/кластеров
   ▼
Поиск людей по описанию (POST /v1/ask)
     «нужны CTO небольших SaaS в Европе, кто жаловался на цены Anthropic»
     → AI переводит в фильтр-DSL + скорит кандидатов → выборка с обоснованием
```

## Слои и границы

| Компонент | Порт | Отвечает за | НЕ делает |
|---|---|---|---|
| `packages/crm-db` | — | Postgres-схема (drizzle) + миграции + клиент | HTTP, AI, env |
| `apps/crm-api` | :3400 | ingest, фильтры, все AI-вызовы (server-side) | доступ к engine/commerce БД |
| `apps/crm-web` | :3300 | UI поверх /v1 crm-api (same-origin через Caddy) | секреты, прямые AI-вызовы |

Связь с движком — ТОЛЬКО клиентский `/v1/messages` по `CRM_ENGINE_KEY` (обычный
Anthropic-совместимый запрос). Control API движка CRM не трогает.

## Данные (packages/crm-db, Postgres `CRM_DATABASE_URL`)

- `contacts` — ядро: имя, AI-summary («кто это человек»), статус (new/enriched/qualified/archived).
- `contact_channels` — каналы связи (type+value, уникальны глобально) → дедупликация при ингесте.
- `contact_attributes` — ОТКРЫТОЕ пространство признаков: `key` (snake_case), `value` (jsonb,
  скаляр или массив), `confidence` 0..1, `evidence` (цитата/факт-основание), `source` (какой
  парсер/ран). Upsert по (contact_id, key) — свежая классификация побеждает.
- `attribute_registry` — живой реестр признаков: описание, тип значения, примеры, статус
  `proposed→active|merged`, `merged_into`. Пополняется автоматически, курируется AI.
- `smart_views` — AI-сегменты: фильтр-DSL, rationale (почему сегмент полезен), created_by
  ai|human, счётчик контактов, refreshed_at.
- `ingest_runs` — журнал ранов парсеров: принято/починено AI/отклонено + drift jsonb.
- `ai_audit` — каждое решение нейронки (kind, model, вход-сводка, выход) — прозрачность и разбор.

## Фильтр-DSL (исполняемый слой для AI-решений)

AI не выполняет SQL — она выдаёт декларативный фильтр, который исполняет код:

```json
{ "all": [ {"key":"role","op":"in","value":["cto","founder"]},
           {"key":"geo_country","op":"eq","value":"UK"} ],
  "any": [ {"key":"buying_intent","op":"gte","value":0.6},
           {"key":"pain_points","op":"contains","value":"api cost"} ],
  "none": [ {"key":"risk_flags","op":"exists"} ] }
```

Опы: `eq, neq, in, contains, exists, gte, lte, regex`. `all` — И, `any` — ИЛИ (достаточно
одного), `none` — НИ ОДНОГО. Значение признака-массива матчится поэлементно. Это единственное
«жёсткое» место системы — намеренно: DSL исполняется детерминированно, а ПРИДУМЫВАЕТ фильтры AI.

## Где именно зашиты AI-решения (и где их нет)

1. **Парсер**: классификация контакта (промпт из CRM_PARSING_SPEC.md) — признаки придумывает AI.
2. **Ингест**: починка несоответствующего формата — AI-адаптер (`kind=ingest_repair`).
3. **Registry**: описания/мерж/нормализация ключей — AI-куратор (`kind=registry_curate`).
4. **Сегментация**: изобретение smart views — AI-сегментатор (`kind=views_refresh`).
5. **Поиск**: описание ЦА → фильтр + скоринг — AI (`kind=ask`).

Хардкода по смыслу нет: код не знает ни одного названия признака и ни одного сегмента.

## Env (crm-api, сервер: `/etc/apitoken/crm.env`)

```
CRM_DATABASE_URL=postgres://…/apitoken_crm
CRM_INGEST_KEY=…            # ключ для парсеров (x-crm-ingest-key), см. secrets/CRM.md
CRM_ENGINE_KEY=sk-pool-…    # ключ «CRM & Parsing» из secrets/CRM.md
CRM_ENGINE_URL=https://api.apitoken.sale   # можно локальный движок
CRM_AI_MODEL=claude-sonnet-5
```

## Проверка

```bash
pnpm --filter @claude-api/crm-db build && pnpm --filter @claude-api/crm-api build \
  && pnpm --filter @claude-api/crm-api typecheck && pnpm --filter @claude-api/crm-web build
```
