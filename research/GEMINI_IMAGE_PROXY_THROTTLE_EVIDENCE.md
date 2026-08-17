# Gemini image-generation limits: proxy throttle, not account quota

Date: 2026-08-16 → 2026-08-17
Scope: production Gemini OAuth pool, image model `gemini-3.1-flash-image`
Status: complete; root cause identified and reproduced
Related: `research/GEMINI_IMAGE_LIMIT_LIVE_EVIDENCE.md`, `docs/engine/GEMINI_PROVIDER.md`,
`docs/ops/GEMINI_CALIBRATION.md`

This document records the full investigation arc, the controlled experiments, and the final
root cause. All identifiers are sanitised: profiles are P1..P8 / plan labels, no tokens, emails,
project ids, proxy urls, or customer content appear here.

## Executive summary

What looked like a per-subscription image quota on Gemini is, in fact, **throttling of the
per-profile proxy IP**. Google returns 429 (including a convincing `QUOTA_EXHAUSTED` with a
multi-hour `RetryInfo`) for requests that exit through a given profile's proxy, while the very
same account, on the same origin and endpoint, succeeds immediately when the request goes direct.

The practical consequence: the image "limits" we measured earlier measured the proxy, not the
subscription. The account has far more image capacity than the proxied path ever showed.

## The question

Does `gemini-3.1-flash-image` have its own quota separate from text Gemini models, and why did the
official Antigravity client (AGY) keep generating images while our production gateway reported the
subscription as exhausted?

## Method (controlled, budget-capped, evidence-backed)

- Paid image generation ran through the retained exact-profile calibration runner against the live
  production pool, one request per measured turn, no retry of a settled request id, with a hard
  aggregate budget (raised from $20 to $30, then $50; final spend ≈ **$27.71**).
- Each turn was verified against the immutable turn event (authoritative usage) and a post-turn
  quota snapshot; ambiguous in-flight attempts were reserved conservatively.
- A load phase drove concurrency waves (ramped and fixed) to find admission ceilings.
- To separate gateway behaviour from Google behaviour, requests were also issued **directly to the
  origin from the server**, bypassing the gateway selector and any local cooldown, never printing
  tokens, bodies, or images.

## Timeline and findings

### 1. Routing drift (fixed earlier)

The local official AGY used `https://daily-cloudcode-pa.googleapis.com/v1internal:generateContent`,
while production still pointed at a legacy origin. After aligning the route, all seven fleet
profiles generated real images (7/7). Prior `0 of 7` was an artefact of the stale origin, not a
quota signal. This stage is documented in `research/GEMINI_IMAGE_LIMIT_LIVE_EVIDENCE.md`.

### 2. Three distinct 429 shapes (initial model)

Under load, Google's 429s separated into:

| `error_reason` | hint | catalogue remaining | initial reading |
|---|---|---|---|
| `RATE_LIMIT_EXCEEDED` | 1–4 s | ~99% | honest per-concurrency throttle |
| transient `other` | ~23 min | ~99% | suspicious |
| `QUOTA_EXHAUSTED` | up to ~4 h | 88–99% | treated as honest exhaustion |

A first "confirmation" of `QUOTA_EXHAUSTED` used a direct origin request and got the same refusal,
so it was classified as account-truth. That reading was wrong (see §4): the direct probe itself
went through the same proxy.

### 3. Two gateway defects found and fixed

- **Self-block on transient hinted 429** (`dd78c81c`): any retry hint was honoured verbatim, so a
  ~23-minute hint on a non-exhaustion reason parked a healthy profile. Fix: `generation_429_cool_secs`
  honours a hint in full only for `QUOTA_EXHAUSTED` and caps other hinted 429s at
  `rate_limit_unknown_cool_secs` (default 60 s).
- **Cooling lost on blue-green restart** (`c8ad9bbd`): cooling deadlines lived only in process
  memory, so each slot swap forgot genuine exhaustion and burned a live request rediscovering it.
  Fix: deadlines persist to `gemini-cooldown-state.json` in the writable data directory; only
  still-active deadlines are restored.

Both shipped through the standard merge gate with `deploy/watchdog` GREEN.

### 4. The decisive A/B/C/D experiment (root cause)

On a single Ultra profile, same origin (`daily-cloudcode-pa.googleapis.com`), same endpoint
(`/v1internal:generateContent`), within one minute:

| path | network | result |
|---|---|---|
| production gateway | profile proxy | `429 QUOTA_EXHAUSTED`, 1672 s hint |
| direct from server | **profile proxy** | `429 QUOTA_EXHAUSTED`, 1342 s hint |
| AGY on a laptop | **direct** | **200, image in ~34 s** |
| direct from server | **direct (proxy removed)** | **200, image in ~8 s** |

Origin, endpoint, headers, and body were identical across all four. The only variable that changed
the outcome was the proxy. The earlier "confirmation" of `QUOTA_EXHAUSTED` was invalid because the
direct probe reused the profile's proxy; removing the proxy turned the same account's refusal into
an immediate success.

### 5. Conclusion

- Google throttles image generation **per proxy IP**, not per subscription. Even `QUOTA_EXHAUSTED`
  is proxy-scoped, not account-scoped.
- The catalogue `remainingFraction` does not track this: it stayed at 88–99% throughout, including
  during genuine-looking refusals.
- The per-account image ceiling was therefore never truly measured; every ceiling we hit was the
  proxy's.

## What this changes operationally

- A fleet-wide or per-profile image "exhaustion" observed through the proxy must not be read as the
  subscription's real capacity.
- The previously recorded concurrency ceilings (~18 fleet-wide, ~4–6 on Ultra) describe the proxied
  path, not the account.
- The cooldown fixes remain correct in kind: honouring `QUOTA_EXHAUSTED` verbatim is right, because
  it is an authoritative provider refusal for that network path — it is just not a statement about
  the account.

## Follow-ups (not done here)

- Confirm across the fleet that each profile's "exhaustion" maps to its proxy IP (log correlation).
- Evaluate proxy IP rotation / a proxy pool per profile so a 429 on one IP does not park the
  profile.
- If a true per-account image limit is ever needed, measure it on the direct path, not via proxy.
