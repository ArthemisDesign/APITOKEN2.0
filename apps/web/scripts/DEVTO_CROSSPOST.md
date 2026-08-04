# dev.to cross-post — instructions and log

> An off-page SEO/GEO channel: republishing articles from the learn cluster on dev.to (DR ~90) with
> a `canonical_url` pointing at the original. Strategic context — `research/GEO_GITHUB_STRATEGY.md`
> (brand mentions correlate with AI visibility at 0.664; comparative content yields 2.4×
> brand mentions). This file is the operational side: how to publish and what is already published.
> **Update the log at the bottom on every publication.**

## How it works

- The `devto-crosspost.mjs` script pulls content from the live markdown gateway
  `https://apitoken.sale/md/docs/learn/<slug>` — exactly what is in production gets published.
- It strips the YAML front matter and the H1 (dev.to renders the title itself), makes
  relative links absolute (`/models` → `https://apitoken.sale/models`), and appends an
  "Originally published at …" footer.
- It sets `canonical_url` to the original: no duplicate-content penalty, search weight
  consolidates on apitoken.sale, and dev.to shows "Originally published at" under the header.
- Publication goes through the Forem API v1 (`POST https://dev.to/api/articles`), with retry on 429.

## Publishing

```bash
cd apps/web
node scripts/devto-crosspost.mjs <slug> [<slug>...] [--dry-run]
```

- Key: env `DEVTO_API_KEY` or `~/.config/apitoken/devto.env` (lives on the user's Mac).
  Account: `api_token_46fac5c7112fe23`. Key reissue: dev.to Settings → Extensions.
- Slugs come from `apps/web/src/lib/learn.ts` (EN versions; we do not post localized ones).
- Run `--dry-run` first and eyeball the title/description/start of the body.
- After publishing, open the URL and check: tables are rendered, links point to
  apitoken.sale, and "Originally published at" is present.

## Channel rules

1. **Cadence of 2–3 articles per week, maximum.** The whole cluster at once is a spam signal for
   dev.to and Google. dev.to rate limit: ~1 post / 5 min (the script retries on its own).
2. **What to post first** (in descending SEO/GEO value):
   - comparative (`apitoken-vs-*`, `*-vs-*`) — 2.4× brand mentions in LLMs;
   - articles with price figures/tables — "statistics" (+31% GEO visibility, Princeton KDD'24);
   - integration articles with a copyable config (`claude-api-key-for-cursor`, `claude-api-aider`,
     `claude-api-litellm`) — the developer copies the example together with the base_url.
3. **Tags**: exactly 4, lowercase, no hyphens. The per-slug map is in `TAGS` inside the script;
   for a new slug add an entry there (default `ai, claude, api, llm`).
4. **Do not edit the text for dev.to by hand** — the gateway is the only source of truth.
   Content edits are made in `learn.ts` (and reach both the site and future cross-posts).
5. Do NOT mention the pool/subscription/rotation mechanics in posts or comments — publicly we are
   an "Anthropic-compatible API provider" (risk framing from GEO_GITHUB_STRATEGY.md §3).

## What this actually gives (honestly) and what to measure

A canonical is not a classic backlink but an attribution signal; in-body links on
dev.to are nofollow. The channel's value lies elsewhere: an indexable surface on a DR90 domain
that AI engines actively cite; brand mentions (the top correlate of AI visibility);
a chance for the dev.to version to rank for long-tail queries where our domain is still weak.
The effect is cumulative — consistency works, not a one-off dump.

Measure once every 2 weeks: referral traffic from dev.to in the site analytics; positions of the
dev.to posts for their own queries; manual questions to ChatGPT/Perplexity ("cheapest claude api",
"openrouter alternative for claude") — whether our posts made it into the citations.

## Adjacent channel: Zen (automatic via RSS)

The feed `https://apitoken.sale/zen.xml` (route `src/app/zen.xml/route.ts`) serves the RU versions
of all learn articles in Zen's native format. Hookup is one-time, done by hand by the owner:
Zen channel → Studio → "Website" → merge the site and the channel → provide the feed URL.
Once connected, publication is fully automatic (new/updated articles from
`learn.ts`/`learn-ru.ts` flow in on their own). The copies carry `noindex` so they do not cannibalize
our /ru pages in Yandex (toggled in `ZEN_CATEGORIES` in the route). RSS updates
of a piece work for 7 days after upload; manual editing in Studio disables them.

vc.ru: there is NO official API (only an internal one; auto-posting is unstable and gets banned) —
so semi-manually: the agent prepares an adaptation in a native tone (a case study/guide, not an ad),
a human pastes it into the editor. Cadence ≤1/week.

## Publication log

| Date | Slug | dev.to URL |
|---|---|---|
| 2026-07-28 | cheapest-claude-api | https://dev.to/api_token_46fac5c7112fe23/cheapest-claude-api-up-to-80-discount-2ne8 |
| 2026-07-28 | apitoken-vs-openrouter | https://dev.to/api_token_46fac5c7112fe23/apitokensale-vs-openrouter-for-claude-3gmj |
| 2026-07-28 | claude-code-without-subscription | https://dev.to/api_token_46fac5c7112fe23/use-claude-code-without-a-subscription-1i6h |

Candidate queue: `claude-api-key-for-cursor`, `apitoken-vs-anthropic-direct`,
`claude-api-prompt-caching`, `claude-api-litellm`, `claude-api-aider`,
`claude-code-api-key`, `claude-api-pricing-explained`.
