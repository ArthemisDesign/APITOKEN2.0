# crates/metering — CLAUDE.md

**Role:** exact token counting → USD equivalent. Guarantee: **no token escapes the count.**

**Boundaries (hard):**
- Pure math/parsing. Only dependency is `serde_json`. NO network/DB/env/HTTP.
- All Anthropic token buckets are counted SEPARATELY (they have different prices): input, output,
  cache_read, cache_write_5m, cache_write_1h, web_search. Adding a bucket — add it to `Usage`,
  `cost_nanodollars`, `total_tokens`, and the tests as well.
- Money amounts are counted in INTEGER nanodollars (1 USD = 1e9 nano; $/Mtoken × 1000 = nano/token — an integer).
  No f64 in money counting.
- The Gemini catalog also lives only here: paid-tier effective-dated rates, uncached/audio/cached
  input, candidate+thinking output, the diagnostic tool-prompt subset, long-context and Search.
  A missing `toolUsePromptTokenCount` is not subtracted from the authoritative `promptTokenCount`
  and not invented: the subset is not metered a second time. Gemini 2.5 Search is counted per grounded
  prompt, Gemini 3 — per query. A new model/price epoch may be added only with an official link and
  an exact-rate test; a separately metered server tool must not slip through for free.
- The Codex catalog and the ChatGPT Fast credit multiplier also live only here. Fast is a tier of an
  existing model, not a separate model id: GPT-5.6/5.5 = 2.5x, GPT-5.4 = 2x. Change it only per the
  published OpenAI table with an exact-multiplier test.
- GPT Image 2 metering lives in `openai_image`: exact alias/snapshot tariff identity and five
  disjoint legs (fresh/cached text input, fresh/cached image input, image output). It is the official
  OpenAI API replacement tariff used by `forward` for customer image settlement, not ChatGPT native
  credits or subscription quota. If authoritative cached subsets are present, validate each subset
  and derive fresh input by subtraction; if absent, charge all corresponding input as fresh. Use
  checked i128 arithmetic. This pure authority does not itself publish a model or grant catalog access.
- Versioned model/tariff identity is capability only, not product access. The exact canonical map,
  alias generation, immutable schedule ID/epoch and typed reserve modifiers live here; access still
  requires a separate product catalog and account policy. An unknown/historical ID is not turned into
  an invented canonical identity, and legacy conservative pricing remains a separate contract.
  The one audited exception is the live dated id `claude-haiku-4-5-20251001` (the id upstream lists
  and the router catalog publishes): `anthropic_tariff_capability_at` maps it EXACTLY to the bare
  canonical `claude-haiku-4-5` (the router rewrites the alias to the dated id on the wire, so the
  engine sees the dated id). There is no generic date-suffix stripping — any other dated/historical
  id still returns the typed fallback.
- Hot tariff override family keys: every price lookup also has a `*_matched_tariff_at` twin
  (`anthropic_matched_tariff_at`, `codex_matched_tariff_at`, `gemini_matched_tariff_at`,
  `glm_matched_tariff_at`, `kimi_matched_tariff_at`) that returns the SAME prices plus the tariff
  family key the resolution used, so a hot override row in the engine's `pricing_tariff_overrides`
  table (registry authority, schema checkpoint 0036) can target exactly that family. Family keys are
  the compiled schedule identities minus their date/version suffix: Anthropic heuristic branches
  (`anthropic/standard/<branch>`, `anthropic/fast/<branch>`), per-model catalog keys
  (`openai/codex/<upstream>`, `google/gemini/<id>`, `zhipu/glm/<official_id>`,
  `moonshot/kimi/<official_id>`), per-model native credit cards (`chatgpt/codex-credits/<upstream>`,
  `zhipu/glm-credits/<official_id>`) and the single image card `openai/gpt-image-2`
  (`openai_image_tariff_family`). The helpers share one branch table with the existing matchers, so
  helper prices can never diverge from them (regression tests assert equality across model/timestamp
  matrices); a model no branch recognizes has no family and keeps its existing conservative
  fallback. Adding a model or a price epoch means adding its family key in the same table row.
  The additive `*_compiled_tariffs_at(now)` / `*_compiled_credit_rates()` /
  `openai_image_compiled_tariff()` enumerators list every family with its compiled price vector
  for seeding/diffing against the override table; they are built from the same constants/catalog
  rows the matchers read, and the dead-after-flip `anthropic/standard/sonnet-5-intro` is
  enumerated only while `now < SONNET5_STD_START`.

**Invariants (verify with tests):**
- 1M tokens of any bucket = the exact official rate (test `prices_exact_per_million`).
- Stream: input/cache from `message_start`, output from the LAST `message_delta` (cumulative).
- Gemini SSE likewise uses the last full cumulative `usageMetadata`; split/malformed frames
  do not panic and do not overwrite the last valid snapshot.
- An alias and the concrete variants of the same Codex model must return the same Fast multiplier.
- `gpt-5.6` and `gpt-5.6-sol` must share one canonical/tariff identity; a new price epoch changes
  the schedule ID but not the alias generation. Codex capability also records the audited max-output
  limit so the dormant snapshot builder can reject runtime-config drift without a second reserve formula.
  Long-context and Fast/geo modifiers are recorded separately.
- Broken input → `Usage::default()` (zeros), NEVER panics.
- i128 — overflow is impossible even on a billion tokens.

**How `forward` uses it:** tee the upstream response → on completion `usage_from_sse` (stream)
or `usage_from_response_json` (non-stream) → `cost_with_multiplier` → debit the key's balance.
Meter ONLY a successful response (429/rotation are not metered).
Two multiplier-rounding contracts coexist deliberately: `apply_multiplier` (half-up) serves the
legacy scalar/strict paths as immutable history, `apply_multiplier_floor` (exact contract floor)
serves every release-v2 settlement — the release-v2 reserve floors its hold, so the final debit
must match it exactly. A new release-v2 money path uses the floor helper, never the half-up one.

**Verification:** `cargo test -p metering` (ALL must pass — this is about money).
