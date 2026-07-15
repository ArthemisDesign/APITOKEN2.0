# apiToken.sale web frontend

Next.js App Router frontend for the independently deployed commercial customer UI. It talks to the
NestJS API at `backend.apitoken.sale`; it never calls the engine Control API and never receives the
engine control key.

## Local development

```bash
cp apps/web/.env.example apps/web/.env.local
pnpm --filter @claude-api/web dev
```

For a local commercial API, set `NEXT_PUBLIC_BACKEND_URL=http://127.0.0.1:3000/v1` and configure the
API's `PUBLIC_APP_BASE_URL` to the exact frontend origin. Browser requests include the API's secure
session cookie with `credentials: include`.

## Vercel

Import the monorepo as a Vercel project with:

- Root Directory: `apps/web`
- Framework Preset: Next.js
- Environment: `NEXT_PUBLIC_BACKEND_URL=https://backend.apitoken.sale/v1`
- Optional documentation host: `NEXT_PUBLIC_DOCS_URL=https://docs.apitoken.sale` (defaults to the standalone `/docs` portal)
- Production domain: `apitoken.sale`

The repository-level `pnpm-lock.yaml` is the dependency lock. Keep `apitoken.sale` as the canonical
origin because the backend CORS and mutation-origin checks intentionally allow one exact frontend
origin.

## Visual audit

Build or run the site on port 3001, then use the repository's Chrome DevTools Protocol capture tool.
It injects deterministic dashboard API fixtures, waits for fonts and animation frames, captures the
full CSS-pixel page, and writes a JSON manifest next to the PNG files.

```bash
pnpm --filter @claude-api/web build
pnpm --filter @claude-api/web exec next start -p 3001
AUDIT_SCOPE=all node apps/web/scripts/capture-site.mjs
```

For a focused review, select named captures without changing the script:

```bash
AUDIT_SCOPE=dashboard \
AUDIT_FILTER=dashboard-overview-russian,dashboard-security-dark \
SCREENSHOT_DIR=.artifacts/focused-audit \
node apps/web/scripts/capture-site.mjs
```

To regression-test a hard-reloaded dashboard subview returning directly to Overview, add
`AUDIT_VERIFY_ROUTING=1` to a dashboard audit command.

## Implemented customer capabilities

- email/password registration, verification, resend, login, logout, forgot/reset password;
- Google and GitHub OAuth when the backend reports those providers enabled;
- authoritative balance, reserved amount, spend, B2C progress and B2B pricing;
- API-key listing, one-time secret issuance, and revocation;
- engine ledger;
- arbitrary positive whole-USD Cryptomus checkout creation.

Referral rewards, promo codes, Telegram, rich analytics, checkout history, editable profiles, password
change, session management, and TOTP remain visible only as non-interactive future areas. The frontend
does not simulate them or store fake server data in browser storage.
