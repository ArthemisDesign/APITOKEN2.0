# Platega payments integration

Platega is the default payment provider of the commerce perimeter. The client frontend
(`apps/web/src/lib/api.ts`) sends only `provider: "platega"`, and in `createCheckoutSchema`
(`packages/contracts/src/index.ts`) the `provider` field has `default("platega")`.

Adapter — `packages/payments/src/platega.ts` (`PlategaProvider`), wiring and env —
`apps/api/src/payments.module.ts`, webhook — `apps/api/src/payments.controller.ts`,
processing — `apps/api/src/checkout.service.ts`, safety net — `apps/worker/src/platega-reconcile.service.ts`.

## Selected flow

Platega accepts payment in RUB (SBP / ERIP / card) and in USD (crypto). The client's balance
is metered in whole USD, so USD → RUB conversion is needed only to decide
how much to charge the buyer. The engine credit is always the USD recorded in the checkout
(a database-level invariant), never the actually paid rubles.

```text
an authenticated client sends whole-USD digits, e.g. "37"
  → POST /v1/checkouts {"amountUsd":"37","provider":"platega","paymentMethod":2}
  → the commerce API stores the checkout: user, 37 USD (bigint)
  → the adapter converts USD → RUB at the Rapira USDT/RUB rate + margin
     (for crypto method 13 — charged directly in USD)
  → Platega POST /transaction/process, payload = checkoutId (UUID)
  → the browser is redirected to the returned Platega pay URL
  → Platega POSTs the status change to
     https://backend.apitoken.sale/v1/payments/platega/webhook
  → the backend verifies the X-Secret / X-MerchantId headers
  → the backend independently re-verifies the transaction via GET /transaction/{id}
  → the backend cross-checks the checkoutId from the payload against the local checkout
  → the paid payment and one engine-credit job are stored atomically
```

## Checkout creation

Endpoint: `POST /v1/checkouts` under a server session. The body is validated by
`createCheckoutSchema` (`packages/contracts/src/index.ts`):

- `amountUsd` — `wholeUsdSchema`: only digits of positive whole USD
  (JSON numbers, decimal points, signs, and leading zeros are rejected);
- `provider` — `z.enum(["cryptomus", "platega"])`, defaults to `"platega"`;
- `paymentMethod` — optional Platega method id (2 SBP, 3 ERIP, 11 card,
  12 international, 13 crypto); other providers ignore the field.

The `MIN_TOPUP_USD` / `MAX_TOPUP_USD` limits are checked by `CheckoutService.create`
before calling the provider. If `paymentMethod` is not passed, the adapter uses
`PLATEGA_DEFAULT_PAYMENT_METHOD` (defaults to 2 — SBP). In `apps/web` in production,
only the methods actually enabled on our merchant account are available (SBP + crypto).

The adapter calls `POST {PLATEGA_API_BASE_URL}/transaction/process` with
`X-MerchantId` / `X-Secret` headers and a body containing: `paymentMethod`, `paymentDetails` (charge
amount and currency), `description`, `return`/`failedUrl` (the `/dashboard?view=credits` pages
with `paymentReturn=success|cancel` and `checkoutId`), `payload = checkoutId`. The response provides
`transactionId` (becomes the checkout's `providerPaymentId`) and `redirect` (payment URL).
`expiresIn` arrives as a `"HH:MM:SS"` duration from the moment of creation and is converted
into an absolute `expiresAt`.

## USD → RUB conversion

For all methods except `usdMethods` (defaults to `[13]` — crypto), the charge amount
is computed from the public Rapira rate (`PLATEGA_RATE_URL`,
`https://api.rapira.net/open/market/rates`): the `askPrice` (fallback — `close`)
of the USDT/RUB pair is taken, `PLATEGA_FX_MARGIN_BPS` (basis points, 0–5000,
covering the Platega fee and rate drift) is added on top, and the result is rounded up
to a whole RUB. The crypto method is metered directly in USD so the buyer sees
dollars. In both cases the engine credit is the whole USD recorded in the checkout.

## Webhook and its authorization

`POST /v1/payments/platega/webhook` is a public route excluded from the origin guard
(`apps/api/src/origin.guard.ts`). Authorization is header-only: `X-Secret` and
`X-MerchantId` (normalized in NestJS to `x-merchantid`, without the hyphen) are compared
in constant time against `PLATEGA_SECRET` and `PLATEGA_MERCHANT_ID`; Platega has no
HMAC signature. Invalid headers — `PlategaWebhookAuthError` → 401, before any
other work. If both variables are unset, the handler responds as for an
unconfigured provider.

The callback body (`id`, `amount`, `currency`, `status`, `paymentMethod`, `payload`)
is validated by a zod schema, but the callback itself is only a wake-up signal: crediting requires
an independent `GET /transaction/{id}`. Then `processPlategaWebhook` checks:

- the transaction `id` from the callback matches the re-verified one;
- the `payload` (checkoutId) is present and matches the checkout found by
  `providerPaymentId` in the local database.

The amount is taken from the local checkout (`checkout.amountUsd`), not from the callback body and not
from the Platega response. Event identity is `{id}:{STATUS}` (status in uppercase);
deduplication via `webhook_events` in `applyVerifiedCheckoutPaymentEvent` (`packages/db`)
makes repeat delivery idempotent.

The callback URL is built from `PUBLIC_API_BASE_URL` (`/v1/payments/platega/webhook`) and
passed to the adapter constructor, but the adapter does not send it in requests to the Platega API:
the webhook address must be configured in the Platega merchant dashboard.

## Status policy

| Platega status | Normalized state | Credit action |
|---|---|---|
| `CONFIRMED` | paid | allowed after all local checks |
| `PENDING` and others | pending | none |
| `CANCELED`, `CANCELLED` | canceled | none |
| `CHARGEBACKED` | refunded | cancel an unclaimed credit or durably compensate a possibly delivered one |

`CHARGEBACKED` is terminal even when the original engine-credit response was lost. Commerce stores
the refund state and its negative adjustment atomically; the adjustment is claimable only after the
paired positive credit is durably confirmed, so retries converge to one top-up and one debit.

## Reconcile polling in the worker

`apps/worker/src/platega-reconcile.service.ts` is the safety net for an undelivered
webhook. The poller starts only when `PLATEGA_MERCHANT_ID` and `PLATEGA_SECRET` are set,
and on a `PLATEGA_RECONCILE_MS` cycle (defaults to 30 s, minimum 5 s) selects
pending Platega checkouts in batches of 50: no younger than `PLATEGA_RECONCILE_MIN_AGE_S`
(defaults to 15 s) and no older than 2 days. Each checkout is re-verified via
`verifyPayment`; the `pending` status is skipped, a `payload` mismatch with the checkout
is logged and skipped, and everything else is applied via the same
`applyVerifiedCheckoutPaymentEvent`. Double crediting is impossible: the
`id:STATUS` event identifier is deduplicated via `webhook_events` together with the webhook.

## Configuration

```text
PUBLIC_API_BASE_URL=https://backend.apitoken.sale
PLATEGA_MERCHANT_ID=<merchant UUID from the Platega dashboard>
PLATEGA_SECRET=<X-Secret from the Platega dashboard>
PLATEGA_FX_MARGIN_BPS=0            # margin in bps on top of the Rapira rate (0–5000)
PLATEGA_DEFAULT_PAYMENT_METHOD=2   # default method: 2 SBP
PLATEGA_RATE_URL=https://api.rapira.net/open/market/rates
# worker:
PLATEGA_API_BASE_URL=https://app.platega.io
PLATEGA_RECONCILE_MS=30000
PLATEGA_RECONCILE_MIN_AGE_S=15
```

`PLATEGA_MERCHANT_ID` and `PLATEGA_SECRET` must be set only together: exactly one of the two is
a configuration error (`apps/api/src/config.ts`). Without the pair the adapter is not registered,
Platega checkout creation responds with 503, and the reconcile poller is silently disabled.
Secrets belong only in the deployment environment, never in the repository or the browser.

## Implemented HTTP contract

```text
POST /v1/checkouts                    {"amountUsd":"37","provider":"platega","paymentMethod":2}
GET  /v1/checkouts/{checkout UUID}
POST /v1/payments/platega/webhook     raw Platega JSON + X-Secret / X-MerchantId headers
```

Checkout creation and status require a valid server session; user
identity is never accepted from the body, URL, or a custom header. Webhook
processing is public, header-authorized, independently re-verified via
`GET /transaction/{id}`, cross-checked against the local checkout, and idempotent. A paid
checkout enqueues exactly `amountUsd * 1_000_000_000` nanoUSD.
