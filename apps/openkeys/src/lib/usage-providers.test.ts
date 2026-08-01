import { describe, expect, it } from "vitest";
import { aggregateUsageProviders, usageProviderOf, type UsageModelRow } from "./usage-providers";

const model = (overrides: Partial<UsageModelRow>): UsageModelRow => ({
  model: "claude-opus-4-8",
  requests: 1,
  input_tokens: 10,
  output_tokens: 5,
  cache_read_tokens: 0,
  cache_write_5m_tokens: 0,
  cache_write_1h_tokens: 0,
  official_nano: "100",
  charged_nano: "40",
  ...overrides,
});

describe("разбивка USAGE по API", () => {
  it("относит GPT-модели к OpenAI, Gemini-модели к Google, а текущие остальные модели к Claude", () => {
    expect(usageProviderOf("gpt-5.6-sol")).toBe("openai");
    expect(usageProviderOf("GPT-5.6-sol")).toBe("openai");
    expect(usageProviderOf("gemini-3.6-flash")).toBe("gemini");
    expect(usageProviderOf("gemini-3.6-flash", "gemini")).toBe("gemini");
    expect(usageProviderOf("claude-opus-4-8")).toBe("claude");
    expect(usageProviderOf("claude-opus-4-8", "openai")).toBe("openai");
    expect(usageProviderOf("gpt-5.6-sol", "anthropic")).toBe("claude");
  });

  it("суммирует запросы, токены и bigint-деньги отдельно", () => {
    const [claude, openai, gemini] = aggregateUsageProviders([
      model({}),
      model({ model: "gpt-5.6-sol", requests: 2, input_tokens: 20, official_nano: "9007199254740993" }),
      model({ model: "gemini-3.6-flash", provider: "gemini", requests: 3, output_tokens: 7, charged_nano: "60" }),
    ]);

    expect(claude).toMatchObject({ requests: 1, tokens: 15, officialNano: 100n, chargedNano: 40n });
    expect(openai).toMatchObject({ requests: 2, tokens: 25, officialNano: 9_007_199_254_740_993n });
    expect(gemini).toMatchObject({ provider: "gemini", label: "Gemini / Google", requests: 3, tokens: 17, chargedNano: 60n });
  });
});
