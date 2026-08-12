# @claude-api/admin — apiToken.sale operations admin panel

Next.js 16 / React 19 (App Router), port 3700. Replacement for the single-file panel
`crates/server/src/admin-panel.html` + `admin-panel.js` — same visual style,
same endpoints, same Russian labels.

## Security model

No secrets, and there never will be: the browser uses same-origin relative paths
(`/overview`, `/admin/*`, `/openkeys-admin/*`, `/partner-admin/*`); authentication
(forward_auth) and server-side keys are injected by Caddy. No env files and no
`NEXT_PUBLIC_*` keys. All data loading happens in client components.

## Structure

- `src/lib/api.ts` — `api<T>(path, opts)` / `send<T>(path, method, body)`: typed fetch
  for same-origin JSON, `ApiError` with the status and the message from the body.
- `src/lib/resources.ts` — URL-keyed external request cache: exact-URL in-flight deduplication,
  retained last-good data (also while a filter/window URL changes), orphan request cancellation
  and targeted mounted-only revalidation.
  `src/lib/realtime.ts` owns one `EventSource` per producer feed and applies only `change`/`resync`
  resource prefixes; heartbeat cannot initiate a request. Multi-source screens use independent
  URL resources, so their ready sections render without waiting for the slowest endpoint. The sidebar ↻ explicitly
  refreshes only resources on the current screen. An unmounted cohort never performs a hidden
  request; its stale error card is removed on unsubscribe. Source
  error registry: `subscribeErrors(listener)` / `getErrors()`
  (`ResourceError { key, message, dismissed }`), `dismissError(key)`.
- `src/lib/toast.ts` — `toast(message, kind?)` (`kind: "ok" | "bad"`, defaults to
  `"ok"`; bad lives 9 s and has a ×, ok — 5 s) + `<Toaster/>` (mounted in the layout).
- `src/lib/dialog.tsx` — `dialog(options): Promise<Record<string,string> | null>`,
  a promise-based replacement for prompt/confirm; `options: { title, message?, fields?: [{ name,
  label, type?, value? }], confirmLabel?, danger? }`. null means cancelled (Esc/overlay/
  "Отмена" — Cancel button), Enter submits. `<DialogHost/>` is mounted in the layout.
- `src/lib/csv.ts` — `downloadCsv(filename, header, rows)` (`;`, RFC 4180, BOM),
  `buildCsv`/`csvCell` for tests, `csvDate()` → `YYYY-MM-DD` for the file name.
- `src/lib/sources.ts` — `sourceName(path)`: API path → Russian source label
  (map from admin-panel.js; query is stripped, unknown path returned as-is).
- `src/lib/format.ts` — formatters 1:1 from `admin-panel.js`: `nanoMoney` (integer
  nanoUSD strings via BigInt — the only way to display money), `money`
  (commerce legacy fields in dollars, display only), `formatDate`, `ago`,
  `duration`, `ageText`, `ratio`, `plural`, `count`, `windowLabel`.
- `src/lib/nav.ts` — `NAV` (sidebar source of truth), `isNavItemActive`.
- `src/lib/theme.ts` — `THEME_STORAGE_KEY` (`apitoken-admin-theme:v1`), `toggleTheme`.
- `src/lib/types.ts` — backend payload types (all fields optional).
- `src/components/ui.tsx` — `PageHead`, `SectionHeader`, `CardGrid`, `StatCard`,
  `Banner`, `Dot`, `Pill`, `TableCard`, `EmptyRow`, `LoadingGrid`, `Modal`
  (Esc/overlay close, Tab trap, focus restore; `wide` for wide modals).
- `src/components/sidebar.tsx` — sidebar with navigation, realtime health, current-screen refresh, and theme.
- `src/components/error-center.tsx` — `<ErrorCenter/>` (mounted in the layout):
  red cards for failing sources with ↻/×, reads the shared request-cache error registry.
- `src/components/spend-stats-modal.tsx` — the "Кто тратит" ("Who is spending") modal (`/spend-stats`,
  24h/7d/30d windows + arbitrary range, charged vs real-API and OpenKeys summary).
  Wiring: `const { openSpendStats, spendStatsModal } = useSpendStatsModal()`,
  `openSpendStats` — into `StatCard.onClick`/`onClick` of the "потрачено" ("spent") header,
  `{spendStatsModal}` — at the end of the page. The `SpendStatsResponse`, `SpendPeriod`
  types and the `isOpenkeys` helper are exported.
- `src/app/page.tsx` — Overview (the reference page; port the others following it).
- `src/app/business` — B2B invitations and scalar commercial terms. The client table shows one
  row per customer, the default discount, engine-account state and the aggregate delivery state of
  the complete default-plus-provider bundle. The editor presents the default separately from
  provider overrides; an empty provider value means inheritance, not a zero discount. The clients
  and invitations are separate operator sections, while conversion from the Users page uses a
  dedicated B2B dialog that explains the new base term before submission.
- `src/app/users` — the general customer table renders the exact persisted scalar for both B2C and
  B2B. B2C is still one flat price, but historical/dormant `4000` rows must remain visible as a 60%
  discount; the UI and CSV never substitute today's common `5000` value for a stored condition.
- `src/app/partners` — partner accounting and payout operations. The readiness block consumes the
  additive Sales chain proof and shows the public hot-wallet address, exact USDT nanoUSD and BNB
  wei balances, current eligible requirements and payout window. Missing/malformed chain evidence
  is unavailable, never a fabricated zero; the page remains read-only.
- `src/app/paying-users/page.tsx` — one read-only control room with independently filtered
  `Клиенты` and `OpenKeys` cohorts. Only the active cohort mounts its realtime request. Commerce
  defaults to `funding=spenders`, retains the selected funding filter and always sends
  `include_usage=true`. The default contains every positive selected-window spender, including
  mixed/legacy/unattributed rows, with `spend_only` explicitly distinct from strict
  `bonus_only`. Its ledger separates all-spender window spend from lifetime money revenue and strict
  bonus-only (not revenue). Commerce rows expand into producer-authored provider/model usage with
  `complete|partial|unavailable` coverage; partial totals cover only available accounts and unavailable
  never means zero. Commerce CSV emits one row per user × provider × model, or one status row when
  models are empty/unavailable, preserving exact counter/nanoUSD strings as spreadsheet text and
  formula-safing untrusted text. OpenKeys uses same-origin `/openkeys-admin/paying-keys`, lists every
  non-removed warehouse and delivered key with an explicit lifecycle, shows lifetime spend separately
  from selected-window usage, and provides global server sorting by spend/nominal/dates/status in both
  directions. Exact local wire types and the same expandable model-usage pattern are mandatory.
- `src/app/subscriptions/codex-capacity-board.tsx` — compact GPT summary of shared-plan capacity,
  native-credit/API-$ windows, and masked-email homes. Raw calibration, token-capacity, and
  profitability matrices are intentionally not surfaced in the operator UI.
- `src/app/subscriptions/claude-capacity-board.tsx` — masked-email subscription windows only: state,
  quota/reset, and exact API-$ for 5h/7d. Backend evidence and the tariff catalog are not duplicated
  in the tables.
- `src/app/subscriptions/gemini-capacity-board.tsx` — masked-email profiles with provider quota/reset and
  exact workload API-$ for 5h/7d. Under degraded authority the quota stays visible, saleable money
  shows `обновляем` ("updating"); profiles outside rotation are excluded from the fleet total.
- `src/app/proxies` — bounded proxy inventory and unchanged explicit/idempotent renewal flow. The table
  renders the full producer-validated `account_email` as the sole identity exception and searches it,
  but fail-closed removes every `dead` or non-`bound` row. Subscription and proxy expiries at or before
  the exact 72-hour boundary are marked independently from `inventory.observed_at` (or browser time
  when invalid); null expiry is not marked. Credentials, proxy URL/IP and every other identity remain
  forbidden and nothing is persisted by the UI.
- `src/app/api/health/route.ts` — `GET /api/health` → `{"ok":true}` for the watchdog.

## Page conventions

1. A page is `'use client'`; one source uses `useResource<T>(actualUrl)`, while a multi-source
   screen uses `useResources<T>({ section: actualUrl, … })`. Do not join independent page reads
   behind `Promise.all`: each section must become visible and degradable on its own. Never add
   `setInterval`, focus/visibility revalidation or a
   heartbeat handler: producer `change`/`resync` events are the automatic update authority.
2. Russian labels — verbatim from `admin-panel.js`.
3. Money — only `nanoMoney` over integer strings; JS number for amounts is forbidden.
4. While there is no data (`data === undefined`) — `PageHead` + `LoadingGrid`.
5. Memoize heavy tables (`React.memo`/`useMemo`); hoist static JSX out of
   page components.
6. Show action errors via `toast(..., "bad")`, success via `toast(...)`;
   confirmations and input — via `dialog()` (not `window.confirm/prompt`).
7. "Потрачено" ("Spent") in account tables is clickable: `onClick={openSpendStats}`
   from `useSpendStatsModal()` + `title="Разбивка: сутки / 7 дней / 30 дней"`.
8. Table export — `downloadCsv(filename, header, rows)`, file name with a date
   via `csvDate()` (e.g. `users-2026-07-31.csv`).
9. The `/paying-users` page uses only exact decimal strings from
   `/admin/finance/paying-users`; `bonus_only`/`spend_only` classification comes only from
   `funding_kind`, and provider/funding/usage amounts or counters must not be reconstructed from zero
   totals, float USD, model names or the top-50 `/spend-stats`.

## Commands

```bash
pnpm dev          # next dev -p 3700
pnpm build
pnpm start        # next start -H 127.0.0.1 -p 3700
pnpm typecheck
pnpm test         # vitest run
```
