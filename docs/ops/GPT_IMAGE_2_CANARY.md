# GPT Image 2 private Codex OAuth canary

This runbook covers the private exact-profile canary for the dormant Stage 1 GPT Image 2
implementation. It uses the existing sealed Codex OAuth roster and `CodexConfig`; it does not add an
image API key, image origin, or image-specific environment variable. It is not an `AppState` member,
HTTP/customer route, catalog entry, router preset, billing path, production default, public
capability, or publication authorization.

The command is a blocked dry-run surface. It validates an exact-profile generation/edit plan, but
`--execute` fails closed before Codex configuration, `/wham/usage`, or an image request. The fixed
provider wire requires `quality: "auto"` and `size: "auto"`; neither the replacement-price estimate nor
a caller-supplied `--budget-nanousd` proves an enforceable worst-case charge bound. The dormant
execution and durable evidence code remains behind that explicit Stage 1 blocker.

## Exact wire and scope

The canary reuses the normal Codex provider configuration, OAuth profile, proxy, client, refresh, and
pool state. The image request goes to exactly one of:

- `POST {CodexConfig.base_url}/images/generations` for a prompt without references;
- `POST {CodexConfig.base_url}/images/edits` when `--reference` is supplied one to five times.

Both are JSON requests. The implementation always supplies `model: "gpt-image-2"` and the automatic
controls `background: "auto"`, `quality: "auto"`, and `size: "auto"`. An edit additionally supplies
`images`, each as `{"image_url":"data:image/png;base64,..."}`. Stage 1 deliberately exposes no
mask, streaming, JPEG/WebP output, arbitrary size/quality/background, input-fidelity, batch count, or
other image controls.

The request inherits the existing Codex native wire headers: OAuth `Authorization: Bearer ...`,
`ChatGPT-Account-ID`, `originator`, pinned Codex `User-Agent`, and pinned `version`. Image requests add
one fresh `x-codex-image-turn-id`. No credential, account id, prompt, reference bytes, or private path
is printed in the plan/checkpoint.

The reusable `forward::codex::images` API supports the existing automatic home selection and holds a
normal `TurnSlot` for the complete attempt. Automatic execution may move to another home only after a
final response proves a pre-execution account rejection: the first `401` gets the existing single
forced refresh and one same-home retry, and only the resulting `401/403` or a final `429` is eligible
for automatic rotation. A `400/404/409/422`, any other status, an invalid success body, timeout,
connection/body failure, or any otherwise ambiguous outcome is terminal and is never replayed. The
canary uses the exact-home variants, so even a final auth/quota rejection is returned for the selected
profile rather than moving the paid call to another profile.

A successful response must contain a bounded, decodable PNG in `data[0].b64_json` plus valid
`created`, `background`, `quality`, and `size` metadata. `output_format`, when present, must be `png`.
Provider `usage` is optional: the canary records only its allow-listed numeric projection when it is
present. Missing usage remains missing. Stage 1 does not invent usage, feed the dormant image tariff
into customer metering, reserve or settle money, or interpret the response as ChatGPT native credits
or billing.

## Local file contract

Local request validation is complete before the blocked dry-run plan:

- `--profile` must be an opaque valid Codex roster profile id, never an email/account id.
- `--prompt-file` must be a regular non-symlink Unix file with exact mode `0600`, stable across
  inspection/open, containing nonempty UTF-8 of at most 512 bytes and 512 Unicode characters.
- Each repeated `--reference` must be a stable regular non-symlink PNG, 1..=16 MiB. The decoder rejects
  animation, dimensions outside 1..=4096, or decoded size above 16 MiB. At most five references are
  accepted; aggregate transport and decoded limits are enforced by `forward`.
- `--output` must end in `.png`; `--checkpoint` must end in `.json` and have a UTF-8 basename. The two
  paths must differ, must not already exist, and must not be symlinks.
- Each target parent must already exist as a non-symlink directory and must not be group- or
  world-writable. Parent identity and target nonexistence are rechecked before publication.
- Output and checkpoint are written through exclusive private temporary files, synced, published
  without overwrite as one recoverable pair, and left with exact mode `0600`.

Dry-run performs no network request, does not read the Codex environment/configuration, and creates no
artifact. It prints one sanitized JSON plan with `state: "blocked"`, `executable: false`, exact profile,
operation, prompt/reference counts, budget, estimate, default cap, blocker, and compile-time
implementation SHA when available.

## Budget estimate

The integer estimate uses the dormant official OpenAI replacement tariff only:

```text
prompt UTF-8 bytes × fresh text-input rate
+ reference PNG bytes × fresh image-input rate
+ 196 × image-output rate
```

Treating encoded input bytes as image tokens is deliberately conservative for this narrow canary, and
all arithmetic is checked integer nanoUSD. It is still not an authoritative token count, image
`countTokens`, a reserve proof, a ChatGPT credit quote, or a maximum provider charge. The transport
cannot stop an already dispatched generation when this estimate is reached. A numeric
`--budget-nanousd` therefore records intent only and cannot unlock execution.

## Blocked dry-run

Generation plan:

```bash
cargo run -p claude-api -- openai-image-canary \
  --profile opaque_profile_id \
  --prompt-file /private/canary/prompt.txt \
  --output /private/canary/result.png \
  --checkpoint /private/canary/checkpoint.json \
  --budget-nanousd 9000000
```

Edit plan (repeat `--reference` up to five times):

```bash
cargo run -p claude-api -- openai-image-canary \
  --profile opaque_profile_id \
  --prompt-file /private/canary/prompt.txt \
  --reference /private/canary/reference-1.png \
  --reference /private/canary/reference-2.png \
  --output /private/canary/result.png \
  --checkpoint /private/canary/checkpoint.json \
  --budget-nanousd 150000000
```

Do not add `--execute` in Stage 1: it requires an exact compile-time implementation SHA and then returns
the stable blocker `stage1_paid_dispatch_blocked_until_quality_auto_size_auto_worst_case_is_proven`
before loading settings or touching the network. The dormant path behind that gate constructs the
existing Codex gateway, runs the free exact-profile `/wham/usage` auth/quota preflight, holds one
`TurnSlot`, and journals one non-replayed generation/edit attempt. It must not be enabled merely by
raising the caller budget.

If a later reviewed change proves an enforceable ceiling and enables the gate, successful evidence is
designed to bind operation/profile/model, dimensions, provider metadata, optional sanitized usage,
sanitized request id, output SHA-256, exact implementation SHA, numeric budget, and estimate without
copying prompt/reference data into logs.

## Remaining gate

No live generation or edit was performed in this worktree. Stage 1 remains private and dormant.
Before publication, a separate change and controlled live run on an exact clean GREEN implementation
SHA must still prove real image output, the authoritative terminal usage/credit behavior (or explicitly
establish that no usable authority exists), every capability proposed for publication, and safe
non-duplicating failure semantics. The generic model gate also requires incremental SSE; this Stage 1
wire intentionally advertises no streaming and does not satisfy that requirement.

Until those blockers close, do not add GPT Image 2 to `AppState`, HTTP/customer routes, public docs,
model/catalog/pricing releases, router presets, storefronts, systemd/defaults, or billing/settlement.
