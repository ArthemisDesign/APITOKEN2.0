import { describe, expect, it } from "vitest";
import { z } from "zod";
import { parseFeedPage } from "./sync.service.js";

const rowSchema = z.object({
  id: z.string().regex(/^\d+$/).transform(BigInt),
  value: z.string(),
});

describe("sales feed page cursor", () => {
  it("advances across a canonical page containing only filtered source rows", () => {
    expect(parseFeedPage({ items: [], nextCursor: "42" }, rowSchema, "usage_events"))
      .toEqual({ items: [], nextCursor: 42n });
  });

  it("keeps compatibility with the legacy array response during a rolling deploy", () => {
    expect(parseFeedPage([
      { id: "7", value: "first" },
      { id: "9", value: "second" },
    ], rowSchema, "usage_events")).toEqual({
      items: [
        { id: 7n, value: "first" },
        { id: 9n, value: "second" },
      ],
      nextCursor: 9n,
    });
  });

  it("rejects a producer cursor that would skip a returned item", () => {
    expect(() => parseFeedPage({
      items: [{ id: "10", value: "event" }],
      nextCursor: "9",
    }, rowSchema, "usage_events")).toThrow("cursor behind its items");
  });
});
