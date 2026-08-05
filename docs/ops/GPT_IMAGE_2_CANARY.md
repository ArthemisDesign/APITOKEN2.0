# GPT Image 2 private Codex OAuth canary

This runbook covers the private GPT Image 2 canary through the existing sealed Codex OAuth pool. It
adds no image API key, relay, image origin, or image-specific environment variable. It is not an
`AppState` member, HTTP/customer route, catalog entry, router preset, billing path, production default,
public capability, or publication authorization.

Generation and edit use the native subscription endpoints:

- `POST {CodexConfig.base_url}/images/generations` for a prompt without references;
- `POST {CodexConfig.base_url}/images/edits` when `--reference` is supplied one to five times.

Both request `model: "gpt-image-2"`, `background: "opaque"`, `quality: "low"`, and
`size: "1024x1024"`. Edit adds strict PNG data URLs under `images[].image_url`. The current native
Codex request proves these controls and reference-guided edit; it does not prove masks, partial-image
streaming, JPEG/WebP output, output compression, multiple outputs, or Responses API multi-turn image
state. Those features remain absent rather than being silently ignored.

## Pool and retry contract

The canary uses the normal Codex configuration, sealed OAuth roster, proxy, refresh family, official
client identity, health and quota admission. `--profile` may name an opaque roster id; if omitted, the
runner selects the first currently admitted opaque id without dispatching an image request. It then
freezes that id, runs the existing free `/wham/usage` OAuth/quota preflight, and performs at most one
logical exact-home image attempt.

A received first `401` permits the existing single forced refresh and one same-home replay with the
same `x-codex-image-turn-id`. The canary never rotates the paid call to another home. A final auth/quota
rejection, client rejection, unexpected status, invalid success, timeout, connection/body failure, or
other ambiguous outcome is terminal. An ambiguous attempt is never replayed.

## Fixed generation ceiling

The private generation request has a conservative OpenAI API replacement-price ceiling of
`8_560_000` nanoUSD (`$0.00856`):

```text
low 1024x1024 output                         $0.00600
512 prompt bytes × $5 / 1M, treated as tokens  0.00256
                                                    -------
maximum authorized replacement estimate       $0.00856
```

This deliberately treats every allowed UTF-8 prompt byte as one fresh text token. It is not ChatGPT
native-credit pricing, a customer reserve, or settlement authority. The runner accepts only integer
`--budget-nanousd`, requires it to exceed the repository default `100000` nanoUSD, and permits paid
generation only when it is at least `8560000`.

There is no corresponding normative edit ceiling yet. GPT Image 2 processes image inputs at high
fidelity, but the native wire has no free image token counter and stored PNG bytes are not a valid token
bound. Any request with references therefore reports
`paid_dispatch_requires_edit_ceiling_proof` and fails before configuration or network access even when
the caller supplies a larger budget.

## Local file and evidence contract

Validation happens before any network access:

- `--profile`, when present, must be an opaque Codex roster profile id, never an email/account id.
- `--prompt-file` must be a stable regular non-symlink Unix file with exact mode `0600`, containing
  nonempty UTF-8 of at most 512 bytes and 512 Unicode characters.
- Each repeated `--reference` must be a stable regular non-symlink PNG, 1..=16 MiB. Animation,
  dimensions outside 1..=4096, decoded size above 16 MiB, more than five references, and aggregate
  transport/decoded overflow are rejected.
- `--output` must end in `.png`; `--checkpoint` must end in `.json` and have a UTF-8 basename. They must
  differ and not already exist. Their parent directories must already exist, be non-symlinks, and not
  be group- or world-writable.
- Internal recovery state is created in an exclusive mode-`0700` run directory. Artifacts are written
  and synced as mode-`0600`, then externally published without overwrite.

Dry-run omits `--execute`. It reads no Codex environment/configuration, makes no request, and creates no
artifact. Its sanitized JSON plan contains the operation, profile selector (`auto-admitted` when
omitted), fixed controls, counts, authorization budget, required ceiling, blocker, and compile-time SHA
when available; it never contains prompt text, references, credentials, account identity, or paths.

Paid generation additionally requires an exact lowercase 40-hex compile-time
`CLAUDE_API_IMPLEMENTATION_SHA`. A successful checkpoint requires all of:

- exact frozen home and image turn identity;
- one bounded, fully decoded PNG;
- dimensions `1024x1024`;
- returned `background=opaque`, `quality=low`, and `size=1024x1024`; `output_format`, when present,
  must be `png`;
- terminal numeric usage after allow-list sanitization;
- the locally generated image turn id; a sanitized provider request-id header is retained when present
  but is optional because the official Codex `ImageResponse` contract does not require one;
- output SHA-256 and exact implementation SHA.

Missing usage or mismatched home, local turn, or controls receive a specific `evidence_*` journal state.
For a parsed mismatch, the private mode-`0600` journal retains only returned exact-home/turn flags,
dimensions, timestamp, controls, allow-listed numeric usage, sanitized optional request id, and image
SHA-256. The rejected image bytes are neither persisted nor published, and the journal is diagnostic—not
publication evidence. Prepared, transport-error, and success journals do not contain this `returned`
object; successful authoritative evidence remains in the separate checkpoint.

## Commands

Generation dry-run, allowing the pool to select and freeze an admitted home:

```bash
claude-api openai-image-canary \
  --prompt-file /private/canary/prompt.txt \
  --output /private/canary/result.png \
  --checkpoint /private/canary/checkpoint.json \
  --budget-nanousd 8560000
```

The first implementation SHA `3f67d43c0ae541979fee66823d251e2e3eea33e0` is deployed and
watchdog-GREEN, but its separately authorized `$0.00856` attempt is complete and withdrawn. Controller
delivery `7a334604f41c367e898073567a7aa0d481614839` stopped before network access because it tried to source
systemd syntax from `config.env` as Bash. Delivery
`2c7dabcce85be9a691597d6b5ab765fe4868a3b6` reached a parsed image result but withheld it because the
provider supplied no optional request-id header. Its exact recovery root is a terminal non-replay fence.

Corrected implementation SHA `012fccc471142fc51a46563da3a87564d674b39f` is independently
watchdog-GREEN. Delivery `d7b394fc5e6b9b603e1e0ab3982038f5479ba2e8` then made its one
authorized `$0.00856` generation through the sealed pool. The endpoint returned a parsed image, but the
returned metadata did not exactly echo `opaque/low/1024x1024`; the canary recorded
`evidence_controls_mismatch`, published neither PNG nor checkpoint, and permanently fenced the attempt.
That older recovery journal intentionally contains no returned controls or usage, so the attempt cannot
prove which field differed and cannot be promoted as partial evidence. The controller now performs only a
non-network verification of that exact SHA/budget/journal and the absence of all output artifacts; it
cannot replay the paid call. The diagnostic journal contract described above applies only to a future
attempt running an implementation that contains it; it does not retroactively enrich this fenced journal.

Edit can be validated as a blocked plan by repeating `--reference` up to five times. Do not add
`--execute` until a reviewed normative input-image ceiling and separate numeric authorization exist.

## Publication gate

The original implementation remains deployed, but its production generation was withdrawn without
GREEN evidence and no live edit was performed. Before publication, a corrected implementation and a new
controlled production run must prove generation 2xx, real PNG, terminal usage, returned controls, local
turn attribution, and
non-duplicating failure semantics. A separately bounded live edit must then prove the data-URL reference
wire and every edit control to be advertised.

The public Image API guide documents partial-image streaming, masks, output formats and Responses API
multi-turn editing, but the current native Codex subscription client does not expose those request
fields. They cannot be claimed for this pool without separate native-wire and live proof. Until every
applicable gate closes, do not add GPT Image 2 to customer routes, public models/catalogs, pricing
releases, router presets, OpenKeys, the website, public docs, admin, systemd defaults, or billing.
