# Vercel product analytics

The web app uses Vercel Web Analytics for page views and anonymous custom product events. Custom
events are emitted only after an action succeeds unless the event name explicitly says `Submitted`
or `Failed`. Browser-first milestones use `localStorage`; they describe the first observation in that
browser and are not account-authoritative identity records.

## Funnel and milestone events

| Journey stage | Events | Useful properties |
| --- | --- | --- |
| Acquisition | `First Touch`, `Referral Captured` | coarse source/medium, clean landing path, language, referral present |
| Authentication | `Sign Up Submitted`, `Sign Up Succeeded`, `Sign Up Failed`, `Login Submitted`, `Login Succeeded`, `Login Failed`, `First Login`, `Email Verified` | method, verification required, invited/referred, numeric HTTP status |
| Activation | `Dashboard Opened`, `First Dashboard Open`, `Dashboard Section Viewed`, `API Key Created`, `First API Key Created` | section, customer type, label present, 2FA used |
| Monetization | `Checkout Created`, `First Checkout Created`, `First Top Up`, `Promo Redeemed` | provider, payment method, coarse amount bucket |
| Value | `First API Usage` | detected in dashboard |
| Retention/security | `API Key Renamed`, `API Key Revoked`, `Profile Updated`, `Two Factor Enabled`, `Two Factor Disabled` | no identifying values |

`First Top Up` and `First API Usage` are emitted when authoritative dashboard data first shows the
milestone to that browser. This makes them resilient to payment-provider redirects and API calls that
happen outside the website.

## Privacy contract

Product events must never contain email addresses, user/account/key/checkout ids, raw referral or
promo codes, API keys, form values, error messages, full referrers, or URLs with query strings. Exact
top-up amounts are reduced to a coarse bucket. UTM source/medium values are accepted only when they
match a short conservative character allowlist; everything else becomes `other`.

The enforcement boundary is `src/lib/product-analytics.ts`. New events should go through
`trackProductEvent` or `trackFirstProductEvent` and be added to this taxonomy.

## Suggested Vercel funnels

1. Acquisition to activation: `First Touch` → `Sign Up Succeeded` → `First Dashboard Open` →
   `First API Key Created`.
2. Activation to value: `First API Key Created` → `First API Usage`.
3. Monetization: `Dashboard Section Viewed` where `section=credits` → `Checkout Created` →
   `First Top Up`.
4. Authentication quality: compare `Login Submitted` with `Login Succeeded` and `Login Failed`,
   grouped by `method`.
