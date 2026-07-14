# Cryptomus payment integration

Official documentation:

- Request signing: https://doc.cryptomus.com/merchant-api/request-format
- Invoice creation: https://doc.cryptomus.com/merchant-api/payments/creating-invoice
- Payment information: https://doc.cryptomus.com/merchant-api/payments/payment-information
- Webhooks: https://doc.cryptomus.com/merchant-api/payments/webhook
- Webhook testing: https://doc.cryptomus.com/merchant-api/payments/testing-webhook
- Refunds: https://doc.cryptomus.com/merchant-api/payments/refund

## Chosen flow

Use a hosted **invoice**, not a static wallet. The user types an arbitrary positive **whole USD**
amount; decimal points, JSON numbers, signs, leading zeroes and floats are rejected. Each local
checkout stores that amount and a unique `checkoutId`; the customer chooses the cryptocurrency and
network on Cryptomus's hosted page. There is no product catalog.

```text
authenticated client submits whole USD digits such as "37"
  -> commercial API stores user, engine account, 37 USD and 37000000000 nanoUSD
  -> Cryptomus POST /v1/payment uses checkoutId as idempotent order_id
  -> browser redirects to the returned pay.cryptomus.com URL
  -> Cryptomus POSTs signed status changes to
     https://backend.apitoken.sale/v1/payments/cryptomus/webhook
  -> backend verifies the webhook signature
  -> backend rechecks the UUID with signed POST /v1/payment/info
  -> backend matches checkoutId, expected whole USD amount and commercial user
  -> a paid payment and one engine-credit job are persisted atomically
```

The adapter sends `is_payment_multiple: true`, so a customer can complete an underpayment while the
invoice is alive. `order_id` is the local checkout ID and Cryptomus documents duplicate invoice
creation with the same order ID as idempotent.

## Authentication and callback verification

API requests send the merchant UUID in the `merchant` header. `sign` is
`MD5(base64(exact JSON request body) + payment API key)`, as required by Cryptomus. The payout key
is not needed and must not be configured in this service.

Webhook signatures arrive in the JSON body. The adapter removes `sign`, serializes the remaining
object in its original property order using Cryptomus's PHP-compatible slash escaping, calculates
the same hash with the payment API key and compares it in constant time. A valid webhook is still
only a wake-up signal: crediting requires the independent `/v1/payment/info` result.

At the edge proxy, allow Cryptomus's documented webhook source IP `91.227.144.54` for this route in
addition to signature checking. Do not apply that IP rule inside application code, because the app
may only see a trusted reverse proxy address.

## Status policy

| Cryptomus status | Normalized state | Credit action |
|---|---|---|
| `paid`, `paid_over` | paid | eligible after all local checks |
| `confirm_check`, `wrong_amount`, other nonterminal states | pending | none |
| `fail`, `cancel`, `system_fail` | canceled | none |
| `refund_process`, `refund_fail` | pending | none |
| `refund_paid` | refunded | never add positive credit |

For `paid_over`, grant only the locally requested top-up. Never convert the excess payment into
extra engine credit automatically. Refunds require a separate operator policy.

## Configuration

```text
PUBLIC_API_BASE_URL=https://backend.apitoken.sale
PUBLIC_APP_BASE_URL=https://apitoken.sale
MIN_TOPUP_USD=1
MAX_TOPUP_USD=10000
CRYPTOMUS_MERCHANT_ID=<merchant UUID>
CRYPTOMUS_PAYMENT_API_KEY=<payment API key>
```

Both Cryptomus credentials are optional in development but must be set together. Secrets belong in
the deployment environment, never in the repository or browser.

## Implemented HTTP contract

```text
POST /v1/checkouts                         {"amountUsd":"37","provider":"cryptomus"}
GET  /v1/checkouts/{checkout UUID}
POST /v1/payments/cryptomus/webhook        raw Cryptomus JSON
```

Checkout creation and status require a valid server-side session. User identity is never accepted
from the body, URL or a custom user header. Webhook processing is public, signature-verified,
independently rechecked through `/v1/payment/info`, amount-checked and idempotent. A paid checkout
queues exactly `amountUsd * 1_000_000_000` nanoUSD.

## What remains before live payments

Before launch:

1. Point `backend.apitoken.sale` to the API through HTTPS and configure the webhook IP rule.
2. Add real Cryptomus credentials to the deployment environment.
3. Use Cryptomus's test-webhook endpoint to exercise the deployed callback.
4. Run one controlled invoice through every relevant state before enabling customers.
