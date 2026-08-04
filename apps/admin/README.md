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
- `src/lib/usePoll.ts` — `usePoll(key, fetcher, { interval })`: SWR-like polling
  (deduplication by key, pause on a hidden tab, revalidation on focus,
  stale-while-revalidate). `revalidateAll()` — the ↻ button in the sidebar.
  Source error registry: `subscribeErrors(listener)` / `getErrors()`
  (`PollError { key, message, dismissed }`), `dismissError(key)`,
  `refreshPoller(key)` — appears/clears itself based on the outcome of each fetch.
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
- `src/components/sidebar.tsx` — sidebar with navigation, refresh, and theme.
- `src/components/error-center.tsx` — `<ErrorCenter/>` (mounted in the layout):
  red cards for failing sources with ↻/×, reads the usePoll error registry.
- `src/components/spend-stats-modal.tsx` — the "Кто тратит" ("Who is spending") modal (`/spend-stats`,
  24h/7d/30d windows + arbitrary range, charged vs real-API and OpenKeys summary).
  Wiring: `const { openSpendStats, spendStatsModal } = useSpendStatsModal()`,
  `openSpendStats` — into `StatCard.onClick`/`onClick` of the "потрачено" ("spent") header,
  `{spendStatsModal}` — at the end of the page. The `SpendStatsResponse`, `SpendPeriod`
  types and the `isOpenkeys` helper are exported.
- `src/app/page.tsx` — Overview (the reference page; port the others following it).
- `src/app/paying-users/page.tsx` — a separate read-only control room for paying customers only:
  fleet-wide paid/spend summary, Claude/GPT/Gemini provider rail, and a server-paginated table.
- `src/app/subscriptions/codex-capacity-board.tsx` — compact GPT summary of shared-plan capacity,
  native-credit/API-$ windows, and masked-email homes. Raw calibration, token-capacity, and
  profitability matrices are intentionally not surfaced in the operator UI.
- `src/app/subscriptions/claude-capacity-board.tsx` — masked-email subscription windows only: state,
  quota/reset, and exact API-$ for 5h/7d. Backend evidence and the tariff catalog are not duplicated
  in the tables.
- `src/app/subscriptions/gemini-capacity-board.tsx` — masked-email profiles with provider quota/reset and
  exact workload API-$ for 5h/7d. Under degraded authority the quota stays visible, saleable money
  shows `обновляем` ("updating"); profiles outside rotation are excluded from the fleet total.
- `src/app/pricing/activation-control.tsx` — a separate 5-second fail-closed poller of the bounded
  activation snapshot: release pair, Stage 8 freshness/blockers, engine head, jobs/receipts, and
  explicit cutover/recovery staging. Mutation requires a reason + the exact phrase and a repeated fresh GET;
  canary/maintenance controls are forbidden here.
- `src/app/api/health/route.ts` — `GET /api/health` → `{"ok":true}` for the watchdog.

## Page conventions

1. A page is `'use client'`, data via `usePoll("page-key", load, { interval })`;
   all sources in a single `Promise.all` with `.catch(() => null)` per source
   (degradation is silent, blocks show "—" / "источник недоступен" ("source unavailable")).
   Intervals as in `admin-panel.js`: Overview — 30 s, Subscriptions and System — 10 s,
   the rest — no automatic polling (focus/↻ button only).
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
9. The `/paying-users` page uses only the exact nanoUSD fields of
   `/admin/finance/paying-users`; provider amounts must not be reconstructed from float USD or
   the top-50 `/spend-stats`.

## Commands

```bash
pnpm dev          # next dev -p 3700
pnpm build
pnpm start        # next start -H 127.0.0.1 -p 3700
pnpm typecheck
pnpm test         # vitest run
```
