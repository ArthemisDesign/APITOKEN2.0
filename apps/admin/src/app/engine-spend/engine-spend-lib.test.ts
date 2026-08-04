import { describe, expect, it } from "vitest";
import {
  accountClassLabel,
  accountTitle,
  buildEngineSpendAccountsCsvRows,
  buildEngineSpendCsvRows,
  discountLabel,
  filterEngineSpendAccounts,
  providerLabel,
} from "./engine-spend-lib";

describe("engine spend formatting", () => {
  it("скидка считается от real-API и не делит на ноль", () => {
    expect(discountLabel(40, 100)).toBe("60%");
    expect(discountLabel(0, 0)).toBe("—");
    expect(discountLabel(10, undefined)).toBe("—");
  });

  it("подписи провайдеров человекочитаемы, неизвестный отдаётся как есть", () => {
    expect(providerLabel("openai")).toBe("GPT (Codex)");
    expect(providerLabel("anthropic")).toBe("Claude (подписки)");
    expect(providerLabel("google")).toBe("Gemini");
    expect(providerLabel("kimi")).toBe("kimi");
    expect(providerLabel(undefined)).toBe("—");
  });

  it("аккаунт клиента подписывается email, прочие — handle", () => {
    expect(accountTitle({ account: "acct_1", handle: "user:u1", owner: { email: "a@b.c" } })).toBe("a@b.c");
    expect(accountTitle({ account: "acct_2", handle: "openkeys-x", owner: null })).toBe("openkeys-x");
    expect(accountTitle({ account: "acct_3" })).toBe("acct_3");
    expect(accountClassLabel("client")).toBe("клиент");
    expect(accountClassLabel("openkeys")).toBe("OpenKeys");
    expect(accountClassLabel("internal")).toBe("внутренний");
  });

  it("CSV моделей содержит окно расхода построчно", () => {
    expect(buildEngineSpendCsvRows([
      { model: "gpt-5.6-sol", provider: "openai", requests: 6, charge_usd: 60, real_usd: 150 },
    ])).toEqual([["gpt-5.6-sol", "openai", 6, 60, 150, "60%"]]);
  });
});

describe("engine spend account filter", () => {
  const rows = [
    { account: "acct_c", account_class: "client" as const, owner: { email: "a@b.c" } },
    { account: "acct_o", account_class: "openkeys" as const, handle: "openkeys-x" },
    { account: "acct_i", account_class: "internal" as const, handle: "crm-parsing" },
  ];

  it("пустой фильтр не трогает выборку", () => {
    expect(filterEngineSpendAccounts(rows, "")).toHaveLength(3);
  });

  it("показывает только запрошенный класс — в том числе только OpenKeys", () => {
    expect(filterEngineSpendAccounts(rows, "openkeys").map((row) => row.account)).toEqual(["acct_o"]);
    expect(filterEngineSpendAccounts(rows, "client").map((row) => row.account)).toEqual(["acct_c"]);
    expect(filterEngineSpendAccounts(rows, "internal").map((row) => row.account)).toEqual(["acct_i"]);
  });

  it("CSV аккаунтов отдаёт класс и идентификаторы", () => {
    expect(buildEngineSpendAccountsCsvRows([
      { account: "acct_o", handle: "openkeys-x", account_class: "openkeys", requests: 3, charge_usd: 9, real_usd: 9 },
    ])).toEqual([["openkeys-x", "OpenKeys", "openkeys-x", "acct_o", 3, 9, 9]]);
  });
});
