import { describe, expect, it } from "vitest";
import {
  APITOKEN_DOCS_URL,
  OPENKEYS_PUBLIC_ORIGIN,
  UNIVERSAL_CONNECTIONS,
  universalKeyHandoverText,
} from "./universal-key";
import { OPENKEYS_SUPPORTED_MODELS } from "@claude-api/contracts";

describe("универсальный ключ OpenKeys", () => {
  it("разводит один ключ на отдельные Claude и OpenAI подключения", () => {
    expect(UNIVERSAL_CONNECTIONS.claude).toMatchObject({
      baseUrl: "https://router.apitoken.sale",
      docsPath: "/docs/claude",
      authHeader: "x-api-key",
    });
    expect(UNIVERSAL_CONNECTIONS.openai).toMatchObject({
      baseUrl: "https://router.apitoken.sale/v1",
      docsPath: "/docs/openai",
      authHeader: "Authorization: Bearer",
    });
  });

  it("формирует компактное готовое сообщение с одним секретом, номиналом и ссылками", () => {
    const text = universalKeyHandoverText({
      faceValue: "$50",
      secret: "sk-pool-test-secret",
      viewUrl: `${OPENKEYS_PUBLIC_ORIGIN}/profile/test-token`,
    });

    expect(text).toContain("Ваш API-ключ на $50 готов");
    expect(text.match(/sk-pool-test-secret/g)).toHaveLength(1);
    expect(text).toContain("🤖 Доступные модели");
    for (const model of OPENKEYS_SUPPORTED_MODELS) expect(text).toContain(model);
    expect(text).toContain("Также доступны Gemini и Kimi");
    expect(text).toContain(APITOKEN_DOCS_URL);
    expect(text).toContain(`${OPENKEYS_PUBLIC_ORIGIN}/profile/test-token`);
    expect(text).toContain("Номинал $50; списание 1:1");
  });

  it("использует список моделей из ответа выпуска, если он передан", () => {
    const text = universalKeyHandoverText({
      faceValue: "$10",
      secret: "sk-pool-custom",
      viewUrl: `${OPENKEYS_PUBLIC_ORIGIN}/profile/custom`,
      supportedModels: ["claude-test", "gpt-test"],
    });

    expect(text).toContain("Claude: claude-test");
    expect(text).toContain("GPT: gpt-test");
    expect(text).not.toContain("claude-opus-5");
  });
});
