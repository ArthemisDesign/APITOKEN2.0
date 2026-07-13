# Transactional email

The authentication flow is complete without being coupled to an email vendor. `apps/api` creates
hashed, expiring, single-use verification/reset tokens and writes a durable outbox job in the same
transaction. The raw token is AES-256-GCM encrypted with `AUTH_TOKEN_ENCRYPTION_KEY`; it is never
stored in `auth_tokens` or written to logs. `apps/worker` claims jobs with `FOR UPDATE SKIP LOCKED`,
decrypts the token only in memory, renders text/HTML, sends through SMTP, and retries failures.

## Client routes

```text
POST /v1/auth/register          {email,password,inviteToken?} -> verificationRequired:true
POST /v1/auth/email/verify      {token} -> session cookie
POST /v1/auth/email/resend      {email} -> always accepted when well-formed
POST /v1/auth/password/forgot   {email} -> always accepted when well-formed
POST /v1/auth/password/reset    {token,password}
```

Password registration does not create a Rust engine account until verification succeeds. Resetting
a password proves control of the email, marks it verified, consumes every outstanding reset token,
and revokes all existing sessions.

## Application configuration

Generate the shared encryption key once and install the same value in API and worker environments:

```bash
openssl rand -base64 32 | tr '+/' '-_' | tr -d '='
```

Worker SMTP configuration:

```text
EMAIL_DELIVERY_MODE=smtp
EMAIL_FROM=no-reply@apitoken.sale
SMTP_HOST=mail.apitoken.sale
SMTP_PORT=465
SMTP_SECURE=true
SMTP_USERNAME=no-reply@apitoken.sale
SMTP_PASSWORD=<secret>
PUBLIC_APP_BASE_URL=https://apitoken.sale
```

Use `EMAIL_DELIVERY_MODE=disabled` during development when no SMTP server is available; jobs remain
queued. A local SMTP capture server can be used with `SMTP_SECURE=false`.

## Self-hosted SMTP requirements

For reliable delivery, use a dedicated hostname/IP and configure forward DNS, matching PTR/reverse
DNS, SPF, 2048-bit DKIM, DMARC, and TLS before production. Keep transactional mail separate from
marketing traffic. Verification links target `/verify-email`; reset links target `/reset-password`
on `PUBLIC_APP_BASE_URL`.
