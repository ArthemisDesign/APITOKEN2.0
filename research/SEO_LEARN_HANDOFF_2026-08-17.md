# SEO learn-cluster rework — handoff (2026-08-17)

State as of the `preview/seo-parity-dedup` branch (merged to master the same day).
This is a work-journal/handoff for the agent continuing the SEO effort, not a
product instruction.

## The problem that started it

Google Search Console showed ~950 known pages but only 40 indexed. The cause:
the provider-parity generator (`learn-provider-parity.ts` + `learn-provider-depth.ts`)
produced 119 articles (41 topics × 3 providers) by interpolating shared text
templates with provider facts. Cross-provider pairs shared whole paragraphs
verbatim — textbook near-duplicates. Google responded with "Discovered/Crawled —
not indexed" and "Duplicate" for the bulk of the cluster.

## What was done (all on the branch, each its own commit)

1. **Parity generator deleted.** Both files removed; `learn.ts` no longer spreads
   parity articles or runs `enrichExistingProviderParityContent` (the 14 manual
   provider pages serve pure hand-written content again). Catalog pinned by test
   to exactly 71 articles = 47 core + 16 provider + 8 image-seo. Sitemap learn
   URLs: 760 → 284 (71 × 4 locales). The 476 parity URLs now 404 — deliberate,
   they were never meaningfully indexed; no redirect map exists.
2. **Per-slug file layout.** All learn data is split for conflict-free parallel
   editing (`learn.ts` is now ~460 lines of framework, no article bodies):
   - `apps/web/src/lib/learn-core/<slug>.ts` — 47 EN core articles (`export const article: LearnArticle`)
   - `apps/web/src/lib/learn-provider-en/<slug>.ts` — 16 EN provider articles
   - `apps/web/src/lib/learn-image-seo/<slug>.ts` — 8 specs, each embedding all 4 locales via helpers in `./shared`
   - `apps/web/src/lib/learn-core-{ru,zh,ko}/<slug>.ts` and `learn-provider-{ru,zh,ko}/<slug>.ts` — localizations (`export const content: LocalizedContent`)
   - `apps/web/src/lib/learn-shared.ts` — `BASE`, `OPENAI_BASE`, `KEY`, `cta()`, `quickSetupSteps` (the latter now unused by articles)
   - provider locale dirs have a local `shared.ts` with `sourceBlock` (reuses EN code blocks — code is never translated)
3. **Full editorial rewrite, EN first.** All 47 core + 16 provider EN articles:
   direct answer to the search intent in the first paragraph, 5–8 sections with
   unique intent-specific H2s, real config snippets/tables/steps, 4–6 long-tail
   FAQs, `updated: "2026-08-17"`. The 8 image specs were rewritten in all four
   locales at once (spec format enforces parity by construction).
4. **Localization waves.** All 63 core+provider entries reworked for **ru** and
   **zh**: native-quality renderings with exact structural parity to EN (section
   count, block-type sequence, table dimensions, FAQ count — enforced by
   `learn.test.ts`). Model IDs, URLs, headers, prices and code stay verbatim.

## Current state / known gaps

- **Korean is deferred by owner decision.** `learn-core-ko/` and
  `learn-provider-ko/` still hold the OLD short translations (old structure,
  but valid rendering — pages work, copy is just the pre-rewrite version).
  Because of this, `learn.test.ts` excludes `ko` via `STRUCTURE_SYNCED_LOCALES`
  in two tests: structure parity and per-locale protocol strings. When the KO
  wave is done, remove that filter.
- Suite status on merge: `pnpm --filter @claude-api/web test` fully green,
  typecheck green.

## Remaining work (priority order)

1. **KO localization wave** — 63 files (`learn-core-ko/`, `learn-provider-ko/`).
   Mirror the EN source structure exactly (same rules as ru/zh). Hard
   requirements from tests: pinned guardrail terms **평생 누적 지출 한도** and
   **만료일** in `claude-api-rate-limits` / `claude-api-key-security`; protocol
   strings per locale (`gpt-5.6-terra`, `x-goog-api-key`, `gemini-3.6-flash`,
   `x-api-key`, `kimi/kimi-for-coding`, `apitoken/kimi/kimi-for-coding`,
   `ANTHROPIC_DEFAULT_OPUS_MODEL=k3`, `claude --model k3`, Kimi Code TOML block).
   After the wave: drop `STRUCTURE_SYNCED_LOCALES` in `learn.test.ts` so `ko`
   is covered again. Watch Korean orthography — one wave member produced
   mojibake syllables (묣로/본누스-class typos); proofread or grep for them.
2. **GSC follow-up.** Resubmit the sitemap (284 learn URLs now), spot-request
   indexing for the hub pages, then watch the Pages report for 2–4 weeks:
   the rewritten articles should move out of "Discovered — not indexed";
   the 476 parity 404s will drop out of the report on their own.
3. **Quality passes.** (a) Re-run a cross-article similarity probe over the
   rewritten EN set (target: no pair above ~5% shingle Jaccard outside shared
   price tables). (b) Spot-check facts against `src/lib/models.ts` — the
   rewrite was instructed to preserve facts and the claims guard
   (`public-product-truth.test.ts`) stayed green, but a human-grade fact audit
   of 5–10 random articles is cheap insurance. (c) OG images exist only for
   en/ru article routes (`opengraph-image.tsx`) — consider zh/ko coverage.
4. **Do NOT re-expand programmatically.** The parity generator was the lesson:
   no template-interpolated article families. New articles join as hand-written
   per-slug files; the 71-article catalog test must be bumped deliberately.

## Working agreements for the next agent

- Worktree per AGENTS.md (`./deploy/agent-worktree.sh create preview/<slug> <slug>`
  — this is customer-frontend work, so the `preview/` prefix and human preview
  review before merge apply).
- Fact preservation is enforced socially and by test: never invent prices,
  limits, model IDs or capabilities; `public-product-truth.test.ts` and the
  pinned-string assertions in `learn.test.ts` (guardrail terms, protocol
  strings, Kimi streaming claim) are the contract.
- Verify with `pnpm --filter @claude-api/web test` and
  `pnpm --filter @claude-api/web typecheck` from the worktree root.
