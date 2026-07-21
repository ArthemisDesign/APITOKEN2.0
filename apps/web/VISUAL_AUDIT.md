# Web visual audit

The web app has a deterministic Chrome-based visual audit in
[`scripts/capture-site.mjs`](scripts/capture-site.mjs). It is the preferred way to review responsive,
theme, and localization changes before they are pushed.

The audit does two jobs:

1. It captures full-page PNG screenshots at explicit viewport, theme, language, and route
   combinations.
2. It can run browser-level assertions for layout, routing, persistence, and interactions.

Dashboard requests are intercepted in the browser and answered with deterministic fixtures, so the
audit does not need a running API, a real account, or production data. The site itself must be running.

## Requirements

- Install the workspace dependencies with `pnpm install`.
- Use a Chromium browser. The script searches, in order:
  - `CHROME_PATH`;
  - Google Chrome on macOS;
  - Chromium on macOS;
  - `google-chrome` or `chromium` in the standard Linux locations.
- Run commands from the repository root unless a command says otherwise.

The capture tool uses Chrome DevTools Protocol directly. It does not require Playwright, Selenium, or
a browser extension.

## Recommended production-build audit

Build and start the exact optimized app that will be deployed:

```bash
pnpm --filter @claude-api/web build
pnpm --filter @claude-api/web exec next start -p 3001
```

Leave that process running. In a second terminal, run an audit:

```bash
AUDIT_SCOPE=all node apps/web/scripts/capture-site.mjs
```

By default the site URL is `http://localhost:3001` and artifacts are written to
`.artifacts/site-audit/` at the repository root.

Development mode can be used for a quick iteration, but the final audit should use a production build:

```bash
pnpm --filter @claude-api/web dev -- --port 3001
```

## Common commands

Capture all public-site states:

```bash
pnpm --filter @claude-api/web audit:screenshots
```

Capture all dashboard states:

```bash
pnpm --filter @claude-api/web audit:dashboard
```

Capture the public site and dashboard together:

```bash
AUDIT_SCOPE=all node apps/web/scripts/capture-site.mjs
```

Capture named states only:

```bash
AUDIT_SCOPE=dashboard \
AUDIT_FILTER=dashboard-keys-light,dashboard-keys-mobile-russian-dark \
SCREENSHOT_DIR=.artifacts/api-keys-review \
node apps/web/scripts/capture-site.mjs
```

Audit a server running on another local port or a deployed preview:

```bash
SITE_URL=http://localhost:3100 \
AUDIT_SCOPE=dashboard \
node apps/web/scripts/capture-site.mjs
```

Do not run the dashboard audit against production unless using the current fixture interception. The
fixture prevents dashboard browser requests from reaching the configured API.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SITE_URL` | `http://localhost:3001` | Origin to navigate to. |
| `SCREENSHOT_DIR` | `.artifacts/site-audit` | Directory for PNG files, `manifest.json`, and the temporary Chrome profile. |
| `AUDIT_SCOPE` | `site` | Capture group: `site`, `dashboard`, or `all`. |
| `AUDIT_FILTER` | empty | Comma-separated capture names. Empty means every capture in the selected scope. |
| `CHROME_PATH` | auto-detected | Explicit Chrome or Chromium executable. |
| `AUDIT_VERIFY_KEYS` | automatic | `1` forces API-key assertions; `0` disables them. They run automatically when a selected capture starts with `dashboard-keys-`. |
| `AUDIT_VERIFY_CREDITS` | automatic | `1` forces Credits assertions; `0` disables them. They run automatically when a selected capture starts with `dashboard-topup-`. |
| `AUDIT_VERIFY_DOCS_THEME` | automatic | `1` forces docs theme assertions; `0` disables them. They run automatically when a selected capture starts with `docs-`. |
| `AUDIT_VERIFY_HERO` | automatic | `1` forces landing hero-offer assertions; `0` disables them. They run automatically when a selected capture starts with `home-`. |
| `AUDIT_VERIFY_PRICING` | automatic | `1` forces pricing-card assertions; `0` disables them. They run automatically when a selected capture starts with `pricing-cards-`. |
| `AUDIT_VERIFY_ROUTING` | `0` | Tests dashboard subview routing and hard-reload fallback behavior. |
| `AUDIT_VERIFY_PROFILE` | `0` | Tests dashboard profile behavior. |
| `AUDIT_VERIFY_SITE_ROUTING` | `0` | Tests persistent public-site navigation state. |
| `AUDIT_VERIFY_COMPLIANCE` | `0` | Tests compliance-page routing while preserving language, theme, and authentication shell state. |

`AUDIT_FILTER` names must belong to the chosen `AUDIT_SCOPE`. The script fails with a clear error when
no names match, which catches typos and incorrect scope selection.

## Current capture matrix

Capture definitions live near the top of `scripts/capture-site.mjs`. Each definition is a tuple:

```js
[name, route, viewportWidth, viewportHeight, theme, language]
```

`language` is optional and defaults to `en`. Current conventions are:

- desktop: `1440 x 1000`;
- tablet: `768–820 x 1000–1024`;
- mobile: `390 x 844`;
- themes: `light` and `dark`;
- localized states: `en` and `ru` where applicable.

The public-site scope includes home, plans, models, docs, integrations, authentication, terms,
privacy, and support states. The dashboard scope includes overview, API keys, credits, usage, support,
promos, profile, and security states.

The `header-*` matrix covers the public header at 1920px desktop, 1280px laptop, the 1240px
navigation-collapse boundary, 768px tablet, 390px mobile, and 320px narrow mobile widths. It includes
authenticated, Russian, light/dark, and open-menu states so navigation wrapping and clipping can be
reviewed independently of page content.

The landing hero-offer matrix covers English and Russian, light and dark themes, and desktop and
mobile layouts. `verifyHeroOfferLayout()` checks the card hierarchy, compact height, metadata
alignment, vertical rhythm, equal tier-row heights, exact top-up/discount/API values, shared table
columns, clipping, and horizontal overflow. Run that matrix on its own with:

```bash
AUDIT_SCOPE=site \
AUDIT_FILTER=home-desktop,home-dark,home-russian-light,home-russian,home-mobile,home-mobile-dark,home-mobile-russian-light,home-mobile-russian-dark \
AUDIT_VERIFY_HERO=1 \
SCREENSHOT_DIR=.artifacts/hero-offer-review \
node apps/web/scripts/capture-site.mjs
```

The `pricing-cards-*` matrix covers the plans-page top-up and B2B cards in English and Russian, light
and dark themes, and desktop and mobile layouts. Its browser assertions keep both outer cards on the
same surface, prevent the B2B terms panel from becoming an oversized billboard, compare panel and CTA
geometry, and check for clipping or horizontal overflow.

For a feature that must work across devices, languages, and themes, create explicit names for every
required state. The API-key filter audit is the reference pattern:

```text
dashboard-keys-light
dashboard-keys-dark
dashboard-keys-russian-light
dashboard-keys-russian-dark
dashboard-keys-mobile-light
dashboard-keys-mobile-dark
dashboard-keys-mobile-russian-light
dashboard-keys-mobile-russian-dark
```

## What happens during a capture

For each selected state, the script:

1. Sets Chrome device metrics with a CSS-pixel scale of `1`.
2. Stores the requested language and theme in `localStorage`.
3. Navigates with a unique `__audit` query value so repeated routes are really reloaded.
4. Verifies the document language, using the visible language control if hydration has not applied it.
5. Applies the requested theme, waits for fonts, stabilizes reveal animations, and removes Next.js
   development portals.
6. Waits through two animation frames so the final DOM and font metrics are stable.
7. Measures the full CSS page size and captures a full-page PNG.
8. Records the capture metadata in `manifest.json`.

The manifest contains the name, route, theme, language, measured width and height, and PNG filename.
Keep it with the screenshots when an audit result needs to be reviewed later.

## Dashboard fixtures

`dashboardFixtureScript` is injected before any page JavaScript runs. It provides a stable signed-in
user and deterministic responses for dashboard endpoints such as profile, balance, API keys, usage,
pricing, and top-up history.

When a dashboard design depends on new API data:

1. Add the smallest representative fixture to `dashboardFixtureScript`.
2. Match the real endpoint path and response shape.
3. Include enough variation to expose layout problems, such as active and disabled records, long
   translated labels, empty values, or multiple history rows.
4. Keep fixture data obviously synthetic and never paste credentials, production tokens, or customer
   data into the script.
5. Add an interaction assertion when the UI changes state after a click.

The API-key fixture includes enabled, revoked, near-limit, expiring, expired, and limit-reached
records. That lets the audit verify `4 / 1 / 5` filter counts, every policy state, search, the
responsive desktop/tablet/mobile table-card layout, the TOTP-enabled create flow and payload, and the
revoke confirmation/error path.

## Browser assertions

Screenshots require human review, while assertion functions catch repeatable regressions. Add both for
important UI work.

An assertion function should:

- navigate to a fresh URL with a unique audit query parameter;
- wait for a stable, feature-specific selector;
- inspect geometry with `getBoundingClientRect()` and computed styles;
- test horizontal overflow with `scrollWidth - clientWidth`;
- verify translated labels and the active theme;
- exercise real controls with `clickSelector()`;
- wait for the expected state with `waitForCondition()`;
- throw an error containing serialized browser state that explains a failure;
- print one concise success line after all cases pass.

The API-key assertion currently checks nine English/Russian, light/dark, and desktop/tablet/mobile
cases. It verifies alignment, table/card spacing, single-row controls, equal mobile tab widths, no
horizontal overflow, translated accessibility labels, counts, search and filter interactions, all
policy states, the complete TOTP-enabled create flow, and revoke error/focus restoration behavior.

## Adding a new visual audit

1. Add capture tuples to `siteCaptures` or `dashboardCaptures`.
2. Use stable, descriptive names: `area-feature-device-language-theme`.
3. Add or extend deterministic dashboard fixtures when required.
4. Add stable classes or `data-*` attributes to production markup for interaction targets. Prefer
   semantic attributes over text matching.
5. Add a `verifyFeatureName(client)` function for measurable behavior.
6. Decide whether verification should run automatically based on the capture-name prefix or behind a
   dedicated `AUDIT_VERIFY_*` variable.
7. Run the smallest focused command while iterating.
8. Run the complete required matrix from a production build before pushing.
9. Open every PNG and inspect content, spacing, clipping, contrast, translations, and mobile wrapping.
10. Run the normal web checks as well:

```bash
pnpm --filter @claude-api/web typecheck
pnpm --filter @claude-api/web test
pnpm --filter @claude-api/web lint
pnpm --filter @claude-api/web build
```

## Review checklist

For each required screenshot, confirm:

- the correct route and page title are present;
- the selected language is actually rendered, not only encoded in the filename;
- the theme is correct and contrast is readable;
- left and right content rails align;
- vertical spacing is intentional and consistent;
- controls do not look like unrelated cards or content surfaces;
- long Russian labels do not clip or force unwanted rows;
- mobile content has no horizontal scrolling;
- buttons and tabs have clear selected, hover-capable, and disabled states;
- data cards and secondary help panels remain visually distinct;
- no loading state, Next.js overlay, or animation-hidden content is captured.

An audit is complete only when the command exits successfully and every generated image has been
visually inspected. Assertions supplement visual review; they do not replace it.

## Artifacts and repository hygiene

Audit images belong under `.artifacts/` or another ignored temporary directory. Do not commit PNGs,
the generated `manifest.json`, or `.chrome-profile` unless a task explicitly requires checked-in
reference images.

To retain multiple iterations, give each run a separate directory:

```bash
SCREENSHOT_DIR=.artifacts/profile-redesign-final \
AUDIT_SCOPE=dashboard \
AUDIT_FILTER=dashboard-profile-light,dashboard-profile-dark \
node apps/web/scripts/capture-site.mjs
```

Stop the local Next.js server after the review.

## Troubleshooting

### Chrome cannot be found

Set the executable explicitly:

```bash
CHROME_PATH="/path/to/chrome" node apps/web/scripts/capture-site.mjs
```

### The audit opens the wrong port

Set `SITE_URL` to the exact origin used by the running app.

### No screenshots matched

Check that every `AUDIT_FILTER` name exists in the selected `AUDIT_SCOPE`. Capture names are exact and
case-sensitive.

### A screenshot has the wrong language

Keep the unique navigation query behavior in `capturePage()`. It prevents Chrome from reusing a mounted
English page after `localStorage` changes. Also make sure the page renders the shared `.lang` controls
or explicitly handles localization in its audit.

### The dashboard shows a login or loading state

Confirm that the requested path is recognized by `dashboardFixtureScript` and that new API endpoints
are intercepted with the exact path used by the frontend.

### The page is clipped or twice the expected size

Keep `deviceScaleFactor: 1` and use `cssContentSize` from `Page.getLayoutMetrics()`. The fallback
`contentSize` can be reported in physical pixels on Retina displays.

### A verification times out

The feature-specific readiness selector may be stale, the fixture may not match the frontend response,
or a route may have changed. Read the serialized browser state in the thrown error before increasing a
timeout; fixed sleeps should be a last resort.
