import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

import AdminsPage from "./page";
import { isLastActiveAdmin, parseDomainsInput } from "./lib";

describe("Админы (admins page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<AdminsPage />);
    expect(html).toContain("Админы");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("parseDomainsInput", () => {
  const allowed = ["admin.apitoken.sale", "crm.apitoken.sale"];

  it("разбирает список через запятую: trim и дедупликация", () => {
    expect(parseDomainsInput(" admin.apitoken.sale , crm.apitoken.sale,admin.apitoken.sale ", allowed)).toEqual([
      "admin.apitoken.sale",
      "crm.apitoken.sale",
    ]);
  });

  it("отклоняет пустой список", () => {
    expect(parseDomainsInput("", allowed)).toBeNull();
    expect(parseDomainsInput(" , ,", allowed)).toBeNull();
  });

  it("отклоняет домены вне разрешённых", () => {
    expect(parseDomainsInput("evil.example.com", allowed)).toBeNull();
    expect(parseDomainsInput("admin.apitoken.sale, evil.example.com", allowed)).toBeNull();
  });
});

describe("isLastActiveAdmin", () => {
  it("true только для единственного активного администратора", () => {
    const accounts = [
      { id: "a1", status: "active" },
      { id: "a2", status: "disabled" },
    ];
    expect(isLastActiveAdmin(accounts, "a1")).toBe(true);
    expect(isLastActiveAdmin(accounts, "a2")).toBe(false);
  });

  it("false, когда активных несколько", () => {
    const accounts = [
      { id: "a1", status: "active" },
      { id: "a2", status: "active" },
    ];
    expect(isLastActiveAdmin(accounts, "a1")).toBe(false);
    expect(isLastActiveAdmin(accounts, "a2")).toBe(false);
  });

  it("false, когда активных нет (отключать disabled не требуется)", () => {
    expect(isLastActiveAdmin([{ id: "a1", status: "disabled" }], "a1")).toBe(false);
  });
});
