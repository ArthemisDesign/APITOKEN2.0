# DigiSeller — провайдер отключён (адаптер без точки входа)

**Статус: недоступен для клиентов.** Адаптер DigiSeller существует в коде
(`packages/payments/src/digiseller.ts`, `DigiSellerProvider`) и регистрируется в реестре
провайдеров (`apps/api/src/payments.module.ts`) при заданных `DIGISELLER_*` переменных,
но **создать чекаут через DigiSeller сейчас невозможно**:

- `PaymentProviderCode = "cryptomus" | "platega"` (`apps/api/src/checkout.service.ts`)
  и `paymentProviderSchema = z.enum(["cryptomus", "platega"])`
  (`packages/contracts/src/index.ts`) не содержат `"digiseller"` — запрос
  `POST /v1/checkouts` с `provider: "digiseller"` отклоняется валидацией;
- HTTP-точки входа для оплаты нет: ни эндпоинта завершения платежа
  (`/v1/payments/digiseller/complete`), ни вебхука в `apps/api/src/payments.controller.ts`
  не существует. Никакой код не вызывает ни `createCheckout()`, ни `verifyUniqueCode()`
  адаптера.

Исторические платежи и чекауты с `provider = "digiseller"` остаются в БД и видны в
admin-finance отчётах (выручка по дням, воронка чекаута по провайдерам —
`apps/api/src/admin-finance.service.ts`). Это единственное живое присутствие
провайдера в рантайме.

Официальная документация DigiSeller:

- API index: https://my.digiseller.com/inside/api.asp
- Checkout/payment methods: https://my.digiseller.com/inside/api_payment.asp
- API login and purchase verification: https://my.digiseller.com/inside/api_general.asp?view=settings
- Swagger: https://api.digiseller.com/swagger/ui/index

## Что реализовано в адаптере (задел, не подключённый к рантайму)

Мы — DigiSeller **seller**. Протокол «Setup individual payment methods» с HMAC из
документации — для компаний, реализующих платёжный метод внутри DigiSeller, а не для
колбеков продавца; использовать его для аутентификации наших продаж нельзя.

Запроектированный (но не включённый) флоу продавца:

```text
коммерческий API создаёт локальный чекаут
  → браузер POSTит форму товара на https://oplata.info/asp2/pay.asp
    с checkout_id + HMAC checkout_sig в GET-параметрах платёжного URL
  → DigiSeller обрабатывает оплату
  → DigiSeller редиректит на настроенный completion URL с uniquecode и tracking-параметрами
  → бэкенд трактует редирект как недоверенный сигнал пробуждения
  → бэкенд получает короткоживущий seller API token
  → GET /api/purchases/unique-code/{uniquecode}?token=...
  → сверка item_id, checkout tracking и ожидаемой суммы целых USD
  → атомарное сохранение платежа + постановка engine credit
```

`GET /api/purchase/info/{invoice_id}` (`verifyPayment()`) — для последующей сверки и
проверки возвратов. Начислять по return URL или телу уведомления в одиночку нельзя.

## Аутентификация seller API

`POST https://api.digiseller.com/api/apilogin` принимает `seller_id`, Unix-timestamp и
`SHA256(api_key + timestamp)`. Токен живёт около двух часов; адаптер кэширует его,
обновляет за минуту до истечения и сериализует конкурентные обновления. Необходимое
разрешение API-ключа — **Operations → Invoice details**.

## Статусы платежа

`invoice_state` из Purchase Info мапится так:

| DigiSeller | Значение | Нормализованное состояние |
|---:|---|---|
| 1 | ожидается оплата | pending |
| 2 | отменён | canceled |
| 3 | успешная оплата | paid |
| 4 | просрочен | canceled |
| 35 | возврат не завершён покупателем | refunded |
| 5 | возврат | refunded |

Только состояние `3` может ставить положительный engine credit. Состояния возврата
требуют отдельной коммерческой политики и никогда не должны молча выдавать новый
положительный кредит.

## Правила идентичности и сумм (как задумано)

- Invoice ID DigiSeller — provider payment ID, глобально уникальный в нашей БД.
- Идентичность события провайдера — `invoice_id:invoice_state`: один переход на статус.
- `item_id` должен совпадать с нашей конфигурацией товара, но никогда не определяет кредит.
- Введённая пользователем сумма локального чекаута авторитетна; каталога товаров нет.
- Введённые целые USD хранятся bigint; кредит — `amountUsd * 1_000_000_000` nanoUSD.
- DigiSeller должен в итоге взыскивать ровно сумму чекаута в целых USD. Существующий
  product-form адаптер — только задел: продавец-сторона механизма переменной цены
  пока не подтверждена и не реализована.
- Связь платежа с чекаутом — через `checkout_id` + HMAC `checkout_sig` в GET-параметрах
  платёжного URL; Purchase lookup может вернуть их как base64 `query_string`, и адаптер
  принимает связь только при валидном HMAC.

## Что нужно для включения провайдера

1. Расширить `paymentProviderSchema` (`packages/contracts`) и `PaymentProviderCode`
   (`apps/api/src/checkout.service.ts`) значением `"digiseller"`. Это контрактное
   изменение (expand-only): производитель первым, потребители — после зелёного
   `deploy/watchdog` на SHA производителя.
2. Добавить публичный эндпоинт завершения платежа (например,
   `/v1/payments/digiseller/complete`) в `apps/api/src/payments.controller.ts`:
   приём `uniquecode` + tracking-параметров, полная идемпотентность (DigiSeller
   документирует повторный запрос при неудаче первого редиректа), исключение из
   origin-гарда, обработка через `verifyUniqueCode()` и
   `applyVerifiedCheckoutPaymentEvent`.
3. Подтвердить и реализовать seller-side механизм переменной цены, чтобы DigiSeller
   взыскивал ровно сумму чекаута.
4. Задать конфигурацию в окружении деплоя (значениям не место в репозитории):

```text
DIGISELLER_SELLER_ID
DIGISELLER_API_KEY
DIGISELLER_PRODUCT_ID
DIGISELLER_CHECKOUT_TRACKING_SECRET
```

5. Настроить notification/completion URL в кабинете DigiSeller и провести одну
   контролируемую покупку, чтобы зафиксировать точный метод, content type, имена
   параметров и ожидаемое подтверждение колбека: контракт колбека настраивается на
   аккаунте, а публичная документация перенаправляет в закрытые настройки продавца.
