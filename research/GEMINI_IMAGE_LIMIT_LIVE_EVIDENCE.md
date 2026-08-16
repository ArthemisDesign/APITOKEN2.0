# Gemini image-generation limit evidence — 2026-08-16

> **Route correction (2026-08-16):** the 429 fleet result below measured the retained gateway's
> then-pinned `cloudcode-pa.googleapis.com` image origin. Current first-party Antigravity CLI 1.1.10
> was subsequently observed using `daily-cloudcode-pa.googleapis.com` for the same exact
> `gemini-3.1-flash-image` model. Production now uses that daily origin and has generated a verified
> 1K image on every routable subscription. The old result is evidence about its route/error window,
> not evidence that the subscriptions lack image capability.

## Conclusion

The corrected production route generated a real image on **7 of 7** routable subscriptions: six
Google AI Pro profiles and one Google AI Ultra profile. This disproves the earlier fleet-wide
interpretation that none of the subscriptions could generate images. The strongest causal evidence
is the corrected origin followed by fleet success, but endpoint alone is not proven to be the sole
cause: the local AGY account differed from the production roster and Google capacity also changed
over time.

The evidence does **not** establish a separate per-profile image quota. Google continued to project
the same positive catalog fraction for image and text model rows. The old origin produced an
image-specific shared rate/backend wall while text worked; the corrected origin admitted image
turns. The safe conclusion is therefore a route/backend-specific admission incident, not exhausted
independent image allowances and not a durable undocumented numeric RPM limit.

## Corrected-route production evidence

The image origin fix was deployed in GREEN master `b782c04b`. Bounded exact-profile probes then used
one free `countTokens` preflight per paid generation, no profile rotation, UUIDv4 immutable-event
attribution, terminal usage parity, and a resolved post-turn provider quota snapshot.

- All **7/7** routable profiles returned HTTP 200 with one real `inlineData` image part and exactly
  **1,120 image output tokens** for a 1K request.
- Actual cost per successful image was **$0.0675425–$0.0679780**; total for the seven successful
  images was **$0.4745545**.
- The first pass proved four profiles. Two requests exceeded the original 240-second client timeout
  without a terminal event or image billing, and one returned HTTP 503 without image billing.
- At the person's explicit request, only those three profiles were checked again. All succeeded;
  the slowest completed in **256.8 seconds**, proving that 240 seconds was not a viable image probe
  timeout. The runner now uses a 600-second transport default without adding retries.
- Cheap text controls on the same three profiles had already returned terminal responses with
  authoritative event parity and cost **$0.0002727** in total, isolating the initial failures from
  general subscription health.
- The initial 128-output-token route probe settled a text-only `MAX_TOKENS` response without an
  image and cost **$0.0003960**. It is a harness-ceiling artifact, not an image failure. Image legs
  now reserve 4,096 output tokens.
- Total spend after the route correction, including the seven images, three text controls, and the
  deliberately retained text-only ceiling artifact, was **$0.4752232**. Free `countTokens` calls and
  the HTTP 503/transport-ambiguous attempts produced no image billing.

Every claimed success is tied to its exact calibration UUID and matching immutable usage. Four of
seven successful probes observed no other same-profile request id in their evidence window; three
overlapped unrelated production activity, so this result proves exact turn attribution but does not
claim that the whole fleet was idle.

## Historical old-route production evidence

Read-only analysis covered the production Gemini journal from 2026-07-25 through 2026-08-16 and the
protected sanitized capacity projection. No raw provider bodies, customer content, credential,
email, project, proxy, or stable profile identity was retained.

- 429 generation attempts: **429**, covering **230** customer request ids and **7** routable profiles.
- Every generation 429 named only `gemini-3.1-flash-image`.
- 32 requests exhausted rotation; 25 reached all 7/7 profiles.
- Every event carried a fresh matching catalog row with one positive bucket, zero zero/unknown
  buckets, and **89.35–100% remaining**.
- Provider status was `RESOURCE_EXHAUSTED`; no `QuotaFailure`, named quota subject, or retry hint was
  safely extractable. Identical process-local error shapes crossed profiles within each runtime
  lifetime.
- The authoritative recent-turn ring contained 512 successful text turns on the same seven profiles
  and six text models; 434 text successes overlapped the image-429 window.

These facts contradict independent exhausted subscription quotas and support a common image-only
wall while the text surface remains usable.

## Controlled live reproduction

The retained calibration runner was first repaired to send the runtime's exact image contract
`responseModalities=[TEXT,IMAGE]` (GREEN master `b96e78a7`). The first paid text control then exposed
that the protected preflight catalog ignored hot tariff overrides; the runner correctly stopped after
immutable settlement rather than accepting a mismatched money identity. The projection was repaired
to publish the same effective prices and `<family>/v<version>` identity as request admission (GREEN
master `4e8fe9ba`). The already-paid turn was not repeated.

After the second fix, exact-profile one-shot probes used the production SSH transport, free
`countTokens` preflight, immutable request attribution, and post-turn provider quota snapshots:

| Plan | Request | Result | Catalog remaining at refusal | Text control |
|---|---|---|---:|---|
| Google AI Ultra | 1K image | HTTP 429 `RESOURCE_EXHAUSTED` | 99.97% | text turn settled immediately before it |
| Google AI Pro | 1K image | HTTP 429 `RESOURCE_EXHAUSTED` | 100.00% | subsequent text turn returned full terminal proof |

Both 429s occurred on their exact target only; neither image request was retried or rotated. The Pro
text control on the same exact profile returned canonical model identity, real visible output,
terminal `STOP`, authoritative usage equal to the immutable event, a resolved post-turn quota
snapshot, and no foreign same-profile turn. Its actual official-API equivalent was 96,500 nanoUSD
($0.0000965). The Ultra text turn settled 51,700 nanoUSD but hit its deliberately small 128-token
output cap, so it is retained as paid evidence but not counted as full response coverage.

Those two old-route live refusals additionally logged fresh positive catalog evidence with the same
image-only safe error shape as the historical incidents. No successful image was billed during that
old-route experiment; corrected-route successes are recorded separately above.

## What the catalog means

Within each current subscription snapshot, the catalog publishes the same remaining fraction and
reset time for `gemini-3.1-flash-image`, `gemini-2.5-flash-lite`, and the other Gemini model rows.
Consequently the apparent image “bucket” is a model-labelled projection of a shared Gemini catalog
fraction, not evidence of a separately decrementing image allowance. The independent behavior is in
generation admission/backend limiting: image can be rejected while text succeeds.

## Operator consequence

- Do not interpret positive image catalog quota as proof that image generation is presently usable.
- Do not fan one image request across the fleet on this error shape: it multiplies identical upstream
  attempts without escaping the shared wall.
- Treat image health/rate limiting separately from text health and keep the short RPM-style cooling.
- A future provider change can alter this behavior; retain bounded 429 diagnostics and exact-profile
  canaries rather than hard-coding an undocumented numeric Google limit.

## 2026-08-16 — three distinct 429 shapes and the self-block fix

Load testing on the corrected route (≈$21.60 of image generation, ~350 images) separated three
provider 429 signals that the gateway previously collapsed into one long cooling:

| `error_reason` | observed hint | catalogue | verdict |
|---|---|---|---|
| `RATE_LIMIT_EXCEEDED` | 1–4 s | ~99% | honest per-concurrency throttle; short cool is correct |
| transient `other` | 1376 s (~23 min) | ~99% | **false**: the same account kept generating via the official Antigravity client while the gateway parked it |
| `QUOTA_EXHAUSTED` | up to ~14 690 s (~4 h) | 88–99% | **honest**: a direct origin bypass reproduces the refusal immediately |

The catalogue `remainingFraction` never tracks the image allowance that `QUOTA_EXHAUSTED` measures:
it sat at 88–99% throughout, even as profiles hit genuine exhaustion.

Two production defects were found and fixed:

- **Self-block on transient hinted 429** (`dd78c81c`): the gateway honoured any retry hint
  verbatim, so a 23-minute `RetryInfo` on a non-exhaustion reason parked a healthy profile. A new
  `generation_429_cool_secs` now honours a hint in full only for `QUOTA_EXHAUSTED` and caps any
  other hinted 429 at `rate_limit_unknown_cool_secs` (default 60 s). Live validation: during the
  final drain the handler applied `QUOTA_EXHAUSTED` hints verbatim while keeping
  `RATE_LIMIT_EXCEEDED` short.
- **Cooling lost on blue-green restart** (`c8ad9bbd`): cooling deadlines lived only in process
  memory, so each slot swap forgot genuine exhaustion and burned a live request rediscovering it.
  Deadlines now persist to `gemini-cooldown-state.json` in the writable data directory and only
  still-active ones are restored.

Measured ceilings: the fleet sustains roughly 18 concurrent image requests before
`RATE_LIMIT_EXCEEDED` appears (≈2–3 per Pro profile); Ultra tolerates about 4–6 concurrent and a
far larger image allowance than Pro before its own genuine `QUOTA_EXHAUSTED`.
