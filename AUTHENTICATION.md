# Authentication and authorization

## Implemented email/password flow

```text
POST /v1/auth/register  {"email":"user@example.com","password":"at least 12 characters","inviteToken"?:"..."}
POST /v1/auth/login     {"email":"user@example.com","password":"..."}
POST /v1/auth/email/verify      {"token":"..."}
POST /v1/auth/email/resend      {"email":"user@example.com"}
POST /v1/auth/password/forgot   {"email":"user@example.com"}
POST /v1/auth/password/reset    {"token":"...","password":"..."}
GET  /v1/auth/google            (redirect; optional `?invite=...`)
GET  /v1/auth/github            (redirect; optional `?invite=...`)
GET  /v1/auth/me
POST /v1/auth/logout
GET  /v1/auth/providers
```

Registration normalizes email to lowercase, hashes passwords with Argon2id (`m=19456`, `t=2`,
`p=1`), creates the commercial user and atomically queues a provider-neutral verification job.
It does not provision a Rust engine account or create a session until verification succeeds.

Without an invitation, registration creates a B2C Starter profile at 60% off. A valid B2B token is
single-use, bound to the normalized registration email, expires, and is consumed in the same
transaction as the user. Only its SHA-256 hash is stored. B2B accounts receive the invitation's
manual price and do not participate in progressive B2C tiers. See `PRICING.md`.

Login failures use the same external response for an unknown email and a wrong password. A dummy
Argon2 verification reduces timing-based email discovery. Password login is always blocked until
verification completes. Forgot-password and resend responses do not reveal whether an account exists.

Login and registration limits are enforced in PostgreSQL by hashed email/IP buckets, so they work
across multiple API instances without storing raw emails in the rate-limit table. A successful login
clears its buckets.

## Sessions

- The browser receives a random 256-bit opaque token. It contains no user data or authorization.
- PostgreSQL stores only `SHA-256(token)`, the owning user, expiry and revocation state.
- Production cookie: `__Host-apitoken_session; Path=/; Secure; HttpOnly; SameSite=Lax`.
- The cookie has no `Domain`, so it is host-only to `backend.apitoken.sale`.
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

## Email delivery

PostgreSQL `email_outbox` stores durable jobs; `auth_tokens` stores hashed, expiring, single-use
verification/reset tokens. Raw tokens are AES-256-GCM encrypted in the outbox and decrypted only in
the worker process. SMTP jobs use leases, exponential retry and provider message IDs. Full setup is
in `EMAIL_INTEGRATION.md`.

## Google and GitHub authentication

Optional configuration is all-or-none:

```text
GOOGLE_CLIENT_ID
GOOGLE_CLIENT_SECRET
GOOGLE_REDIRECT_URI=https://backend.apitoken.sale/v1/auth/google/callback
GITHUB_CLIENT_ID
GITHUB_CLIENT_SECRET
GITHUB_REDIRECT_URI=https://backend.apitoken.sale/v1/auth/github/callback
```

Google uses authorization code + PKCE and verifies state, browser binding, nonce, ID-token signature,
issuer, audience and `email_verified`. GitHub uses authorization code + PKCE with the minimal
`user:email` scope, then reads `/user/emails` and accepts only a provider-verified address. Provider
tokens are not retained. Both providers mark the local email verified and therefore skip our email.
The immutable Google `sub` or GitHub numeric user ID is the identity key, never the email address.

Never identify or automatically link a Google account by email alone. A verified Google identity
whose email already belongs to a password account must be linked only from an authenticated account
settings flow (or after separate proof of ownership). This prevents account takeover through unsafe
email-based identity merging.

## Remaining production work

1. Deploy the self-hosted SMTP server and install its credentials in the worker.
2. Register Google and GitHub applications with the documented callback URLs.
3. Add an authenticated provider-linking flow for emails that already own an account.
4. Add trusted-edge rate limiting and trusted proxy/IP handling.
5. Add session management UI (list devices, revoke all sessions, password change and reauthentication).
