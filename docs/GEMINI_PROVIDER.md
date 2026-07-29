# Native Gemini paid-project provider

Gemini is a third, isolated provider surface. It does not share a process, router, credential state,
or capacity pool with Claude or OpenAI/Codex:

```text
gemini.api.apitoken.sale
        │ native /v1beta only
        ▼
Caddy 127.0.0.1:8794
        ▼
claude-api-gemini.service (runtime 127.0.0.1:8795)
        │ CLAUDE_API_PROVIDER=gemini
        ▼
N paid Google Developer API projects
        ▼
https://generativelanguage.googleapis.com
```

The three engine processes share only the fenced PostgreSQL billing authority, customer keys, and
ledger. A Gemini fault cannot change Claude/OpenAI routing, and no request header can select another
provider process.

## Credential policy: paid API only

Each profile is an API key belonging to a billing-enabled Google Cloud/AI Studio project. Google
applies Developer API limits per project, so two keys from the same project are not independent
capacity and are rejected as a duplicate quota domain.

Gemini CLI / Google AI consumer OAuth is deliberately unsupported. It is not an upstream credential
for this gateway: the Gemini CLI terms restrict those credentials to Gemini CLI, and the CLI FAQ
distinguishes subscription entitlement from Developer API billing. Never copy OAuth tokens,
`oauth_creds.json`, browser cookies, or Gemini CLI auth stores into this service.

Authoritative references:

- [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing)
- [Gemini API rate limits](https://ai.google.dev/gemini-api/docs/rate-limits)
- [Native GenerateContent API](https://ai.google.dev/api/generate-content)
- [Models API](https://ai.google.dev/api/models)
- [Gemini CLI terms and privacy](https://github.com/google-gemini/gemini-cli/blob/v0.53.0/docs/resources/tos-privacy.md)
- [Gemini CLI FAQ](https://github.com/google-gemini/gemini-cli/blob/v0.53.0/docs/resources/faq.md)

## Provision a multi-project pool

Create an operator-owned directory and one file per credential. The gateway accepts only an absolute
path to a regular, non-symlink file inaccessible to group and other users.

```bash
sudo install -d -o deploy -g deploy -m 0700 /srv/claude-api/data/gemini/keys
sudo install -o deploy -g deploy -m 0600 /dev/stdin \
  /srv/claude-api/data/gemini/keys/gemini_01
sudo install -o deploy -g deploy -m 0600 /dev/stdin \
  /srv/claude-api/data/gemini/keys/gemini_02
```

Write `/srv/claude-api/data/gemini/profiles.json` without embedding keys:

```json
{
  "profiles": [
    {
      "id": "gemini_01",
      "project_id": "paid-project-one",
      "api_key_file": "/srv/claude-api/data/gemini/keys/gemini_01",
      "proxy": "http://proxy.example:8080"
    },
    {
      "id": "gemini_02",
      "project_id": "paid-project-two",
      "api_key_file": "/srv/claude-api/data/gemini/keys/gemini_02"
    }
  ]
}
```

Set the profiles document itself to mode 0600 as well: an optional proxy URL may contain credentials,
so the gateway rejects a symlink or a file readable by group/other users.

Profile `id` is the only identity exported to metrics. Keep it stable and non-identifying. A project
ID is normalized with `trim + lowercase`, validated as a Google Cloud project ID, used only for
duplicate detection, and never logged or exported. A per-profile proxy is optional; every profile
always owns a separate HTTP client, connection pool, in-flight count, cooling state, and auth state.

Set the shared environment file:

```bash
CLAUDE_API_GEMINI_ENABLED=1
CLAUDE_API_GEMINI_PROFILES_FILE=/srv/claude-api/data/gemini/profiles.json
CLAUDE_API_GEMINI_MODELS=gemini-3.6-flash,gemini-3.5-flash,gemini-3.1-flash-lite,gemini-2.5-pro,gemini-2.5-flash,gemini-2.5-flash-lite
```

Production upstream is pinned to `https://generativelanguage.googleapis.com`. Arbitrary hosts,
userinfo, paths, query strings, and fragments are rejected at startup. Literal HTTP loopback is
available only for tests with `CLAUDE_API_GEMINI_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1`.

## Client contract

Use the same apitoken customer key and billing account as the other surfaces, but send requests only
to the Gemini hostname. Header authentication is supported in native Google form:

```http
x-goog-api-key: sk-pool-...
```

Existing `x-api-key` and `Authorization: Bearer ...` customer forms also authorize the gateway. The
customer credential is stripped before forwarding; Google receives only the selected profile key.
`?key=`, `?api_key=`, case variants, and percent-encoded variants are rejected.

Supported native endpoints:

- `GET /v1beta/models`
- `GET /v1beta/models/{model}`
- `POST /v1beta/models/{model}:generateContent`
- `POST /v1beta/models/{model}:streamGenerateContent`
- `POST /v1beta/models/{model}:countTokens`

All other paths return a Gemini-shaped `404`; the public Caddy vhost allows only `/v1beta/*`,
`/health`, and `/balance`. OpenAI compatibility is intentionally not part of this provider version.

```bash
curl https://gemini.api.apitoken.sale/v1beta/models/gemini-2.5-flash:streamGenerateContent \
  -H "x-goog-api-key: $APITOKEN_KEY" \
  -H 'content-type: application/json' \
  -d '{"contents":[{"role":"user","parts":[{"text":"2+2?"}]}]}'
```

The gateway forces `alt=sse` for streaming and preserves upstream event bytes. It may rotate only
before the first upstream chunk. After returning the response, a disconnect never triggers another
attempt; a detached task continues draining Google to final `usageMetadata` for exact settlement.

## Rotation and fault ownership

| Upstream result | Profile state | Pool behavior | Final public result |
|---|---|---|---|
| `2xx` | healthy; cooling cleared | return/stream | native success |
| `429 RESOURCE_EXHAUSTED` | cool by `Retry-After`, `google.rpc.RetryInfo`, or default | try another project without spending transport budget | one Gemini `429` |
| `401` / `403` | unauthenticated quarantine | try another project | Gemini `503` if all failed by auth/backend |
| network, `408`, `409`, `425`, `5xx` | short transport cooling | rotate within transport retry budget | Gemini `503` |
| other `4xx` | remains healthy | no rotation; deterministic request/model/safety error | original Google response |
| all profiles already cooling | unchanged | none available | Gemini `429` with `Retry-After` |

Selection is least-in-flight with round-robin tie-breaking. Quota is project-scoped, auth and
cooling are profile-scoped, and none of these states affect Claude subscriptions or Codex homes.

## Metering and settlement

Pricing is a pinned, effective-dated paid-tier catalog in `crates/metering`; a remote alias or Google
pricing-page edit cannot silently change customer money. Integer nanodollar arithmetic accounts for:

- uncached input and the audio input rate;
- cached text and cached audio;
- candidate plus thinking output;
- `toolUsePromptTokenCount`;
- Gemini 2.5 Pro long-context tiers;
- Google Search per grounded prompt for Gemini 2.5 and per query for Gemini 3.

Reservation is durable before the upstream call, delivery is durably marked before client bytes,
and settlement uses the last complete `usageMetadata`. For metered keys,
`generationConfig.maxOutputTokens` is clamped before the Google request to the largest conservative
input/tool/output reserve the live balance can afford; the gateway never relies on a partial hold
for an uncapped generation. Ledger attribution is provider `google`.
Shutdown drains streams until its deadline, then aborts stalled upstream reads, settles the last
known snapshot, crosses the stream barrier, and only afterward flushes billing.

Google Maps and File Search fail closed because they can add provider SKUs not fully represented by
the current ledger. Unknown future tool types also fail closed. Google Search has a dedicated price
path; URL Context, code execution, computer use, and function calls are allowed because their
billable tokens are represented by native usage metadata for the supported models.

## Deployment, rollback, and operations

Use the managed infrastructure/watchdog workflow, not an ad-hoc combined process:

| Purpose | Address/unit |
|---|---|
| runtime readiness | `http://127.0.0.1:8795/ready` |
| stable Caddy origin | `http://127.0.0.1:8794` |
| public API | `https://gemini.api.apitoken.sale` |
| systemd | `claude-api-gemini.service` |

Engine releases carry `.gemini-provider-v1`. `engine-bluegreen.sh` pre-drains the old process,
restarts it from the selected immutable release, requires direct/stable readiness and a native public
envelope, then commits enablement. Rolling back to a release without the marker stops and disables
Gemini while restoring Claude/OpenAI.

Provider-only kill switch is `CLAUDE_API_GEMINI_ENABLED=0` followed by the normal health-gated
provider rollout. The fixed Gemini service stays ready and returns a native `NOT_FOUND` envelope,
while no paid project is loaded or contacted. This keeps monitoring/routing truthful and survives
watchdog reconciliation. For an immediate incident stop, stopping the unit is temporary only: the
watchdog will restore the configured state.

```bash
# /srv/claude-api/data/config.env
CLAUDE_API_GEMINI_ENABLED=0
```

Useful checks:

```bash
systemctl status claude-api-gemini.service
journalctl -u claude-api-gemini.service --since '-30 min'
curl --fail http://127.0.0.1:8795/ready
curl --fail http://127.0.0.1:8794/ready
curl --resolve gemini.api.apitoken.sale:443:127.0.0.1 \
  https://gemini.api.apitoken.sale/v1beta/models/gemini-provider-probe:generateContent \
  -H 'content-type: application/json' -d '{}'
```

The public probe returns native `UNAUTHENTICATED` when enabled or `NOT_FOUND` when the kill switch is
active, proving that the hostname reached the Gemini router without exposing a credential.

Key metrics, all scraped with `provider="gemini"`:

- `claude_api_gemini_enabled`
- `claude_api_gemini_profiles{,_available,_authenticated}`
- `claude_api_gemini_profile_authenticated{profile=...}`
- `claude_api_gemini_profile_cooling_until_seconds{profile=...}`
- `claude_api_gemini_profile_inflight_requests{profile=...}`
- `claude_api_gemini_soonest_ready_seconds`
- shared request/upstream counters scoped to provider `gemini`

Prometheus also runs a public `gemini-http` blackbox probe. Alert procedures are in `MONITORING.md`
under `GeminiProviderDown`, `GeminiNoAvailableProfiles`, `GeminiProfileUnauthenticated`, and
`GeminiUpstreamRateLimited`.
