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

import FinancePage from "./page";
import {
  buildRevenueSeries,
  clampPercent,
  clampRefundOffset,
  funnelShare,
  customerClassName,
} from "./finance-lib";

describe("Финансы (finance page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<FinancePage />);
    expect(html).toContain("Финансы");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("finance-lib", () => {
  it("customerClassName: класс клиента в ярлык, неизвестное — как есть", () => {
    expect(customerClassName("b2b")).toBe("B2B");
    expect(customerClassName("b2c")).toBe("B2C");
    expect(customerClassName("custom")).toBe("custom");
  });

  it("clampPercent: округление и зажим в 0–100, мусор → 0", () => {
    expect(clampPercent(55.5)).toBe(56);
    expect(clampPercent(-3)).toBe(0);
    expect(clampPercent(250)).toBe(100);
    expect(clampPercent("12.2")).toBe(12);
    expect(clampPercent(undefined)).toBe(0);
    expect(clampPercent("abc")).toBe(0);
  });

  it("funnelShare: доля от созданных с одним знаком, без созданных → 0", () => {
    expect(funnelShare(50, 200)).toBe(25);
    expect(funnelShare(1, 3)).toBe(33.3);
    expect(funnelShare(10, 0)).toBe(0);
    expect(funnelShare(undefined, 100)).toBe(0);
  });

  it("clampRefundOffset: откат ушедшей страницы на последнюю", () => {
    expect(clampRefundOffset(0, 25, 100)).toBeNull();
    expect(clampRefundOffset(75, 25, 100)).toBeNull();
    expect(clampRefundOffset(100, 25, 100)).toBe(75);
    expect(clampRefundOffset(25, 25, 26)).toBeNull();
    expect(clampRefundOffset(50, 25, 26)).toBe(25);
    expect(clampRefundOffset(50, 25, 0)).toBeNull();
    expect(clampRefundOffset(50, 25, undefined)).toBeNull();
  });

  it("buildRevenueSeries: основная линия + по линии на провайдера, null разрывает", () => {
    const series = buildRevenueSeries({
      series: [
        { day: "2026-07-01", total_usd: 10, by_provider: { stripe: 5_000_000_000, yookassa: null } },
        { day: "2026-07-02", total_usd: 20, by_provider: { stripe: 8_000_000_000 } },
      ],
      totals: { total_usd: 30, payments_count: 2, by_provider: { stripe: 13, yookassa: 0 } },
    });
    expect(series.map((item) => item.label)).toEqual(["выручка $/день", "stripe", "yookassa"]);
    const ts1 = Date.parse("2026-07-01T00:00:00Z") / 1000;
    const ts2 = Date.parse("2026-07-02T00:00:00Z") / 1000;
    expect(series[0].points).toEqual([
      { ts: ts1, value: 10 },
      { ts: ts2, value: 20 },
    ]);
    // nanoUSD → USD только для отображения графика.
    expect(series[1].points).toEqual([
      { ts: ts1, value: 5 },
      { ts: ts2, value: 8 },
    ]);
    // Отсутствующее значение провайдера за день — разрыв линии (null).
    expect(series[2].points).toEqual([
      { ts: ts1, value: null },
      { ts: ts2, value: null },
    ]);
  });

  it("buildRevenueSeries: пустой ответ — только основная пустая серия", () => {
    const series = buildRevenueSeries({});
    expect(series).toEqual([{ label: "выручка $/день", points: [] }]);
  });
});
