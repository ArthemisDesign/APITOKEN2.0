# SALES_PORTAL.md — партнёрское направление (partners.apitoken.sale)

Третий bounded context репозитория (после движка и коммерции): **многоуровневая партнёрская
программа для сейлзов**. Отдельный продукт, отдельный домен, отдельная БД, отдельный визуальный
стиль (никак не связан с apitoken.sale). Бренд в UI: **APIToken Partners**.

```
engine (Rust)  ←Control API─  commerce (apps/api + worker)  ←internal sales API (SALES_CONTROL_KEY)→  sales (sales-api + sales-web)
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

Контур **двусторонний**, оба направления под одним серверным ключом `SALES_CONTROL_KEY`
(заголовок `x-api-key`, сравнение timing-safe). Ключ один и тот же в env обоих сервисов
(`/etc/apitoken/api.env` и `/etc/apitoken/sales.env`); без него контур выключен, причём
стороны ведут себя по-разному: commerce-фид без env отвечает **404** (гард прячет эндпоинты),
sales-internal без env — **401**.

### Commerce → Sales: фиды и профили (`apps/api/src/sales-feed.controller.ts`)

`@Controller("internal/sales")` под гардом `SalesFeedGuard`. Курсорная модель `after_id` как у
ledger-фида движка; строки моложе 10 с скрыты (лаг закрывает гонку bigserial/commit). Мусор в
`after_id`/`limit` — не ошибка, а дефолт (курсор 0, дефолтный лимит).

- `GET /v1/internal/sales/attributions?after_id&limit` (лимит дефолт 500, макс 1000) — из
  `referral_attributions` (пишется при регистрации с `referralCode`, уникальна по user_id).
  Ответ `{items:[{id,userId,code,createdAt}]}`; `nextCursor` нет — курсор читателя = max `id`
  страницы (строки идут по возрастанию `id`).
- `GET /v1/internal/sales/usage-events?after_id&limit` (дефолт 1000, макс 2000) — из
  `pricing_usage_events`; курсор — колонка `feed_seq bigserial` (миграция 0012). Ответ
  `{items:[{id,userId,amountNano,occurredAt}], nextCursor}`. `amountNano` = `real_funded`:
  реально-оплаченная часть списания (бесплатная вычтена по принципу «бесплатное тратится
  первым»), комиссия считается только с неё. Фид отдаёт **только атрибутированных** юзеров
  (join с `referral_attributions` на стороне commerce): расход обычных клиентов не копится в
  буфере sales, а `nextCursor` движется по watermark страницы, чтобы отфильтрованный хвост не
  пересканировался вечно.
- `GET /v1/internal/sales/topups?after_id&limit` (дефолт 500, макс 1000) — оплаченные
  `payments`; курсор — микросекунды epoch от `paid_at` (не `feed_seq`: оплата наступает позже
  insert, и просроченный `feed_seq` выпал бы из курсора навсегда). Тоже фильтруется по
  атрибуции. Ответ `{items:[{id,paymentId,userId,amountNano,paidAt}], nextCursor}`.
- `POST /v1/internal/sales/referral-discount` — «пол» скидки сейлза для реферала партнёра.
  Тело `{userId, floorBps (0..9500), override?, actorId?}` → `{applied, multiplierBp}`.
  Клиент остаётся b2c и идёт по обычным тир-правилам; floor лишь гарантирует цену не хуже
  скидки сейлза: эффективный mult = `min(тир-mult, 10000 − floorBps)`. По умолчанию пол
  **монотонен** (`GREATEST`, лучшее клиенту) — его пишут три независимых источника (промо,
  партнёрская ссылка, sales-фид); `override=true` — абсолютная запись с понижением (партнёр или
  админ из sales-кабинета), `floorBps=0` — явный сброс. Только b2c-профили (business-b2b или
  нет тира → `applied:false`). Идемпотентно; мультипликатор доставляется в движок через
  durable `engine_pricing_jobs`.
- `POST /v1/internal/sales/referral-profiles` — профили рефералов для витрины партнёра. Тело
  `{userIds: uuid[] (макс 500)}` → `{items:[{userId, customerType (b2c/b2b), multiplierBp,
  discountPercent, referralFloorBps, cumulativeTopupNano, balanceNano, status}]}`. Только по
  явному списку user_id, который sales-api формирует из закреплённых за партнёром рефералов —
  партнёр видит лишь своих. `balanceNano` и живой `multiplierBp` читаются из движка
  (авторитет денег) с параллелизмом 8; недоступный движок-аккаунт не роняет страницу — поля
  деградируют до `null`/значений из `customer_profiles`.

Потребители на стороне sales (`apps/sales-api`, ходят по `COMMERCE_BASE_URL`):

- `sync.service.ts` — синк-цикл по курсорам (хранятся в sales-БД, интервал `SYNC_INTERVAL_MS`,
  дефолт 60 с): атрибуции → закрепление юзера за партнёром + атомарный claim одноразовой
  скидочной ссылки (победитель получает floor через `POST referral-discount` — либо впервые,
  либо как идемпотентный backfill, если синхронное применение при регистрации упало);
  топапы → `referred_topups` (только история/аналитика, комиссий не создают); usage-события →
  комиссии (идемпотентны по `commerce_event_id`); события, пришедшие раньше атрибуции их
  юзера, буферизуются в `pending_referral_events` и догоняются replay. 404 фида (сторона
  commerce ещё не задеплоена) — не ошибка, повтор на следующем тике; курсор продвигается
  только по успешно обработанным строкам (at-least-once).
- `commerce.service.ts` — `referralProfiles` для витрины партнёра (**best-effort**: при
  недоступности commerce возвращается пустая карта, витрина деградирует до локальных полей —
  траты/комиссия) и `setReferralDiscount` (шлёт `override=true`; **не** best-effort — ошибки
  транспорта пробрасываются вызывающему, партнёр должен знать результат).

Schema-readiness checkpoint для multi-discount: sales-миграция
`0014_usage_attribution_buffer.sql` заранее расширяет `pending_referral_events`, чтобы replay не
терял immutable commission authority, пока атрибуция пользователя ещё не пришла. Legacy spend и
deposit сохраняют полностью `NULL`-форму новых полей. Атрибутированный buffered spend обязан
нести непустые `provider_id`/`snapshot_digest`, `account_class=b2c`, `pricing_mode=track`,
`commission_eligible=true` и положительный `paid_funded_nano`, точно равный `amount_nano`.
Сопутствующий constraint на `partner_usage_events` запрещает атрибутированную комиссию вне той же
B2C track authority. На этом expand-only SHA producer/consumer фида ещё используют прежний payload;
запись новых полей включается отдельным application-коммитом только после зелёных
`deploy/migration` и `deploy/watchdog` миграционного SHA.

### Sales → Commerce: промо и регистрация (`apps/sales-api/src/internal.controller.ts`)

Commerce ходит в sales-api по `SALES_API_URL` тем же `SALES_CONTROL_KEY`.

- `POST /v1/internal/promo/redeem` — погашение партнёрского промокода (вызывается из
  `apps/api/src/promo.service.ts`, публичный `POST /v1/promo/redeem`). Тело
  `{code, commerceUserId}` → `{valueNano, partnerId, referralCode, redemptionRef, discountBps,
  alreadyRedeemed}`. Атомарно и идемпотентно по (code, user): повторное погашение тем же
  юзером возвращает тот же `redemptionRef`, поэтому кредит движка на стороне commerce
  идемпотентен по ref (ретраи безопасны). Одноразовый код; один промо на юзера (409); код
  недоступен, если партнёр не active или промо выключено. Commerce дальше сам: кредитует
  движок (до 3 попыток), best-effort атрибутирует незакреплённого юзера к владельцу кода, при
  `discountBps>0` применяет скидку-«пол» с локальными ретраями — async-фид промо-скидку **не**
  переприменяет (он деривит floor только из `partner_discount_links`).
- `POST /v1/internal/partners/referral-discount` — атомарный claim персональной скидочной
  ссылки. Тело `{code, commerceUserId}` → `{discountBps}`. First-wins, идемпотентно по
  (code, user): ссылка закрепляется за первым владельцем одним UPDATE и НИКОГДА не даёт скидку
  второму; обычный реф-код или ссылка, погашенная другим, → 0. Вызывается из
  `apps/api/src/auth.service.ts` при первой активации движок-аккаунта (парольная регистрация,
  подтверждение email, OAuth) — синхронно, чтобы реферал видел свою ставку с первого захода; полностью best-effort
  (таймаут 4 с, сбой → async-фид применит floor владельцу на следующем тике).
- `GET /v1/internal/partners/resolve?code` → `{found:false}` либо `{found:true, partnerId,
  referralDiscountBps}` — резолв реф-кода активного партнёра (`Cache-Control: no-store`).
  Эндпоинт жив, но текущий код commerce его **не вызывает**: claim-эндпоинт выше заменил пару
  resolve+consume, закрыв окно, где read-only резолв выдавал floor нескольким регистрациям
  одной ссылки.

Правила: sales не открывает commerce/engine PostgreSQL и не импортирует `@claude-api/db`;
commerce симметрично не открывает sales-БД — всё через HTTP под ключом. Деньги — только
integer nanoUSD decimal-строками; email конечных пользователей партнёру не отдаются (в
кабинете только маска user-id).

## Атрибуция на главном сайте

`apps/web`: `?ref=CODE` на `/register` сохраняется в localStorage на 30 дней (первый код
побеждает), при регистрации уходит как `referralCode`. Коммерция пишет её best-effort в
`referral_attributions` (уникальна по user_id). Ref пробрасывается и через OAuth-регистрацию:
соцкнопки передают его в `oauthUrl` (`apps/web/src/lib/api.ts`), `beginOAuth` сохраняет код в
OAuth-транзакции (переживает редирект к провайдеру), а `completeOAuth` для **нового** аккаунта
пишет атрибуцию и синхронно применяет скидку-«пол» через `POST
/v1/internal/partners/referral-discount` — как и парольный путь, реферал видит ставку с первого
захода. Ограничение «только новый аккаунт» принципиально: иначе существующий клиент мог бы
залогиниться с чужим `?ref=` и само-выдать себе скидку + сжечь чужую одноразовую ссылку.

Полное продуктовое руководство по всей программе (вход, атрибуция, комиссия, уровни, кошелёк,
периоды, кабинет, админка, языки) — `docs/sales/PARTNER_PROGRAM.md`.

## Выплаты по периодам

Полумесячные периоды (1–15, 16–конец, UTC), лок 7 дней, окно выплат 3 дня, авто-ролловер
непокрытого, минимум `SALES_MIN_PAYOUT_USD` ($10), выплаты на привязанный BSC-кошелёк.
Считается из `commission_entries` + `payouts` без отдельной таблицы. Полное описание —
`docs/sales/SALES_PAYOUT_PERIODS.md`. Код: `periods.ts` (+тесты) и `payout-periods.ts`. Отправка
выплат (on-chain) — отдельная предстоящая система.

## Комиссионная математика (sales-db)

Для usage-события суммой `A` у пользователя партнёра P0:
- level 0: `A * P0.commission_bps / 10000` (целочисленный floor);
- level N: `amount(level N-1) * Pn.sub_commission_bps / 10000` вверх по цепочке родителей;
- стоп: нет родителя, сумма 0, уровень > 10 или родитель suspended.
Записи идемпотентны через уникальный `commerce_event_id`; расчёт в одной транзакции со вставкой
события. Баланс к выплате = confirmed-комиссии − (выплаченные + активные заявки).

## Env (apps/sales-api)

`SALES_DATABASE_URL`, `SALES_TOKEN_ENCRYPTION_KEY`, `SALES_ADMIN_KEY`, `SALES_CONTROL_KEY`
(тот же, что у apps/api), `COMMERCE_BASE_URL` (прод: стабильный Caddy-balancer
`http://127.0.0.1:8791`), `PUBLIC_SALES_BASE_URL`, `PUBLIC_MAIN_SITE_URL`, SMTP как у worker (Brevo),
`SALES_SESSION_TTL_SECONDS`, `SYNC_INTERVAL_MS`. Полный список — `apps/sales-api/.env.example`.

## Деплой (В ПРОДЕ с 2026-07-19)

https://partners.apitoken.sale работает. Как устроено на 84.32.48.2:

- БД `sales` (роль `sales`) в commerce-Postgres (`deploy-commerce-postgres-1`, :5433).
  Миграции: `node <realpath релиза>/packages/sales-db/dist/migrate.js` с env из
  `/etc/apitoken/sales.env`. **Готча:** запускать по разыменованному SHA-пути, не через
  симлинк `current` — гвард `isDirectExecution` сравнивает realpath и молча выходит.
- systemd: `apitoken-sales-api.service` (:3100) и `apitoken-sales-web.service` (:3200,
  `next start -H 127.0.0.1` — bind ТОЛЬКО на loopback). **Готча:** sales-web нужен `AF_NETLINK`
  в `RestrictAddressFamilies`, иначе Next падает на `uv_interface_addresses`.
- **sales в watchdog-пайплайне (автодеплой).** Класс путей `wd_path_is_sales`
  (`apps/sales-api/*`, `apps/sales-web/*`, `packages/sales-db/*`, shared build-файлы) с отдельным
  baseline `/var/lib/apitoken/watchdog/sales.sha`. После зелёных тестов watchdog зовёт
  `deploy/sales-deploy.sh <sha>`: промоут тестированного кандидата в неизменяемый релиз
  `/opt/apitoken/sales-releases/<sha>` → миграции sales-db (advisory-lock, expand-only) →
  атомарный свап `sales-releases/current` → рестарт обоих юнитов → health-gate
  (`/v1/health` + `/` по 200, до 60с) → **rollback симлинка** при провале. Контекст статуса —
  `deploy/sales`. У sales СВОЙ release root, НЕ на общем commerce-`current` (там commerce
  blue-green — трогать нельзя). Юниты смотрят на `sales-releases/current`.
  Ручной аварийный деплой (если понадобится) — тем же `sales-deploy.sh <sha>` из кандидата.
- Env: `/etc/apitoken/sales.env` (все ключи: SALES_DATABASE_URL, SALES_TOKEN_ENCRYPTION_KEY,
  `SALES_ADMIN_KEY` — ключ входа в /admin, SALES_CONTROL_KEY, SMTP Brevo). Тот же
  `SALES_CONTROL_KEY` добавлен в `/etc/apitoken/api.env` — включает фид.
- Telegram-вход включается на сервере: в `/etc/apitoken/sales.env` добавить
  `TELEGRAM_BOT_TOKEN` (от BotFather) и `TELEGRAM_BOT_USERNAME` (без @), у бота выполнить
  `/setdomain` → `partners.apitoken.sale`, затем `systemctl restart apitoken-sales-api`.
  Пока не задано — `/v1/auth/telegram*` отвечает 503, сайт показывает «sign-in unavailable».
- Caddy: vhost `partners.apitoken.sale` (`/v1/*`→:3100, остальное→:3200, same-origin куки;
  старый `sales.apitoken.sale` — 301 на partners) и
  loopback `http://127.0.0.1:8791` — стабильный health-gated origin commerce-backend поверх
  blue-green слотов 3000/3001 (аналог 8790 для движка); `COMMERCE_BASE_URL=http://127.0.0.1:8791`.
  `sales-deploy.sh` атомарно приводит root-only production env к этому адресу до рестарта API.
- Синк проверен на живых данных: курсоры прошли всю историю usage-событий и топапов; фид
  отвечает 401 без ключа; verify-письмо реально ушло через Brevo.

## Идеи развития (не реализовано)

- Серверная кука атрибуции вместо localStorage.
- Промо-материалы в кабинете (баннеры, UTM-конструктор), витрина статистики кликов
  (сейчас считаем только регистрации и расход).
- Автовыплаты USDT через провайдера; минимальный порог выплаты.
- Уведомления партнёру (email/TG) о новых рефералах и начислениях.
- Персональные лендинги/промокоды на скидку, финансируемые из комиссии партнёра.
