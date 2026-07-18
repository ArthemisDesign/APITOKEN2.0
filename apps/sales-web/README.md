# @claude-api/sales-web

Partner-facing site for the APIToken multi-level sales/referral program
(**APIToken Partners**), deployed at `https://sales.apitoken.sale`.

Standalone brand and bounded context: it shares nothing visually or in code with
the main client site `apps/web`. Dark fintech partner cabinet: landing, partner
registration/auth, dashboard (overview, referrals, team, payouts, settings) and a
minimal key-gated admin area at `/admin`.

All data comes from the sales backend (`apps/sales-api`) over HTTP with cookie
auth (`sales_session`, `credentials: 'include'`). Money is transported as decimal
strings of nanoUSD (1 USD = 1e9) and formatted with BigInt only — see
`src/lib/api.ts`.

## Run

```bash
pnpm install                                  # at the repo root
pnpm --filter @claude-api/sales-web dev       # http://localhost:3200
pnpm --filter @claude-api/sales-web build     # production build
pnpm --filter @claude-api/sales-web typecheck
```

## Env

Copy `.env.example` to `.env.local`:

| Variable | Default | Purpose |
|---|---|---|
| `NEXT_PUBLIC_SALES_API_URL` | `http://127.0.0.1:3100` | Base URL of the sales-api backend |

## Structure

- `src/app/` — App Router pages: landing `/`, auth (`/register`, `/login`,
  `/verify-email`, `/forgot-password`, `/reset-password`), cabinet under
  `/dashboard/*` (client-side auth guard in `dashboard/layout.tsx`), admin `/admin`
  (x-admin-key typed into the UI, kept in `sessionStorage` only).
- `src/components/ui.tsx` — shared primitives (Button, Card, Input, Table, Badge,
  Notice, CopyButton, states).
- `src/components/earnings-chart.tsx` — pure-SVG 30-day earnings bar chart.
- `src/lib/api.ts` — typed API client + nanoUSD helpers (`formatUsd`, `usdToNano`,
  `formatBps`).
- `src/app/globals.css` — the whole design system (CSS custom properties, no
  Tailwind).
