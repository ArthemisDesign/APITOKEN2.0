import { describe, expect, it } from "vitest";
import { sourceName } from "./sources";

describe("sourceName", () => {
  it("известные пути → русские подписи из admin-panel.js", () => {
    expect(sourceName("/admin/dashboard")).toBe("Коммерческая сводка");
    expect(sourceName("/spend-stats")).toBe("Статистика расхода");
    expect(sourceName("/settlement-health")).toBe("Settlement движка");
    expect(sourceName("/kimi-subs")).toBe("KIMI-подписки");
    expect(sourceName("/glm-subs")).toBe("GLM-подписки");
  });

  it("query-строка отрезается", () => {
    expect(sourceName("/admin/users?limit=50&offset=0")).toBe("Пользователи");
    expect(sourceName("/admin/finance/paying-users?days=30")).toBe("Платящие клиенты");
  });

  it("неизвестный путь возвращается как есть (без query)", () => {
    expect(sourceName("/unknown?x=1")).toBe("/unknown");
  });
});
