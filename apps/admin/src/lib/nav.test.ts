import { describe, expect, it } from "vitest";
import { NAV, isNavItemActive, navLabelForPath } from "./nav";

// Пути, которые Caddy на admin.apitoken.sale проксирует мимо фронтенда
// (deploy/Caddyfile, блок admin.apitoken.sale): data-роуты движка и
// префиксы других backend'ов. Роут страницы приложения НЕ может совпасть
// с ними — запрос уйдёт в backend раньше Next.js (регрессия: страница
// «Подписки» жила на /subs и отдавала сырой JSON движка).
const RESERVED_EXACT = ["/overview", "/capacity", "/metrics", "/subs", "/spend-stats", "/codex-subs", "/gemini-subs"];
const RESERVED_PREFIXES = ["/admin/", "/openkeys-admin/", "/partner-admin/"];

describe("NAV", () => {
  const hrefs = NAV.flatMap((group) => group.items.map((item) => item.href));

  it("не содержит роутов, занятых data-роутами Caddy", () => {
    for (const href of hrefs) {
      expect(RESERVED_EXACT, `${href} совпадает с data-роутом движка`).not.toContain(href);
      for (const prefix of RESERVED_PREFIXES) {
        expect(href.startsWith(prefix), `${href} попадает под backend-префикс ${prefix}`).toBe(false);
      }
    }
  });

  it("роуты уникальны", () => {
    expect(new Set(hrefs).size).toBe(hrefs.length);
  });

  it("isNavItemActive: корень точный, остальные по префиксу", () => {
    expect(isNavItemActive("/", "/")).toBe(true);
    expect(isNavItemActive("/users", "/")).toBe(false);
    expect(isNavItemActive("/subscriptions", "/subscriptions")).toBe(true);
  });

  it("navLabelForPath возвращает подпись активного раздела", () => {
    expect(navLabelForPath("/subscriptions")).toBe("Подписки");
    expect(navLabelForPath("/paying-users")).toBe("Платящие");
    expect(navLabelForPath("/sales/calculator")).toBe("Калькулятор");
    expect(navLabelForPath("/")).toBe("Сводка");
    expect(navLabelForPath("/no-such-page")).toBe("Сводка");
  });
});
