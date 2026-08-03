import { describe, expect, it } from "vitest";
import { API_PRODUCTS, apiTypeOf, parseApiType } from "./api-product";

describe("OpenKeys API products", () => {
  it("keeps legacy batches on the existing Claude product", () => {
    expect(apiTypeOf(null)).toBe("anthropic");
    expect(apiTypeOf(undefined)).toBe("anthropic");
    expect(apiTypeOf("anthropic")).toBe("anthropic");
  });

  it("accepts only the two supported issuance products", () => {
    expect(parseApiType("openai")).toBe("openai");
    expect(parseApiType("anthropic")).toBe("anthropic");
    expect(parseApiType("gpt")).toBeNull();
    expect(parseApiType(1)).toBeNull();
  });

  it("uses a dedicated OpenAI-compatible host and guide", () => {
    expect(API_PRODUCTS.openai.baseUrl).toBe("https://router.apitoken.sale/v1");
    expect(API_PRODUCTS.openai.docsPath).toBe("/docs/openai");
    expect(API_PRODUCTS.anthropic.docsPath).toBe("/docs/claude");
  });
});
