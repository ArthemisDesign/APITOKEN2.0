# Cryptomus payment integration

Official documentation:

- Request signing: https://doc.cryptomus.com/merchant-api/request-format
- Invoice creation: https://doc.cryptomus.com/merchant-api/payments/creating-invoice
- Payment information: https://doc.cryptomus.com/merchant-api/payments/payment-information
- Webhooks: https://doc.cryptomus.com/merchant-api/payments/webhook
- Webhook testing: https://doc.cryptomus.com/merchant-api/payments/testing-webhook
- Refunds: https://doc.cryptomus.com/merchant-api/payments/refund

## Chosen flow

Use a hosted **invoice**, not a static wallet. Each local checkout has a fixed fiat amount and a
unique `checkoutId`; the customer chooses the cryptocurrency and network on Cryptomus's hosted
page. This keeps pricing in our local product catalog and avoids maintaining blockchain addresses.

```text
authenticated client selects a local product
  -> commercial API creates a local checkout with user, engine account, product and USD value
  -> Cryptomus POST /v1/payment uses checkoutId as idempotent order_id
  -> browser redirects to the returned pay.cryptomus.com URL
  -> Cryptomus POSTs signed status changes to
     https://api.apitoken.sale/v1/payments/cryptomus/webhook
  -> backend verifies the webhook signature
  -> backend rechecks the UUID with signed POST /v1/payment/info
  -> backend matches checkoutId, product, expected fiat amount and commercial user
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

For `paid_over`, grant only the purchased local product value. Never convert the excess payment
into extra engine credit automatically. Refunds require a separate operator/product policy.

## Configuration

```text
PUBLIC_API_BASE_URL=https://api.apitoken.sale
CRYPTOMUS_MERCHANT_ID=<merchant UUID>
CRYPTOMUS_PAYMENT_API_KEY=<payment API key>
```

Both Cryptomus credentials are optional in development but must be set together. Secrets belong in
the deployment environment, never in the repository or browser.

## What remains before live payments

The provider adapter, request signing, invoice creation, signed webhook parsing and authoritative
payment lookup are implemented. Public checkout/webhook controllers intentionally wait for the
local product catalog and checkout-session model: without them there is no safe source for user,
engine account, expected amount or engine-credit value.

Before launch:

1. Add product and checkout-session persistence.
2. Add authenticated checkout creation and the public webhook controller.
3. Point `api.apitoken.sale` to the API through HTTPS and configure the webhook IP rule.
4. Use Cryptomus's test-webhook endpoint to exercise the deployed callback.
5. Run one controlled invoice through every relevant state before enabling customers.
