# apiToken.sale OpenCode router plugin

The canonical config plugin for OpenCode. On startup it fetches the personal `/v1/models`,
translates the authoritative limits/capabilities and current prices into OpenCode's native model schema, and
adds Fast entries with the original model ID only when the `priority` tier is published. Modalities,
attachments, tool calling, structured output, reasoning, and variants come from
`apitoken.capabilities`; there are no heuristics based on `owned_by` or a substring in the model ID. Router-owned presets
are not added to the OpenCode provider list: they have a dynamic model and a variable price.

You can install the plugin by copying `apitoken-router.js` to
`~/.config/opencode/plugin/apitoken-router.js` (or into the auto-load directory
`~/.config/opencode/plugins/`). For clients, the site offers a one-click installer
`https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/install.sh`,
which downloads the published copy of this file from the
`apitokensale-admin/apitoken.sale` repository (`opencode/apitoken-router.js`) and adds the
`apitoken` provider to `~/.config/opencode/opencode.jsonc`. When the plugin changes, the published
copy must be updated in the same release. The `apitoken` provider in `opencode.jsonc` must
use `@ai-sdk/openai-compatible`, `https://router.apitoken.sale/v1`, and a literal
`sk-pool-…` key or the standard OpenCode placeholder `{env:NAME}`.

The plugin advertises only text output for all models. This is a deliberate OpenCode 1.18.11 limitation:
its `@ai-sdk/openai-compatible` 2.0.41 does not decode native Gemini `inlineData` and does not accept
OpenRouter image metadata in a Chat message. Native image generation in the gateway is not disabled —
`google/gemini-3.1-flash-image` must be called through the Gemini
`generateContent`/`streamGenerateContent`, where the image is returned in
`candidates[].content.parts[].inlineData`.

During a temporary catalog outage the plugin can restore only capability metadata from the
local last-good cache schema v2 (`catalog-v2.json`). The snapshot is AES-256-GCM encrypted and bound
to the exact credential/base URL,
has mode `0600`, a 15-minute freshness TTL, and a maximum stale age of 7 days. Cached models are explicitly
marked `[stale metadata; pricing unavailable]`; `cost` is not cached and is not shown in OpenCode until the next successful
live discovery. A different key, a different URL, an expired, corrupted, or unknown-version cache is rejected; the old v1 is not reused because it lacks
authoritative modality/control fields.

Check:

```bash
pnpm --filter @claude-api/opencode-router-plugin test
```

The plugin file intentionally has exactly one ESM export — a default factory. OpenCode 1.18.11 tries to
load every export of the module as a plugin factory, so even a test-only named export breaks the whole
provider at startup; the export shape is pinned by a unit test and a real `opencode models apitoken` smoke.
