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

vi.mock("next/navigation", () => ({
  useRouter: () => ({ replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

import PartnersPage from "./page";
import { I18nProvider } from "@/lib/i18n";
import {
  bnbMoney,
  clampOffset,
  eligibleSumNano,
  partnerName,
  payoutReasonText,
  payoutWalletReadiness,
  shortWallet,
} from "./helpers";

describe("Партнёры (partners page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<I18nProvider><PartnersPage /></I18nProvider>);
    expect(html).toContain("Партнёрская программа");
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

  it("bnbMoney: форматирует integer wei без float и сохраняет малый gas", () => {
    expect(bnbMoney("0")).toBe("0 BNB");
    expect(bnbMoney("10000000000000")).toBe("0.00001 BNB");
    expect(bnbMoney("2500000000000000")).toBe("0.0025 BNB");
    expect(bnbMoney("broken")).toBe("—");
  });

  it("payoutWalletReadiness: пустой кошелёк виден даже без текущих переводов", () => {
    expect(payoutWalletReadiness({
      configured: true,
      chain: {
        ready: true,
        hotWalletAddress: "0x1234567890abcdef",
        usdtBalanceNano: "0",
        bnbBalanceWei: "0",
        gasCostPerTransferWei: "10000000000000",
      },
    }, [])).toMatchObject({ kind: "warn", title: "Hot wallet пуст", requiredUsdtNano: "0", requiredBnbWei: "0" });
  });

  it("payoutWalletReadiness: считает exact покрытие USDT и BNB текущего списка", () => {
    const items = [
      { eligible: true, payableNano: "12000000000" },
      { eligible: true, payableNano: "5953884700" },
      { eligible: false, payableNano: "999000000000" },
    ];
    const base = {
      configured: true,
      chain: {
        ready: true,
        hotWalletAddress: "0x1234567890abcdef",
        usdtBalanceNano: "17953884700",
        bnbBalanceWei: "20000000000000",
        gasCostPerTransferWei: "10000000000000",
      },
    };

    expect(payoutWalletReadiness(base, items)).toMatchObject({
      kind: "ok",
      requiredUsdtNano: "17953884700",
      requiredBnbWei: "20000000000000",
      eligibleCount: 2,
    });
    expect(payoutWalletReadiness({
      ...base,
      chain: { ...base.chain, bnbBalanceWei: "19999999999999" },
    }, items)).toMatchObject({ kind: "bad", title: "Не хватает BNB" });
  });

  it("payoutWalletReadiness: неполный или недоступный ответ не превращает в нулевой баланс", () => {
    expect(payoutWalletReadiness({ configured: true }, [])).toMatchObject({
      kind: "bad",
      title: "Состояние кошелька не получено",
    });
    expect(payoutWalletReadiness({
      configured: true,
      chain: { ready: false, issue: "read_unavailable" },
    }, [])).toMatchObject({ kind: "bad", title: "Кошелёк не удалось проверить" });
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

  it("partnerName: только текущий Commerce email", () => {
    expect(partnerName({ email: "a@b.c" })).toBe("a@b.c");
    expect(partnerName({})).toBe("Commerce email недоступен");
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
