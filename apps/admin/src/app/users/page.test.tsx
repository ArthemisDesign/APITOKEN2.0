import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { ReactNode } from "react";

// next/link вне рантайма Next подменяем обычной ссылкой.
vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode; className?: string; style?: unknown }) => (
    <a href={props.href} className={props.className} style={props.style as never}>
      {props.children}
    </a>
  ),
}));

import UsersPage, { GIFT_CREDIT_REASON, UserProviderSpend } from "./page";
import { parseBusinessDiscount } from "./business-conversion-dialog";
import {
  buildUsersCsvRows,
  clampedOffset,
  formatUserProviderNano,
  tierLabel,
  userProviderRails,
  usersQuery,
  INITIAL_USER_PAGE,
} from "./users-lib";

describe("Пользователи (users page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<UsersPage />);
    expect(html).toContain("Пользователи");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("names admin credit as a gift rather than external payment evidence", () => {
    expect(GIFT_CREDIT_REASON).toBe("admin panel gift credit (not an external payment)");
  });
});

describe("B2B conversion", () => {
  it("принимает только целую скидку в операторском диапазоне", () => {
    expect(parseBusinessDiscount("0")).toBe(0);
    expect(parseBusinessDiscount(" 63 ")).toBe(63);
    expect(parseBusinessDiscount("95")).toBe(95);
    expect(parseBusinessDiscount("96")).toBeNull();
    expect(parseBusinessDiscount("10.5")).toBeNull();
    expect(parseBusinessDiscount("")).toBeNull();
  });
});

describe("usersQuery", () => {
  it("по умолчанию — limit/offset/sort/dir в порядке легаси, без пустых фильтров", () => {
    expect(usersQuery(INITIAL_USER_PAGE)).toBe("limit=50&offset=0&sort=created_at&dir=desc");
  });

  it("добавляет q/status/auth только при непустых значениях", () => {
    expect(usersQuery({ ...INITIAL_USER_PAGE, q: "a@b.c", status: "active", auth: "google" })).toBe(
      "limit=50&offset=0&sort=created_at&dir=desc&q=a%40b.c&status=active&auth=google",
    );
  });

  it("пробелы в q кодируются как в URLSearchParams легаси", () => {
    expect(usersQuery({ ...INITIAL_USER_PAGE, q: "ivan petrov" })).toContain("q=ivan+petrov");
  });
});

describe("clampedOffset", () => {
  it("валидный offset не трогает", () => {
    expect(clampedOffset(0, 50, 120)).toBeNull();
    expect(clampedOffset(50, 50, 120)).toBeNull();
    expect(clampedOffset(100, 50, 120)).toBeNull();
  });

  it("offset за концом откатывает на последнюю валидную страницу", () => {
    expect(clampedOffset(150, 50, 120)).toBe(100);
    expect(clampedOffset(120, 50, 120)).toBe(100);
    expect(clampedOffset(50, 50, 47)).toBe(0);
  });

  it("пустой total не считается промахом (источник мог деградировать)", () => {
    expect(clampedOffset(100, 50, 0)).toBeNull();
  });
});

describe("tierLabel", () => {
  it("B2B по customer_type", () => {
    expect(tierLabel({ customer_type: "b2b", multiplier_bp: 3700 })).toBe("B2B −63%");
  });

  it("B2C показывает сохранённый flat-множитель, включая dormant 4000 bp", () => {
    expect(tierLabel({ customer_type: "b2c", multiplier_bp: 5000 })).toBe("B2C −50%");
    expect(tierLabel({ customer_type: "b2c", multiplier_bp: 4000 })).toBe("B2C −60%");
    expect(tierLabel({ customer_type: "b2c" })).toBe("B2C");
  });
});

describe("provider rails", () => {
  it("масштабирует пять rails относительно крупнейшего расхода без float-денег", () => {
    const rails = userProviderRails({
      anthropic_nano: "26630000000",
      openai_nano: "1250000000",
      google_nano: "812800000",
      kimi_nano: "79100000",
      other_nano: "0",
    });
    expect(rails.map(({ label, shareBp, available }) => ({ label, shareBp, available }))).toEqual([
      { label: "Claude", shareBp: 10_000, available: true },
      { label: "GPT", shareBp: 469, available: true },
      { label: "Gemini", shareBp: 305, available: true },
      { label: "Kimi", shareBp: 29, available: true },
      { label: "Другие", shareBp: 0, available: true },
    ]);
  });

  it("не превращает отсутствующий или malformed producer field в известный ноль", () => {
    expect(userProviderRails(undefined).every((rail) => !rail.available)).toBe(true);
    const rails = userProviderRails({ anthropic_nano: "not-money", openai_nano: "1000000000" });
    expect(rails[0]).toMatchObject({ available: false, amountNano: null, shareBp: 0 });
    expect(rails[1]).toMatchObject({ available: true, amountNano: "1000000000", shareBp: 10_000 });
  });

  it("форматирует крупные суммы компактно, а малые — с четырьмя знаками как в макете", () => {
    expect(formatUserProviderNano("567000000000")).toBe("$567");
    expect(formatUserProviderNano("92770000000")).toBe("$92.77");
    expect(formatUserProviderNano("280000000")).toBe("$0.2800");
    expect(formatUserProviderNano("40000000")).toBe("$0.0400");
    expect(formatUserProviderNano("1")).toBe("<$0.0001");
    expect(formatUserProviderNano("wrong")).toBe("—");
  });

  it("SSR выводит пять подписанных строк и честный residual вместо Images", () => {
    const html = renderToString(<table><tbody><tr><UserProviderSpend user={{
      email: "rail@example.com",
      provider_spend_30d: {
        anthropic_nano: "26630000000",
        openai_nano: "1250000000",
        google_nano: "812800000",
        kimi_nano: "79100000",
        other_nano: "0",
      },
    }} /></tr></tbody></table>);
    expect(html).toContain("user-provider-stack");
    expect(html).toContain("Claude");
    expect(html).toContain("GPT");
    expect(html).toContain("Gemini");
    expect(html).toContain("Kimi");
    expect(html).toContain("Другие");
    expect(html).not.toContain("Images");
    expect(html).toContain("$26.63");
    expect(html).toContain("$0.0791");
  });
});

describe("buildUsersCsvRows", () => {
  it("собирает колонки таблицы: деньги сырыми числами, даты ISO, пропуски пустыми", () => {
    const rows = buildUsersCsvRows([
      {
        id: "u1",
        email: "a@b.c",
        display_name: "Ivan",
        status: "active",
        customer_type: "b2c",
        multiplier_bp: 4000,
        tier: 1,
        balance_usd: 12.5,
        spent_usd: 100,
        spent_30d_usd: 7,
        provider_spend_30d: {
          anthropic_nano: "6000000000",
          openai_nano: "1000000000",
          google_nano: "0",
          kimi_nano: "0",
          other_nano: "0",
        },
        cumulative_topup_usd: 50,
        payments: { paid_total_usd: 49.99, paid_count: 2 },
        api_keys: { active: 1, total: 3 },
        last_seen_at: "2026-07-30T10:00:00Z",
        created_at: "2026-01-05T12:00:00Z",
      },
      { id: "u2", email: "c@d.e", status: "disabled", customer_type: "b2b" },
    ]);
    expect(rows[0]).toEqual([
      "a@b.c",
      "Ivan",
      "active",
      "B2C −60%",
      12.5,
      100,
      7,
      "'6000000000",
      "'1000000000",
      "'0",
      "'0",
      "'0",
      50,
      49.99,
      2,
      1,
      3,
      "2026-07-30T10:00:00Z",
      "2026-01-05T12:00:00Z",
    ]);
    // Минимальная строка: счётчики ключей — нули, остальные пропуски — "".
    expect(rows[1]).toEqual(["c@d.e", "", "disabled", "B2B", "", "", "", "", "", "", "", "", "", "", "", 0, 0, "", ""]);
  });
});
