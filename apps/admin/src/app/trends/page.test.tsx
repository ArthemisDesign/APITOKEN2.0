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

import TrendsPage from "./page";

describe("Тренды (trends page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<TrendsPage />);
    expect(html).toContain("Тренды");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});
