# DigiSeller payment integration

Official documentation:

- API index: https://my.digiseller.com/inside/api.asp
- Checkout/payment methods: https://my.digiseller.com/inside/api_payment.asp
- API login and purchase verification: https://my.digiseller.com/inside/api_general.asp?view=settings
- Swagger: https://api.digiseller.com/swagger/ui/index

## Correct integration role

We are a DigiSeller **seller**. The "Setup individual payment methods" HMAC protocol in the payment
documentation is for companies implementing a payment method inside DigiSeller; it is not the
seller-sale callback protocol and must not be used to authenticate our sales.

The safe seller flow is:

```text
commercial API creates local checkout session
  → browser POSTs DigiSeller product form to https://oplata.info/asp2/pay.asp
    with signed checkout tracking in that URL's GET query
  → DigiSeller processes payment
  → DigiSeller redirects to our configured completion URL with `uniquecode` and tracking parameters
  → backend treats the redirect/retry as an untrusted wake-up signal
  → backend obtains a short-lived seller API token
  → GET /api/purchases/unique-code/{uniquecode}?token=...
  → obtain the authoritative invoice ID and purchase facts
  → validate successful code lookup, item_id, local checkout tracking and expected product value
  → atomically persist payment + enqueue engine credit
```

`GET /api/purchase/info/{invoice_id}` is used afterward for reconciliation and refund-state checks.

Never credit from the browser return URL or notification body alone.

The product setting shown as "automatic unique-code verification" should point to a public HTTPS
backend route such as `https://api.example.com/v1/payments/digiseller/complete`. DigiSeller adds the
`uniquecode` GET parameter and preserves GET parameters that were present on the payment-page URL.
If the first redirect fails, DigiSeller documents an additional request to the completion URL, so the
handler must be completely idempotent.

## Authentication

`POST https://api.digiseller.com/api/apilogin` accepts `seller_id`, a Unix timestamp, and
`SHA256(api_key + timestamp)`. The returned access token lasts about two hours. The adapter caches
it, refreshes one minute early and serializes concurrent refreshes. Required API-key permission for
verification is **Operations → Invoice details**.

## Payment states

Purchase Info `invoice_state` maps as follows:

| DigiSeller | Meaning | Normalized state |
|---:|---|---|
| 1 | payment expected | pending |
| 2 | canceled | canceled |
| 3 | successful payment | paid |
| 4 | overdue | canceled |
| 35 | refund not completed by buyer | refunded |
| 5 | refund | refunded |

Only state `3` may enqueue a positive engine credit. Refund states require a separate commercial
policy; they must never silently issue another positive credit.

## Identity and amount rules

- DigiSeller invoice ID is the provider payment ID and must be globally unique in our database.
- Provider event identity is `invoice_id:invoice_state`, allowing one transition per state.
- `item_id` must match a configured DigiSeller product owned by us.
- The amount of API credit comes from our local product catalog, not a callback field.
- Purchase `amount`/`amount_usd` are recorded for reconciliation and checked against expectations.
- DigiSeller's automatic completion redirects with the `uniquecode` plus GET parameters originally
  placed on the payment-page URL. We place an opaque `checkout_id` plus HMAC `checkout_sig` there.
- Purchase lookup can also expose these parameters as base64 `query_string`; the adapter accepts the
  association only when the HMAC verifies.

## Provider abstraction

`packages/payments` defines `PaymentProviderAdapter`:

- `createCheckout()` returns either a redirect or form-POST action.
- `verifyPayment()` returns normalized, independently verified payment facts.
- Provider adapters never decide engine credit value and never write the database.

Adding Stripe, crypto or another provider means implementing this interface. Shared persistence,
webhook deduplication, product valuation and engine-credit processing remain provider-independent.

## Configuration still required

```text
DIGISELLER_SELLER_ID
DIGISELLER_API_KEY
DIGISELLER_PRODUCT_ID
DIGISELLER_CHECKOUT_TRACKING_SECRET
```

Do not add these values to repository files. Before enabling production, configure DigiSeller's
product-sale notification URL and run one controlled purchase to capture the exact notification
method, content type, parameter names and acknowledgement DigiSeller expects. That callback contract
is account-configured and the public documentation redirects to authenticated seller settings.
