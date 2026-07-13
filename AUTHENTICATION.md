# Authentication and authorization

## Implemented email/password flow

```text
POST /v1/auth/register  {"email":"user@example.com","password":"at least 12 characters","inviteToken"?:"..."}
POST /v1/auth/login     {"email":"user@example.com","password":"..."}
GET  /v1/auth/me
POST /v1/auth/logout
GET  /v1/auth/providers
```

Registration normalizes email to lowercase, hashes passwords with Argon2id (`m=19456`, `t=2`,
`p=1`), creates the commercial user, queues a provider-neutral `verify_email` outbox job and
provisions one Rust engine account. An engine provisioning failure leaves an explicit `error` state;
it never creates an unowned checkout or silently shares another account.

Without an invitation, registration creates a B2C Starter profile at 60% off. A valid B2B token is
single-use, bound to the normalized registration email, expires, and is consumed in the same
transaction as the user. Only its SHA-256 hash is stored. B2B accounts receive the invitation's
manual price and do not participate in progressive B2C tiers. See `PRICING.md`.

Login failures use the same external response for an unknown email and a wrong password. A dummy
Argon2 verification reduces timing-based email discovery. Set `REQUIRE_VERIFIED_EMAIL=true` after
the email delivery worker is connected. Production configuration requires this flag. When enabled,
registration creates no session and login remains blocked until verification completes.

Login and registration limits are enforced in PostgreSQL by hashed email/IP buckets, so they work
across multiple API instances without storing raw emails in the rate-limit table. A successful login
clears its buckets.

## Sessions

- The browser receives a random 256-bit opaque token. It contains no user data or authorization.
- PostgreSQL stores only `SHA-256(token)`, the owning user, expiry and revocation state.
- Production cookie: `__Host-apitoken_session; Path=/; Secure; HttpOnly; SameSite=Lax`.
- The cookie has no `Domain`, so it is host-only to `api.apitoken.sale`.
- Login and registration always issue a fresh session; logout revokes only that exact session.
- Private responses use `Cache-Control: no-store`; logout also sends `Clear-Site-Data`.
- All unsafe browser requests must have `Origin` exactly equal to `PUBLIC_APP_BASE_URL`.
- The Cryptomus webhook is the only current origin-check exemption and authenticates by its own
  signature plus authoritative provider lookup.

The frontend must call the API with credentials enabled. CORS accepts only the configured app
origin and allows credentials.

## Authorization invariant

Controllers never accept `userId` from request bodies, query parameters, URL ownership parameters
or custom user headers. `SessionAuthGuard` resolves the current user from the server-side session.
Repositories still include `user_id` in every private lookup as defense in depth. Therefore knowing
another checkout UUID is insufficient to read or manipulate it.

Background workers use narrow internal database jobs, not browser sessions. Payment-provider
webhooks can change only the checkout encoded when the invoice was created and independently
verified through the provider API.

## Future email infrastructure

`apps/api/src/auth-providers.ts` defines `EmailDeliveryProvider`. PostgreSQL `email_outbox` stores
provider-neutral jobs; `auth_tokens` stores hashed, expiring, single-use verification/reset tokens.
The future email worker will claim an outbox job, mint the one-time link, send it through SMTP or a
transactional provider, and record provider delivery identity. Raw one-time tokens must never be
stored in `auth_tokens` or logs.

## Future Google OpenID Connect

Optional configuration is all-or-none:

```text
GOOGLE_CLIENT_ID
GOOGLE_CLIENT_SECRET
GOOGLE_REDIRECT_URI=https://api.apitoken.sale/v1/auth/google/callback
```

`GoogleIdentityProvider` is the adapter boundary and `auth_identities` stores the immutable Google
`sub` under a unique `(provider, subject)` constraint. The implementation must verify authorization
`state`, OIDC `nonce`, signature, issuer, audience and redirect URI.

Never identify or automatically link a Google account by email alone. A verified Google identity
whose email already belongs to a password account must be linked only from an authenticated account
settings flow (or after separate proof of ownership). This prevents account takeover through unsafe
email-based identity merging.

## Remaining production work

1. Connect an email provider/worker and expose verification and password-reset completion routes.
2. Implement the Google adapter and state/nonce transaction store using the existing identity table.
3. Add trusted-edge rate limiting as defense in depth around the PostgreSQL limits.
4. Configure HTTPS and trusted proxy/IP handling before relying on recorded client IPs.
5. Add session management UI (list devices, revoke all sessions, password change and reauthentication).
