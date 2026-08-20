export const GEMINI_BATCH_MARKDOWN = `## Gemini Batch API

Gemini Batch runs independent non-streaming GenerateContent requests asynchronously. Use either \`https://router.apitoken.sale\` (recommended) or \`https://gemini.api.apitoken.sale\`; only the hostname changes. All routes use \`x-goog-api-key: sk-pool-…\`.

| Method | Path | Purpose |
|---|---|---|
| POST | \`/v1beta/models/{model}:batchGenerateContent\` | Create an operation |
| GET | \`/v1beta/batches/{id}\` | Poll and read item results |
| GET | \`/v1beta/batches?pageSize={n}&pageToken={token}\` | List account operations |
| POST | \`/v1beta/batches/{id}:cancel\` | Request cancellation |
| DELETE | \`/v1beta/batches/{id}\` | Delete a terminal operation |
| POST | \`/upload/v1beta/files\` | Start/continue resumable JSONL upload |
| GET/DELETE | \`/v1beta/files/{id}\` | Read/delete account-scoped file |
| GET | \`/v1beta/files/{id}:download\` | Stream or range-download an active file |

### Create and poll inline requests

\`\`\`bash
export APITOKEN_API_KEY="sk-pool-…"
export GEMINI_BASE="https://router.apitoken.sale"

curl -fsS "$GEMINI_BASE/v1beta/models/gemini-3.6-flash:batchGenerateContent" \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "content-type: application/json" \\
  -H "Idempotency-Key: product-summary-2026-09-01" \\
  -d '{"batch":{"displayName":"Product summaries","inputConfig":{"requests":[
    {"request":{"contents":[{"parts":[{"text":"Summarize product A."}]}]}},
    {"request":{"contents":[{"parts":[{"text":"Summarize product B."}]}]}}
  ]}}}'
\`\`\`

Save the returned \`name\` (\`batches/batch-…\`). Poll \`GET /v1beta/{name}\` with a delay until \`done\` is true. Read every \`response.inlinedResponses[]\` entry: it contains either a complete \`response\` or an item-level \`error\`. \`metadata.batchStats\` counters are decimal strings. \`done: true\` does not by itself mean that every item succeeded.

\`Idempotency-Key\` is an apiToken.sale extension. Exact replay returns the existing operation; reuse with different content returns \`409 ABORTED\`. Without it, every POST creates another job.

### JSONL file input

Each nonempty line is one object, not a JSON array:

\`\`\`jsonl
{"key":"product-a","request":{"contents":[{"parts":[{"text":"Summarize product A."}]}]}}
{"key":"product-b","request":{"contents":[{"parts":[{"text":"Summarize product B."}]}]}}
\`\`\`

Start a resumable upload with \`POST /upload/v1beta/files\`, \`X-Goog-Upload-Protocol: resumable\`, \`X-Goog-Upload-Command: start\`, and the required \`X-Goog-Upload-Header-Content-Length\`. Follow the relative \`X-Goog-Upload-URL\`; send non-final 8 MiB chunks with exact \`X-Goog-Upload-Offset\`, use zero-body command \`query\` after an ambiguous response, and finalize with \`upload, finalize\`. Pass the returned \`files/{id}\` as \`batch.inputConfig.fileName\`.

### Resource lifecycle

- List jobs with \`GET /v1beta/batches?pageSize=20\`; pass the returned \`nextPageToken\` on the next call.
- Read the complete \`batches/{id}\` with \`GET /v1beta/{name}\`. Request cancellation with \`POST /v1beta/{name}:cancel\`; already dispatched items may finish. Delete with \`DELETE /v1beta/{name}\` only after the operation becomes terminal.
- List files with \`GET /v1beta/files?pageSize=20\`. Get metadata at \`GET /v1beta/files/{id}\`; full downloads stream without buffering and one standard \`Range: bytes=start-end\` request returns 206 for resumable bounded reads. Delete with \`DELETE /v1beta/files/{id}\` only while no live Batch references it.

### Limits and differences from Google

- Inline create body and one JSONL line: 20 MiB. Up to 100,000 items, 100 nonterminal jobs/account, 8 MiB per upload chunk, 48-hour upload TTL, 48-hour queue deadline, 42-day terminal-result retention.
- The account must have enough available balance for the complete conservative hold when the Batch is created. Standard Gemini pricing and the normal account discount apply; there is no separate Batch discount or completion-time SLA.
- This is not Vertex AI Batch: no GCS, BigQuery, Google Cloud IAM, webhooks, embedding/update Batch methods, or image-output Batch models.
- \`inputConfig.requests\` is a direct array; Google SDK serializers that send an \`InlinedRequests\` wrapper are not accepted. Use the documented raw HTTP schema.
- Gateway files belong to the apiToken.sale account, not a Google project. Foreign Google \`files/...\` are invisible. \`fileData\` with gateway files is not currently supported; use JSONL \`fileName\`, or synchronous \`inlineData\`.
- Inline jobs return \`response.inlinedResponses\`. File-input jobs remain \`done:false\` until an ordered encrypted JSONL output file is atomically published, then return \`metadata.output.responsesFile\`; download that file and correlate each line by \`key\`.
- Operation times are Unix-second strings, not RFC 3339. Batch list supports \`pageSize\`/\`pageToken\`.
- Ultra subscriptions use up to 20 durable Batch slots; every other plan uses 2. Starts on one subscription are separated by a random durable 2–5 second interval.
- Full file downloads stream; single-range downloads support resumption. A file referenced by live work cannot be deleted.`;
