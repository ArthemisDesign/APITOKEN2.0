# Gemini 3 Flash private generation route — live probe evidence — 2026-08-03

## Review metadata

- Provider / slug: Google Gemini subscription pool / `gemini-3-flash-preview` (public),
  `gemini-3-flash` and `gemini-3-flash-agent` (private Antigravity catalogue rows).
- Review date and timezone: 2026-08-03, Europe/Moscow.
- Relationship to prior work: this journal supersedes one specific conclusion of
  `research/GEMINI_3_FLASH_PREVIEW.md` (2026-08-02). That journal stays untouched as a
  historical snapshot; its withdrawal decision was correct *for the public wire id* it
  probed. The new fact is that the same model family is servable today under the private
  catalogue id, exactly the `gemini-3.1-pro-preview` → `gemini-pro-agent` pattern.
- Owned live plan used: one locally authorized Google AI Ultra profile
  (`loadCodeAssist`: `currentTier=free-tier`, `paidTier=g1-ultra-tier`), addressed through
  a fresh loopback OAuth grant issued for this probe only.
- Execution location: fully local macOS `darwin/arm64` workstation. Nothing ran on the
  production host; no engine slot, credential envelope, customer key or customer traffic
  was involved.

## Probe method

- OAuth: pinned Antigravity installed-app client identity (`crates/gemini-credential`),
  `https://accounts.google.com/o/oauth2/v2/auth` with PKCE S256, `access_type=offline`,
  `prompt=select_account consent`, loopback redirect `http://localhost:51121/oauth-callback`.
  The account owner approved once in the browser per session; probe tokens were deleted
  after each session.
- Transport: direct HTTPS to `https://daily-cloudcode-pa.sandbox.googleapis.com`
  (production-configured Antigravity origin), headers limited to `Authorization`,
  `Content-Type: application/json`, `User-Agent: antigravity/hub/2.2.1 darwin/arm64`
  (the tuple the two independent implementations corroborate).
- Wrapper: the exact reviewed Antigravity envelope from
  `crates/forward/src/gemini/api.rs` (`wrap_code_assist_request`):
  `{model, project, request: {contents, generationConfig, sessionId}, userAgent:
  "antigravity", requestType: "agent", requestId: "agent/<unix-ms>/<uuid>/<n>"}`.
- Bounds: every generation leg was preceded by a free `countTokens`; generation used
  `maxOutputTokens` 8–256 with synthetic one-line prompts. Total session spend was ~25
  minimal turns; official-rate API-dollar equivalent is far below the `$0.0001` admission
  micro-smoke cap, and real cost was subscription quota only.
- Fallback surface: `https://cloudcode-pa.googleapis.com` (legacy Code Assist) was also
  exercised; every leg there returned `429 RESOURCE_EXHAUSTED` (that transport's free-tier
  quota was exhausted on this account), so all evidence below is the Antigravity surface.

## Owned catalogue snapshot (2026-08-03, this account)

`v1internal:fetchAvailableModels` rows:

```
chat_20706, chat_23310, claude-opus-4-6-thinking, claude-sonnet-4-6,
gemini-2.5-flash, gemini-2.5-flash-lite, gemini-2.5-flash-thinking, gemini-2.5-pro,
gemini-3-flash, gemini-3-flash-agent,
gemini-3.1-flash-image, gemini-3.1-flash-lite, gemini-3.1-pro-high, gemini-3.1-pro-low,
gemini-3.5-flash-extra-low, gemini-3.5-flash-low,
gemini-3.6-flash-high, gemini-3.6-flash-low, gemini-3.6-flash-medium, gemini-3.6-flash-tiered,
gemini-pro-agent, gpt-oss-120b-medium,
tab_flash_lite_preview, tab_jump_flash_lite_preview
```

Note that `gemini-2.5-flash`, `gemini-2.5-flash-lite` and even `gemini-2.5-pro` have rows
on this Ultra account: absence from the public Antigravity marketing table is not absence
from an account's quota catalogue. These rows are internal quota/billing bucket
identities, not public model names (effort/tier/client encoded in the id).

## Candidate public-id matrix (all legs: countTokens preflight, then generation)

| Public Developer API id | countTokens | generate | Verdict |
|---|---|---|---|
| `gemini-2.5-flash` (control) | 200 (total=1) | **200**, text `ok`, usage prompt 6 / candidates 1, `modelVersion=gemini-2.5-flash` | served — probe transport and account proven valid |
| `gemini-3.5-flash-lite` | 200 | 404 NOT_FOUND | not served on this transport |
| `gemini-3-flash-preview` | 200 | 404 NOT_FOUND | public id still disabled — prior withdrawal holds |
| `gemini-2.5-pro` | 200 | 503 UNAVAILABLE "No capacity available for model gemini-2.5-pro" | quota row exists; generation capacity-blocked |
| `gemini-2.0-flash` | 200 | 404 NOT_FOUND | not served |
| `gemini-2.0-flash-lite` | 200 | 404 NOT_FOUND | not served |
| `gemini-2.5-flash-thinking` (private row, bonus) | 200 | **200**, text `ok`, real usage | served; private tier row, never a public id |

## New finding: the Gemini 3 Flash generation route is alive under the private id

| Wire id | countTokens | generate | Evidence |
|---|---|---|---|
| `gemini-3-flash` | 200 | **200** | `modelVersion=gemini-3-flash` (canonical echo), usage with `thoughtsTokenCount` |
| `gemini-3-flash-agent` | 200 | **200** | `modelVersion=gemini-default` (alias echo) |
| `gemini-3-flash-preview` (control) | 200 | 404 NOT_FOUND | public id dead, as before |

Bounded capability follow-ups on `gemini-3-flash`:

| Leg | Result |
|---|---|
| non-stream, `maxOutputTokens=64` | 200, real text `ok`, usage prompt 6 / candidates 1 / thoughts 59, `modelVersion=gemini-3-flash` |
| SSE `alt=sse`, `maxOutputTokens=64` | 200 but 1 frame, no text: thinking consumed the budget — budget artifact, not a transport defect |
| SSE `alt=sse`, `maxOutputTokens=256` | 200, 2 frames, joined text `ok`, terminal usage prompt 6 / candidates 1 / thoughts 76 |
| SSE on `gemini-3-flash-agent`, `maxOutputTokens=256` | 200, 2 frames, text `ok`, terminal usage, `modelVersion=gemini-default` |
| SSE control `gemini-2.5-flash`, `maxOutputTokens=64` | 200, 2 frames, text `ok` — same framing shape |
| `thinkingConfig.thinkingLevel=low` | 200, text `ok`, usage with thoughts |
| `thinkingConfig.thinkingLevel=high` | 200, text `ok`, usage with thoughts |

## Why public ids 404: the Gemini 3 family is served only under private ids

Direct "who are you" probes (synthetic prompt, `maxOutputTokens=192`) show the pattern is
not specific to the preview model — every Gemini 3-family *public* id 404s on the raw
Antigravity wire, while mapped private ids serve:

| Wire id | HTTP | `modelVersion` echo | Self-report (unreliable) |
|---|---|---|---|
| `gemini-3.6-flash` (public) | 404 | — | — |
| `gemini-3.1-pro-preview` (public) | 404 | — | — |
| `gemini-3.6-flash-medium` (private, production route) | 200 | `gemini-3.6-flash` (canonical) | "I am Gemini 3.6 Flash…" |
| `gemini-pro-agent` (private, production route) | 200 | `gemini-pro-default` | generic |
| `gemini-3.5-flash-low` (private, production route) | 200 | `gemini-default` | generic |
| `gemini-3-flash` (private, this finding) | 200 | `gemini-3-flash` (canonical) | "I am Gemini 1.5" (hallucinated) |
| `gemini-3-flash-agent` (private) | 200 | `gemini-default` | "I am Gemini 1.5" |

Interpretation:

- Self-reported identity is worthless for admission (the same backend answers "I am
  Gemini 1.5"); the authoritative signals are the `modelVersion` echo and usage vector.
- `gemini-3-flash` echoes its own canonical name — the same evidence class as
  `gemini-3.6-flash-medium` → `gemini-3.6-flash`. The `-agent`/`-low` ids echo
  `gemini-default` / `gemini-pro-default` and behave as aliases.
- This validates the existing `wire_model_id` design in
  `crates/forward/src/gemini/config.rs`: the Antigravity backend authenticates plans and
  routes generation through private effort/tier ids; public marketing names are not wire
  identities for the Gemini 3 family.
- The 2026-08-02 journal correctly refused to *guess* a private alias without live proof.
  This probe supplies exactly that missing owned live evidence for `gemini-3-flash`.

## Limits of this evidence (what is NOT proven)

- Single account, single plan (Google AI Ultra), single day. Per repo doctrine, plan
  coverage must be established per profile type; nothing here proves Free/Pro/Enterprise.
- No cache, audio, tool, Search-grounding or long-context legs were run for
  `gemini-3-flash`; the publication gate requires the full matrix plus every published
  thinking level, incremental SSE and terminal authoritative usage on the exact candidate
  SHA through the engine, not a raw curl-equivalent probe.
- `minimal`/`medium` thinking levels were not exercised (only low/high plus default).
- Whether upstream bills `gemini-3-flash` generation against the `gemini-3-flash-agent`
  quota row, the non-agent row, or both, was not measured (no quota delta attribution).
- The `gemini-default` echo of `gemini-3-flash-agent` makes it a weaker route candidate
  than `gemini-3-flash`; it must not become the wire id.
- Google can flip any of these routes at any time; today’s 404/503 rows can change, and
  today’s 200 rows can disappear.

## Recommended follow-up (not executed here)

1. Research/implementation commit: amend the dormant `gemini-3-flash-preview` support to
   map the public id to wire id `gemini-3-flash` (keep quota join on
   `gemini-3-flash-agent` + `gemini-3-flash`), rewrite upstream `modelVersion` to the
   public id as the existing private-route code already does, and reference this journal.
2. Controlled live gate on the exact implementation SHA through a non-public engine slot:
   full capability matrix per `docs/CHANGE_CHECKLISTS.md` «Новая модель» with the
   `$0.0001` admission cap, on every owned paid plan type.
3. Publication commit only after that gate is green: systemd env, router presets,
   `packages/contracts` (new additive capability generation — frozen generation 4 stays
   rejected forever), web catalog, admin, provider doc update.
4. Periodic bounded re-probe of `gemini-2.5-pro` (503 "no capacity" is transient by
   definition) and `gemini-3.5-flash-lite`.

## Secret-hygiene confirmation

- [x] probe OAuth tokens were used only in-process and deleted after each session;
- [x] no bearer/refresh token, cookie or credential envelope was copied anywhere;
- [x] no raw email/account/subject/project id retained (project recorded as `<set>`);
- [x] no customer traffic, key, balance or engine slot was touched;
- [x] all prompts were synthetic one-liners; no private error bodies retained;
- [x] all evidence above uses bounded status/usage classes only.
