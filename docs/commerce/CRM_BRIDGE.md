# CRM ↔ Commerce referral bridge

This is the live server-side contract for a reusable, least-privilege bridge between the standalone
CRM and Commerce. It lets CRM issue one opaque referral link for one CRM object and read only the
Commerce registrations attributed to that link. The bridge does not share databases and does not
give CRM a general customer, admin, Sales, or engine credential.

The public link continues to use the ordinary partner program. Sales owns the partner and alias;
Commerce owns registration, pricing and money; CRM owns its contact, public tracking wrapper and
click evidence.

## Trust boundaries and identifiers

There are three distinct identifiers:

- `externalRef` is a CRM-generated UUID. It is an opaque correlation reference, not an email,
  name, Telegram handle, chat ID or contact UUID embedded in a URL. It is sent only over internal
  authenticated calls.
- `referralAlias` is a random public code issued by Sales for `(source=crm, externalRef)`. It is
  stored by the existing Commerce registration flow as `referral_attributions.code` and resolves
  through the ordinary partner commission path.
- CRM's public tracking token belongs only to CRM `/r/:token`. It is not a Commerce customer ID and
  is never accepted by the internal bridge as authorization.

The bridge principal cannot choose a partner. `CRM_REFERRAL_PARTNER_CODE` is server configuration;
the request body contains only `externalRef`. Sales binds the external reference immutably to one
active partner, so an idempotent replay returns the same alias and a changed owner fails closed.

`partner_discount_links` and `referral_floor_bps` are not used. Referral attribution controls the
partner commission relationship; it does not silently create or promise a customer discount.

## Configuration

Commerce API enables the routes only when all of the following are valid:

- `CRM_CONTROL_KEY`: a dedicated key of at least 32 characters, sent by CRM as `x-api-key`;
- `CRM_REFERRAL_PARTNER_CODE`: the active ordinary partner that owns CRM acquisition;
- `SALES_API_URL` and `SALES_CONTROL_KEY`: the existing Commerce→Sales boundary used only by
  Commerce to issue/replay the alias.

The CRM key must not equal or replace `SALES_CONTROL_KEY`, `COMMERCIAL_ADMIN_KEY`, an engine Control
key, a browser session or a human credential. When the CRM key is absent, the routes return 404.
Responses use `Cache-Control: no-store`.

## Internal HTTP contract

Contracts are additive at `schemaVersion: 1`. Consumers ignore unknown response fields but reject a
missing or malformed required field. Every `externalRef` is a UUID; there is deliberately no lookup
by email or arbitrary Commerce `userId`.

### Ensure a referral link

`POST /v1/internal/crm/referral-link`

```json
{
  "externalRef": "10000000-0000-4000-8000-000000000001"
}
```

Response:

```json
{
  "schemaVersion": 1,
  "externalRef": "10000000-0000-4000-8000-000000000001",
  "referralAlias": "r_opaque_random_alias",
  "destinationUrl": "https://apitoken.sale/?ref=r_opaque_random_alias&utm_source=crm&utm_medium=direct_sales&utm_campaign=crm-referral&utm_content=r_opaque_random_alias",
  "createdAt": "2026-08-16T10:00:00.000Z"
}
```

The operation is idempotent through the Sales `(source, externalRef)` binding. The destination has
the fixed `PUBLIC_APP_BASE_URL` origin. It contains the public alias, never `externalRef`, a CRM
contact ID or conversation text. A CRM consumer should validate the configured HTTPS origin before
persisting or redirecting to this URL.

Creating or copying this URL proves only `link_created`. It is not evidence of `link_sent`.

### Read a scoped referral profile

`GET /v1/internal/crm/referral-profile?externalRef=<uuid>`

The server replays the same immutable Sales binding, then queries Commerce only through the returned
alias. It cannot broaden the query to another email or user.

The response contains:

- `attributionStatus`: `none`, `unique` or `ambiguous`;
- `registrations[]`: every new Commerce account durably attributed to this alias;
- `asOf`: the Commerce snapshot time.

One link may be forwarded. Two registrations therefore produce `ambiguous`; Commerce does not pick
the newest account or claim either email is the CRM contact. Each registration is labelled
`binding: link_attributed`. CRM must not automatically copy this email into `contact_channels`.

Each registration exposes the minimum useful snapshot:

- candidate ID, email, `emailVerified`, registration time, customer and engine status;
- default `multiplierBp` (live engine value when available, otherwise explicitly `defaultState=saved`)
  and saved provider overrides, with `discountBps = 10000 - multiplierBp`;
- exact decimal nanoUSD strings: `paidTopupNano`, `refundedNano`, `usageSpentNano`,
  `customerFundedSpentNano`, and nullable `balanceNano`;
- `liveState`: `complete`, `unavailable` or `not_provisioned`.

`paidTopupNano` is gross verified payment funding, including a payment that later became refunded or
disputed. `refundedNano` reports those terminal returns separately. `usageSpentNano` is full billed
usage. `customerFundedSpentNano` sums the live scalar
`pricing_usage_events.real_funded_nano`, not a top-up/balance guess. CRM referral aliases were added
after the scalar writer became authoritative, so registrations reachable through this bridge are a
post-scalar cohort. The bridge never reads retired pricing-attribution incident evidence to recover
arbitrary pre-cutover users. The live balance and default multiplier come from one batch
`EngineClient.getAccounts` call.

If the engine is unavailable, `balanceNano` is `null` and `liveState=unavailable`; the API never
fabricates zero. Saved money evidence remains available. If Sales alias issuance/replay is
unavailable or conflicts, the entire request fails with 503 instead of widening the lookup.

## Lifecycle facts in CRM

The Commerce route provides referral creation and registration facts. CRM retains the other local
facts separately:

| State | Required evidence | Does not prove |
|---|---|---|
| `link_created` | Sales alias and CRM wrapper committed | sent, opened or registered |
| `link_sent` | exact outgoing archived message ID containing the tracked URL | opened |
| `link_opened` | CRM `/r/:token` received a valid token | human visit; messenger preview may prefetch |
| `customer_registered` | Commerce attribution row for the alias | the account is the intended CRM person |

There is currently no Commerce `landing_confirmed` event/feed in this contract. A CRM redirect hit
must be displayed as “link opened” or “transition recorded”, not “the person visited the site”. If a
future first-party landing confirmation or ordered event feed is added, it is an additive producer
contract and receives its own migration/rollout documentation.

## Privacy, logging and failure rules

- The internal profile is the only PII boundary. Sales feeds remain email-free.
- Public routes and query strings return no email, customer data, money, `externalRef` or API key.
- Logs and metric labels must use route/status templates, never aliases, keys, email or raw token
  paths.
- CRM may cache the minimum profile as an explicitly stale projection. Transport failure must never
  turn the last known registration into `none` or live-unavailable money into zero.
- Multiple registrations stay visible as a conflict. Identity resolution is an explicit audited CRM
  operation outside this bridge.
- No component opens another bounded context's database. CRM never calls Sales or the engine
  directly; Commerce calls the engine only through `packages/engine-client`.

## Deployment order

The bridge is producer-first and migration-first:

1. deploy `packages/sales-db/migrations/0021_external_referral_aliases.sql` alone and wait for both
   migration and watchdog GREEN;
2. deploy the Sales external-alias producer/resolver and wait for watchdog GREEN;
3. deploy these Commerce internal routes and verify authenticated ensure/profile plus negative-key
   checks;
4. only then configure and deploy the CRM consumer, redirect and UI.

Do not combine steps 1 and 2 into one production rollout. CRM remains disabled safely while
`COMMERCE_BRIDGE_URL`/`COMMERCE_BRIDGE_KEY` are absent.
