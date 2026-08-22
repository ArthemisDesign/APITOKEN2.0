# Referral system audit — 2026-08-22

This is an append-only local implementation and production-readiness audit of the Commerce-account
partner program. It covers the existing Sales ledger/authority core and the unshipped Commerce
Dashboard/Admin consumer. It is not production acceptance: no branch was pushed, no Vercel preview
was created and no production deployment or legacy cutover was performed during this audit.

## Verdict

The local implementation is coherent and passes the relevant production builds, contract tests,
browser checks and full PostgreSQL integration suites. The new product identity is the existing
Commerce account: a person signs in with the same email at `apitoken.sale`, Commerce resolves that
email to its immutable account UUID server-side, and browser views receive the current email rather
than Commerce/Sales UUIDs. Team income is a retained share of a member's fixed commission pool, not
an additive markup, and every application and database boundary caps that share at 2,000 bps (20%).

The consumer is **not releasable yet**. Exact Sales producer commit
`cbc6c22321838908b83664711763fcf89c6699c9` additively supplies nullable
`customerCommerceUserId` on every protected request view. On the audit date the commit exists only
on `origin/fix/referral-request-account-identity` and is not an ancestor of `origin/master`.
Commerce deliberately validates that producer response strictly; deploying the consumer first
would turn incomplete request projections into safe 503 responses. The required sequence is:

1. merge and obtain production-GREEN `deploy/watchdog` evidence for producer SHA `cbc6c223…`;
2. rebase/revalidate this Commerce consumer against the resulting `origin/master`;
3. publish a human-reviewable `preview/*` Web deployment and obtain approval;
4. merge the Commerce consumer and verify production parity;
5. only then remove/redirect legacy Sales UI surfaces in a separate cutover;
6. remove the dormant internal promo producer only after dependency search proves that it has no
   deployed consumer.

## Identity and authorization

- Partner access is manual. An ordinary Dashboard account receives program terms and a Telegram
  contact CTA; it receives no partner data or mutation authority.
- Admin onboarding and the Users-page action accept Commerce email. Commerce looks up the account
  and sends an immutable UUID across the authenticated `SALES_CONTROL_KEY` boundary; the browser
  cannot supply that UUID as identity.
- Partner reads take identity only from the verified Commerce session. Admin mutations resolve the
  selected Commerce user on the server and pass a verified actor plus idempotency evidence.
- Team invitations target an existing active Commerce account by email. Sales stores one open
  UUID-bound invitation per account; expiry, consumption and revocation are terminal evidence.
- Strict Zod response schemas reject missing/invalid Sales fields. The client has a six-second
  timeout, does not retry mutations and converts authentication/upstream/schema failures to a
  non-leaking 503.
- Browser partner, Team, request and payout projections are email-only and strip internal
  Commerce/Sales identifiers. Email/code strings use `translate="no"` where automatic translation
  would corrupt identity.

Evidence: `packages/db/src/referral-accounts.ts`, `apps/api/src/referral-sales.client.ts`,
`apps/api/src/referral.service.ts`, `apps/api/src/referral.controller.ts`, and their adjacent tests.

## Commission conservation and Team authority

For eligible paid usage `S` and the direct partner commission `C`, the single platform-funded gross
pool is `floor(S × C / 10,000)`. At every Team edge the child's configured retained-share rate
withholds a portion of the gross arriving at that child. The child receives `gross - withheld`; the
withheld amount becomes the next parent's gross. Therefore the sum of all member/ancestor net rows
equals the original direct gross pool. The platform never pays an additive parent commission.

The audit verified all of these independent guards:

- pure integer `bigint` calculation floors at each edge and conserves the pool across multiple
  levels;
- hard 20% bounds exist in request schemas, domain validation, partner/invite checks and PostgreSQL
  constraints/triggers;
- only the platform/admin controls a member's direct commission; a parent can edit only its direct
  edge, the delegated Team ceiling and explicitly delegable B2B/Team permissions;
- a parent cannot grant either an edge rate or a descendant ceiling above its own ceiling;
- lowering a ceiling clamps direct children, all descendants and still-open invitations leaf-first
  in one transaction;
- only active Commerce memberships whose `program_started_at` is not later than the usage event
  participate; an ineligible parent stops the chain without taking money from the child;
- cycles and chains beyond ten payable levels fail closed;
- version-2 ledger rows store `gross_amount_nano`, `withheld_amount_nano` and net `amount_nano`, and
  database triggers independently recompute the expected chain before accepting an insert;
- immutable version-1 financial history remains untouched for legacy memberships.

Evidence: `packages/sales-db/src/commissions.ts`, migrations 0024 and 0026, plus
`commissions.test.ts`, `commerce-partner-membership.integration.test.ts`,
`commerce-partners.integration.test.ts`, `team-override-controls.integration.test.ts` and
`partner-consumer-hardening.integration.test.ts`.

## B2B delegation and requests

- Direct B2B self-service is disabled by default. Admin chooses a per-partner ceiling up to 95% and
  independently chooses whether that authority can be delegated.
- A delegated grant is source-bound to the direct parent. A parent cannot change a direct
  platform-issued grant, delegate without its own delegable grant, or exceed its own ceiling.
- Narrowing or revoking a source grant clamps/revokes every inherited descendant and open
  invitation leaf-first before the source row changes; database triggers reject partial cascades.
- Partners without self-service authority submit a durable request. Commission, B2B conversion and
  B2B pricing requests require a reason and idempotency key. Only admin decides them. B2B effects
  become applied only after the durable Commerce operation succeeds.
- Provider terms are explicit, restricted to the supported provider set and cannot be approved
  above what the partner requested.

Evidence: `packages/sales-db/src/partner-authority.ts`, migration 0025,
`partner-authority.integration.test.ts`, `partner-b2b-grant.integration.test.ts`,
`partner-requests.integration.test.ts`, and the Sales/Commerce controller tests.

## Refunds, debt and payouts

- Commission rows remain immutable positive earning evidence. Refunds/disputes create signed
  adjustments instead of rewriting history.
- Net, debt, payable and available values include adjustments. Paid-above-net is debt; requested
  and approved payouts are committed and cannot be offered again; rejected payouts do not reduce
  what is owed.
- Current accrual, locked half-month periods, unlock dates, minimum payout, payout history and chain
  state are exposed without deleting accounting evidence.
- Payout construction is fenced by accounting cursors, wallet validity, payout window, batch state,
  chain balance and nonce/broadcast ownership. The unified Admin keeps the protected on-chain
  execution endpoints until their contract is additively moved.

Evidence: `packages/sales-db/src/analytics.ts`, `payout-periods.ts`, reversal and payout integration
tests, and `apps/sales-api/src/payout/*`.

## Product and visual audit

The Dashboard Referral section reuses the existing Dashboard shell, cards, tables, controls,
light/dark tokens and RU/EN provider. Its five subviews are URL-addressable. The overview uses the
same stacked-column geometry as Usage and exposes provider totals for Claude, GPT and Gemini. The
ordinary, disabled and active membership states are distinct. Mobile layouts keep all five tabs
visible and isolate wide data tables in horizontal scroll containers without page-level overflow.

The Admin partner area uses the main Admin shell and `/admin/referral/*` Commerce routes. Onboarding,
directory, settings, requests, payout review and the Users-page action are email-first. Controls
separate platform commission, retained Team share, delegated Team ceiling and B2B authority. New
partner screens contain no Telegram, promo or legacy partner-login workflow.

The final browser pass covered 1440 px desktop and 390 px mobile, English/Russian and light/dark.
It checked five URL tabs, 30 provider-stacked columns, accessible chart labels, long email handling,
20% input maxima, absence of UUID/promo copy, the ordinary-account CTA, zero page-level overflow and
the invitation-revocation confirmation dialog. The dialog keeps keyboard focus inside, supports
Escape, and requires confirmation before the destructive mutation. A separate navigation pass also
proved that EN/RU sidebar switches preserve the mounted Dashboard shell and do not repeat
`/v1/auth/me` or `/v1/account`; the redundant Support-section identity fetch found by that pass was
removed by passing the already-loaded Dashboard account id into the shared support content.

The unified Admin onboarding screen was also rendered from its production build at 1440 px and
390 px in both language/theme combinations. Its mobile partner subnavigation keeps all five routes
visible, controls remain inside the viewport, the identity badge follows the selected language and
the complete authority form collapses to one readable column.

## Promo and legacy cutover boundary

The public Commerce promo controller/service and Dashboard/Admin promo workflow are removed. The
Sales internal promo producer and historical promo/ledger/audit rows remain intentionally dormant.
Deleting them in this consumer change would violate the expand-only contract and could destroy
financial evidence. Their eventual removal is a separate producer-last change after deployed
consumer discovery is empty.

Likewise, legacy Sales accounts and cabinets are not zeroed or deleted by this implementation. They
must remain available until the Commerce consumer is production-GREEN and parity is demonstrated;
then access can be disabled/redirected while immutable commission, adjustment, request, invitation
and payout evidence remains retained.

## Verification performed

All commands ran in the isolated local worktree. Temporary PostgreSQL clusters were initialized
under `/tmp`, received the complete migration chain, were used only for tests, then were stopped and
moved to the local Trash.

| Surface | Result |
|---|---|
| Web | typecheck; production build; focused ESLint; 42 files / 196 tests |
| Web browser audit | desktop/mobile, RU/EN, light/dark; Referral semantic and overflow checks passed |
| Admin | typecheck; production build; changed-file ESLint; 39 files / 455 tests |
| Commerce DB with PostgreSQL | migrations; 23 files / 153 tests |
| Commerce API | build/typecheck; final no-DB run 37 files / 237 tests (188 passed, 49 DB-dependent skipped); the earlier DB-enabled 236-test suite and the subsequently added email-only Admin projection test passed |
| Sales DB with PostgreSQL | migrations/build/typecheck; 27 files / 179 tests |
| Sales API with PostgreSQL | build/typecheck; 21 files / 132 tests |
| Admin browser audit | production build at 1440/390 px; RU/EN, light/dark and all five partner tabs |

No statement in this audit claims a preview, merge, production deployment, DNS redirect, production
data reset or live parity check. Those remain blocked by the producer-first gate above and the
person's explicit instruction to perform local work only.

The complete-site capture runner's unrelated server-document check currently observes `lang="en"`
on `/ru/docs`; the focused Referral run disabled only that global assertion. This audit therefore
does not claim that localized Docs defect is fixed or that the entire public site audit is green.
