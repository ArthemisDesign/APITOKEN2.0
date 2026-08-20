export type DocsLanguage = "en" | "ru";

export const GEMINI_BATCH_ROUTER_BASE = "https://router.apitoken.sale";
export const GEMINI_BATCH_DIRECT_BASE = "https://gemini.api.apitoken.sale";

export const GEMINI_BATCH_ENDPOINTS = [
  ["POST", "/v1beta/models/{model}:batchGenerateContent", "create"],
  ["GET", "/v1beta/batches/{id}", "poll"],
  ["GET", "/v1beta/batches?pageSize={n}&pageToken={token}", "list"],
  ["POST", "/v1beta/batches/{id}:cancel", "cancel"],
  ["DELETE", "/v1beta/batches/{id}", "delete"],
  ["POST", "/upload/v1beta/files", "upload"],
  ["GET", "/v1beta/files", "files-list"],
  ["GET", "/v1beta/files/{id}", "file-get"],
  ["GET", "/v1beta/files/{id}:download", "file-download"],
  ["DELETE", "/v1beta/files/{id}", "file-delete"],
] as const;

export const GEMINI_BATCH_CREATE_CURL = `export APITOKEN_API_KEY="sk-pool-…"
export GEMINI_BASE="${GEMINI_BATCH_ROUTER_BASE}"
# Direct alternative:
# export GEMINI_BASE="${GEMINI_BATCH_DIRECT_BASE}"

OPERATION=$(curl -fsS \
  "$GEMINI_BASE/v1beta/models/gemini-3.6-flash:batchGenerateContent" \
  -H "x-goog-api-key: $APITOKEN_API_KEY" \
  -H "content-type: application/json" \
  -H "Idempotency-Key: product-summary-2026-09-01" \
  -d '{
    "batch": {
      "displayName": "Product summaries",
      "inputConfig": {
        "requests": [
          {"request":{"contents":[{"role":"user","parts":[{"text":"Summarize product A."}]}]}},
          {"request":{"contents":[{"role":"user","parts":[{"text":"Summarize product B."}]}]}}
        ]
      }
    }
  }')

BATCH_NAME=$(printf '%s' "$OPERATION" | jq -r .name)
echo "$BATCH_NAME"`;

export const GEMINI_BATCH_POLL_CURL = `while :; do
  OPERATION=$(curl -fsS \
    "$GEMINI_BASE/v1beta/$BATCH_NAME" \
    -H "x-goog-api-key: $APITOKEN_API_KEY")
  [ "$(printf '%s' "$OPERATION" | jq -r .done)" = "true" ] && break
  sleep 5
done

# Every entry contains either .response or .error.
printf '%s' "$OPERATION" | jq '.response.inlinedResponses[]'`;

export const GEMINI_BATCH_JSONL = `{"key":"product-a","request":{"contents":[{"role":"user","parts":[{"text":"Summarize product A."}]}]}}
{"key":"product-b","request":{"contents":[{"role":"user","parts":[{"text":"Summarize product B."}]}]}}`;

export const GEMINI_BATCH_UPLOAD_CURL = `FILE=batch-input.jsonl
SIZE=$(wc -c < "$FILE" | tr -d ' ')

# 1. Start a resumable upload.
curl -fsS -D upload.headers -o /dev/null \
  -X POST "$GEMINI_BASE/upload/v1beta/files" \
  -H "x-goog-api-key: $APITOKEN_API_KEY" \
  -H "x-goog-upload-protocol: resumable" \
  -H "x-goog-upload-command: start" \
  -H "x-goog-upload-file-name: batch-input.jsonl" \
  -H "x-goog-upload-header-content-type: application/jsonl" \
  -H "x-goog-upload-header-content-length: $SIZE"

UPLOAD_PATH=$(awk 'BEGIN{IGNORECASE=1} /^x-goog-upload-url:/{gsub("\\r",""); print $2}' upload.headers)

# 2. Upload and finalize this file in one chunk (up to 8 MiB).
# For larger files, repeat command=upload with exact 8 MiB chunks and
# increasing x-goog-upload-offset, then use "upload, finalize" on the last chunk.
curl -fsS "$GEMINI_BASE$UPLOAD_PATH" \
  -X POST \
  -H "x-goog-api-key: $APITOKEN_API_KEY" \
  -H "x-goog-upload-protocol: resumable" \
  -H "x-goog-upload-command: upload, finalize" \
  -H "x-goog-upload-offset: 0" \
  -H "content-length: $SIZE" \
  --data-binary "@$FILE" > uploaded-file.json

FILE_NAME=$(jq -r .file.name uploaded-file.json)

# 3. Use the returned files/{id} as Batch JSONL input.
curl -fsS \
  "$GEMINI_BASE/v1beta/models/gemini-3.6-flash:batchGenerateContent" \
  -H "x-goog-api-key: $APITOKEN_API_KEY" \
  -H "content-type: application/json" \
  -d "{\"batch\":{\"displayName\":\"JSONL summaries\",\"inputConfig\":{\"fileName\":\"$FILE_NAME\"}}}"`;

export const GEMINI_BATCH_MANAGE_CURL = `# List Batches (save nextPageToken for the next page).
curl -fsS "$GEMINI_BASE/v1beta/batches?pageSize=20" \
  -H "x-goog-api-key: $APITOKEN_API_KEY"

# Read one operation. BATCH_NAME is the complete batches/{id} from create.
curl -fsS "$GEMINI_BASE/v1beta/$BATCH_NAME" \
  -H "x-goog-api-key: $APITOKEN_API_KEY"

# Request cancellation. Items already dispatched may still finish.
curl -fsS -X POST "$GEMINI_BASE/v1beta/$BATCH_NAME:cancel" \
  -H "x-goog-api-key: $APITOKEN_API_KEY" \
  -H "content-type: application/json" -d '{}'

# Delete only after the operation is terminal.
curl -fsS -X DELETE "$GEMINI_BASE/v1beta/$BATCH_NAME" \
  -H "x-goog-api-key: $APITOKEN_API_KEY"

# Files list supports pageSize, but currently has no pageToken.
curl -fsS "$GEMINI_BASE/v1beta/files?pageSize=20" \
  -H "x-goog-api-key: $APITOKEN_API_KEY"

FILE_ID="file-…" # ID without the files/ prefix
curl -fsS "$GEMINI_BASE/v1beta/files/$FILE_ID" \
  -H "x-goog-api-key: $APITOKEN_API_KEY"

# Public download supports active files up to 20 MiB.
curl -fsS "$GEMINI_BASE/v1beta/files/$FILE_ID:download" \
  -H "x-goog-api-key: $APITOKEN_API_KEY" -o downloaded.bin

# Deletion fails while a live Batch references the file.
curl -fsS -X DELETE "$GEMINI_BASE/v1beta/files/$FILE_ID" \
  -H "x-goog-api-key: $APITOKEN_API_KEY"`;

export const GEMINI_BATCH_PARSE_TS = `const operation = await response.json();
if (!operation.done) {
  console.log("Still running:", operation.metadata?.state);
  return;
}
if (operation.error) throw new Error(operation.error.message);

const stats = operation.metadata?.batchStats ?? {};
const total = BigInt(stats.requestCount ?? "0");
const failed = BigInt(stats.failedRequestCount ?? "0");

for (const [index, item] of
     (operation.response?.inlinedResponses ?? []).entries()) {
  if (item.error) {
    console.error(index, item.error.status, item.error.message);
  } else {
    console.log(index, item.response?.candidates ?? []);
  }
}`;
