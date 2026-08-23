import { describe, expect, it } from "vitest";
import { normalizeTableHead } from "./table-labels";

describe("normalizeTableHead", () => {
  it("сжимает пробелы заголовка колонки в одну подпись карточки", () => {
    expect(normalizeTableHead("провайдеры\n  30д")).toBe("провайдеры 30д");
    expect(normalizeTableHead("  клиент  ")).toBe("клиент");
    expect(normalizeTableHead("")).toBe("");
  });
});
