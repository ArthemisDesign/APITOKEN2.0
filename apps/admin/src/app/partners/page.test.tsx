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

import PartnersPage from "./page";
import { clampOffset, eligibleSumNano, partnerName, payoutReasonText, shortWallet } from "./helpers";

describe("Партнёры (partners page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<PartnersPage />);
    expect(html).toContain("Партнёры");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("partners helpers", () => {
  it("shortWallet: первые 6 и последние 4 символа через многоточие", () => {
    expect(shortWallet("0x1234567890abcdef")).toBe("0x1234…cdef");
    // Короткий адрес: slice(-4) берёт последние 4 символа даже при пересечении.
    expect(shortWallet("0xabc")).toBe("0xabc…xabc");
  });

  it("eligibleSumNano: суммирует payableNano только по eligible, BigInt-арифметика", () => {
    expect(
      eligibleSumNano([
        { eligible: true, payableNano: "1000000000" },
        { eligible: false, payableNano: "999000000000" },
        { eligible: true, payableNano: "2500000000" },
        { eligible: true },
      ]),
    ).toBe("3500000000");
    expect(eligibleSumNano([])).toBe("0");
  });

  it("payoutReasonText: eligible / ждёт окна / ярлыки причин / fallback", () => {
    expect(payoutReasonText({ eligible: true })).toBe("eligible");
    expect(payoutReasonText({ reason: "ok" })).toBe("ждёт окна");
    expect(payoutReasonText({ reason: "below_minimum" })).toBe("ниже минимума");
    expect(payoutReasonText({ reason: "no_wallet" })).toBe("нет кошелька");
    expect(payoutReasonText({ reason: "inactive" })).toBe("неактивен");
    expect(payoutReasonText({ reason: "zero" })).toBe("нет суммы");
    expect(payoutReasonText({ reason: "custom_reason" })).toBe("custom_reason");
    expect(payoutReasonText({})).toBe("нельзя");
  });

  it("partnerName: @telegram приоритетнее email/displayName, иначе тире", () => {
    expect(partnerName({ telegramUsername: "ivan", email: "a@b.c" })).toBe("@ivan");
    expect(partnerName({ email: "a@b.c", displayName: "Иван" })).toBe("a@b.c");
    expect(partnerName({ displayName: "Иван" })).toBe("Иван");
    expect(partnerName({})).toBe("—");
  });

  it("clampOffset: offset за границей total → последняя полная страница", () => {
    expect(clampOffset(0, 50, 120)).toBe(0);
    expect(clampOffset(50, 50, 120)).toBe(50);
    expect(clampOffset(100, 50, 120)).toBe(100);
    expect(clampOffset(150, 50, 120)).toBe(100);
    expect(clampOffset(50, 50, 10)).toBe(0);
    expect(clampOffset(50, 50, 0)).toBe(50); // total 0 — кламп не применяется (как в легаси)
  });
});
