# DigiSeller — provider disabled (adapter without an entry point)

**Status: unavailable to clients.** The DigiSeller adapter exists in the code
(`packages/payments/src/digiseller.ts`, `DigiSellerProvider`) and is registered in the
provider registry (`apps/api/src/payments.module.ts`) when the `DIGISELLER_*` variables
are set, but **it is currently impossible to create a checkout via DigiSeller**:

- `PaymentProviderCode = "cryptomus" | "platega"` (`apps/api/src/checkout.service.ts`)
  and `paymentProviderSchema = z.enum(["cryptomus", "platega"])`
  (`packages/contracts/src/index.ts`) do not contain `"digiseller"` — a
  `POST /v1/checkouts` request with `provider: "digiseller"` is rejected by validation;
- there is no HTTP entry point for payment: neither a payment completion endpoint
  (`/v1/payments/digiseller/complete`) nor a webhook exists in `apps/api/src/payments.controller.ts`.
  No code calls either the adapter's `createCheckout()` or `verifyUniqueCode()`.

Historical payments and checkouts with `provider = "digiseller"` remain in the database and are visible in
admin-finance reports (revenue by day, checkout funnel by provider —
`apps/api/src/admin-finance.service.ts`). This is the provider's only live presence
in the runtime.

Official DigiSeller documentation:

- API index: https://my.digiseller.com/inside/api.asp
- Checkout/payment methods: https://my.digiseller.com/inside/api_payment.asp
- API login and purchase verification: https://my.digiseller.com/inside/api_general.asp?view=settings
- Swagger: https://api.digiseller.com/swagger/ui/index

## What is implemented in the adapter (groundwork not wired into the runtime)

We are a DigiSeller **seller**. The "Setup individual payment methods" protocol with HMAC from
the documentation is for companies implementing a payment method inside DigiSeller, not for
seller callbacks; it must not be used to authenticate our sales.

The designed (but not enabled) seller flow:

```text
the commerce API creates a local checkout
  → the browser POSTs the product form to https://oplata.info/asp2/pay.asp
    with checkout_id + HMAC checkout_sig in the payment URL GET parameters
  → DigiSeller processes the payment
  → DigiSeller redirects to the configured completion URL with uniquecode and tracking parameters
  → the backend treats the redirect as an untrusted wake-up signal
  → the backend obtains a short-lived seller API token
  → GET /api/purchases/unique-code/{uniquecode}?token=...
  → cross-check of item_id, checkout tracking, and the expected whole-USD amount
  → atomic payment storage + engine credit enqueue
```

`GET /api/purchase/info/{invoice_id}` (`verifyPayment()`) — for subsequent reconciliation and
refund checks. Crediting based on the return URL or the notification body alone is not allowed.

## Seller API authentication

`POST https://api.digiseller.com/api/apilogin` accepts `seller_id`, a Unix timestamp, and
`SHA256(api_key + timestamp)`. The token lives for about two hours; the adapter caches it,
refreshes it one minute before expiry, and serializes concurrent refreshes. The required
API key permission is **Operations → Invoice details**.

## Payment statuses

`invoice_state` from Purchase Info maps as follows:

| DigiSeller | Meaning | Normalized state |
|---:|---|---|
| 1 | awaiting payment | pending |
| 2 | canceled | canceled |
| 3 | successful payment | paid |
| 4 | expired | canceled |
| 35 | refund not completed by the buyer | refunded |
| 5 | refund | refunded |

Only state `3` may enqueue a positive engine credit. Refund states
require a separate commercial policy and must never silently issue a new
positive credit.

## Identity and amount rules (as designed)

- The DigiSeller Invoice ID is the provider payment ID, globally unique in our database.
- Provider event identity is `invoice_id:invoice_state`: one transition per status.
- `item_id` must match our product configuration but never determines the credit.
- The user-entered amount of the local checkout is authoritative; there is no product catalog.
- Entered whole USD are stored as bigint; the credit is `amountUsd * 1_000_000_000` nanoUSD.
- DigiSeller must ultimately charge exactly the checkout amount in whole USD. The existing
  product-form adapter is groundwork only: the seller side of the variable-price
  mechanism is not yet confirmed and not implemented.
- The payment-to-checkout binding is via `checkout_id` + HMAC `checkout_sig` in the
  payment URL GET parameters; the Purchase lookup may return them as a base64
  `query_string`, and the adapter accepts the binding only with a valid HMAC.

## What is needed to enable the provider

1. Extend `paymentProviderSchema` (`packages/contracts`) and `PaymentProviderCode`
   (`apps/api/src/checkout.service.ts`) with the value `"digiseller"`. This is a contract
   change (expand-only): the producer first, consumers — after a green
   `deploy/watchdog` on the producer's SHA.
2. Add a public payment completion endpoint (for example,
   `/v1/payments/digiseller/complete`) in `apps/api/src/payments.controller.ts`:
   accepting `uniquecode` + tracking parameters, full idempotency (DigiSeller
   documents a repeated request if the first redirect fails), exclusion from the
   origin guard, processing via `verifyUniqueCode()` and
   `applyVerifiedCheckoutPaymentEvent`.
3. Confirm and implement the seller-side variable-price mechanism so that DigiSeller
   charges exactly the checkout amount.
4. Set the configuration in the deployment environment (the values do not belong in the repository):

```text
DIGISELLER_SELLER_ID
DIGISELLER_API_KEY
DIGISELLER_PRODUCT_ID
DIGISELLER_CHECKOUT_TRACKING_SECRET
```

5. Configure the notification/completion URL in the DigiSeller dashboard and run one
   controlled purchase to pin down the exact method, content type, parameter
   names, and expected callback confirmation: the callback contract is configured on the
   account, and the public documentation redirects to the closed seller settings.
