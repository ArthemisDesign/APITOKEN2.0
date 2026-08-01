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

import SystemPage, { systemVerdict, type SystemOverview } from "./page";

const baseOverview = (patch: Partial<SystemOverview> = {}): SystemOverview => ({
  subs: 3,
  ref_mult: 2,
  target_headroom: 2,
  supply: {
    avail_usd: { "7d": 40, "1d": 30, "5h": 20 },
    cap_usd: { "5h": 20, "7d": 100 },
    consumed_usd: { "5h": 1, "7d": 5 },
    util: { "5h": 0.05, "7d": 0.5 },
    health: { healthy: 3, cooling: 0, suspect: 0, dead: 0 },
  },
  demand: { balance_usd: 500, reserved_usd: 1, spent_usd: 9, active_accounts: 4, potential_realapi_usd: 2500 },
  headroom: { "5h": 8, "7d": 8 },
  coverage: { "7d": 0.5 },
  recommend: { subs_needed: 1, gap: -2 },
  ...patch,
});

describe("Система (system page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<SystemPage />);
    expect(html).toContain("Система");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("systemVerdict", () => {
  it("ok, когда запас выше цели и нет охлаждающихся подписок", () => {
    const verdict = systemVerdict(baseOverview());
    expect(verdict.kind).toBe("ok");
    expect(verdict.title).toBe("Запаса ёмкости хватает");
    expect(verdict.detail).toContain("цель ×2 выдержана");
  });

  it("bad при headroom ниже критического порога (<1)", () => {
    const verdict = systemVerdict(baseOverview({ headroom: { "5h": 0.4, "7d": 8 } }));
    expect(verdict.kind).toBe("bad");
    expect(verdict.title).toBe("Дефицит ёмкости — нужно +1 подписок");
  });

  it("bad, когда все подписки в cooling", () => {
    const overview = baseOverview();
    overview.supply = { ...overview.supply, health: { healthy: 0, cooling: 3 } };
    const verdict = systemVerdict(overview);
    expect(verdict.kind).toBe("bad");
  });

  it("warn при положительном gap — рекомендация докупить", () => {
    const verdict = systemVerdict(baseOverview({ recommend: { subs_needed: 5, gap: 2 } }));
    expect(verdict.kind).toBe("warn");
    expect(verdict.detail).toContain("рекомендуется +2 подписок");
  });

  it("warn, когда запас ниже целевого headroom", () => {
    const verdict = systemVerdict(baseOverview({ headroom: { "5h": 1.5, "7d": 8 } }));
    expect(verdict.kind).toBe("warn");
    expect(verdict.detail).toContain("запас ниже цели ×2");
  });

  it("warn при coverage выше ёмкости (×>1)", () => {
    const verdict = systemVerdict(baseOverview({ coverage: { "7d": 62.5 } }));
    expect(verdict.kind).toBe("warn");
    expect(verdict.detail).toContain("балансы клиентов ×62.5 к ёмкости");
  });

  it("warn при частичном cooling", () => {
    const overview = baseOverview();
    overview.supply = { ...overview.supply, health: { healthy: 2, cooling: 1 } };
    const verdict = systemVerdict(overview);
    expect(verdict.kind).toBe("warn");
    expect(verdict.detail).toContain("1 подписок остывают");
  });

  it("null headroom (нет спроса) рендерится как ∞ и не даёт false-critical", () => {
    const verdict = systemVerdict(baseOverview({ headroom: { "5h": null, "7d": null } }));
    expect(verdict.kind).toBe("ok");
    expect(verdict.detail).toContain("headroom 5h ∞ / 7d ∞");
  });

  it("exact authority без current remaining предупреждает и не трактуется как нулевая ёмкость", () => {
    const overview = baseOverview();
    overview.supply = {
      ...overview.supply,
      authority: "exact_provider_turns_and_quota_fractions",
      avail_usd: { "5h": null, "7d": null },
    };
    const verdict = systemVerdict(overview);
    expect(verdict.kind).toBe("warn");
    expect(verdict.title).toBe("Точная ёмкость временно неизвестна");
    expect(verdict.detail).toContain("prior/EMA не подставляется");
  });
});
