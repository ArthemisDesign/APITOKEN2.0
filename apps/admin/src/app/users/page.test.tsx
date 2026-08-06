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

import UsersPage from "./page";
import { buildUsersCsvRows, clampedOffset, tierLabel, usersQuery, INITIAL_USER_PAGE } from "./users-lib";

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
    expect(tierLabel({ customer_type: "b2b" })).toBe("B2B");
  });

  it("B2C — единый flat-тариф без тир-лестницы", () => {
    expect(tierLabel({ customer_type: "b2c" })).toBe("B2C −50%");
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
        tier: 1,
        balance_usd: 12.5,
        spent_usd: 100,
        spent_30d_usd: 7,
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
      "B2C −50%",
      12.5,
      100,
      7,
      50,
      49.99,
      2,
      1,
      3,
      "2026-07-30T10:00:00Z",
      "2026-01-05T12:00:00Z",
    ]);
    // Минимальная строка: счётчики ключей — нули, остальные пропуски — "".
    expect(rows[1]).toEqual(["c@d.e", "", "disabled", "B2B", "", "", "", "", "", "", 0, 0, "", ""]);
  });
});
