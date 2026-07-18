# SALES_PORTAL.md — партнёрское направление (partners.apitoken.sale)

Третий bounded context репозитория (после движка и коммерции): **многоуровневая партнёрская
программа для сейлзов**. Отдельный продукт, отдельный домен, отдельная БД, отдельный визуальный
стиль (никак не связан с apitoken.sale). Бренд в UI: **APIToken Partners**.

```
engine (Rust)  ←Control API─  commerce (apps/api + worker)  ←internal sales feed─  sales (sales-api + sales-web)
```

## Что это

- Вход и онбординг — **только через Telegram** (официальный Login Widget; бот задаётся
  `TELEGRAM_BOT_TOKEN`/`TELEGRAM_BOT_USERNAME`, домен виджета биндится в BotFather `/setdomain`).
  Новый партнёр создаётся ТОЛЬКО по инвайту, выписанному на его telegram-юзернейм: админ (вкладка
  Onboarding, корневые сейлзы) или партнёр (вкладка Team, суб-сейлзы) указывает `@username` и
  отправляет человеку ссылку `partners.apitoken.sale/register?invite=CODE`; тот подтверждает вход
  через Telegram — аккаунт сразу active, пароль/почта не нужны. Email/password-поля в partners —
  legacy первой волны.
- Партнёр получает **реф-код** и ссылку `https://apitoken.sale/register?ref=CODE`.
- Пользователи, пришедшие по ссылке, атрибутируются партнёру. Партнёр зарабатывает
  `commission_bps` от **расхода** (charge-ledger) своих пользователей.
- **Многоуровневость:** партнёр может приглашать суб-партнёров (инвайт-ссылка
  `partners.apitoken.sale/register?invite=CODE`). С комиссии суб-партнёра его родитель получает
  `sub_commission_bps` — «процент с процента», цепочка вверх до 10 уровней.
- Условия (bps) индивидуальны для каждого партнёра, задаются в админке.
- Выплаты: партнёр подаёт заявку с доступного баланса, админ одобряет/отклоняет/помечает оплаченной.

## Компоненты

| Путь | Что | Порт (dev) |
|---|---|---|
| `packages/sales-db` | своя PostgreSQL БД `sales` (Drizzle, свои миграции, advisory-lock migrate) | — |
| `apps/sales-api` | NestJS/Fastify бэкенд: auth, кабинет партнёра, админка, синк-цикл, email-outbox | 3100 |
| `apps/sales-web` | Next.js фронт: лендинг, кабинет, `/admin` | 3200 |

## Граница sales ↔ commerce (единственная)

HTTP-фид в `apps/api` под ключом `SALES_CONTROL_KEY` (заголовок `x-api-key`), выключен пока env
не задан. Курсорная модель `after_id` как у ledger-фида движка; строки моложе 10 с скрыты
(лаг закрывает гонку bigserial/commit):

- `GET /v1/internal/sales/attributions?after_id&limit` — из `referral_attributions`
  (пишется при регистрации с `referralCode`).
- `GET /v1/internal/sales/usage-events?after_id&limit` — из `pricing_usage_events`
  (курсор — новая колонка `feed_seq bigserial`, миграция 0012).
- `GET /v1/internal/sales/topups?after_id&limit` — оплаченные `payments`; курсор —
  микросекунды `paid_at` (не `feed_seq`: оплата наступает позже insert).

Правила: sales не открывает commerce/engine PostgreSQL и не импортирует `@claude-api/db`;
деньги — только integer nanoUSD decimal-строками; email конечных пользователей партнёру не
отдаются (в кабинете только маска user-id).

## Атрибуция на главном сайте

`apps/web`: `?ref=CODE` на `/register` сохраняется в localStorage на 30 дней (первый код
побеждает), при регистрации уходит как `referralCode`. Коммерция пишет её best-effort в
`referral_attributions` (уникальна по user_id). TODO: пронести ref через OAuth-регистрацию
(сейчас только парольная).

## Комиссионная математика (sales-db)

Для usage-события суммой `A` у пользователя партнёра P0:
- level 0: `A * P0.commission_bps / 10000` (целочисленный floor);
- level N: `amount(level N-1) * Pn.sub_commission_bps / 10000` вверх по цепочке родителей;
- стоп: нет родителя, сумма 0, уровень > 10 или родитель suspended.
Записи идемпотентны через уникальный `commerce_event_id`; расчёт в одной транзакции со вставкой
события. Баланс к выплате = confirmed-комиссии − (выплаченные + активные заявки).

## Env (apps/sales-api)

`SALES_DATABASE_URL`, `SALES_TOKEN_ENCRYPTION_KEY`, `SALES_ADMIN_KEY`, `SALES_CONTROL_KEY`
(тот же, что у apps/api), `COMMERCE_BASE_URL` (прод: `http://127.0.0.1:3000`… через локальный
слот backend), `PUBLIC_SALES_BASE_URL`, `PUBLIC_MAIN_SITE_URL`, SMTP как у worker (Brevo),
`SALES_SESSION_TTL_SECONDS`, `SYNC_INTERVAL_MS`. Полный список — `apps/sales-api/.env.example`.

## Деплой (В ПРОДЕ с 2026-07-19)

https://partners.apitoken.sale работает. Как устроено на 84.32.48.2:

- БД `sales` (роль `sales`) в commerce-Postgres (`deploy-commerce-postgres-1`, :5433).
  Миграции: `node <realpath релиза>/packages/sales-db/dist/migrate.js` с env из
  `/etc/apitoken/sales.env`. **Готча:** запускать по разыменованному SHA-пути, не через
  симлинк `current` — гвард `isDirectExecution` сравнивает realpath и молча выходит.
- systemd: `apitoken-sales-api.service` (:3100) и `apitoken-sales-web.service` (:3200),
  оба из `/opt/apitoken/releases/current` (обновляются watchdog-релизом; после деплоя новые
  версии подтянутся при рестарте юнитов — авторестарт в blue-green НЕ включён, рестартить
  руками после значимых изменений sales-кода). **Готча:** sales-web нужен `AF_NETLINK` в
  `RestrictAddressFamilies`, иначе Next падает на `uv_interface_addresses`.
- Env: `/etc/apitoken/sales.env` (все ключи: SALES_DATABASE_URL, SALES_TOKEN_ENCRYPTION_KEY,
  `SALES_ADMIN_KEY` — ключ входа в /admin, SALES_CONTROL_KEY, SMTP Brevo). Тот же
  `SALES_CONTROL_KEY` добавлен в `/etc/apitoken/api.env` — включает фид.
- Caddy: vhost `partners.apitoken.sale` (`/v1/*`→:3100, остальное→:3200, same-origin куки) и
  loopback `http://127.0.0.1:8791` — стабильный health-gated origin commerce-backend поверх
  blue-green слотов 3000/3001 (аналог 8790 для движка); `COMMERCE_BASE_URL=http://127.0.0.1:8791`.
  **Внимание:** systemd-юниты и Caddy-блоки применены на хосте вручную и НЕ закоммичены в
  `systemd/`/`deploy/Caddyfile` — коммит этих путей ставит watchdog в pending до подтверждения
  оператором (см. CONTRIBUTING); синхронизировать отдельным осознанным шагом.
- Синк проверен на живых данных: курсоры прошли всю историю usage-событий и топапов; фид
  отвечает 401 без ключа; verify-письмо реально ушло через Brevo.

## Идеи развития (не реализовано)

- Реф через OAuth-регистрацию; серверная кука атрибуции вместо localStorage.
- Промо-материалы в кабинете (баннеры, UTM-конструктор), витрина статистики кликов
  (сейчас считаем только регистрации и расход).
- Автовыплаты USDT через провайдера; минимальный порог выплаты.
- Уведомления партнёру (email/TG) о новых рефералах и начислениях.
- Персональные лендинги/промокоды на скидку, финансируемые из комиссии партнёра.
