import { describe, expect, it, vi } from "vitest";
import { buildCsv, csvCell, csvDate } from "./csv";

describe("csvCell", () => {
  it("простые значения — как есть, null/undefined → пустая строка", () => {
    expect(csvCell("текст")).toBe("текст");
    expect(csvCell(42)).toBe("42");
    expect(csvCell(null)).toBe("");
    expect(csvCell(undefined)).toBe("");
  });

  it("RFC 4180: кавычки, разделитель и переводы строк → обёртка + удвоение", () => {
    expect(csvCell('сказал "привет"')).toBe('"сказал ""привет"""');
    expect(csvCell("a;b")).toBe('"a;b"');
    expect(csvCell("a\nb")).toBe('"a\nb"');
    expect(csvCell("a\rb")).toBe('"a\rb"');
  });
});

describe("buildCsv", () => {
  it("разделитель ';' (Excel-RU), строки через CRLF, BOM в начале", () => {
    const csv = buildCsv(["имя", "сумма"], [["клиент;1", "100"], ["клиент2", '5"']]);
    expect(csv.startsWith("\uFEFF")).toBe(true);
    expect(csv).toBe('\uFEFFимя;сумма\r\n"клиент;1";100\r\nклиент2;"5"""');
  });
});

describe("csvDate", () => {
  it("ISO-дата YYYY-MM-DD", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-31T19:01:30Z"));
    expect(csvDate()).toBe("2026-07-31");
    vi.useRealTimers();
  });
});
