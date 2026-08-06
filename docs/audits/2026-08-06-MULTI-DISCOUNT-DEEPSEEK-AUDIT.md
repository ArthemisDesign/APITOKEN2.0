# Multi-discount prices update — DeepSeek full audit — 2026-08-06

**Subject.** Full independent audit of the prices update per the approved target contract in
[`docs/commerce/MULTI-DISCOUNT.md`](../commerce/MULTI-DISCOUNT.md): the live release-v2 pricing
authority, B2C/B2B/OpenKeys/service economics, funding normalization, welcome bonus, referral
paid-funded commission, progressive-machinery removal, money invariants, migrations, tests, and
the production state claimed by the Definition of Done (dated 2026-08-06).

**Audited snapshot.** `origin/master @ b0807bd3659dcc27b1f5e856f0b74cc3e3111520` (head at audit
time; "generation 13" cutover receipt dated 2026-08-04). Previous audit baseline for comparison:
`7b000805a961e7b7aa54f38c4c2bd268bfdc11fa` (2026-08-05).

**Method.** Performed in an isolated worktree, no subagents. Source audit against
`MULTI-DISCOUNT.md` sections 1–11; expand-only migration verification across commerce/engine/sales/
OpenKeys; production read-only audit of the commerce and engine PostgreSQL databases over SSH
(`deploy@84.32.48.2`, root via operator key, queries read-only) plus GitHub deploy statuses of the
host lanes. Gates run: `cargo build --workspace`, focused Rust tests, `pnpm build`, `pnpm
typecheck`, `pnpm test` (all packages), DB integration suites against the local commerce postgres,
sales-db integration suite, `deploy/docs-check.sh`.

---

## Executive verdict

The prices update is **live and materially conformant**: the production state claimed by the
Definition of Done (release generation 13 head, funding normalization ready for the full
inventory, passed Stage 8 evidence, B2C global 5000 bps, B2B negotiated provider rules, OpenKeys
strict 1:1, service `meter_only`, $5 welcome bonus, all host deploy lanes green) was verified
read-only in production and holds. The engine's authoritative billing paths resolve release-v2
rules exactly, and the release-v2 data plane is the live writer for all three fixed-plane
providers.

Two findings from the 2026-08-05 audit remain open (floor rounding in final settlement; Sales v2
conflicting replay accepted silently), and one residual presentation issue (admin pricing editor
still offers a `track` option for global B2C; the service-policy editor can still be saved
post-cutover without changing the release authority). All three are described below with exact
locations and the required remediation.

---

## What was verified in production (read-only, 2026-08-06)

### Cutover and release authority (engine `claude_engine` DB)

- `pricing_release_head_v2`: **generation 13 / head_version 1** — matches the DoD.
- `pricing_release_versions`: 20 rows, max generation 26 (dormant futures prepared, head stays 13).
- `pricing_release_activations_v2`: exactly **one** activation, `to_generation=13`,
  `activated_ts=1785875698` — single-head atomic cutover, no canary (DoD item 12).
- Commerce `pricing_release_activation_receipts_v2`: exactly one durable receipt
  (`cutover | 13 | head_version 1 | 2026-08-04 20:34:58+00`).
- Assignments: engine base 8440 (b2b 80, b2c 2828, openkeys 5072, service 460) + **22** append-only
  assignment extensions; commerce assignments_v2 11030 — full-inventory classification (DoD item 3).
- Stage 8 evidence: one persisted `passed=true` row, target 13 / recovery 14, legacy inflight
  count 1 (audit-only, no drain), blockers 0, fresh TTL at activation time (DoD item 10).

### B2C / B2B / OpenKeys / service economics

- B2C global rule: `release-v2:b2c:global` v3 = `global: discount 5000 bps / payable 5000`.
  Ledger parity: **17048 of 17048** B2C release-v2 charges resolved with `rule_scope=global`,
  `payable_multiplier_bp=5000` — no off-policy B2C charge found (DoD item 16).
- B2B: 80 active assignments; policy rules carry negotiated provider discounts (samples:
  anthropic 7000/google 6500/openai 7000; anthropic 8500 ×3; anthropic 8500 alone; anthropic
  9500/openai 9500/google 9500). 35 provider rules total; no B2C inheritance — the global B2C
  policy never prices B2B (DoD item 6).
- OpenKeys: 5072 assignments; 512 release-v2 charges, **all** with `payable_multiplier_bp=10000`
  and `discount_bps=0` — strictly 1:1, no override ever billed (DoD item 6).
- Service: 460 assignments, 69 policy versions **all** `billing_mode=meter_only`, zero service
  policies carry rules; 196 release-v2 `meter_only` request snapshots; zero `amount_nano <> 0`
  rows for meter_only — no customer debit, no 402 semantics (DoD item 7).

### Funding and welcome bonus

- `pricing_funding_normalizations_v2`: **7980 / 7980 status=ready**; 6418 refreshed after
  2026-08-04 — the online backfill finished for the full inventory without a global stop (DoD
  item 9).
- `funding_lots_v2` welcome bonus: **93 lots `welcome_bonus`**, of which 9 at exactly
  `5000000000` (8 created post-cutover) and 84 at the legacy `4000000000` — new issuance is
  exactly $5, previously issued $4 bonuses were not retroactively increased (contract §5,
  decision 8).
- Release-v2 ledger: 19201 request snapshots v2 vs 2861 legacy; 18889 release-v2 charge rows —
  the release data plane is the live writer (DoD item 13).

### Referral paid-funded commission

- Commerce emits release-v2 usage lineage (18445 rows attribution_schema_version=2 with
  `release_v2` snapshot kind; 16716 commission-eligible with `paid_funded_nano > 0`).
- Sales sync cursor advanced to id 89080; sales v2 tables currently hold 0 rows because the two
  referred users with release-v2 usage were converted to B2B (their attribution rows are
  `b2b`, commission_eligible=false) — the contract-correct "commission stops at conversion"
  outcome, not a pipeline stall. V1 (1413 rows) remains immutable history.
- New sales v2 rows are written only for B2C referred users with positive exact paid funding;
  welcome-bonus-funded usage is excluded by construction.

### Deployment state (host watchdog lanes)

- Engine release `current → 2f090625` (the last SHA touching `crates/`; zero crate commits after
  it) — engine deployment is current.
- Commerce release `current → b0807bd3` (head SHA) — commerce deployment is current.
- `deploy/watchdog`, `deploy/engine`, `deploy/backend`, `deploy/migration`, `deploy/tests`,
  `deploy/sales`, `deploy/openkeys`, `deploy/admin`, `deploy/devbot`: **success** on the head SHA
  (DoD item 15). The Vercel frontend context is intentionally out of scope for this audit.

---

## Findings

### High — final customer debit still violates the floor contract (open from 2026-08-05)

The contract (§3) requires:

```text
charged_nano = floor(official_nano * payable_multiplier_bp / 10000)
```

- Reserve-time release-v2 hold floors correctly (`crates/registry/src/pricing/release_v2.rs:647`,
  plain integer division on non-negative values).
- Final settlement still uses `metering::apply_multiplier` (half-up, `+5000` before division):
  Anthropic `crates/forward/src/meter.rs:516`, OpenAI `crates/forward/src/codex/billing.rs:1185`
  and `:1323`, Gemini `crates/forward/src/gemini/billing.rs:979`; the registry trusts the
  adapter-computed `actual_nano` (`crates/registry/src/pricing/postgres.rs` settlement path).
- Stage 8 shadow repeats the half-up formula (`crates/registry/src/pricing/shadow.rs:1667`),
  so evidence can pass while validating the wrong rounding.

**Production evidence:** of 18884 release-v2 charges consistent with the half-up formula, **152
rows deviate from the contract floor formula by exactly +1 nanoUSD** (the `(x*mult+5000)/10000`
vs `x*mult/10000` boundary). Six larger deviations are B2B Codex rows where the customer is
charged on the requested output cap while `official_nano` records full upstream usage — that is
the documented provider-adapter contract (§7.2), not a rounding error.

Materially the exposure is ~1 nanoUSD per request (negligible), but it is an exact financial
contract violation that the 2026-08-05 audit already flagged and the DoD status implies closed.

**Required remediation:** one checked release-v2 floor helper used in reserve, all final provider
settlement paths, and Stage 8; legacy half-up behavior retained only for immutable legacy
snapshots. Then regenerate Stage 8 evidence on the exact new SHA.

### High — Sales v2 still silently accepts conflicting immutable replays (open from 2026-08-05)

Both pending and finalized v2 event inserts use `ON CONFLICT (commerce_event_id) DO NOTHING`
without comparing the persisted evidence (`packages/sales-db/src/commissions-v2.ts:110` and
`:130`). A replay carrying a different user, partner, amount/funding composition,
release/snapshot identity, or timestamp is accepted as "duplicate"/"buffered" — an incorrect
first insert can never be exposed or repaired by the authoritative retry. The v1 implementation
performs the correct immutable-field comparison; the v2 writer does not. No commits touched
`packages/sales-db` since the 2026-08-05 baseline, so the code is unchanged. The integration
suite covers only exact replays, not divergent ones.

**Required remediation:** on conflict, load and compare every immutable v2 field and fail the
sync page on any divergence; add PostgreSQL tests for both pending and finalized conflicts.

### Medium — admin pricing editor: residual track option and service-policy save without authority

- The admin global-B2C editor still passes `allowTrack` (`apps/admin/src/app/pricing/page.tsx:95`;
  `apps/admin/src/app/business/policy-editor.tsx:279` renders "прогрессивный тариф"), and the
  shared schema still accepts `pricingMode: "track"` (`packages/contracts/src/index.ts:2184`,
  `packages/db/src/pricing-policy-write.ts:176`). Post-cutover the global-B2C **save** is refused
  with `release_cycle_required` (`pricing-policy-write.ts:1042`), so no new track record can be
  written to the authority — but the editor still *offers* the control, which contradicts §9
  ("must not offer track/tier controls") and DoD item 14 (no UI surface presents them).
- The service-policy editor (`apps/admin/src/app/pricing/page.tsx:232`) still PATCHes discount
  rules to `/admin/service-policies/:id` post-cutover. Only `global_b2c` is guarded by
  `release_cycle_required`; a service save versions the legacy policy document and reports
  success ("ожидаем exact ACK") while the release authority (service = `meter_only`, no rules)
  is untouched. The engine would reject the legacy delivery post-cutover, so no money moves —
  but the panel can still report a successful save that changes nothing.

**Required remediation:** remove the `track` option from the editor and the `track` literal from
the editor schema; extend the `release_cycle_required` guard to `service` owner-type saves
(service policy edits post-cutover require a new release cycle).

### Low — residual legacy presentation fields

`apps/api/src/admin.service.ts:768-771` still serializes `tier` / `tier_window_spent_usd` and
the paying-users rows carry `tier` (`packages/db/src/admin-finance.ts:121`); the admin UI
labels (`tierLabel`, `payingTierLabel`) already render only "B2B" / "B2C −50%", so this is
legacy-column passthrough pending the late physical schema cleanup (§6), not an active pricing
surface. Residual web code paths (`apps/web/src/app/dashboard/sections/usage.tsx:501`,
`apps/web/src/app/register/register-form.tsx:121`) can still render a "progressive" label for
immutable pre-cutover rows; they are unreachable for new data. Acceptable per §6, worth a sweep
when the legacy columns are physically removed.

---

## Verified as sound

- Rule precedence model → provider → global, non-stacking; fail-closed when no rule applies
  (`crates/registry/src/pricing/postgres.rs:4652-4681`).
- OpenKeys admission follows the runtime with scoped switches optional by owner decision (§4);
  catalog membership bypass for openkeys matches the documented decision; a disabled master
  switch still closes a provider.
- B2C→B2B conversion and every B2B policy save chain per-account strict enforcement
  (`packages/db/src/strict-chain.ts`, worker sweep) with a post-cutover stand-down guard; DoD
  item 17 verified by code and integration tests.
- Money is integer-only everywhere in the audited paths (bigint/nanoUSD strings); no float
  arithmetic found in pricing, settlement, funding, or referral.
- Migrations 0026–0042 (commerce), 0023–0033 (engine), 0015 (sales), 0007 (OpenKeys) are
  expand-only: `ALTER TABLE ADD COLUMN`, `CREATE TABLE`, `CREATE FUNCTION`, `CREATE INDEX`,
  `NOT VALID` constraint swaps; no existing migration was edited or deleted; none ran against
  production without the watchdog gate.
- Welcome bonus: exact idempotent `$5` claim + engine credit with anti-fraud gates
  (`apps/api/src/signup-bonus.ts`); bonus-first, then paid funding allocation; referral
  commission basis = exact `paid_funded_nano`.
- Stage 8/9 durability: raw-first evidence persistence, request-before-network activation,
  exact lost-ACK replay (`unchanged`), forward recovery generation 14 prepared.

---

## Verification record

Passed locally in the isolated worktree (head `b0807bd3`):

```text
cargo build --workspace                       green
cargo test -p registry pricing::release_v2    5 passed
cargo test -p forward dormant_release         3 passed
cargo test -p metering                        94 passed
pnpm build / pnpm typecheck                   green
pnpm test                                     db 105 | api 148 | sales-api 40 |
                                              openkeys 120 | admin | web — all green
DB integration (local postgres):
  strict-chain + provisioning-v2              12 passed
  shadow-rollout-v2 + stage8-evidence         25 passed
  migration + policy-write + stage5          23 passed
sales-db integration (created local sales DB, ran migrations 0000–0015)  81 passed
deploy/docs-check.sh                          exit 0
bash -n deploy/*.sh                           green
git diff --check                              clean
```

Production read-only checks: all queries in "What was verified in production" ran as plain
`SELECT`s against the commerce and `claude_engine` databases. No production state was modified;
no deployment or migration was performed by this audit. The Vercel frontend lane was excluded
from scope at the owner's request.

## Recommended next steps

1. Fix the floor helper in final settlement and Stage 8, then regenerate Stage 8 evidence.
2. Reject conflicting Sales v2 replays with field comparison + tests.
3. Remove the admin `track` option and guard service policy saves post-cutover.
4. Sweep residual legacy presentation fields together with the late physical schema cleanup.
