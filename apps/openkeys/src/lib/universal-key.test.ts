import { describe, expect, it } from "vitest";
import {
  OPENKEYS_PUBLIC_ORIGIN,
  UNIVERSAL_CONNECTIONS,
  universalKeyHandoverText,
} from "./universal-key";

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

  it("формирует полное сообщение покупателю без второго ключа и второго USAGE", () => {
    const text = universalKeyHandoverText({
      faceValue: "$50",
      secret: "sk-pool-test-secret",
      viewUrl: `${OPENKEYS_PUBLIC_ORIGIN}/profile/test-token`,
    });

    // Ровно один раз на каждое подключение: захардкоженное число молча расходится с реестром,
    // как только появляется новый провайдер, и тогда тест начинает проверять вчерашний текст.
    const connections = Object.keys(UNIVERSAL_CONNECTIONS).length;
    expect(text.match(/sk-pool-test-secret/g)).toHaveLength(connections);
    // Ключ один и баланс общий — второго секрета в выдаче быть не должно.
    expect(text.match(/sk-pool-/g)).toHaveLength(connections);
    expect(text).toContain("ANTHROPIC_BASE_URL=https://router.apitoken.sale");
    expect(text).toContain("OPENAI_BASE_URL=https://router.apitoken.sale/v1");
    expect(text).toContain("GOOGLE_GEMINI_BASE_URL=https://router.apitoken.sale");
    expect(text).toContain(`${OPENKEYS_PUBLIC_ORIGIN}/docs/claude`);
    expect(text).toContain(`${OPENKEYS_PUBLIC_ORIGIN}/docs/openai`);
    // Kimi выбирается идентификатором модели, а не отдельным base URL — без этой строки
    // покупатель получил бы переменные Claude и не понял, как обратиться к Kimi.
    expect(text).toContain("kimi/k3");
    expect(text.match(/Остаток и расход:/g)).toHaveLength(1);
  });
});
