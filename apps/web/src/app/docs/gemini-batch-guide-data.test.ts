import { describe, expect, it } from "vitest";
import {
  GEMINI_BATCH_CREATE_CURL,
  GEMINI_BATCH_DIRECT_BASE,
  GEMINI_BATCH_ENDPOINTS,
  GEMINI_BATCH_JSONL,
  GEMINI_BATCH_MANAGE_CURL,
  GEMINI_BATCH_PARSE_TS,
  GEMINI_BATCH_ROUTER_BASE,
  GEMINI_BATCH_UPLOAD_CURL,
} from "./gemini-batch-guide-data";
import { buildApiReferenceMarkdown } from "@/lib/md-pages";

describe("Gemini Batch public documentation contract", () => {
  it("documents every supported public route and both origins", () => {
    expect(GEMINI_BATCH_ROUTER_BASE).toBe("https://router.apitoken.sale");
    expect(GEMINI_BATCH_DIRECT_BASE).toBe("https://gemini.api.apitoken.sale");
    expect(GEMINI_BATCH_ENDPOINTS.map((row) => `${row[0]} ${row[1]}`)).toEqual(expect.arrayContaining([
      "POST /v1beta/models/{model}:batchGenerateContent",
      "GET /v1beta/batches/{id}",
      "POST /v1beta/batches/{id}:cancel",
      "DELETE /v1beta/batches/{id}",
      "POST /upload/v1beta/files",
      "GET /v1beta/files/{id}:download",
    ]));
  });

  it("keeps copy-paste examples on the exact gateway subset", () => {
    expect(GEMINI_BATCH_CREATE_CURL).toContain('"requests": {');
    expect(GEMINI_BATCH_CREATE_CURL).toContain("Idempotency-Key");
    expect(GEMINI_BATCH_CREATE_CURL).toContain("x-goog-api-key");
    expect(GEMINI_BATCH_JSONL.split("\n")).toHaveLength(2);
    expect(GEMINI_BATCH_UPLOAD_CURL).toContain("x-goog-upload-command: start");
    expect(GEMINI_BATCH_UPLOAD_CURL).toContain("upload, finalize");
    expect(GEMINI_BATCH_UPLOAD_CURL).toContain('"fileName"');
    expect(GEMINI_BATCH_PARSE_TS).toContain("inlinedResponses");
    expect(GEMINI_BATCH_PARSE_TS).toContain("item.error");
    expect(GEMINI_BATCH_MANAGE_CURL).toContain("$BATCH_NAME:cancel");
    expect(GEMINI_BATCH_MANAGE_CURL).toContain("-X DELETE");
    expect(GEMINI_BATCH_MANAGE_CURL).toContain(":download");
    expect(GEMINI_BATCH_MANAGE_CURL).toContain("currently has no pageToken");
  });

  it("keeps machine-readable docs in parity with the HTML guide", () => {
    const markdown = buildApiReferenceMarkdown();
    expect(markdown).toContain("## Gemini Batch API");
    expect(markdown).toContain("/upload/v1beta/files");
    expect(markdown).toContain("fileData");
    expect(markdown).toContain("responsesFile");
    expect(markdown).toContain("### Resource lifecycle");
    expect(markdown).toContain("POST /v1beta/{name}:cancel");
    expect(markdown).not.toContain("batches and fine-tuning are not available");
  });
});
