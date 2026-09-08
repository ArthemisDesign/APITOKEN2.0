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
- Production environment Branch Tracking: branch is `master`
- Custom pre-production environment Branch Tracking: branch starts with `preview/`
- Standard Preview environment Branch Tracking: disabled, so unassigned task branches do not deploy

Frontend tasks use a unique `preview/<task-slug>` branch from worktree creation. Once a verified
frontend commit is pushed, Vercel creates the corresponding pre-production deployment. The agent must
send its exact URL and review focus to the person and wait for approval before merging to `master`;
never reuse a shared `staging` branch. Non-frontend branches keep their normal prefixes and do not
create Vercel deployments.

Optional `GOOGLE_SITE_VERIFICATION`, `YANDEX_SITE_VERIFICATION`, and `BING_SITE_VERIFICATION`
environment variables add the corresponding webmaster ownership meta tags during the production build.
After deployment, submit `https://apitoken.sale/sitemap.xml` in each webmaster console.

The repository-level `pnpm-lock.yaml` is the dependency lock. Keep `apitoken.sale` as the canonical
origin because the backend CORS and mutation-origin checks intentionally allow one exact frontend
origin.

The root short-referral gateway accepts only seven-character lowercase codes matching
`[0-9][a-z0-9]{6}` (for example, `https://apitoken.sale/3kgj45g`). `src/proxy.ts` resolves such a
code server-to-server through the standalone CRM's public `/r/:code` tracker and returns a `303`
only when CRM supplies an attributed destination on the exact `https://apitoken.sale/` origin.
Ordinary named pages never call CRM; upstream errors, bare landing redirects, malformed locations,
and off-origin redirects fail closed as an uncached `404`. The browser never sees the CRM wrapper
origin and no browser cookie or authorization header is forwarded to CRM.

`vercel.json` runs `scripts/vercel-ignore-build.sh` before allocating a frontend build. The script
compares the current checkout with `VERCEL_GIT_PREVIOUS_SHA` across `apps/web` and the root lockfile,
workspace definition, Node version, and workspace manifest. Because Vercel clones only a short Git
history, it fetches an
otherwise-missing previous commit by exact SHA. It skips only after a conclusive unchanged diff;
missing variables, invalid or unfetchable commits, changes, and comparison errors all fail closed to
a normal build instead of failing the deployment. Verify the contract with
`bash apps/web/scripts/vercel-ignore-build.test.sh` from the repository root.

## Visual audit

Landing, static docs and B2B navigation use equal-width language segments. Theme controls retain the
original desktop symbols on every viewport: sun means the current light theme, moon means the current
dark theme. U+FE0E and `font-variant-emoji:text` enforce monochrome text presentation on phones;
do not replace these with a separate mobile icon set or invert their state mapping. Accessible labels
describe the action. `theme:v1` is authoritative across app and landing, with `apitoken-theme` mirrored
for existing pages. Mobile landing headers keep a stable 68px content row plus the device safe-area inset,
with an opaque background. On the two home landings, the unscrolled header, hero and top canvas are
the same coral surface; the scrolled header is opaque paper/ink. Browser `theme-color` follows the
header and the selected site theme. Do not introduce a separate paper strip above the hero.
The hero does not wait for the desktop entrance animation on touch devices.
Run `scripts/test-mobile-chrome.mjs` to verify header position/scroll stability, language-thumb geometry,
theme persistence, mobile menus and locale navigation across the six static pages in both themes.
It is read-only and accepts `SITE_URL`, `PLAYWRIGHT_MODULE`, `CHROME_PATH`, `AUDIT_OUTPUT`, optional
`QUICK=1` (320/390px only), and `BROWSER=webkit` when that Playwright browser is installed.
The cross-page theme checks always cover 320/390/1440px, stale landing storage, toggling in the app,
return navigation and reloads; `THEME_ONLY=1` selects just those checks. Dashboard checks need preview fixtures.
Browser-engine emulation does not replace checking Safari's physical-device safe-area behavior.

The Referral workspace follows the Usage visual system: separate coral/inverted/neutral KPI cards,
rounded provider tiles, a coral earnings chart with an inverted summary, and sentence-case headings.
The chart palette is shared through `DASHBOARD_CHART_COLORS`; provider logos retain their brand colors.
Referral-only presentation lives in `src/app/dashboard/sections/referral.css` and must not alter
Usage or the partner API/commission/payout contracts. All five tabs, access states and forms remain.
Use `scripts/test-referral-style.mjs` against a local preview-fixture build to compare surfaces with
Usage and check all tabs at 320/390/768/1024/1440 px in EN/RU and light/dark themes. Configure
`SITE_URL`, `PLAYWRIGHT_MODULE` and `CHROME_PATH` as described in `VISUAL_AUDIT.md`; optional `QUICK=1`
limits the suite to desktop and phone. Results and screenshots go to `.artifacts/referral-review`
or `AUDIT_OUTPUT`. The suite checks tab routing, search, chart keyboard tooltips, field sizing,
and page/dialog overflow. It opens and dismisses edit dialogs without submitting changes.

The complete workflow, capture matrix, fixture conventions, assertion patterns, and troubleshooting
guide are documented in [`VISUAL_AUDIT.md`](VISUAL_AUDIT.md).

Build or run the site on port 3001, then use the repository's Chrome DevTools Protocol capture tool.
It injects deterministic dashboard API fixtures, waits for fonts and animation frames, captures the
full CSS-pixel page, and writes a JSON manifest next to the PNG files.

The language gate observes `document.documentElement.lang` when the first body content is parsed,
before hydration effects. Localized routes use the synchronous bootstrap in the root `<head>` so
the shared shell stays statically generated and CDN-cacheable; the gate must not require a
request-bound `headers()` read that would silently turn every page into per-request SSR.

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
- authoritative balance, reserved amount, spend, flat B2C pricing and B2B pricing;
- rolling usage split by provider, model, token bucket and API key, with provider-stacked UTC-day bars;
- API-key listing, one-time secret issuance, and revocation;
- engine ledger;
- account-bound Referral workspace with unavailable/disabled/active states, email-only Team and
  customer identity, retained-share controls, commission/B2B requests, wallet/payout history and a
  Usage-style provider-stacked earnings chart;
- arbitrary positive whole-USD Cryptomus checkout creation.

An account without partner membership sees the standard terms and an explicit contact link to
`https://t.me/bozinodev`; opening the section does not create a membership or public application.
Telegram authentication, password change, session management, and TOTP remain visible only as
non-interactive future areas. Promo-code issuance and redemption are retired and have no dashboard
surface; old promo URLs fall back to Overview. The frontend does not simulate removed capabilities
or store fake server data in browser storage. The order workspace is intentionally not exposed.
