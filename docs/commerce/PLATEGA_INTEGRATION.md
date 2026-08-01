# Интеграция платежей Platega

Platega — дефолтный платёжный провайдер коммерческого контура. Клиентский фронт
(`apps/web/src/lib/api.ts`) шлёт только `provider: "platega"`, а в `createCheckoutSchema`
(`packages/contracts/src/index.ts`) поле `provider` имеет `default("platega")`.

Адаптер — `packages/payments/src/platega.ts` (`PlategaProvider`), подключение и env —
`apps/api/src/payments.module.ts`, вебхук — `apps/api/src/payments.controller.ts`,
обработка — `apps/api/src/checkout.service.ts`, safety-net — `apps/worker/src/platega-reconcile.service.ts`.

## Выбранный флоу

Platega принимает оплату в RUB (СБП / ERIP / карта) и в USD (крипта). Баланс клиента
тарифицируется в целых USD, поэтому конвертация USD → RUB нужна только чтобы решить,
сколько взыскать с покупателя. Кредит движка — всегда записанный в чекауте USD
(инвариант на уровне БД), никогда не фактически оплаченные рубли.

```text
аутентифицированный клиент шлёт целые USD-цифры, например "37"
  → POST /v1/checkouts {"amountUsd":"37","provider":"platega","paymentMethod":2}
  → коммерческий API сохраняет чекаут: пользователь, 37 USD (bigint)
  → адаптер конвертирует USD → RUB по курсу Rapira USDT/RUB + маржа
     (для крипто-метода 13 — взыскание сразу в USD)
  → Platega POST /transaction/process, payload = checkoutId (UUID)
  → браузер редиректится на возвращённый pay-URL Platega
  → Platega POSTит смену статуса на
     https://backend.apitoken.sale/v1/payments/platega/webhook
  → бэкенд проверяет заголовки X-Secret / X-MerchantId
  → бэкенд независимо перепроверяет транзакцию через GET /transaction/{id}
  → бэкенд сверяет checkoutId из payload с локальным чекаутом
  → оплаченный платёж и одна задача engine-credit сохраняются атомарно
```

## Создание чекаута

Эндпоинт: `POST /v1/checkouts` под серверной сессией. Тело валидируется
`createCheckoutSchema` (`packages/contracts/src/index.ts`):

- `amountUsd` — `wholeUsdSchema`: только цифры положительных целых USD
  (JSON-числа, десятичная точка, знаки и ведущие нули отклоняются);
- `provider` — `z.enum(["cryptomus", "platega"])`, по умолчанию `"platega"`;
- `paymentMethod` — необязательный id метода Platega (2 СБП, 3 ERIP, 11 карта,
  12 international, 13 крипта); другие провайдеры игнорируют поле.

Лимиты `MIN_TOPUP_USD` / `MAX_TOPUP_USD` проверяет `CheckoutService.create`
до обращения к провайдеру. Если `paymentMethod` не передан, адаптер берёт
`PLATEGA_DEFAULT_PAYMENT_METHOD` (по умолчанию 2 — СБП). В `apps/web` на проде
доступны только реально включённые на нашем мерчанте методы (СБП + крипта).

Адаптер вызывает `POST {PLATEGA_API_BASE_URL}/transaction/process` с заголовками
`X-MerchantId` / `X-Secret` и телом: `paymentMethod`, `paymentDetails` (сумма и валюта
взыскания), `description`, `return`/`failedUrl` (страницы `/dashboard?view=credits`
с `paymentReturn=success|cancel` и `checkoutId`), `payload = checkoutId`. Ответ даёт
`transactionId` (становится `providerPaymentId` чекаута) и `redirect` (URL оплаты).
`expiresIn` приходит как длительность `"HH:MM:SS"` от момента создания и переводится
в абсолютный `expiresAt`.

## Конвертация USD → RUB

Для всех методов, кроме `usdMethods` (по умолчанию `[13]` — крипта), сумма взыскания
считается по публичному курсу Rapira (`PLATEGA_RATE_URL`,
`https://api.rapira.net/open/market/rates`): берётся `askPrice` (fallback — `close`)
пары USDT/RUB, сверху добавляется `PLATEGA_FX_MARGIN_BPS` (базисные пункты, 0–5000,
покрывает комиссию Platega и дрейф курса), результат округляется вверх до целого RUB.
Крипто-метод тарифицируется сразу в USD, чтобы покупатель видел доллары. В обоих
случаях кредит движка — записанный в чекауте целый USD.

## Вебхук и его авторизация

`POST /v1/payments/platega/webhook` — публичный маршрут, исключённый из origin-гарда
(`apps/api/src/origin.guard.ts`). Авторизация — только заголовками: `X-Secret` и
`X-MerchantId` (в NestJS нормализуется в `x-merchantid`, без дефиса) сравниваются
в константном времени с `PLATEGA_SECRET` и `PLATEGA_MERCHANT_ID`; HMAC-подписи у
Platega нет. Невалидные заголовки — `PlategaWebhookAuthError` → 401, до любой
другой работы. Если обе переменные не заданы, обработчик отвечает как для
несконфигурированного провайдера.

Тело колбека (`id`, `amount`, `currency`, `status`, `paymentMethod`, `payload`)
валидируется zod-схемой, но сам колбек — только сигнал пробуждения: начисление требует
независимого `GET /transaction/{id}`. Дальше `processPlategaWebhook` проверяет:

- `id` транзакции из колбека совпадает с перепроверенным;
- `payload` (checkoutId) присутствует и совпадает с чекаутом, найденным по
  `providerPaymentId` в локальной БД.

Сумма берётся из локального чекаута (`checkout.amountUsd`), а не из тела колбека и не
из ответа Platega. Идентичность события — `{id}:{STATUS}` (статус в верхнем регистре);
дедупликация по `webhook_events` в `applyVerifiedCheckoutPaymentEvent` (`packages/db`)
делает повторную доставку идемпотентной.

Callback-URL собирается из `PUBLIC_API_BASE_URL` (`/v1/payments/platega/webhook`) и
передаётся в конструктор адаптера, но в запросах к Platega API адаптер его не шлёт:
адрес вебхука должен быть настроен в кабинете мерчанта Platega.

## Политика статусов

| Статус Platega | Нормализованное состояние | Действие по кредиту |
|---|---|---|
| `CONFIRMED` | paid | допустим после всех локальных проверок |
| `PENDING` и прочие | pending | нет |
| `CANCELED`, `CANCELLED` | canceled | нет |
| `CHARGEBACKED` | refunded | никогда не добавлять положительный кредит |

## Reconcile-поллинг в worker

`apps/worker/src/platega-reconcile.service.ts` — страховка на случай недоставленного
вебхука. Поллер стартует только при заданных `PLATEGA_MERCHANT_ID` и `PLATEGA_SECRET`
и циклом `PLATEGA_RECONCILE_MS` (по умолчанию 30 с, минимум 5 с) выбирает
pending-чекауты Platega батчами по 50: не моложе `PLATEGA_RECONCILE_MIN_AGE_S`
(по умолчанию 15 с) и не старше 2 суток. Каждый чекаут перепроверяется через
`verifyPayment`; статус `pending` пропускается, расхождение `payload` с чекаутом
логируется и пропускается, остальное применяется тем же
`applyVerifiedCheckoutPaymentEvent`. Двойное начисление невозможно: идентификатор
события `id:STATUS` дедуплицируется по `webhook_events` совместно с вебхуком.

## Конфигурация

```text
PUBLIC_API_BASE_URL=https://backend.apitoken.sale
PLATEGA_MERCHANT_ID=<merchant UUID из кабинета Platega>
PLATEGA_SECRET=<X-Secret из кабинета Platega>
PLATEGA_FX_MARGIN_BPS=0            # маржа в bps поверх курса Rapira (0–5000)
PLATEGA_DEFAULT_PAYMENT_METHOD=2   # метод по умолчанию: 2 СБП
PLATEGA_RATE_URL=https://api.rapira.net/open/market/rates
# worker:
PLATEGA_API_BASE_URL=https://app.platega.io
PLATEGA_RECONCILE_MS=30000
PLATEGA_RECONCILE_MIN_AGE_S=15
```

`PLATEGA_MERCHANT_ID` и `PLATEGA_SECRET` задаются только вместе: ровно одна из двух —
ошибка конфигурации (`apps/api/src/config.ts`). Без пары адаптер не регистрируется,
создание чекаутов Platega отвечает 503, а reconcile-поллер молча выключен.
Секреты — только в окружении деплоя, никогда в репозитории или браузере.

## Реализованный HTTP-контракт

```text
POST /v1/checkouts                    {"amountUsd":"37","provider":"platega","paymentMethod":2}
GET  /v1/checkouts/{checkout UUID}
POST /v1/payments/platega/webhook     raw JSON Platega + заголовки X-Secret / X-MerchantId
```

Создание и статус чекаута требуют валидной серверной сессии; идентичность
пользователя никогда не принимается из тела, URL или кастомного заголовка. Обработка
вебхука публична, авторизована заголовками, независимо перепроверена через
`GET /transaction/{id}`, сверена с локальным чекаутом и идемпотентна. Оплаченный
чекаут ставит в очередь ровно `amountUsd * 1_000_000_000` nanoUSD.
