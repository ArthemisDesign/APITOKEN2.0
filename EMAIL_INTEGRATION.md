# Transactional email

The authentication flow is complete without being coupled to an email vendor. `apps/api` creates
hashed, expiring, single-use verification/reset tokens and writes a durable outbox job in the same
transaction. The raw token is AES-256-GCM encrypted with `AUTH_TOKEN_ENCRYPTION_KEY`; it is never
stored in `auth_tokens` or written to logs. `apps/worker` claims jobs with `FOR UPDATE SKIP LOCKED`,
decrypts the token only in memory, renders text/HTML, sends through SMTP, and retries failures.

Email-bound B2B invitations use the same encrypted-token outbox. The invitation row owns the job
before a user exists; its email includes the negotiated discount, expiry, and `/register?invite=`
link. Revoking or rotating an invitation cancels any unsent old job. An invitation created without
an email does not enqueue mail and is returned to the admin panel as a copy-only link.

## Client routes

```text
POST /v1/auth/register          {email,password,inviteToken?} -> verificationRequired:true
POST /v1/auth/email/verify      {token} -> session cookie
POST /v1/auth/email/resend      {email} -> always accepted when well-formed
POST /v1/auth/password/forgot   {email} -> always accepted when well-formed
POST /v1/auth/password/reset    {token,password}
POST /v1/auth/business-invite/preview {token} -> validity, discount, expiry, masked recipient
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
SMTP_HOST=smtp-relay.brevo.com
SMTP_PORT=587
SMTP_SECURE=false
SMTP_USERNAME=<Brevo SMTP login>
SMTP_PASSWORD=<Brevo SMTP key>
PUBLIC_APP_BASE_URL=https://apitoken.sale
```

Port 587 starts as a plain SMTP connection and upgrades with STARTTLS. In production, the worker
sets Nodemailer's `requireTLS` option and refuses to authenticate or deliver if that upgrade fails.
`SMTP_SECURE=true` remains supported for providers using implicit TLS on port 465. Store the SMTP
key only in the root-owned worker environment file; never commit it.

Use `EMAIL_DELIVERY_MODE=disabled` during development when no SMTP server is available; jobs remain
queued. A local SMTP capture server can be used with `SMTP_SECURE=false`.

## Sender-domain requirements

Verify the sending domain with the SMTP provider and publish its SPF and DKIM records. Merge the
provider into the existing SPF record instead of publishing a second SPF record, and add DMARC.
Do not replace inbound MX records when the existing receiving service should remain active. Keep
transactional mail separate from marketing traffic. Verification links target `/verify-email`;
reset links target `/reset-password` on `PUBLIC_APP_BASE_URL`.
