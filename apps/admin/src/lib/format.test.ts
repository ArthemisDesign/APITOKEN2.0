import { describe, expect, it, vi } from "vitest";
import {
  ageText,
  ago,
  count,
  duration,
  formatDate,
  money,
  nanoMoney,
  plural,
  ratio,
  windowLabel,
} from "./format";

describe("nanoMoney", () => {
  it("форматирует целочисленные nanoUSD-строки через BigInt", () => {
    expect(nanoMoney("1234567890")).toBe("$1.23");
    expect(nanoMoney("1000000000")).toBe("$1.00");
    expect(nanoMoney("123456789012345")).toBe("$123,456.78");
    expect(nanoMoney("0")).toBe("$0.00");
  });

  it("обрезает до центов и не теряет точность на больших суммах", () => {
    expect(nanoMoney("1999999999")).toBe("$1.99");
    expect(nanoMoney("123456789012345678901234567890")).toBe("$123,456,789,012,345,678,901.23");
  });

  it("отрицательные суммы — с минусом «−»", () => {
    expect(nanoMoney("-2500000000")).toBe("−$2.50");
  });

  it("пустой и невалидный ввод → $0.00, как в admin-panel.js", () => {
    expect(nanoMoney(null)).toBe("$0.00");
    expect(nanoMoney(undefined)).toBe("$0.00");
    expect(nanoMoney("abc")).toBe("$0.00");
  });
});

describe("money (легаси-поля в долларах, только отображение)", () => {
  it("форматирует с двумя знаками", () => {
    expect(money(1234.5)).toBe("$1,234.50");
    expect(money(0)).toBe("$0.00");
    expect(money(null)).toBe("$0.00");
  });
});

describe("duration / ageText", () => {
  it("секунды → человекочитаемая длительность", () => {
    expect(duration(0)).toBe("0м");
    expect(duration(420)).toBe("7м");
    expect(duration(5 * 3600 + 12 * 60)).toBe("5ч 12м");
    expect(duration(2 * 86400 + 3 * 3600)).toBe("2д 3ч");
  });

  it("отрицательные и мусорные значения зажимаются в ноль", () => {
    expect(duration(-5)).toBe("0м");
    expect(duration(NaN)).toBe("0м");
  });

  it("ageText: null → тире", () => {
    expect(ageText(null)).toBe("—");
    expect(ageText(90)).toBe("1м");
  });
});

describe("ago", () => {
  it("возраст метки времени", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-31T12:00:00Z"));
    expect(ago("2026-07-31T11:59:40Z")).toBe("сейчас");
    expect(ago("2026-07-31T11:55:00Z")).toBe("5м");
    expect(ago("2026-07-31T09:00:00Z")).toBe("3ч");
    expect(ago("2026-07-29T12:00:00Z")).toBe("2д");
    expect(ago(null)).toBe("—");
    vi.useRealTimers();
  });
});

describe("formatDate", () => {
  it("пустое значение → тире", () => {
    expect(formatDate(null)).toBe("—");
    expect(formatDate(undefined, true)).toBe("—");
  });

  it("ru-RU дата и дата+время", () => {
    expect(formatDate("2026-07-31T12:34:56Z")).toMatch(/^\d{2}\.\d{2}\.\d{4}$/);
    expect(formatDate("2026-07-31T12:34:56Z", true)).toMatch(/^\d{2}\.\d{2}\.\d{4}, \d{2}:\d{2}$/);
  });
});

describe("ratio", () => {
  it("null → ∞, <10 — один знак, иначе целое", () => {
    expect(ratio(null)).toBe("∞");
    expect(ratio(2.54)).toBe("×2.5");
    expect(ratio(42)).toBe("×42");
  });
});

describe("plural / count", () => {
  const forms: [string, string, string] = ["подписка", "подписки", "подписок"];
  it("русская плюрализация", () => {
    expect(plural(1, ...forms)).toBe("подписка");
    expect(plural(2, ...forms)).toBe("подписки");
    expect(plural(5, ...forms)).toBe("подписок");
    expect(plural(11, ...forms)).toBe("подписок");
    expect(plural(21, ...forms)).toBe("подписка");
    expect(plural(22, ...forms)).toBe("подписки");
  });

  it("count добавляет число", () => {
    expect(count(3, ...forms)).toBe("3 подписки");
  });
});

describe("windowLabel", () => {
  it("минуты → подпись окна", () => {
    expect(windowLabel(0)).toBe("окно");
    expect(windowLabel(15)).toBe("15 мин");
    expect(windowLabel(120)).toBe("2 ч");
    expect(windowLabel(4320)).toBe("3 д");
  });
});
