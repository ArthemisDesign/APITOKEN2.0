# Frontend visual QA

The website has a dependency-free screenshot audit powered by the Chrome DevTools protocol. It uses the Chrome/Chromium already installed on the machine, waits for local fonts, makes every reveal-animation target visible, and captures the complete rendered page rather than only the viewport.

## Run it

Start the frontend in one terminal:

```bash
pnpm --filter @claude-api/web dev -- --port 3001
```

Capture the public routes in another terminal:

```bash
pnpm --filter @claude-api/web audit:screenshots
```

The PNG files and `manifest.json` are written to `apps/web/.artifacts/site-audit/` and are intentionally ignored by Git. The audit includes desktop, mobile, light, dark, and a Russian-language home-page pass so translated text wrapping is visible before release.

Optional environment variables:

```bash
SITE_URL=http://localhost:3001 \
SCREENSHOT_DIR=/tmp/apitoken-site-audit \
CHROME_PATH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
pnpm --filter @claude-api/web audit:screenshots
```

The default audit covers the complete homepage at desktop and mobile sizes, the plans page at both sizes, and every distinct public page template: models, docs, integrations, integration guide, login, registration, terms, and privacy. Dark-mode captures are produced for the homepage, plans, models, docs, and registration—the main text and component templates used throughout the site.

## Review checklist

- Check every image in light mode and spot-check the homepage, pricing, models, and authentication flows in dark mode.
- Verify that text remains readable, cards align, and translated copy does not move fixed navigation controls.
- Review desktop and mobile pricing tables for clipping or misleading duplicated copy.
- Re-run `lint`, `typecheck`, `test`, and `build` after visual changes.
