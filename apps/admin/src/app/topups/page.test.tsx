import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { ReactNode } from "react";

// next/link вне рантайма Next подменяем обычной ссылкой (нужен ui.tsx).
vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode; className?: string; style?: unknown }) => (
    <a href={props.href} className={props.className} style={props.style as never}>
      {props.children}
    </a>
  ),
}));

import TopupsPage, {
  TOPUP_CSV_HEADER,
  buildTopupCsvRows,
  clampOffset,
  computeTotals,
  topupsPath,
} from "./page";

describe("Пополнения (topups page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<TopupsPage />);
    expect(html).toContain("Пополнения");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("topupsPath", () => {
  it("собирает query как легаси: limit/offset всегда, фильтры — только непустые", () => {
    expect(topupsPath({ offset: 0, limit: 50, q: "", provider: "", status: "" })).toBe("/admin/topups?limit=50&offset=0");
    expect(topupsPath({ offset: 100, limit: 50, q: "a@b.c", provider: "cryptomus", status: "paid" })).toBe(
      "/admin/topups?limit=50&offset=100&q=a%40b.c&provider=cryptomus&status=paid",
    );
    expect(topupsPath({ offset: 0, limit: 50, q: "", provider: "platega", status: "" })).toBe(
      "/admin/topups?limit=50&offset=0&provider=platega",
    );
  });
});

describe("computeTotals", () => {
  it("берёт totals из ответа, без них деградирует к размеру страницы", () => {
    expect(computeTotals({ payments: [{}], checkouts: [{}, {}] })).toEqual({
      paymentsTotal: 1,
      checkoutsTotal: 2,
      total: 2,
    });
    expect(
      computeTotals({ payments: [{}], checkouts: [], payments_total: 120, checkouts_total: 80 }),
    ).toEqual({ paymentsTotal: 120, checkoutsTotal: 80, total: 120 });
    expect(computeTotals(null)).toEqual({ paymentsTotal: 0, checkoutsTotal: 0, total: 0 });
  });
});

describe("clampOffset", () => {
  it("offset в пределах total не трогает", () => {
    expect(clampOffset(0, 50, 120)).toBe(0);
    expect(clampOffset(50, 50, 120)).toBe(50);
    expect(clampOffset(100, 50, 120)).toBe(100);
  });

  it("offset за пределами total откатывает на последнюю страницу", () => {
    expect(clampOffset(150, 50, 120)).toBe(100);
    expect(clampOffset(300, 50, 17)).toBe(0);
  });

  it("при total=0 не сбрасывает offset (как условие total>0 в легаси)", () => {
    expect(clampOffset(100, 50, 0)).toBe(100);
  });
});

describe("buildTopupCsvRows", () => {
  it("кладёт платежи первыми, чекауты следом, kind различает строки", () => {
    const rows = buildTopupCsvRows(
      [
        {
          email: "a@b.c",
          user_id: "u1",
          provider: "cryptomus",
          amount_usd: 10.5,
          status: "paid",
          credit_status: "confirmed",
          paid_at: "2026-07-01T10:00:00Z",
          provider_payment_id: "pp1",
        },
      ],
      [
        {
          email: "c@d.e",
          provider: "platega",
          amount_usd: 5,
          status: "pending",
          created_at: "2026-07-02T10:00:00Z",
          expires_at: "2026-07-03T10:00:00Z",
        },
      ],
    );
    expect(TOPUP_CSV_HEADER).toHaveLength(11);
    expect(rows).toEqual([
      ["payment", "a@b.c", "u1", "cryptomus", 10.5, "paid", "confirmed", "2026-07-01T10:00:00Z", "", "", "pp1"],
      ["checkout", "c@d.e", "", "platega", 5, "pending", "", "", "2026-07-02T10:00:00Z", "2026-07-03T10:00:00Z", ""],
    ]);
  });
});
