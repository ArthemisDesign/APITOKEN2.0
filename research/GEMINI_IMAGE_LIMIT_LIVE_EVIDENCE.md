# Gemini image-generation limit evidence — 2026-08-16

## Conclusion

Production evidence strongly supports an image-specific shared Google limit or backend wall that is
independent of the ordinary text-generation path. It does **not** establish a separate per-profile
catalog quota: Google advertised the same positive catalog fraction for image and text models, while
image generation alone returned `RESOURCE_EXHAUSTED` across profiles.

The safe evidence cannot distinguish a hidden global RPM/concurrency/policy limit from shared image
backend capacity. Therefore the operationally correct classification is **image-specific shared
rate/backend wall**, not “each subscription exhausted its image quota.”

## Historical production evidence

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

The two current image refusals additionally logged fresh positive catalog evidence with the same
image-only safe error shape as the historical incidents. No successful image was billed during this
experiment.

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
