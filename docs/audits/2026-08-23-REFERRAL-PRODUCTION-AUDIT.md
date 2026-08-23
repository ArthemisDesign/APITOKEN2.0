# Referral system production audit — 2026-08-23

Production acceptance of the Commerce-account partner program after it landed on `master`. It
follows the local audit in
[`2026-08-22-REFERRAL-SYSTEM-AUDIT.md`](2026-08-22-REFERRAL-SYSTEM-AUDIT.md), which explicitly
deferred production verification.

## What shipped

Landed through `deploy/agent-merge.sh` and verified GREEN by `deploy/watchdog`:

| SHA | Content |
|---|---|
| `f3ee64bc` | Referral workspace on the partner-cabinet design, partner access applications with an Admin review queue, explicit Team invitation acceptance, the sidebar invitation dot, migration `0050_referral_applications` |
| `87d773ac` | PostgreSQL integration proof for the access queue and the invitation decision |

`deploy/migration` reported "Tested migration applied before application rollout"; `deploy/backend`,
`deploy/sales`, `deploy/admin`, `deploy/devbot`, `deploy/engine`, `deploy/openkeys` and the Vercel
production deployment all reported success on both SHAs.

## Live production surface

Probed from outside with no credentials. Before the release the three new routes answered `404`;
after it they answer `401`/`403`, which is the shape of a live, session-guarded route:

| Route | Before | After |
|---|---|---|
| `GET /v1/referral/applications/me` | 404 | 401 |
| `POST /v1/referral/applications` | 404 | 403 |
| `GET /v1/referral/invitation` | 404 | 401 |
| `POST /v1/referral/invitation/{accept,decline}` | 404 | 403 |
| `GET /v1/admin/referral/applications` | 404 | 401 |
| `GET /v1/referral`, `/v1/admin/referral/{partners,requests}` | 401 | 401 |

`/v1/internal/*` answers `404` publicly by design (`deploy/Caddyfile`), so the Sales producer stays
loopback-only; it is exercised through the Commerce boundary, not from the internet.

## Stage-by-stage evidence

Every stage below is covered by a suite that runs against real PostgreSQL, not a fake. The full
workspace was green on the merged tree: commerce API 245, admin 456, web 202, sales API 139,
`packages/db` 158, `packages/sales-db` 184.

| Stage | Proof |
|---|---|
| An ordinary account asks for access | `packages/db/src/referral-applications.integration.test.ts`: one open application per account that a repeat submit refreshes, a decision that lands once and refuses a second, a declined account free to apply again, pending-first queue ordering |
| An administrator decides | `apps/api/src/referral-applications.service.test.ts`: approval runs the same `onboardByEmail` as manual onboarding, rejection never touches onboarding, a failed Sales call leaves the application pending, a decided application cannot be decided twice. `apps/admin/.../partner-admin-contract.test.ts` pins the queue route, the required reviewer note and the 20% Team bound in the decision dialog |
| Invitation is offered | `packages/sales-db/src/commerce-invitation-acceptance.integration.test.ts`: reading pending terms never consumes them; resolving state with activation off creates no membership |
| Invitation is accepted or declined | Same suite: acceptance creates the membership on exactly the invited terms (parent link, retained share, Team and B2B ceilings); declining is owner-scoped, final, and leaves nothing to accept. `apps/sales-api/src/commerce-partner.invitations.test.ts` pins the controller contract |
| A Team is built | `team-override-controls`, `partner-authority`, `partner-lifecycle`, `terminal-commerce-invites` integration suites: the retained share is capped at 2,000 bps, narrowing a ceiling clamps descendants, and the database itself rejects an invite whose B2B grant exceeds the inviter's — observed live while writing the new suite |
| Real spend becomes commission | `commissions-v2`, `paid-funded-commission-v2`, `spend-provider-dimension` integration suites: the basis is exact `paid_funded_nano`, so free platform credit earns nothing, each parent cut is withheld from the fixed pool, and the provider split re-groups commission that is already recorded |
| Money leaves | `payout-periods`, `reversal-accounting`, `reconcile`, `payout-batch` integration suites: half-month periods, the 7-day lock, refund reversal and debt |

## Not verified here

- No end-to-end click-through was performed with a real production account: this session holds no
  production credentials, and `observe@84.32.48.2` rejects this machine's key, so the runbook's
  "stop, diagnose from GitHub" rule applies. Production evidence above is the watchdog's component
  verification plus black-box probes.
- No real money moved. The commission chain is proven on production-shaped SQL, not on a live
  payment.
- The Admin queue and the Dashboard gate were reviewed as deployed code and in local browser
  captures against fixtures; the first live application and the first live invitation should still
  be watched by a human.

## Follow-ups

1. Watch the first real application through Admin → Partners → Access applications, and the first
   real invitation acceptance, before announcing the program.
2. Give an agent-usable `observe` key for this machine, or accept GitHub-only diagnosis.

## Second pass — 2026-08-23, after the first production release

### The referral link itself

`apps/sales-api/src/sync-attributions.integration.test.ts` now proves the link on real SQL, which is
the layer where "did the referral actually attach" is decided: a user who arrives with `?ref=CODE`
becomes a `referred_users` row owned by exactly that partner, a replayed feed page does not
duplicate it, a later link from another partner does not steal an existing referral, an unknown code
advances the cursor instead of stalling the feed, and a suspended partner attracts nothing.

The code space was checked end to end for the silent-loss failure mode: the browser stores
`^[A-Za-z0-9_-]{3,32}$`, Commerce lower-cases and re-validates the same shape, Sales matches
`partners.referral_code` exactly, and every generator is lowercase — `p_` + 24 hex for Commerce
partners, 8-character lowercase base32 for legacy ones, with a database CHECK forcing lowercase on
external aliases. A code that survives the browser therefore resolves in Sales.

### Front-end states

`verifyReferralConditionalStates` in `apps/web/scripts/capture-site.mjs` drives the real browser
through every truth the one route can hold: partner (tabs, no gate, no sidebar dot), no access
(gate, two actions), applied (pending state, the request action replaced, Telegram still there),
declined (declined state with the reviewer's note and an apply-again action), invited (invitation
card with its four terms plus the sidebar dot), invited → declined (the card disappears, the gate
stays), and suspended (the disabled card, no tabs, no gate).

### Team

The same run asserts the Team surface: the split is stated as `10% × 20% = 2%`, the tab leads with
four Team-income KPIs, the invitation is e-mail plus one retained-share field with no permission
checkboxes and no native steppers, and the member dialog carries exactly one permission — B2B —
bounded by the owner's own ceiling.

### Admin

The reviewer note is no longer mandatory: a decision is attributable through the admin actor header,
so approving or rejecting an application needs one click and nothing else.

### Old partner portal — prepared, not landed here

The retirement is a `deploy/Caddyfile` change: every browser route on `partners.apitoken.sale`
redirects to `apitoken.sale/dashboard?view=referral` (302, reversible), `/v1/*` keeps serving the
partner API, `/v1/internal/*` stays 404 publicly, and sales-web keeps serving
`admin.partners.apitoken.sale` and its own loopback health gate, so no deployment or verification
path changes. `deploy/Caddyfile` is classified as Ubuntu-host-dependent, so the merge gate runs the
host-installer proofs under Docker; this machine has no Docker, so the change must be landed from
one that does. Nothing else in this audit depends on it.
