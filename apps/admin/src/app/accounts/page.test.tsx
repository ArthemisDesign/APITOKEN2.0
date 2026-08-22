import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { ReactNode } from "react";

// next/link вне рантайма Next подменяем обычной ссылкой.
vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode; className?: string }) => (
    <a href={props.href} className={props.className}>
      {props.children}
    </a>
  ),
}));

import AccountsPage, { clampPartnerOffset, partnerName } from "./page";

describe("Аккаунты (accounts page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<AccountsPage />);
    expect(html).toContain("Аккаунты");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  describe("clampPartnerOffset (зажатие пейджера партнёров)", () => {
    it("не трогает offset в пределах списка", () => {
      expect(clampPartnerOffset(0, 120)).toBe(0);
      expect(clampPartnerOffset(50, 120)).toBe(50);
      expect(clampPartnerOffset(100, 120)).toBe(100);
    });
    it("зажимает offset на последнюю страницу, когда список сократился", () => {
      expect(clampPartnerOffset(150, 120)).toBe(100);
      expect(clampPartnerOffset(50, 50)).toBe(0);
      expect(clampPartnerOffset(200, 3)).toBe(0);
    });
    it("при пустом списке (деградация источника) offset не меняется", () => {
      expect(clampPartnerOffset(50, 0)).toBe(50);
    });
  });

  describe("partnerName (отображаемое имя партнёра)", () => {
    it("предпочитает email, затем displayName, затем @telegram", () => {
      expect(partnerName({ telegramUsername: "agent", email: "a@b.c", displayName: "Agent" })).toBe("a@b.c");
      expect(partnerName({ email: "a@b.c", displayName: "Agent" })).toBe("a@b.c");
      expect(partnerName({ displayName: "Agent" })).toBe("Agent");
      expect(partnerName({ telegramUsername: "agent" })).toBe("@agent");
      expect(partnerName({})).toBe("—");
    });
  });
});
