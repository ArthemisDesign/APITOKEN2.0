# Verify the web app

Use this for runtime verification of `apps/web` UI changes.

1. Build and launch the production app from the repository root:

   ```bash
   pnpm --filter @claude-api/web build
   pnpm --filter @claude-api/web exec next start -p 3001
   ```

2. Wait for `http://127.0.0.1:3001` to respond.
3. Drive the affected routes with the repository CDP harness in `apps/web/scripts/capture-site.mjs`. Set `AUDIT_SCOPE=site`, `dashboard`, or `all` to match the filtered routes; use a fresh `SCREENSHOT_DIR` and a narrow `AUDIT_FILTER`. Matching Credits/docs verifiers run automatically unless explicitly disabled with `AUDIT_VERIFY_CREDITS=0` or `AUDIT_VERIFY_DOCS_THEME=0`.
4. Inspect every generated PNG, not only the script exit status. For responsive work, include desktop, tablet, and 390px mobile captures in both themes where relevant.
5. Stop the production server after evidence is captured.

The dashboard audit supplies deterministic browser-side API fixtures and does not authenticate against or mutate production data. Build may rewrite the generated `apps/web/next-env.d.ts`; never stage that churn unless the task explicitly requires it.
