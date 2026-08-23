import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { ReactNode } from "react";

vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode; className?: string }) => (
    <a href={props.href} className={props.className}>
      {props.children}
    </a>
  ),
}));

vi.mock("next/navigation", () => ({
  usePathname: () => "/",
}));

vi.mock("@/lib/realtime", () => ({
  useRealtimeStatus: () => ({ live: 1, total: 1, state: "live" as const }),
}));

vi.mock("@/lib/resources", () => ({
  refreshMountedResources: vi.fn(),
}));

import { Sidebar } from "./sidebar";
import { I18nProvider } from "@/lib/i18n";
import { NAV } from "@/lib/nav";

describe("Sidebar", () => {
  it("рисует липкую шапку с кнопкой меню и полным списком разделов", () => {
    const html = renderToString(
      <I18nProvider>
        <Sidebar />
      </I18nProvider>,
    );
    expect(html).toContain('id="admin-nav"');
    expect(html).toContain('aria-controls="admin-nav"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).toContain("Открыть меню");
    expect(html).toContain("К содержанию");
    for (const href of NAV.flatMap((group) => group.items.map((item) => item.href))) {
      expect(html).toContain(`href="${href}"`);
    }
  });
});
